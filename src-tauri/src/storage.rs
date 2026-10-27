pub use engine::storage::*;

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use engine::app_state::AppState;

fn storage_dirs(app_handle: &AppHandle) -> Result<StorageDirs, String> {
    let path = app_handle.path();
    Ok(StorageDirs {
        cache: path.app_cache_dir().map_err(|e| e.to_string())?,
        data: path.app_data_dir().map_err(|e| e.to_string())?,
        logs: path.app_log_dir().map_err(|e| e.to_string())?,
    })
}

fn dir_size(path: &PathBuf) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageDirs {
    cache: PathBuf,
    data: PathBuf,
    logs: PathBuf,
}

#[tauri::command]
pub async fn get_storage_usage(app_handle: AppHandle) -> Result<StorageUsage, String> {
    let dirs = storage_dirs(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || StorageUsage {
        thumbnails: dir_size(&dirs.cache.join("thumbnails")),
        ai_denoise: dir_size(&dirs.cache.join("ai_denoise")),
        ai_maps: dir_size(&dirs.cache.join("ai_maps")),
        metadata: dir_size(&dirs.cache.join("exif")),
        immich: dir_size(&dirs.cache.join("immich")),
        models: dir_size(&dirs.data.join("models")),
        logs: dir_size(&dirs.logs),
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_ai_maps_cache() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(crate::effect_maps::clear_disk_cache)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_metadata_cache() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(crate::exif_processing::clear_disk_cache)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_immich_cache(app_handle: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::immich::clear_cache(&app_handle))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_ai_models(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let models_dir = storage_dirs(&app_handle)?.data.join("models");
    let _guard = state.ai_init_lock.lock().await;
    crate::ai_processing::reset_ai_sessions(&state.ai_state);
    if models_dir.exists() {
        fs::remove_dir_all(&models_dir)
            .map_err(|e| format!("Failed to remove AI models: {}", e))?;
    }
    Ok(())
}
