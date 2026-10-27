use engine::app_state::AppState;

pub use engine::cache_utils::*;

#[tauri::command]
pub fn clear_image_caches(state: tauri::State<AppState>) {
    engine::cache_utils::clear_image_caches(&state)
}

#[tauri::command]
pub fn clear_session_caches(state: tauri::State<AppState>) {
    engine::cache_utils::clear_session_caches(&state)
}
