use serde::Serialize;

#[derive(Serialize)]
pub struct CameraConfigChoice {
    pub name: String,
    pub current_value: String,
    pub choices: Vec<String>,
}

#[cfg(feature = "tethering")]
pub struct CameraSession {
    pub context: Option<gphoto2::Context>,
    pub camera: Option<gphoto2::Camera>,
}

#[cfg(not(feature = "tethering"))]
pub struct CameraSession {}

impl CameraSession {
    #[must_use]
    pub fn new() -> Self {
        #[cfg(feature = "tethering")]
        {
            Self {
                context: gphoto2::Context::new().ok(),
                camera: None,
            }
        }
        #[cfg(not(feature = "tethering"))]
        {
            Self {}
        }
    }
}

impl Default for CameraSession {
    fn default() -> Self {
        Self::new()
    }
}

// The session is guarded by a `Mutex` held from one blocking thread at a time.
unsafe impl Send for CameraSession {}
unsafe impl Sync for CameraSession {}

#[cfg(feature = "tethering")]
mod camera_api {
    use super::{CameraConfigChoice, CameraSession};
    use anyhow::{Context as _, Result, anyhow};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::Duration;

    const CONNECT_SETTLE_MS: u64 = 150;
    const CONNECT_RETRY_MS: u64 = 300;
    const DRAIN_PASSES: usize = 15;
    const DRAIN_STEP_MS: u64 = 30;
    const CAPTURE_DRAIN_STEP_MS: u64 = 10;
    const GLITCH_RETRIES: usize = 3;
    const GLITCH_POLL_MS: u64 = 200;
    const AF_SETTLE_MS: u64 = 1_000;

    const BATTERY_KEYS: [&str; 3] = ["batterylevel", "battery", "battery-level"];
    const SETTING_KEYS: [(&str, &[&str]); 8] = [
        ("iso", &["iso", "iso-speed", "ISO"]),
        ("shutterspeed", &["shutterspeed", "exposure-time", "exposurespeed"]),
        ("aperture", &["f-number", "aperture", "fnumber"]),
        ("whitebalance", &["whitebalance", "white-balance"]),
        (
            "colortemperature",
            &[
                "colortemperature",
                "colortemp",
                "wbcolortemperature",
                "whitebalancetemperature",
            ],
        ),
        (
            "exposurecompensation",
            &["exposurecompensation", "exposurebias", "exposure-bias"],
        ),
        (
            "exposuremode",
            &["autoexposuremode", "expprogram", "shootingmode", "exposure-program"],
        ),
        (
            "meteringmode",
            &["meteringmode", "metering-mode", "exposuremeteringmode"],
        ),
    ];

    pub fn list_cameras() -> Result<Vec<String>> {
        let context = gphoto2::Context::new()
            .context("Failed to initialize gphoto2 context")?;
        let cameras = gphoto2::Camera::autodetect(&context)?;

        Ok(cameras
            .into_iter()
            .map(|camera| format!("{} ({})", camera.model, camera.port))
            .collect())
    }

    pub fn connect(session: &Mutex<CameraSession>) -> Result<String> {
        let mut session = lock_session(session);
        session.camera = None;

        std::thread::sleep(Duration::from_millis(CONNECT_SETTLE_MS));

        let context = gphoto2::Context::new()
            .context("Failed to initialize gphoto2 context")?;
        let descriptor = gphoto2::Camera::autodetect(&context)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No camera found"))?;

        let camera = gphoto2::Camera::open(&context, &descriptor.model, &descriptor.port)
            .or_else(|_| {
                std::thread::sleep(Duration::from_millis(CONNECT_RETRY_MS));
                gphoto2::Camera::open(&context, &descriptor.model, &descriptor.port)
            })
            .context("Failed to connect to camera")?;

        force_capture_target(&camera, &context);
        force_drive_mode(&camera, &context);

        drop(camera.capture_preview(&context));

        drain_events(&context, &camera, DRAIN_PASSES, DRAIN_STEP_MS);

        session.context = Some(context);
        session.camera = Some(camera);

        Ok(format!("Connected to {}", descriptor.model))
    }

    pub fn get_settings(
        session: &Mutex<CameraSession>,
    ) -> Result<HashMap<String, CameraConfigChoice>> {
        let session = lock_session(session);
        let (camera, context) = active(&session)?;

        let map = query_settings(camera, context, GLITCH_RETRIES);
        Ok(map)
    }

    pub fn set_setting(
        session: &Mutex<CameraSession>,
        setting_name: &str,
        value: &str,
    ) -> Result<()> {
        let session = lock_session(session);
        let (camera, context) = active(&session)?;

        let mut applied = false;
        let mut last_error: Option<String> = None;

        aliases_for(setting_name)
            .iter()
            .map(|camera_key| attempt_set(camera, context, camera_key, setting_name, value))
            .take_while(|outcome| {
                let applied_outcome = matches!(outcome, SetOutcome::Applied);
                applied |= applied_outcome;
                !applied_outcome
            })
            .filter_map(|outcome| match outcome {
                SetOutcome::Locked(message) | SetOutcome::Failed(message) => Some(message),
                _ => None,
            })
            .for_each(|message| last_error = Some(message));

        if applied {
            return Ok(());
        }
        match last_error {
            Some(message) => Err(anyhow!("{message}")),
            None => Err(anyhow!("Failed to set {setting_name} to {value}")),
        }
    }

