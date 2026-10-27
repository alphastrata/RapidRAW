pub use engine::lens_correction::*;

use engine::app_state::AppState;

#[tauri::command]
pub fn get_lensfun_makers(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    engine::lens_correction::get_lensfun_makers_impl(&state)
}

#[tauri::command]
pub fn get_lensfun_lenses_for_maker(
    maker: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    engine::lens_correction::get_lensfun_lenses_for_maker_impl(maker, &state)
}

#[tauri::command]
pub fn autodetect_lens(
    maker: String,
    model: String,
    camera_model: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<(String, String)>, String> {
    engine::lens_correction::autodetect_lens_impl(maker, model, camera_model, &state)
}

#[tauri::command]
pub fn get_lens_distortion_params(
    maker: String,
    model: String,
    focal_length: f32,
    aperture: Option<f32>,
    distance: Option<f32>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<engine::lens_correction::LensDistortionParams>, String> {
    engine::lens_correction::get_lens_distortion_params_impl(maker, model, focal_length, aperture, distance, &state)
}
