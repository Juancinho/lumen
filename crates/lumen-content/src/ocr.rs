//! Optional lexical image text on the existing writer; native recognition is injected.
use crate::{PassReport, Slice};
use lumen_core::CancellationToken;
use lumen_storage::{StorageError, Store};
use std::time::Instant;

pub const MAX_EDGE: u32 = lumen_image::MAX_OCR_SIDE;
pub const MAX_PIXELS: u64 = lumen_image::MAX_OCR_PIXELS;

pub struct OcrText {
    pub text: String,
    pub language: String,
}

pub enum OcrError {
    Cancelled,
    Failed(&'static str),
}

pub trait Recognizer {
    /// Native work must cooperatively observe cancellation and bound its output/deadline.
    /// # Errors
    /// Cancellation leaves the current image eligible; a stable failure records coverage.
    fn recognize(
        &mut self,
        width: u32,
        height: u32,
        rgb: &[u8],
        stop: &dyn Fn() -> bool,
    ) -> Result<OcrText, OcrError>;
}

/// Enrich existing image FTS without replacing a chunk, visual vector or ANN sequence.
/// # Errors
/// Storage failure. Scope, source admission, cancellation and digest are rechecked at commit.
pub fn run_ocr_slice(
    store: &mut Store,
    scope: &dyn Fn(&str) -> bool,
    cancel: &CancellationToken,
    stop: &dyn Fn() -> bool,
    recognizer: &mut dyn Recognizer,
    after: i64,
    slice: Slice,
) -> Result<PassReport, StorageError> {
    let started = Instant::now();
    let stopped = || cancel.is_cancelled() || stop();
    let mut report = PassReport {
        cursor: after,
        ..PassReport::default()
    };
    'scan: loop {
        let candidates = store.ocr_candidates(report.cursor, 32)?;
        if candidates.is_empty() {
            report.exhausted = true;
            break;
        }
        for candidate in candidates {
            if stopped() {
                report.cancelled = true;
                break 'scan;
            }
            if started.elapsed() >= slice.max_run || report.files >= slice.max_files as u64 {
                break 'scan;
            }
            if !scope(&candidate.path) {
                report.cursor = candidate.item;
                continue;
            }
            let path = lumen_catalog::path::decode(&candidate.path, candidate.raw_path.as_deref());
            let outcome = if candidate.width > MAX_EDGE
                || candidate.height > MAX_EDGE
                || u64::from(candidate.width) * u64::from(candidate.height) > MAX_PIXELS
            {
                Err(OcrError::Failed("ocr:pixel_limit"))
            } else {
                match lumen_image::decode_ocr(&path, Some(&candidate.digest), &stopped) {
                    Ok(image) => recognizer.recognize(
                        image.metadata.width,
                        image.metadata.height,
                        &image.rgb,
                        &stopped,
                    ),
                    Err(lumen_image::Error::Cancelled) => Err(OcrError::Cancelled),
                    Err(lumen_image::Error::Changed) => {
                        store.invalidate_image(candidate.item)?;
                        report.cursor = candidate.item;
                        report.files += 1;
                        continue;
                    }
                    Err(error) => Err(OcrError::Failed(error.code())),
                }
            };
            if stopped() || matches!(outcome, Err(OcrError::Cancelled)) {
                report.cancelled = true;
                break 'scan;
            }
            // Skip/failure also needs current disk identity: never attach coverage to an edit.
            match lumen_image::inspect(&path, &stopped) {
                Ok(metadata) if metadata.digest.as_slice() == candidate.digest => {}
                Err(lumen_image::Error::Cancelled) => {
                    report.cancelled = true;
                    break 'scan;
                }
                Ok(_) => {
                    store.invalidate_image(candidate.item)?;
                    report.cursor = candidate.item;
                    report.files += 1;
                    continue;
                }
                Err(error) => {
                    // Only a failure code is attached: no unverified recognized text survives.
                    if stopped() {
                        report.cancelled = true;
                        break 'scan;
                    }
                    store.write_ocr(&candidate, "", None, "failed", Some(error.code()))?;
                    report.cursor = candidate.item;
                    report.files += 1;
                    report.failed += 1;
                    continue;
                }
            }
            if stopped() || !scope(&candidate.path) {
                report.cancelled = true;
                break 'scan;
            }
            match outcome {
                Ok(output) => {
                    let empty = output.text.trim().is_empty();
                    if store.write_ocr(
                        &candidate,
                        if empty { "" } else { &output.text },
                        Some(&output.language),
                        if empty { "empty" } else { "indexed" },
                        None,
                    )? {
                        report.indexed += 1;
                        report.text_bytes += output.text.len() as u64;
                    }
                }
                Err(OcrError::Failed(reason)) => {
                    let skipped = matches!(
                        reason,
                        "ocr:pixel_limit"
                            | "ocr:text_limit"
                            | "image:unsupported"
                            | "image:source_limit"
                            | "image:pixel_limit"
                    );
                    if store.write_ocr(
                        &candidate,
                        "",
                        None,
                        if skipped { "skipped" } else { "failed" },
                        Some(reason),
                    )? {
                        if skipped {
                            report.skipped += 1;
                        } else {
                            report.failed += 1;
                        }
                    }
                }
                Err(OcrError::Cancelled) => unreachable!(),
            }
            report.cursor = candidate.item;
            report.files += 1;
        }
    }
    report.elapsed = started.elapsed();
    Ok(report)
}
