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
    pub(crate) extension: Option<String>,
    /// Action that Enter runs (`lumen.open`, `lumen.launch`).
    pub(crate) primary_action: String,
    /// Development diagnostics (T110, `LUMEN_DIAGNOSTICS=1` only): never in normal UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostics: Option<ResultDiagnosticsDto>,
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
        use lumen_core::{IconRef, ResultKind};
        Self {
            id: item.id.as_str().to_owned(),
            kind: match item.kind {
                ResultKind::Application => "application",
                ResultKind::Folder => "folder",
                ResultKind::Command => "command",
                _ => "file",
            },
            title: item.title.clone(),
            detail: item.detail.clone().or_else(|| item.subtitle.clone()),
            extension: match &item.icon {
                IconRef::FileExtension(ext) => Some(ext.to_string()),
                _ => None,
            },
            primary_action: item.primary_action.as_str().to_owned(),
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
                    "extension": "md",
                    "primaryAction": "lumen.open"
                }]
            })
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
}
