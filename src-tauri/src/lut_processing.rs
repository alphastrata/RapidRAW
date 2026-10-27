pub use engine::lut_processing::*;

use engine::app_state::AppState;

#[tauri::command]
pub fn load_and_parse_lut(path: String, state: tauri::State<'_, AppState>) -> Result<LutParseResult, String> {
    engine::lut_processing::load_and_parse_lut_impl(path, &state)
}

#[tauri::command]
pub fn list_luts(app_handle: tauri::AppHandle) -> Result<Vec<LutEntry>, String> {
    engine::lut_processing::list_luts_impl(&*crate::host::TauriHost::host(app_handle))
}

#[tauri::command]
pub fn import_luts(
    app_handle: tauri::AppHandle,
    source_paths: Vec<String>,
) -> Result<Vec<LutEntry>, String> {
    engine::lut_processing::import_luts_impl(&*crate::host::TauriHost::host(app_handle), source_paths)
}

#[tauri::command]
pub fn remove_lut(app_handle: tauri::AppHandle, path: String) -> Result<Vec<LutEntry>, String> {
    engine::lut_processing::remove_lut_impl(&*crate::host::TauriHost::host(app_handle), path)
}

#[tauri::command(async)]
pub async fn generate_lut_previews(
    app_handle: tauri::AppHandle,
    luts: Vec<LutPreviewRequest>,
    size: u32,
) -> Result<Vec<LutPreview>, String> {
    engine::lut_processing::generate_lut_previews_impl(crate::host::TauriHost::host(app_handle), luts, size).await
}