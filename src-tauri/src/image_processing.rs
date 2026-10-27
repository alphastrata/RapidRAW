pub use engine::image_processing::*;

use engine::app_state::AppState;

#[tauri::command]
pub fn calculate_auto_adjustments(
    js_adjustments: serde_json::Value,
    mode: Option<engine::image_processing::AutoAdjustMode>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    let settings = crate::app_settings::load_settings(app_handle).unwrap_or_default();
    engine::image_processing::calculate_auto_adjustments(js_adjustments, mode, &state, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sample_white_balance(
    corners: Vec<engine::image_processing::UvPoint>,
    js_adjustments: Option<serde_json::Value>,
    state: tauri::State<'_, AppState>,
) -> Result<engine::image_processing::WhiteBalanceSample, String> {
    engine::image_processing::sample_white_balance(corners, js_adjustments, &state).map_err(|e| e.to_string())
}

