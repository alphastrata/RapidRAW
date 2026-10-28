use engine::app_state::AppState;
use engine::host::Host;
use engine::settings::AppSettings;
use std::path::PathBuf;
use std::sync::Arc;

pub const APP_ID: &str = "io.github.CyberTimon.RapidRAW";

pub struct RapidHost {
    pub state: Arc<AppState>,
    settings_path: PathBuf,
    data_dir: PathBuf,
    models_dir: PathBuf,
    resource_dir: PathBuf,
}

impl RapidHost {
    #[must_use]
    pub fn new() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_ID);
        std::fs::create_dir_all(&data_dir).ok();
        let models_dir = std::env::var_os("RAPIDRAW_MODELS_DIR")
            .map_or_else(|| data_dir.join("models"), PathBuf::from);
        std::fs::create_dir_all(&models_dir).ok();
        let resource_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
            .map_or_else(
                || PathBuf::from("./resources"),
                |exe_dir| exe_dir.join("resources"),
            );
        Self {
            state: Arc::new(AppState::new()),
            settings_path: data_dir.join("settings.json"),
            data_dir,
            models_dir,
            resource_dir,
        }
    }
}

impl Default for RapidHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for RapidHost {
    fn emit_json(&self, event: &str, payload: serde_json::Value) {
        log::debug!("engine event {event}: {payload}");
    }

    fn emit_str(&self, event: &str, payload: &str) {
        log::debug!("engine event {event}: {payload}");
    }

    fn load_settings(&self) -> AppSettings {
        std::fs::read(&self.settings_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn app_data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn display_setup(&self) -> Option<engine::gpu_processing::DisplaySetup> {
        None
    }

    fn resource_dir(&self) -> PathBuf {
        self.resource_dir.clone()
    }

    fn models_dir(&self) -> PathBuf {
        self.models_dir.clone()
    }

    fn app_state(&self) -> Arc<AppState> {
        Arc::clone(&self.state)
    }

    fn on_exit(&self) {
        log::info!("RapidRAW exiting");
    }
}