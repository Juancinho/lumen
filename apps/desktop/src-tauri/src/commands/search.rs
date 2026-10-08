use tauri::AppHandle;

/// Starts root search `query_id` for `text` (T107). Results arrive as `lumen:results`
/// events; returns whether the query was accepted (older ids are dropped).
#[tauri::command]
pub(crate) async fn search(app: AppHandle, query_id: u64, text: String) -> bool {
    crate::search::submit(&app, query_id, text)
}
