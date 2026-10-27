use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value as Json;

use std::sync::Arc;

use crate::app_state::AppState;
use crate::settings::AppSettings;

static EXIT_REGISTERED: AtomicBool = AtomicBool::new(false);

pub trait Host: Send + Sync {
    fn emit_json(&self, event: &str, payload: Json);
    fn emit_str(&self, event: &str, payload: &str);
    fn load_settings(&self) -> AppSettings;
    fn app_data_dir(&self) -> PathBuf;
    fn display_setup(&self) -> Option<crate::gpu_processing::DisplaySetup>;
    fn resource_dir(&self) -> PathBuf;
    fn models_dir(&self) -> PathBuf;
    fn app_state(&self) -> Arc<AppState>;
    fn add_to_thumbnail_queue(&self, state: &AppState, count: usize);
    fn regenerate_thumbnails(&self, paths: Vec<String>, force: bool);
    fn release_deferred(&self, path: &str);
    fn on_exit(&self);
}

pub fn register_exit_once(host: &dyn Host) {
    if !EXIT_REGISTERED.swap(true, Ordering::SeqCst) {
        host.on_exit();
    }
}

pub struct SilentHost;

impl Host for SilentHost {
    fn emit_json(&self, _: &str, _: Json) {}
    fn emit_str(&self, _: &str, _: &str) {}
    fn load_settings(&self) -> AppSettings {
        AppSettings::default()
    }
    fn app_data_dir(&self) -> PathBuf {
        std::env::temp_dir().join("rapidraw")
    }
    fn display_setup(&self) -> Option<crate::gpu_processing::DisplaySetup> {
        None
    }
    fn resource_dir(&self) -> PathBuf {
        std::env::current_dir().unwrap_or_default()
    }
    fn models_dir(&self) -> PathBuf {
        self.app_data_dir().join("models")
    }
    fn app_state(&self) -> Arc<AppState> {
        Arc::new(AppState::new())
    }
    fn add_to_thumbnail_queue(&self, _: &AppState, _: usize) {}
    fn regenerate_thumbnails(&self, _: Vec<String>, _: bool) {}
    fn release_deferred(&self, _: &str) {}
    fn on_exit(&self) {}
}
