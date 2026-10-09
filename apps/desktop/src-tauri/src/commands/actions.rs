use tauri::AppHandle;

use crate::dto::ActionDto;

/// Action Panel entries of a result the user saw (T108).
#[tauri::command]
pub(crate) async fn list_actions(
    app: AppHandle,
    query_id: u64,
    result_id: String,
) -> Result<Vec<ActionDto>, String> {
    crate::actions::list(&app, query_id, &result_id)
}

/// Runs `action_id` on a result (T109). `invocation`: `primary` | `panel` | `shortcut`.
/// False: completed and hidden. True: authorized PDF-page fallback keeps Quick Look open.
#[tauri::command]
pub(crate) async fn run_action(
    app: AppHandle,
    query_id: u64,
    result_id: String,
    action_id: String,
    invocation: String,
) -> Result<bool, String> {
    crate::actions::run(&app, query_id, &result_id, &action_id, &invocation)
}
