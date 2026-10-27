use engine::app_state::AppState;

pub use engine::mask_generation::*;

#[tauri::command(async)]
pub fn generate_mask_overlay(
    mask_def: serde_json::Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    js_adjustments: Option<serde_json::Value>,
    state: tauri::State<'_, AppState>,
) -> Result<tauri::ipc::Response, String> {
    let bytes = engine::mask_generation::generate_mask_overlay(
        mask_def,
        width,
        height,
        scale,
        crop_offset,
        js_adjustments,
        &state,
    )?;
    Ok(tauri::ipc::Response::new(bytes.unwrap_or_default()))
}