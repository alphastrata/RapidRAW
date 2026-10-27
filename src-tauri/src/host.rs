use std::path::PathBuf;
use std::sync::Arc;

use engine::app_state::AppState;
use engine::host::Host;
use engine::settings::AppSettings;
use serde_json::Value as Json;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone)]
pub struct TauriHost {
    pub app_handle: AppHandle,
}

impl TauriHost {
    pub fn host(app_handle: AppHandle) -> Arc<dyn Host> {
        Arc::new(TauriHost { app_handle })
    }
}

impl Host for TauriHost {
    fn emit_json(&self, event: &str, payload: Json) {
        let _ = self.app_handle.emit(event, payload);
    }

    fn emit_str(&self, event: &str, payload: &str) {
        let _ = self.app_handle.emit(event, payload);
    }

    fn load_settings(&self) -> AppSettings {
        crate::app_settings::load_settings(self.app_handle.clone()).unwrap_or_default()
    }

    fn app_data_dir(&self) -> PathBuf {
        self.app_handle
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("rapidraw"))
    }

    fn resource_dir(&self) -> PathBuf {
        self.app_handle
            .path()
            .resource_dir()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default())
    }

    #[cfg(target_os = "linux")]
    fn display_setup(&self) -> Option<engine::gpu_processing::DisplaySetup> {
        None
    }

    #[cfg(not(target_os = "linux"))]
    fn display_setup(&self) -> Option<engine::gpu_processing::DisplaySetup> {
        if !self.load_settings().use_wgpu_renderer.unwrap_or(true) {
            return None;
        }
        let window = self.app_handle.get_webview_window("main")?;
        let inner_size = window
            .inner_size()
            .map_or((1280, 720), |s| (s.width, s.height));
        Some(engine::gpu_processing::DisplaySetup {
            inner_size,
            create_surface: Box::new(move |instance| {
                instance.create_surface(window).map_err(|e| e.to_string())
            }),
        })
    }

    fn models_dir(&self) -> PathBuf {
        let fallback = || {
            std::env::var("HOME")
                .map(|home| {
                    PathBuf::from(home).join(".local/share/io.github.CyberTimon.RapidRAW/models")
                })
                .unwrap_or_else(|_| std::env::temp_dir().join("rapidraw-models"))
        };
        self.app_handle
            .path()
            .app_data_dir()
            .map(|dir| dir.join("models"))
            .unwrap_or_else(|_| fallback())
    }

    fn app_state(&self) -> Arc<AppState> {
        self.app_handle.state::<Arc<AppState>>().inner().clone()
    }

    fn add_to_thumbnail_queue(&self, state: &AppState, count: usize) {
        crate::file_management::add_to_thumbnail_queue(state, count, &self.app_handle);
    }

    fn regenerate_thumbnails(&self, paths: Vec<String>, force: bool) {
        let settings = self.load_settings();
        crate::file_management::regenerate_thumbnails(
            paths.into_iter(),
            &self.app_handle,
            &settings,
            force,
        );
    }

    fn release_deferred(&self, path: &str) {
        crate::ai_jobs::release_deferred(path);
    }

    fn on_exit(&self) {
        crate::register_exit_handler();
    }
}
