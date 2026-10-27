pub use engine::focus_stacking::*;

use engine::app_state::AppState;

#[tauri::command]
pub async fn stitch_focus_stack(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    engine::focus_stacking::stitch_focus_stack_impl(
        paths,
        &state,
        crate::host::TauriHost::host(app_handle),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_focus_stack(
    first_path_str: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    engine::focus_stacking::save_focus_stack_impl(first_path_str, &state).map_err(|e| e.to_string())
}
