use crate::dto::CoreInfoDto;

/// Returns the identity of the linked core (diagnostics/About; smoke-tests the IPC path).
#[tauri::command]
pub(crate) async fn core_info() -> CoreInfoDto {
    lumen_core::core_info().into()
}
