use crate::AppState;
use engine::auto_straighten::rotation_for_image;
use serde_json::Value;

#[tauri::command]
pub fn calculate_auto_straighten(
    js_adjustments: Value,
    state: tauri::State<AppState>,
) -> Result<Option<f64>, String> {
    let (image, is_raw) = {
        let guard = state.original_image.lock().unwrap();
        let loaded = guard
            .as_ref()
            .ok_or("No image loaded for auto straighten")?;
        (loaded.image.clone(), loaded.is_raw)
    };
    Ok(rotation_for_image(&image, is_raw, &js_adjustments))
}