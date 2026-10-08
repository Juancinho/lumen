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
}
