use tauri::AppHandle;

use crate::dto::PreviewDto;

/// Quick Look data for a result the user saw (T105): metadata and a short text excerpt.
#[tauri::command]
pub(crate) async fn preview_result(
    app: AppHandle,
    query_id: u64,
    result_id: String,
) -> Result<PreviewDto, String> {
    let (_, _, item) = crate::actions::lookup(&app, query_id, &result_id)?;
    Ok(crate::preview::preview(&item))
}
