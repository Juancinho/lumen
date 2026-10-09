//! Metadata pass before visual inference; no pixels/text retained in SQLite.
use crate::PassReport;
use lumen_core::CancellationToken;
use lumen_storage::{ContentOutcome, ContentWrite, StorageError, Store};
use std::time::Instant;

/// Bounded headers/orientation/digests for content-enabled images on the existing writer.
/// # Errors
/// Storage failure; per-file admission and I/O errors have stable coverage codes.
pub fn run_image_pass(
    store: &mut Store,
    scope: &dyn Fn(&str) -> bool,
    cancel: &CancellationToken,
    now: &dyn Fn() -> i64,
) -> Result<PassReport, StorageError> {
    let started = Instant::now();
    let mut report = PassReport::default();
    let mut cursor = 0;
    loop {
        let candidates = store.content_candidates(cursor, lumen_image::EXTENSIONS, 1, 32)?;
        let Some(last) = candidates.last() else { break };
        cursor = last.item_id;
        for candidate in candidates.iter().filter(|c| scope(&c.path)) {
            if cancel.is_cancelled() {
                report.cancelled = true;
                break;
            }
            let path = lumen_catalog::path::decode(&candidate.path, candidate.raw_path.as_deref());
            match lumen_image::inspect(&path, &|| cancel.is_cancelled()) {
                Ok(metadata) => {
                    let metadata = lumen_storage::images::ImageMetadata {
                        width: metadata.width,
                        height: metadata.height,
                        orientation: metadata.orientation,
                        format: metadata.format.into(),
                        digest: metadata.digest.to_vec(),
                    };
                    if store.write_image(
                        candidate,
                        &metadata,
                        lumen_image::PREPROCESSING_VERSION,
                        now(),
                    )? {
                        report.indexed += 1;
                        report.chunks += 1;
                    }
                }
                Err(lumen_image::Error::Cancelled) => {
                    report.cancelled = true;
                    break;
                }
                Err(error) => {
                    let retry = matches!(
                        error,
                        lumen_image::Error::Io(_) | lumen_image::Error::Changed
                    );
                    let fingerprint = candidate.fingerprint();
                    store.write_content(
                        &[ContentWrite {
                            item_id: candidate.item_id,
                            fingerprint: &fingerprint,
                            outcome: if retry {
                                ContentOutcome::Failed(error.code())
                            } else {
                                ContentOutcome::Skipped(error.code())
                            },
                        }],
                        1,
                        now(),
                    )?;
                    if retry {
                        report.failed += 1;
                    } else {
                        report.skipped += 1;
                    }
                }
            }
            report.files += 1;
        }
        if report.cancelled {
            break;
        }
    }
    report.elapsed = started.elapsed();
    Ok(report)
}