    pub fn autofocus(session: &Mutex<CameraSession>) -> Result<()> {
        let session = lock_session(session);
        let (camera, context) = active(&session)?;

        let triggered = ["autofocusdrive", "autofocus", "eosviewfinder"]
            .into_iter()
            .find(|af_key| trigger_af(camera, context, af_key));

        camera
            .wait_for_event(context, Duration::from_millis(AF_SETTLE_MS))
            .ok();

        if let Some(key) = triggered
            && let Ok(widget) = camera.get_single_config(context, key)
        {
            widget.set_choice("0").ok();
            widget.set_choice("Off").ok();
            widget
                .set_value(&gphoto2::widget::WidgetValue::Toggle(false))
                .ok();
            camera.set_single_config(context, key, &widget).ok();
        }

        Ok(())
    }

    pub fn get_preview(session: &Mutex<CameraSession>) -> Result<Vec<u8>> {
        let session = lock_session(session);
        let (camera, context) = active(&session)?;

        let file = camera
            .capture_preview(context)
            .context("Failed to capture preview")?;

        Ok(file.data()?.to_vec())
    }

    pub fn capture(
        session: &Mutex<CameraSession>,
        destination_folder: Option<&str>,
        on_progress: &(dyn Fn() + Sync),
    ) -> Result<String> {
        let session = lock_session(session);
        let (camera, context) = active(&session)?;

        std::iter::repeat_with(|| camera.wait_for_event(context, Duration::from_millis(CAPTURE_DRAIN_STEP_MS)))
            .take_while(|event| matches!(event, Ok(shot) if !matches!(shot, gphoto2::CameraEvent::Timeout)))
            .for_each(|_| on_progress());

        let shot = camera
            .capture_image(context)
            .context("Capture failed")?;
        on_progress();

        let file = camera.download(context, &shot).context("Download failed")?;
        on_progress();

        let save_dir = destination_folder.map_or_else(
            || std::env::temp_dir().to_string_lossy().into_owned(),
            str::to_owned,
        );
        let output_path = std::path::Path::new(&save_dir).join(&shot.name);

        std::fs::write(&output_path, file.data()?)
            .context("Failed to save captured file")?;
        on_progress();

        camera.delete_file(context, &shot.folder, &shot.name).ok();

        Ok(output_path.to_string_lossy().into_owned())
    }

    fn lock_session(session: &Mutex<CameraSession>) -> MutexGuard<'_, CameraSession> {
        session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn active(
        session: &CameraSession,
    ) -> Result<(&gphoto2::Camera, &gphoto2::Context)> {
        Ok((
            session
                .camera
                .as_ref()
                .ok_or_else(|| anyhow!("No camera connected"))?,
            session
                .context
                .as_ref()
                .ok_or_else(|| anyhow!("No context initialized"))?,
        ))
    }

