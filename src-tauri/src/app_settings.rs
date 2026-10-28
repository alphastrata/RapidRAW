pub use engine::settings::*;

use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::app_state::AppState;

pub fn get_settings_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let settings_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    if !settings_dir.exists() {
        fs::create_dir_all(&settings_dir).map_err(|e| e.to_string())?;
    }

    Ok(settings_dir.join("settings.json"))
}


static SETTINGS_CACHE: LazyLock<Mutex<Option<AppSettings>>> = LazyLock::new(|| Mutex::new(None));

fn settings_cache() -> std::sync::MutexGuard<'static, Option<AppSettings>> {
    SETTINGS_CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
pub fn load_settings(app_handle: AppHandle) -> Result<AppSettings, String> {
    if let Some(settings) = settings_cache().as_ref() {
        return Ok(settings.clone());
    }

    let settings = read_settings_from_disk(&app_handle)?;
    *settings_cache() = Some(settings.clone());
    Ok(settings)
}

fn read_settings_from_disk(app_handle: &AppHandle) -> Result<AppSettings, String> {
    let path = get_settings_path(app_handle)?;

    let mut settings: AppSettings = if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppSettings::default()
    };

    let all_current_keys = all_available_adjustments();
    let default_included = default_included_adjustments();
    let mut settings_modified = false;

    #[cfg(target_os = "linux")]
    if !settings.linux_gpu_optimization_migrated_v1.unwrap_or(false) {
        if settings.linux_gpu_optimization == Some(true) {
            settings.linux_gpu_optimization = Some(false);
        }
        settings.linux_gpu_optimization_migrated_v1 = Some(true);
        settings_modified = true;
    }

    let is_first_migration = settings.copy_paste_settings.known_adjustments.is_empty();

    if is_first_migration {
        settings.copy_paste_settings.included_adjustments = default_included;
        settings.copy_paste_settings.known_adjustments = all_current_keys.clone();
        settings_modified = true;
    } else {
        let new_features: Vec<String> = all_current_keys
            .difference(&settings.copy_paste_settings.known_adjustments)
            .cloned()
            .collect();

        if !new_features.is_empty() {
            for feature in new_features {
                if default_included.contains(&feature) {
                    settings
                        .copy_paste_settings
                        .included_adjustments
                        .insert(feature.clone());
                }
                settings
                    .copy_paste_settings
                    .known_adjustments
                    .insert(feature);
            }
            settings_modified = true;
        }
    }

    if settings_modified && let Ok(json_string) = serde_json::to_string_pretty(&settings) {
        let _ = fs::write(&path, json_string);
    }

    Ok(settings)
}

#[tauri::command]
pub fn save_settings(settings: AppSettings, app_handle: AppHandle) -> Result<(), String> {
    let previous_gpu_acceleration = load_settings(app_handle.clone())
        .map(|previous| previous.ai_gpu_acceleration)
        .ok();
    let path = get_settings_path(&app_handle)?;
    let json_string = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(path, json_string).map_err(|e| e.to_string())?;
    *settings_cache() = None;

    let state = app_handle.state::<AppState>();
    let cache_size = settings.image_cache_size.unwrap_or(5) as usize;
    state
        .decoded_image_cache
        .lock()
        .unwrap()
        .set_capacity(cache_size);
    crate::storage::apply_cache_limits(&settings);

    if previous_gpu_acceleration != Some(settings.ai_gpu_acceleration) {
        crate::ai_processing::reset_ai_sessions(&state.ai_state);
    }
    Ok(())
}
