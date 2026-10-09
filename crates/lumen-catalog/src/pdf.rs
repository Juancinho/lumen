//! Attach trusted PDF page context to the existing file row (T301, ADR-039).
use lumen_core::{Payload, PdfTarget, ResultItem, ResultKind};
use lumen_storage::ChunkRef;

pub fn enrich(result: &mut ResultItem, reference: &ChunkRef, extension: Option<&str>) {
    if !extension.is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return;
    }
    let Some(page_number) = reference
        .page_number
        .and_then(|p| u32::try_from(p).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return;
    };
    let Some(path) = result
        .payload
        .local_path()
        .map(std::path::Path::to_path_buf)
    else {
        return;
    };
    result.kind = ResultKind::PdfPage;
    result.payload = Payload::Pdf(Box::new(PdfTarget {
        path,
        page_number,
        passage: reference.excerpt.clone(),
    }));
}
