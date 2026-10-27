pub use engine::tethering::CameraConfigChoice;

use std::collections::HashMap;
use tauri::ipc::Response;

#[cfg(feature = "tethering")]
use std::sync::Arc;

#[cfg(feature = "tethering")]
use crate::AppState;

#[cfg(feature = "tethering")]
use tauri::Manager;

#[cfg(feature = "tethering")]
async fn blocking<T>(
    task: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("Task panicked: {error}"))?
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tethering")]
fn camera_session(app: &tauri::AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

#[tauri::command]
pub async fn tether_list_cameras() -> Result<Vec<String>, String> {
    #[cfg(feature = "tethering")]
    {
        blocking(engine::tethering::list_cameras).await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_connect(_app_handle: tauri::AppHandle) -> Result<String, String> {
    #[cfg(feature = "tethering")]
    {
        let session = camera_session(&_app_handle);
        blocking(move || engine::tethering::connect(&session.camera_session)).await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_get_settings(
    _app_handle: tauri::AppHandle,
) -> Result<HashMap<String, CameraConfigChoice>, String> {
    #[cfg(feature = "tethering")]
    {
        let session = camera_session(&_app_handle);
        blocking(move || engine::tethering::get_settings(&session.camera_session)).await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_set_setting(
    _app_handle: tauri::AppHandle,
    _setting_name: String,
    _value: String,
) -> Result<(), String> {
    #[cfg(feature = "tethering")]
    {
        let session = camera_session(&_app_handle);
        blocking(move || engine::tethering::set_setting(&session.camera_session, &_setting_name, &_value)).await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_autofocus(_app_handle: tauri::AppHandle) -> Result<(), String> {
    #[cfg(feature = "tethering")]
    {
        let session = camera_session(&_app_handle);
        blocking(move || engine::tethering::autofocus(&session.camera_session)).await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_get_preview(_app_handle: tauri::AppHandle) -> Result<Response, String> {
    #[cfg(feature = "tethering")]
    {
        let session = camera_session(&_app_handle);
        blocking(move || engine::tethering::get_preview(&session.camera_session).map(Response::new)).await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_capture(
    _app_handle: tauri::AppHandle,
    _destination_folder: Option<String>,
) -> Result<String, String> {
    #[cfg(feature = "tethering")]
    {
        let session = camera_session(&_app_handle);
        blocking(move || {
            engine::tethering::capture(&session.camera_session, _destination_folder.as_deref(), &|| {})
        })
        .await
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}