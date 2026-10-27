pub use engine::ai_denoise::*;

#[tauri::command]
pub fn clear_ai_denoise_cache() -> Result<(), String> {
    engine::ai_denoise::clear_cache_files()
}