    fn aliases_for(setting_name: &str) -> &'static [&'static str] {
        SETTING_KEYS
            .iter()
            .find(|(frontend_key, _)| *frontend_key == setting_name)
            .map_or(&[], |(_, aliases)| aliases)
    }

    fn widget_value(widget: &gphoto2::Widget) -> Option<String> {
        Some(match widget.value().ok()? {
            gphoto2::widget::WidgetValue::Choice(choice) | gphoto2::widget::WidgetValue::Text(choice) => choice,
            gphoto2::widget::WidgetValue::Range(value) => format!("{value}"),
            _ => return None,
        })
    }

    fn queried(
        camera: &gphoto2::Camera,
        context: &gphoto2::Context,
        frontend_key: &str,
        aliases: &[&str],
    ) -> Option<CameraConfigChoice> {
        aliases.iter().find_map(|camera_key| {
            let widget = camera.get_single_config(context, camera_key).ok()?;
            widget_value(&widget).map(|current_value| CameraConfigChoice {
                name: frontend_key.to_string(),
                current_value,
                choices: widget.choices().unwrap_or_default(),
            })
        })
    }

    fn single_pass(
        camera: &gphoto2::Camera,
        context: &gphoto2::Context,
    ) -> HashMap<String, CameraConfigChoice> {
        let mut map: HashMap<String, CameraConfigChoice> = HashMap::new();

        if let Some(mut battery) = queried(camera, context, "batterylevel", &BATTERY_KEYS) {
            battery.choices.clear();
            map.insert(battery.name.clone(), battery);
        }

        SETTING_KEYS
            .iter()
            .filter_map(|(frontend_key, aliases)| {
                queried(camera, context, frontend_key, aliases)
            })
            .for_each(|choice| {
                map.insert(choice.name.clone(), choice);
            });

        map
    }

    fn is_glitched(map: &HashMap<String, CameraConfigChoice>) -> bool {
        map.values().any(|config| {
            config.current_value.contains("65535")
                || config.current_value.contains("Unknown value 0000")
        })
    }

    fn query_settings(
        camera: &gphoto2::Camera,
        context: &gphoto2::Context,
        remaining: usize,
    ) -> HashMap<String, CameraConfigChoice> {
        let map = single_pass(camera, context);

        if !map.is_empty() && !is_glitched(&map) {
            return map;
        }

        drop(camera.wait_for_event(context, Duration::from_millis(GLITCH_POLL_MS)));

        match remaining.checked_sub(1) {
            Some(left) => query_settings(camera, context, left),
            None => map,
        }
    }

    fn force_capture_target(camera: &gphoto2::Camera, context: &gphoto2::Context) {
        if let Ok(widget) = camera.get_single_config(context, "capturetarget")
            && let Some(ram_choice) = widget.choices().unwrap_or_default().into_iter().find(
                |choice| {
                    let lower = choice.to_lowercase();
                    lower.contains("ram") || lower.contains("sdram")
                },
            )
        {
            widget.set_choice(&ram_choice).ok();
            camera
                .set_single_config(context, "capturetarget", &widget)
                .ok();
        }
    }

    fn force_drive_mode(camera: &gphoto2::Camera, context: &gphoto2::Context) {
        ["drivemode", "drive-mode"]
            .into_iter()
            .find_map(|drive_key| {
                let widget = camera.get_single_config(context, drive_key).ok()?;
                widget.choices().unwrap_or_default().into_iter().find(
                    |choice| {
                        let lower = choice.to_lowercase();
                        lower.contains("single") || lower == "1" || lower.contains("standard")
                    },
                )
                .map(|choice| (drive_key, choice, widget))
            })
            .into_iter()
            .for_each(|(drive_key, choice, widget)| {
                widget.set_choice(&choice).ok();
                camera.set_single_config(context, drive_key, &widget).ok();
            });
    }

    fn drain_events(
        context: &gphoto2::Context,
        camera: &gphoto2::Camera,
        passes: usize,
        step_ms: u64,
    ) {
        (0..passes)
            .map(|_| camera.wait_for_event(context, Duration::from_millis(step_ms)))
            .take_while(|event| {
                !matches!(event, Ok(shot) if matches!(shot, gphoto2::CameraEvent::Timeout))
            })
            .for_each(std::mem::drop);
    }

    enum SetOutcome {
        Applied,
        Locked(String),
        Failed(String),
        Skipped,
    }

    fn attempt_set(
        camera: &gphoto2::Camera,
        context: &gphoto2::Context,
        camera_key: &str,
        setting_name: &str,
        value: &str,
    ) -> SetOutcome {
        let Some(widget) = camera.get_single_config(context, camera_key).ok() else {
            return SetOutcome::Skipped;
        };
        let Ok(widget_type) = widget.widget_type() else {
            return SetOutcome::Skipped;
        };

        let accepted = match widget_type {
            gphoto2::widget::WidgetType::Radio | gphoto2::widget::WidgetType::Menu => {
                widget.set_choice(value).is_ok()
            }
            gphoto2::widget::WidgetType::Text => widget.set_text(value).is_ok(),
            _ => matches!(
                value.replace('K', "").trim().parse::<f32>(),
                Ok(numeric) if widget
                    .set_value(&gphoto2::widget::WidgetValue::Range(numeric))
                    .is_ok()
            ),
        };

        if !accepted {
            return SetOutcome::Skipped;
        }

        match camera.set_single_config(context, camera_key, &widget) {
            Ok(()) => SetOutcome::Applied,
            Err(error) => {
                let message = error.to_string().to_lowercase();
                if message.contains("read-only")
                    || message.contains("readonly")
                    || message.contains("locked")
                {
                    SetOutcome::Locked(format!(
                        "{setting_name} is physically locked (e.g. physical dial)."
                    ))
                } else {
                    SetOutcome::Failed(format!("Failed to set {setting_name}: {error}"))
                }
            }
        }
    }

    fn trigger_af(
        camera: &gphoto2::Camera,
        context: &gphoto2::Context,
        af_key: &str,
    ) -> bool {
        let Ok(widget) = camera.get_single_config(context, af_key) else {
            return false;
        };

        if (widget.set_choice("1").is_ok() || widget.set_choice("On").is_ok())
            && camera.set_single_config(context, af_key, &widget).is_ok()
        {
            return true;
        }

        widget
            .set_value(&gphoto2::widget::WidgetValue::Toggle(true))
            .is_ok()
            && camera.set_single_config(context, af_key, &widget).is_ok()
    }
}

#[cfg(feature = "tethering")]
pub use camera_api::{
    autofocus, capture, connect, get_preview, get_settings, list_cameras, set_setting,
};