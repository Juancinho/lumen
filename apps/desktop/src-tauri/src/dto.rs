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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
        }
    }
}

/// Mirrors `ResultsUpdate` in `src/ipc/types.ts` (event `lumen:results`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultsDto {
    pub(crate) query_id: u64,
    pub(crate) done: bool,
    pub(crate) results: Vec<ResultDto>,
}

impl From<&lumen_search::Update> for ResultsDto {
    fn from(update: &lumen_search::Update) -> Self {
        Self {
            query_id: update.query.get(),
            done: update.done,
            results: update.results.iter().map(ResultDto::from).collect(),
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
        assert_eq!(
            serde_json::to_value(ResultsDto::from(&update)).unwrap(),
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
