use crate::host::RapidHost;
use engine::app_state::LoadedImage;
use engine::as_shot::{AsShot, as_shot};
use engine::host::Host;
use image::DynamicImage;
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct Editor {
    pub loaded: Option<LoadedFile>,
    finished: Arc<Mutex<Option<anyhow::Result<DynamicImage>>>>,
    in_flight: Arc<AtomicBool>,
    pub pending_render: bool,
}

pub struct LoadedFile {
    pub path: String,
    pub file_name: String,
    pub image: LoadedImage,
    pub preview: DynamicImage,
    pub adjustments: Value,
    pub texture: Option<egui::TextureHandle>,
}

const PREVIEW_DIM: u32 = 2560;
const RAW_EXTENSIONS: [&str; 20] = [
    "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "dcr", "kdc", "mrw", "pef", "srf",
    "sr2", "x3f", "3fr", "erf", "mef", "mos", "iiq",
];

#[must_use]
pub fn is_raw_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| RAW_EXTENSIONS.iter().any(|k| k.eq_ignore_ascii_case(ext)))
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            loaded: None,
            finished: Arc::new(Mutex::new(None)),
            in_flight: Arc::new(AtomicBool::new(false)),
            pending_render: false,
        }
    }

    /// Loads an image from disk, seeds the engine preview state and schedules a render.
    pub fn open(&mut self, host: &RapidHost, path: &Path) -> anyhow::Result<()> {
        let path_str = path.to_string_lossy().to_string();
        let bytes = std::fs::read(path)?;
        let settings = host.load_settings();
        let is_raw = is_raw_file(path);
        let mut image = engine::image_loader::load_base_image_from_bytes(
            &bytes, &path_str, false, &settings, None,
        )?;
        if is_raw {
            engine::image_processing::apply_cpu_default_raw_processing(&mut image);
        }
        let preview = downscale_for_preview(&image, PREVIEW_DIM);
        let loaded = LoadedImage {
            path: path_str.clone(),
            image: Arc::new(image),
            is_raw,
            as_shot: if is_raw {
                as_shot(&path_str)
            } else {
                AsShot::reference()
            },
        };
        *host.state.original_image
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(loaded.clone());
        self.loaded = Some(LoadedFile {
            file_name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
            path: path_str,
            image: loaded,
            preview,
            adjustments: json!({}),
            texture: None,
        });
        self.pending_render = true;
        Ok(())
    }

    /// Sets one flat adjustment key and schedules a re-render.
    pub fn adjust(&mut self, key: &str, value: impl Into<Value>) {
        if let Some(file) = &mut self.loaded {
            file.adjustments[key] = value.into();
            file.adjustments["activeTools"][tool_of(key)] = json!(true);
            self.pending_render = true;
        }
    }

    /// Drives rendering: uploads finished results and starts new requests. Call every frame.
    pub fn poll(&mut self, host: &Arc<RapidHost>, ctx: &egui::Context) {
        let rendered = self.finished.lock().ok().and_then(|mut slot| slot.take());
        if let Some(rendered) = rendered {
            match rendered {
                Ok(image) => self.upload(&image, ctx),
                Err(error) => log::error!("Render failed: {error:#}"),
            }
        }
        if self.pending_render && !self.in_flight.swap(true, Ordering::Acquire) {
            self.pending_render = false;
            match &self.loaded {
                Some(file) => self.request(host, file),
                None => self.in_flight.store(false, Ordering::Release),
            }
        }
    }

    /// True while a render request is executing off-frame.
    #[must_use]
    pub fn rendering(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }

    fn request(&self, host: &Arc<RapidHost>, file: &LoadedFile) {
        let host_thread = Arc::clone(host);
        let is_raw = file.image.is_raw;
        let as_shot = file.image.as_shot;
        let image = file.preview.clone();
        let adjustments_json = file.adjustments.clone();
        let mut hasher = DefaultHasher::new();
        file.path.hash(&mut hasher);
        adjustments_json.to_string().hash(&mut hasher);
        let cache_key = hasher.finish();
        let (finished, in_flight) = (Arc::clone(&self.finished), Arc::clone(&self.in_flight));
        std::thread::spawn(move || {
            let settings = host_thread.load_settings();
            let render_options =
                engine::image_processing::resolve_render_options_from_settings(&settings, is_raw);
            let adjustments = engine::image_processing::get_all_adjustments_from_json(
                &adjustments_json,
                is_raw,
                as_shot,
                render_options,
            );
            let state = host_thread.app_state();
            let result = render(
                &state,
                &host_thread,
                &image,
                cache_key,
                engine::image_processing::RenderRequest {
                    adjustments,
                    mask_bitmaps: &[],
                    mask_cache_key: None,
                    lut: None,
                    roi: None,
                },
            );
            *finished.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
            in_flight.store(false, Ordering::Release);
        });
    }

    fn upload(&mut self, image: &DynamicImage, ctx: &egui::Context) {
        let rgba = image.to_rgba8();
        let colour_image = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        let options = egui::TextureOptions::LINEAR;
        if let Some(file) = &mut self.loaded {
            if let Some(texture) = file.texture.as_mut() {
                texture.set(colour_image, options);
            } else {
                file.texture = Some(ctx.load_texture("darkroom", colour_image, options));
            }
        }
    }
}

fn render(
    state: &engine::app_state::AppState,
    host: &RapidHost,
    image: &DynamicImage,
    cache_key: u64,
    request: engine::image_processing::RenderRequest<'_>,
) -> anyhow::Result<DynamicImage> {
    let context = engine::image_processing::get_or_init_gpu_context(state, host)?;
    engine::image_processing::process_and_get_dynamic_image(
        &context, state, image, cache_key, request, "rapidraw.darkroom",
    )
}

fn tool_of(key: &str) -> &'static str {
    match key {
        "vibrance" | "saturation" => "colorPresence",
        "clarity" | "dehaze" | "structure" => "presence",
        "sharpness" => "sharpening",
        _ => "basic",
    }
}

#[must_use]
pub(crate) fn downscale_for_preview(image: &DynamicImage, max_dim: u32) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    let long_edge = width.max(height);
    if long_edge <= max_dim {
        return image.clone();
    }
    let scale = f64::from(max_dim) / f64::from(long_edge);
    let nw = (f64::from(width) * scale).round() as u32;
    let nh = (f64::from(height) * scale).round() as u32;
    engine::image_processing::downscale_f32_image(image, nw, nh)
}
#[cfg(test)]
mod tests {
    #[test]
    fn tool_keys_map_to_their_tools() {
        assert_eq!(super::tool_of("vibrance"), "colorPresence");
        assert_eq!(super::tool_of("clarity"), "presence");
        assert_eq!(super::tool_of("sharpness"), "sharpening");
        assert_eq!(super::tool_of("exposure"), "basic");
    }

    #[test]
    fn downscale_keeps_small_images() {
        let image = image::DynamicImage::new_rgb8(320, 160);
        let preview = super::downscale_for_preview(&image, super::PREVIEW_DIM);
        assert_eq!((preview.width(), preview.height()), (320, 160));
    }

    #[test]
    fn downscale_fits_within_the_bound() {
        let image = image::DynamicImage::new_rgb8(3200, 1600);
        let preview = super::downscale_for_preview(&image, 2560);
        assert_eq!(preview.width(), 2560);
        assert_eq!(preview.height(), 1280);
    }
}
