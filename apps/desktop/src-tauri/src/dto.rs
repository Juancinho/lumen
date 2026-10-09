//! Wire DTOs for the UI boundary.
//!
//! Core types stay free of serialization concerns for now; the shell maps them
//! into explicit, camelCase DTOs mirrored by `apps/desktop/src/ipc/types.ts`.
//! Whether domain contracts gain serde derives is decided in T011.

use serde::Serialize;

/// Mirrors `CoreInfo` in `src/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoreInfoDto {
    pub(crate) product_name: String,
    pub(crate) version: String,
}

impl From<lumen_core::CoreInfo> for CoreInfoDto {
    fn from(info: lumen_core::CoreInfo) -> Self {
        Self {
            product_name: info.product_name.to_owned(),
            version: info.version.to_owned(),
        }
    }
}

/// Mirrors `Preview` in `src/ipc/types.ts` (Quick Look, T105).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewDto {
    pub(crate) title: String,
    /// `application` | `file` | `folder` | `command`
    pub(crate) kind: &'static str,
    pub(crate) location: Option<String>,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_ms: Option<u64>,
    /// Start of a text file, if it is one.
    pub(crate) text: Option<String>,
    /// `text` is only the beginning of the file.
    pub(crate) truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) page_number: Option<u32>,
}

/// Mirrors `Size` in `src/ipc/types.ts`: logical px.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPreviewDto {
    pub(crate) page_number: u32,
    pub(crate) page_count: Option<u32>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    /// OS-produced bounded PNG only; no file URL, raw PDF or executable target.
    pub(crate) image: Option<String>,
    pub(crate) unavailable: Option<&'static str>,
}

/// Mirrors `Size` in `src/ipc/types.ts`: logical px.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct SizeDto {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// Mirrors `Appearance` in `src/ipc/types.ts`: the surface the UI paints (T004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppearanceDto {
    /// `acrylic` | `mica` | `solid`
    pub(crate) material: &'static str,
    /// `round` | `square`
    pub(crate) corners: &'static str,
}

impl From<lumen_windows::material::Plan> for AppearanceDto {
    fn from(plan: lumen_windows::material::Plan) -> Self {
        Self {
            material: plan.material.as_str(),
            corners: plan.corners.as_str(),
        }
    }
}

/// Mirrors `ResultView` in `src/ipc/types.ts`: one result row as the UI renders it (T107).
/// Payloads (paths, launch keys) stay in Rust; actions refer to results by `id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultDto {
    pub(crate) id: String,
    /// `application` | `file` | `folder` | `command`
    pub(crate) kind: &'static str,
    pub(crate) title: String,
    pub(crate) detail: Option<String>,
    /// The passage that matched, when the result was found by its contents or meaning
    /// rather than its name (T206): shown instead of the location line.
    pub(crate) snippet: Option<String>,
    pub(crate) extension: Option<String>,
    /// Display context only; trusted action targets and passage offsets stay in Rust.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<CodeContextDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pdf: Option<PdfContextDto>,
    /// Action that Enter runs (`lumen.open`, `lumen.launch`).
    pub(crate) primary_action: String,
    /// Development diagnostics (T110, `LUMEN_DIAGNOSTICS=1` only): never in normal UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostics: Option<ResultDiagnosticsDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodeContextDto {
    pub(crate) symbol: Option<String>,
    pub(crate) language: String,
    pub(crate) repository: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfContextDto {
    pub(crate) page_number: u32,
}

/// Mirrors `ResultDiagnostics` in `src/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultDiagnosticsDto {
    pub(crate) provider: String,
    pub(crate) match_kind: String,
    pub(crate) confidence: f32,
}

/// Mirrors `ActionView` in `src/ipc/types.ts`: one Action Panel entry (T108).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionDto {
    pub(crate) id: String,
    pub(crate) title: String,
    /// `primary` | `common` | `navigation` | `advanced` | `destructive`
    pub(crate) group: &'static str,
    /// Keyboard hint, e.g. `Enter`, `Ctrl+Enter`.
    pub(crate) shortcut: Option<&'static str>,
}

