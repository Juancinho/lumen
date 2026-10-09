use tauri::AppHandle;
use tauri::Manager;

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

/// A page of a remembered result, resolved in Rust. The UI supplies ids/page only.
#[tauri::command]
pub(crate) async fn preview_pdf_page(
    app: AppHandle,
    request_id: u64,
    query_id: u64,
    result_id: String,
    page_number: u32,
) -> Result<crate::dto::PdfPreviewDto, String> {
    if !crate::overlay::is_shown() {
        return Err("overlay hidden".into());
    }
    let (_, _, item) = crate::actions::lookup(&app, query_id, &result_id)?;
    let path = item
        .payload
        .local_path()
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        })
        .ok_or("result has no PDF")?;
    if !(1..=lumen_windows::pdf::MAX_PAGES).contains(&page_number) {
        return Err("invalid PDF page".into());
    }
    let state = app
        .try_state::<crate::pdf_preview::PdfPreviews>()
        .ok_or("preview unavailable")?;
    let answer = state.submit(request_id, query_id, path.to_owned(), page_number)?;
    answer.await.map_err(|_| "preview superseded")?
}

#[tauri::command]
pub(crate) async fn cancel_pdf_preview(app: AppHandle, request_id: u64) {
    if let Some(state) = app.try_state::<crate::pdf_preview::PdfPreviews>() {
        state.cancel(Some(request_id));
    }
}
