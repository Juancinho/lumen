//! Project a matched code passage onto the existing file row (T209).

use lumen_core::builtin::{COPY_SYMBOL, REVEAL_REPOSITORY};
use lumen_core::{Capability, CodeTarget, Payload, ResultItem, ResultKind};
use lumen_extract::{DocKind, kind_for_extension};
use lumen_storage::ChunkRef;

/// Enrich a file result without changing its entity id or replacing its primary action.
pub fn enrich(result: &mut ResultItem, reference: &ChunkRef, extension: Option<&str>) {
    if reference.kind != "code" {
        return;
    }
    let Some(DocKind::Code(language)) = extension.and_then(kind_for_extension) else {
        return;
    };
    let Some(path) = result
        .payload
        .local_path()
        .map(std::path::Path::to_path_buf)
    else {
        return;
    };
    let symbol = reference.symbol.clone().filter(|s| !s.trim().is_empty());
    let repository = reference
        .repository
        .as_deref()
        .map(std::path::PathBuf::from);
    if symbol.is_some() {
        result.capabilities = result.capabilities.with(Capability::CodeSymbol);
        result.secondary_actions.push(COPY_SYMBOL);
    }
    if repository.is_some() {
        result.capabilities = result.capabilities.with(Capability::Repository);
        result.secondary_actions.push(REVEAL_REPOSITORY);
    }
    result.kind = ResultKind::Code;
    result.payload = Payload::Code(Box::new(CodeTarget {
        path,
        symbol,
        language: reference
            .language
            .clone()
            .unwrap_or_else(|| language.as_str().into()),
        repository,
        start_offset: reference.start_offset.and_then(|v| u64::try_from(v).ok()),
        end_offset: reference.end_offset.and_then(|v| u64::try_from(v).ok()),
        passage: reference.excerpt.clone(),
    }));
}