impl From<&lumen_core::ResultItem> for ResultDto {
    fn from(item: &lumen_core::ResultItem) -> Self {
        use lumen_core::{IconRef, MatchKind, Payload, ResultKind};
        Self {
            id: item.id.as_str().to_owned(),
            kind: match item.kind {
                ResultKind::Application => "application",
                ResultKind::Folder => "folder",
                ResultKind::Command => "command",
                ResultKind::Code => "code",
                ResultKind::PdfPage => "pdf-page",
                _ => "file",
            },
            title: item.title.clone(),
            detail: item.detail.clone().or_else(|| item.subtitle.clone()),
            snippet: match (item.kind, item.score.match_kind) {
                (ResultKind::Code | ResultKind::PdfPage, _) => item.subtitle.clone(),
                (_, MatchKind::FullText | MatchKind::Semantic) => item.subtitle.clone(),
                _ => None,
            },
            extension: match &item.icon {
                IconRef::FileExtension(ext) => Some(ext.to_string()),
                _ => None,
            },
            code: match &item.payload {
                Payload::Code(code) => Some(CodeContextDto {
                    symbol: code.symbol.clone(),
                    language: code.language.clone(),
                    repository: code
                        .repository
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|p| p.to_string_lossy().into_owned()),
                }),
                _ => None,
            },
            primary_action: item.primary_action.as_str().to_owned(),
            pdf: match &item.payload {
                Payload::Pdf(pdf) => Some(PdfContextDto {
                    page_number: pdf.page_number.get(),
                }),
                _ => None,
            },
            diagnostics: None,
        }
    }
}

impl ResultDto {
    /// The row plus its provider/score evidence (diagnostics mode).
    pub(crate) fn with_diagnostics(item: &lumen_core::ResultItem) -> Self {
        Self {
            diagnostics: Some(ResultDiagnosticsDto {
                provider: item.provider.as_str().to_owned(),
                match_kind: format!("{:?}", item.score.match_kind).to_lowercase(),
                confidence: item.score.confidence.get(),
            }),
            ..Self::from(item)
        }
    }
}

/// Mirrors `ResultsUpdate` in `src/ipc/types.ts` (event `lumen:results`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultsDto {
    pub(crate) query_id: u64,
    pub(crate) done: bool,
    pub(crate) results: Vec<ResultDto>,
    /// Diagnostics mode only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostics: Option<QueryDiagnosticsDto>,
}

/// Mirrors `QueryDiagnostics` in `src/ipc/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryDiagnosticsDto {
    pub(crate) elapsed_ms: f64,
    pub(crate) failed: Vec<String>,
}

impl ResultsDto {
    /// `diagnostics`: include provider/score evidence and timings (T110).
    pub(crate) fn new(update: &lumen_search::Update, diagnostics: bool) -> Self {
        let row = |item: &lumen_core::ResultItem| {
            if diagnostics {
                ResultDto::with_diagnostics(item)
            } else {
                ResultDto::from(item)
            }
        };
        Self {
            query_id: update.query.get(),
            done: update.done,
            results: update.results.iter().map(row).collect(),
            diagnostics: diagnostics.then(|| QueryDiagnosticsDto {
                elapsed_ms: update.elapsed.as_secs_f64() * 1000.0,
                failed: update
                    .failed
                    .iter()
                    .map(|p| p.as_str().to_owned())
                    .collect(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_info() {
        let dto = CoreInfoDto::from(lumen_core::core_info());
        assert_eq!(dto.product_name, "Lumen");
        assert_eq!(dto.version, lumen_core::core_info().version);
    }

    /// Guards the wire contract consumed by the TypeScript `CoreInfo` type.
    #[test]
    fn serializes_as_camel_case() {
        let dto = CoreInfoDto {
            product_name: "Lumen".into(),
            version: "1.2.3".into(),
        };
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "productName": "Lumen", "version": "1.2.3" })
        );
    }

    #[test]
    fn appearance_wire_shape() {
        use lumen_windows::material::{Corners, Material, Plan, Reason};
        let dto = AppearanceDto::from(Plan {
            material: Material::Acrylic,
            corners: Corners::Round,
            reason: Reason::AsRequested,
        });
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::json!({ "material": "acrylic", "corners": "round" })
        );
    }

    #[test]
    fn results_wire_shape() {
        use lumen_core::builtin::OPEN;
        use lumen_core::{
            CapabilitySet, Confidence, IconRef, MatchKind, Payload, ProviderId, QueryId, ResultId,
            ResultItem, ResultKind, Score,
        };
        let item = ResultItem {
            id: ResultId::new("item:7").unwrap(),
            provider: ProviderId::new("lumen.catalog").unwrap(),
            kind: ResultKind::File,
            title: "notas.md".into(),
            subtitle: None,
            detail: Some("C:\\Users\\Joao".into()),
            icon: IconRef::FileExtension("md".into()),
            score: Score::new(Confidence::CERTAIN, MatchKind::Exact),
            capabilities: CapabilitySet::default(),
            primary_action: OPEN,
            secondary_actions: Vec::new(),
            payload: Payload::Path("C:\\Users\\Joao\\notas.md".into()),
        };
        let update = lumen_search::Update {
            query: QueryId::new(3).unwrap(),
            results: vec![item],
            done: true,
            elapsed: std::time::Duration::ZERO,
            failed: Vec::new(),
        };
        let diag = serde_json::to_value(ResultsDto::new(&update, true)).unwrap();
        assert_eq!(
            diag["results"][0]["diagnostics"]["provider"],
            "lumen.catalog"
        );
        assert_eq!(diag["results"][0]["diagnostics"]["matchKind"], "exact");
        assert_eq!(diag["diagnostics"]["failed"], serde_json::json!([]));
        assert_eq!(
            serde_json::to_value(ResultsDto::new(&update, false)).unwrap(),
            serde_json::json!({
                "queryId": 3,
                "done": true,
                "results": [{
                    "id": "item:7",
                    "kind": "file",
                    "title": "notas.md",
                    "detail": "C:\\Users\\Joao",
                    "snippet": null,
                    "extension": "md",
                    "primaryAction": "lumen.open"
                }]
            })
        );
        // Found by its contents: the passage travels as `snippet`, the folder stays.
        let mut by_content = update.results[0].clone();
        by_content.subtitle = Some("…the matching passage…".into());
        by_content.score = Score::new(Confidence::CERTAIN, MatchKind::FullText);
        let dto = ResultDto::from(&by_content);
        assert_eq!(dto.snippet.as_deref(), Some("…the matching passage…"));
        assert_eq!(dto.detail.as_deref(), Some("C:\\Users\\Joao"));
        by_content.score = Score::new(Confidence::CERTAIN, MatchKind::Prefix);
        assert!(
            ResultDto::from(&by_content).snippet.is_none(),
            "name matches show no snippet"
        );
        by_content.kind = ResultKind::Code;
        by_content.payload = Payload::Code(Box::new(lumen_core::CodeTarget {
            path: "C:\\private\\repo\\client.py".into(),
            symbol: Some("retry".into()),
            language: "python".into(),
            repository: Some("C:\\private\\repo".into()),
            start_offset: Some(50000),
            end_offset: Some(50100),
            passage: "def retry(): pass".into(),
        }));
        let code = serde_json::to_value(ResultDto::from(&by_content)).unwrap();
        assert_eq!(code["kind"], "code");
        assert_eq!(
            code["code"],
            serde_json::json!({"symbol": "retry", "language": "python", "repository": "repo"})
        );
        assert!(code.get("payload").is_none() && code.get("startOffset").is_none());
        by_content.kind = ResultKind::PdfPage;
        by_content.payload = Payload::Pdf(Box::new(lumen_core::PdfTarget {
            path: "C:\\private\\guide.pdf".into(),
            page_number: std::num::NonZeroU32::new(7).unwrap(),
            passage: "private indexed passage".into(),
        }));
        let pdf = serde_json::to_value(ResultDto::from(&by_content)).unwrap();
        assert_eq!(pdf["kind"], "pdf-page");
        assert_eq!(pdf["pdf"], serde_json::json!({ "pageNumber": 7 }));
        assert!(
            pdf.get("payload").is_none()
                && pdf.get("path").is_none()
                && pdf.get("passage").is_none()
        );
    }

    #[test]
    fn action_wire_shape() {
        let dto = ActionDto {
            id: "lumen.reveal".into(),
            title: "Reveal in Explorer".into(),
            group: "navigation",
            shortcut: Some("Ctrl+Enter"),
        };
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::json!({
                "id": "lumen.reveal",
                "title": "Reveal in Explorer",
                "group": "navigation",
                "shortcut": "Ctrl+Enter"
            })
        );
    }

    #[test]
    fn pdf_raster_wire_has_only_image_and_display_metadata() {
        let dto = PdfPreviewDto {
            page_number: 7,
            page_count: Some(128),
            width: Some(678),
            height: Some(960),
            image: Some("data:image/png;base64,cG5n".into()),
            unavailable: None,
        };
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::json!({"pageNumber":7,"pageCount":128,"width":678,"height":960,"image":"data:image/png;base64,cG5n","unavailable":null})
        );
    }

    #[test]
    fn code_display_projection_keeps_executor_targets_and_offsets_private() {
        let dto = CodeContextDto {
            symbol: Some("retry_request".into()),
            language: "python".into(),
            repository: Some("lumen".into()),
        };
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::json!({
                "symbol": "retry_request", "language": "python", "repository": "lumen"
            })
        );
    }
}
