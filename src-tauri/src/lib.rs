#![recursion_limit = "256"]

#[cfg(not(all(target_os = "windows", target_arch = "aarch64")))]
use crate::host::TauriHost;
use mimalloc::MiMalloc;

#[cfg(not(all(target_os = "windows", target_arch = "aarch64")))]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

macro_rules! cli_println {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

mod ai_commands;
pub mod cache_utils;
pub mod focus_stacking;
pub mod host;
pub mod image_processing;
pub mod lens_correction;
pub mod lut_processing;
pub mod mask_generation;
pub use engine::ai_connector;
mod app_settings;
pub use engine::apple_raw;
pub mod auto_straighten;
mod file_management;
pub use engine::formats;
mod ai_denoise;
pub mod guided_perspective;
mod inpainting;
pub use engine::bm3d;
pub mod export_processing;
pub use engine::highlight_reconstruction;
mod window_customizer;

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::panic;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::{DynamicImage, GenericImageView, ImageBuffer, Luma};
use imgref::ImgRef;
use mozjpeg_rs::{Encoder, Preset};
use rgb::{FromSlice, RGBA8};

use serde_json::Value;
use tauri::{
    Emitter, Manager,
    ipc::{Channel, InvokeResponseBody, Response},
};
use tempfile::NamedTempFile;

use crate::cache_utils::{
    calculate_full_job_hash, calculate_geometry_hash, calculate_transform_hash,
};
use crate::formats::is_raw_file;
use crate::gpu_processing::DisplayLayer;
use crate::image_loader::prepare_base_image_cow;
use crate::image_processing::{
    Crop, RenderRequest, apply_coarse_rotation, apply_flip, apply_geometry_warp,
    downscale_f32_image, get_all_adjustments_from_json, get_geometry_params_from_json,
    get_or_init_gpu_context, is_geometry_identity, process_and_get_dynamic_image,
    resolve_render_options, warp_image_geometry,
};
use crate::mask_generation::{
    MaskDefinition, build_warped_image_for_masks, generate_mask_bitmap,
    get_cached_or_generate_masks, resolve_warped_image_for_masks,
};
use crate::window_customizer::PinchZoomDisablePlugin;
pub use adjustment_utils::*;
pub use app_settings::*;
pub use app_state::*;
use engine::paths::parse_virtual_path;
pub use launch_request::*;

#[cfg(target_os = "macos")]
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
extern "C" fn mark_shutting_down() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
extern "C" fn force_exit(_signal: libc::c_int) {
    if SHUTTING_DOWN.load(Ordering::SeqCst) {
        unsafe {
            libc::_exit(process_exit_code());
        }
    }
}

#[cfg(target_os = "macos")]
pub fn register_exit_handler() {
    static ATEXIT: std::sync::Once = std::sync::Once::new();
    unsafe {
        libc::signal(libc::SIGABRT, force_exit as *const () as libc::sighandler_t);
    }
    ATEXIT.call_once(|| unsafe {
        libc::atexit(mark_shutting_down);
    });
}

#[cfg(not(target_os = "macos"))]
pub fn register_exit_handler() {}

const GEOMETRY_PREVIEW_SUPERSAMPLE: f64 = 2.0;

#[derive(serde::Serialize)]
struct ImageDimensions {
    width: u32,
    height: u32,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WgpuTransformPayload {
    pub window_width: f32,
    pub window_height: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub clip_x: f32,
    pub clip_y: f32,
    pub clip_width: f32,
    pub clip_height: f32,
    pub bg_primary: [f32; 4],
    pub bg_secondary: [f32; 4],
    pub pixelated: bool,
    pub split_origin: [f32; 2],
    pub split_normal: [f32; 2],
    pub split_offset: [f32; 2],
}

pub fn generate_transformed_preview(
    state: &tauri::State<AppState>,
    loaded_image: &LoadedImage,
    adjustments: &serde_json::Value,
    preview_dim: u32,
) -> Result<TransformedPreview, String> {
    let mut hasher = DefaultHasher::new();
    loaded_image.path.hash(&mut hasher);
    calculate_transform_hash(adjustments).hash(&mut hasher);
    preview_dim.hash(&mut hasher);
    let cache_key = hasher.finish();

    let mut cache_lock = state
        .full_transformed_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    if let Some((hash, img, scale, offset)) = cache_lock.as_ref()
        && *hash == cache_key
    {
        return Ok((Arc::clone(img), *scale, *offset));
    }

    *cache_lock = None;
    let (preview, scale_for_gpu, unscaled_crop_offset) =
        compute_transformed_preview(state, loaded_image, adjustments, preview_dim)?;
    *cache_lock = Some((
        cache_key,
        Arc::clone(&preview),
        scale_for_gpu,
        unscaled_crop_offset,
    ));

    Ok((preview, scale_for_gpu, unscaled_crop_offset))
}

fn get_or_compute_stage(
    cache: &Mutex<Option<(u64, Arc<DynamicImage>)>>,
    key: u64,
    compute: impl FnOnce() -> Result<Arc<DynamicImage>, String>,
) -> Result<Arc<DynamicImage>, String> {
    let mut cache_lock = cache.lock().unwrap_or_else(|e| e.into_inner());

    if let Some((hash, img)) = cache_lock.as_ref()
        && *hash == key
    {
        return Ok(Arc::clone(img));
    }

    *cache_lock = None;
    let img = compute()?;
    *cache_lock = Some((key, Arc::clone(&img)));
    Ok(img)
}

fn compute_transformed_preview(
    state: &tauri::State<AppState>,
    loaded_image: &LoadedImage,
    adjustments: &serde_json::Value,
    preview_dim: u32,
) -> Result<TransformedPreview, String> {
    let patched_key = patched_cache_key(&loaded_image.path, adjustments);

    let patched = get_or_compute_stage(&state.patched_cache, patched_key, || {
        compute_patched(loaded_image, adjustments)
    })?;

    let (full_w_px, full_h_px) = patched.dimensions();
    let orientation_steps = adjustments["orientationSteps"].as_u64().unwrap_or(0);
    let (oriented_w, oriented_h) = if orientation_steps % 2 == 1 {
        (full_h_px as f64, full_w_px as f64)
    } else {
        (full_w_px as f64, full_h_px as f64)
    };

    let crop: Option<Crop> = serde_json::from_value(adjustments["crop"].clone()).ok();
    let (full_w, full_h) = crop
        .as_ref()
        .and_then(|c| {
            let (x, y) = (c.x.round().max(0.0), c.y.round().max(0.0));
            let (w, h) = (c.width.round(), c.height.round());
            (w > 0.0 && h > 0.0 && x < oriented_w && y < oriented_h)
                .then(|| ((oriented_w - x).min(w), (oriented_h - y).min(h)))
        })
        .unwrap_or((oriented_w, oriented_h));

    let target_scale = (preview_dim as f64 / full_w.max(full_h)).min(1.0);
    let target_w = ((full_w_px as f64 * target_scale).round() as u32).max(1);
    let target_h = ((full_h_px as f64 * target_scale).round() as u32).max(1);

    let has_geometry = !is_geometry_identity(&get_geometry_params_from_json(adjustments));
    let working_target_scale = if has_geometry {
        (target_scale * GEOMETRY_PREVIEW_SUPERSAMPLE).min(1.0)
    } else {
        target_scale
    };

    let working = if working_target_scale < 1.0 {
        let working_w = ((full_w_px as f64 * working_target_scale).round() as u32).max(1);
        let working_h = ((full_h_px as f64 * working_target_scale).round() as u32).max(1);

        let mut hasher = DefaultHasher::new();
        patched_key.hash(&mut hasher);
        (working_w, working_h).hash(&mut hasher);

        get_or_compute_stage(&state.working_cache, hasher.finish(), || {
            Ok(Arc::new(downscale_f32_image(
                &patched, working_w, working_h,
            )))
        })?
    } else {
        *state
            .working_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        Arc::clone(&patched)
    };
    drop(patched);

    let mut hasher = DefaultHasher::new();
    loaded_image.path.hash(&mut hasher);
    calculate_geometry_hash(adjustments).hash(&mut hasher);
    working.dimensions().hash(&mut hasher);
    (target_w, target_h).hash(&mut hasher);
    let warped_key = hasher.finish();

    let warped = get_or_compute_stage(&state.warped_cache, warped_key, || {
        Ok(
            match apply_geometry_warp(Cow::Borrowed(working.as_ref()), adjustments) {
                Cow::Borrowed(_) => Arc::clone(&working),
                Cow::Owned(img) if img.width() > target_w => {
                    Arc::new(downscale_f32_image(&img, target_w, target_h))
                }
                Cow::Owned(img) => Arc::new(img),
            },
        )
    })?;
    drop(working);

    let working_scale = warped.width() as f64 / full_w_px.max(1) as f64;

    let mut hasher = DefaultHasher::new();
    warped_key.hash(&mut hasher);
    crate::cache_utils::calculate_effects_hash(adjustments).hash(&mut hasher);

    let effected = get_or_compute_stage(&state.effects_cache, hasher.finish(), || {
        let relit = crate::relight::apply_relight(Cow::Borrowed(warped.as_ref()), adjustments);
        let fogged = crate::fog::apply_fog(relit, adjustments);
        let blurred = crate::lens_blur::apply_lens_blur(fogged, adjustments);
        Ok(match blurred {
            Cow::Borrowed(_) => Arc::clone(&warped),
            Cow::Owned(img) => Arc::new(img),
        })
    })?;
    drop(warped);

    let max_dim = preview_dim as f64;
    let spatial_adjustments = serde_json::json!({
        "orientationSteps": adjustments["orientationSteps"],
        "rotation": adjustments["rotation"],
        "flipHorizontal": adjustments["flipHorizontal"],
        "flipVertical": adjustments["flipVertical"],
        "crop": crop.as_ref().map(|c| Crop {
            x: c.x * working_scale,
            y: c.y * working_scale,
            width: (c.width * working_scale).min(max_dim),
            height: (c.height * working_scale).min(max_dim),
        }),
    });

    let (transformed, _) = crate::adjustment_utils::apply_spatial_transformations(
        Cow::Borrowed(effected.as_ref()),
        &spatial_adjustments,
    );

    let preview = match transformed {
        Cow::Borrowed(_) => Arc::clone(&effected),
        Cow::Owned(img) => Arc::new(img),
    };

    let scale_for_gpu = if full_w > 0.0 {
        (preview.width() as f64 / full_w) as f32
    } else {
        1.0
    };
    let unscaled_crop_offset = crop.map_or((0.0, 0.0), |c| (c.x as f32, c.y as f32));

    Ok((preview, scale_for_gpu, unscaled_crop_offset))
}

fn patched_cache_key(path: &str, adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    crate::cache_utils::calculate_patch_hash(adjustments).hash(&mut hasher);
    hasher.finish()
}

pub fn cached_patched_image(
    state: &AppState,
    base_image: &DynamicImage,
    adjustments: &serde_json::Value,
) -> Option<Arc<DynamicImage>> {
    let loaded_image = state
        .original_image
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()?;
    if !std::ptr::eq(&*loaded_image.image, base_image) {
        return None;
    }

    let key = patched_cache_key(&loaded_image.path, adjustments);
    let cache = state
        .patched_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match cache.as_ref() {
        Some((hash, image)) if *hash == key => Some(Arc::clone(image)),
        _ => None,
    }
}

fn compute_patched(
    loaded_image: &LoadedImage,
    adjustments: &serde_json::Value,
) -> Result<Arc<DynamicImage>, String> {
    match prepare_base_image_cow(&loaded_image.image, &loaded_image.path, adjustments)
        .map_err(|e| format!("Failed to composite AI patches: {}", e))?
    {
        Cow::Owned(img) => Ok(Arc::new(img)),
        Cow::Borrowed(_) => Ok(Arc::clone(&loaded_image.image)),
    }
}

#[tauri::command]
fn get_image_dimensions(path: String) -> Result<ImageDimensions, String> {
    let (source_path, _) = parse_virtual_path(&path);
    image::image_dimensions(&source_path)
        .map(|(width, height)| ImageDimensions { width, height })
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_wgpu_transform(
    payload: WgpuTransformPayload,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let context = match state
        .gpu_context
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        Some(c) => c.clone(),
        None => return Ok(()),
    };

    tokio::task::spawn_blocking(move || {
        let mut display_lock = context.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(display) = display_lock.as_mut() {
            display.latest_transform.rect = [payload.x, payload.y, payload.width, payload.height];
            display.latest_transform.clip = [
                payload.clip_x,
                payload.clip_y,
                payload.clip_width,
                payload.clip_height,
            ];
            display.latest_transform.window = [payload.window_width, payload.window_height];
            display.latest_transform.bg_primary = payload.bg_primary;
            display.latest_transform.bg_secondary = payload.bg_secondary;
            display.latest_transform.pixelated = if payload.pixelated { 1.0 } else { 0.0 };
            display.latest_transform.split_origin = payload.split_origin;
            display.latest_transform.split_normal = payload.split_normal;
            display.latest_transform.split_offset = payload.split_offset;

            context.queue.write_buffer(
                &display.transform_buffer,
                0,
                bytemuck::bytes_of(&display.latest_transform),
            );
            display.render(&context.device, &context.queue);
        }
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn release_original_preview(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let Some(context) = state
        .gpu_context
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    else {
        return Ok(());
    };

    tokio::task::spawn_blocking(move || {
        if let Some(display) = context
            .display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            display.original_texture = None;
            display.rebind(&context.device);
        }
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))
}

#[allow(clippy::too_many_arguments)]
fn process_preview_job(
    app_handle: &tauri::AppHandle,
    state: tauri::State<AppState>,
    mut adjustments_json: serde_json::Value,
    layer: DisplayLayer,
    is_interactive: bool,
    target_resolution: Option<u32>,
    roi: Option<(f32, f32, f32, f32)>,
    request_analytics: bool,
    compute_waveform: bool,
    active_waveform_channel: Option<&str>,
) -> Result<Vec<u8>, String> {
    let fn_start = std::time::Instant::now();
    let context =
        get_or_init_gpu_context(&state, &*crate::host::TauriHost::host(app_handle.clone()))
            .map_err(|e| e.to_string())?;
    hydrate_adjustments(&state, &mut adjustments_json);
    let adjustments_clone = adjustments_json;

    let image_generation = state.load_image_generation.load(Ordering::SeqCst);
    let loaded_image_guard = state.original_image.lock().unwrap();
    let loaded_image = loaded_image_guard
        .as_ref()
        .ok_or("No original image loaded")?
        .clone();
    drop(loaded_image_guard);

    crate::ai_denoise::ensure_generated(
        TauriHost::host(app_handle.clone()),
        &loaded_image,
        &adjustments_clone,
    );

    let new_transform_hash = calculate_transform_hash(&adjustments_clone);
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let live_quality = settings.live_preview_quality.as_deref().unwrap_or("high");

    let default_preview_dim = settings.editor_preview_resolution.unwrap_or(1920);
    let preview_dim = target_resolution.unwrap_or(default_preview_dim);
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let use_wgpu_renderer = settings.use_wgpu_renderer.unwrap_or(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let use_wgpu_renderer = false;

    let has_roi = roi.is_some();
    let (interactive_divisor, interactive_quality) = match live_quality {
        "performance" => (if has_roi { 1.4_f32 } else { 1.2_f32 }, 75_u8),
        _ => (1.0_f32, 85_u8),
    };

    let mut cached_preview_lock = state
        .cached_preview
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let base_valid = cached_preview_lock
        .as_ref()
        .is_some_and(|c| c.transform_hash == new_transform_hash && c.preview_dim == preview_dim);
    let small_valid = base_valid
        && cached_preview_lock
            .as_ref()
            .is_some_and(|c| c.interactive_divisor == interactive_divisor);

    let (final_preview_base, scale_for_gpu, unscaled_crop_offset) = if base_valid {
        let cached = cached_preview_lock.as_ref().unwrap();
        (
            Arc::clone(&cached.image),
            cached.scale,
            cached.unscaled_crop_offset,
        )
    } else {
        *cached_preview_lock = None;
        *state
            .gpu_image_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;

        generate_transformed_preview(&state, &loaded_image, &adjustments_clone, preview_dim)
            .map_err(|e| e.to_string())?
    };

    let small_preview_base = if small_valid {
        Arc::clone(&cached_preview_lock.as_ref().unwrap().small_image)
    } else {
        let small = if interactive_divisor > 1.0 {
            let target_size = (preview_dim as f32 / interactive_divisor) as u32;
            let (w, h) = final_preview_base.dimensions();
            let (small_w, small_h) = if w > h {
                let ratio = h as f32 / w as f32;
                (target_size, (target_size as f32 * ratio) as u32)
            } else {
                let ratio = w as f32 / h as f32;
                ((target_size as f32 * ratio) as u32, target_size)
            };
            Arc::new(image_processing::downscale_f32_image(
                &final_preview_base,
                small_w,
                small_h,
            ))
        } else {
            Arc::clone(&final_preview_base)
        };

        if is_interactive && base_valid {
            *state
                .gpu_image_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
        }

        small
    };

    *cached_preview_lock = Some(CachedPreview {
        image: Arc::clone(&final_preview_base),
        small_image: Arc::clone(&small_preview_base),
        transform_hash: new_transform_hash,
        scale: scale_for_gpu,
        unscaled_crop_offset,
        preview_dim,
        interactive_divisor,
    });

    drop(cached_preview_lock);

    let (processing_image, effective_scale, jpeg_quality) = if is_interactive {
        let orig_w = final_preview_base.width() as f32;
        let small_w = small_preview_base.width() as f32;
        let scale_factor = if orig_w > 0.0 { small_w / orig_w } else { 1.0 };
        let new_scale = scale_for_gpu * scale_factor;
        (small_preview_base, new_scale, interactive_quality)
    } else {
        (final_preview_base, scale_for_gpu, 94)
    };

    let (preview_width, preview_height) = processing_image.dimensions();

    let pixel_roi = if is_interactive {
        roi.map(|(nx, ny, nw, nh)| crate::gpu_processing::Roi {
            x: (nx * preview_width as f32).round() as u32,
            y: (ny * preview_height as f32).round() as u32,
            width: (nw * preview_width as f32).round() as u32,
            height: (nh * preview_height as f32).round() as u32,
        })
    } else {
        None
    };

    let mask_definitions: Vec<MaskDefinition> = adjustments_clone
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();

    let scaled_crop_offset = (
        unscaled_crop_offset.0 * effective_scale,
        unscaled_crop_offset.1 * effective_scale,
    );

    let masks = get_cached_or_generate_masks(
        &state,
        &loaded_image.path,
        &mask_definitions,
        preview_width,
        preview_height,
        effective_scale,
        scaled_crop_offset,
        &adjustments_clone,
    );

    let is_raw = loaded_image.is_raw;
    let render = resolve_render_options(
        &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
        is_raw,
    );
    let final_adjustments =
        get_all_adjustments_from_json(&adjustments_clone, is_raw, loaded_image.as_shot, render);
    let lut_path = adjustments_clone["lutPath"].as_str();
    let lut = lut_path.and_then(|p| lut_processing::get_or_load_lut(&state, p).ok());

    let wants_analytics = !(is_interactive && pixel_roi.is_some()) && request_analytics;
    let channel_filter = if is_interactive {
        active_waveform_channel.map(|s| s.to_string())
    } else {
        None
    };

    let analytics_config = if wants_analytics {
        state
            .analytics_worker_tx
            .lock()
            .unwrap()
            .clone()
            .map(|tx| crate::AnalyticsConfig {
                path: loaded_image.path.clone(),
                compute_waveform,
                active_waveform_channel: channel_filter,
                sender: tx,
            })
    } else {
        None
    };

    if use_wgpu_renderer {
        let _ = context.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_millis(500)),
        });
    }

    let final_processed_image_result =
        crate::image_processing::process_and_get_dynamic_image_with_analytics(
            &context,
            &state,
            &processing_image,
            new_transform_hash,
            RenderRequest {
                adjustments: final_adjustments,
                mask_bitmaps: &masks.bitmaps,
                mask_cache_key: Some(masks.cache_key ^ image_generation as u64),
                lut,
                roi: pixel_roi,
            },
            "apply_adjustments",
            use_wgpu_renderer.then_some(layer),
            analytics_config,
        );

    if let Ok(final_processed_image) = final_processed_image_result {
        if use_wgpu_renderer {
            if layer == DisplayLayer::Original {
                return Ok(b"WGPU_RENDER".to_vec());
            }
            if state
                .wgpu_presented_generation
                .swap(image_generation, Ordering::SeqCst)
                != image_generation
            {
                let _ = context.device.poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_millis(500)),
                });
            }
            let _ = app_handle.emit(
                "wgpu-frame-ready",
                serde_json::json!({ "path": loaded_image.path }),
            );
            return Ok(b"WGPU_RENDER".to_vec());
        }

        let final_processed_image = Arc::new(final_processed_image);
        let final_rgba_image = match &*final_processed_image {
            DynamicImage::ImageRgba8(img) => img,
            _ => return Err("Expected Rgba8 image from GPU for encoding".to_string()),
        };

        let raw_bytes: &[u8] = final_rgba_image.as_raw();
        let rgba8_pixels: &[RGBA8] = raw_bytes.as_rgba();

        let img_ref = ImgRef::new(
            rgba8_pixels,
            final_rgba_image.width() as usize,
            final_rgba_image.height() as usize,
        );

        let step_start = std::time::Instant::now();

        let encode_result = Encoder::new(Preset::BaselineFastest)
            .quality(jpeg_quality)
            .fast_color(true)
            .encode_imgref(img_ref);

        match encode_result {
            Ok(jpeg_bytes) => {
                if is_interactive {
                    let (roi_w, roi_h) = final_rgba_image.dimensions();
                    let (rx, ry) = if let Some(r) = pixel_roi {
                        (r.x, r.y)
                    } else {
                        (0, 0)
                    };

                    let mut response = Vec::with_capacity(24 + jpeg_bytes.len());
                    response.extend_from_slice(&rx.to_le_bytes());
                    response.extend_from_slice(&ry.to_le_bytes());
                    response.extend_from_slice(&roi_w.to_le_bytes());
                    response.extend_from_slice(&roi_h.to_le_bytes());
                    response.extend_from_slice(&preview_width.to_le_bytes());
                    response.extend_from_slice(&preview_height.to_le_bytes());
                    response.extend_from_slice(&jpeg_bytes);

                    log::info!(
                        "[process_preview_job] interactive ROI {}x{} encode in {:.2?}, total {:.2?}",
                        roi_w,
                        roi_h,
                        step_start.elapsed(),
                        fn_start.elapsed()
                    );
                    Ok(response)
                } else {
                    let (width, height) = final_rgba_image.dimensions();
                    log::info!(
                        "[process_preview_job] full {}x{} q={} encode in {:.2?}, total {:.2?}",
                        width,
                        height,
                        jpeg_quality,
                        step_start.elapsed(),
                        fn_start.elapsed()
                    );
                    Ok(jpeg_bytes)
                }
            }
            Err(e) => Err(format!("Failed to encode preview: {}", e)),
        }
    } else {
        log::error!(
            "[process_preview_job] processing failed after {:.2?}",
            fn_start.elapsed()
        );
        Err("Processing failed".to_string())
    }
}

const ANALYTICS_MIN_INTERVAL: Duration = Duration::from_millis(16);

fn start_analytics_worker(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let (tx, rx): (Sender<AnalyticsJob>, Receiver<AnalyticsJob>) = mpsc::channel();
    *state.analytics_worker_tx.lock().unwrap() = Some(tx);

    std::thread::spawn(move || {
        let mut last_run: Option<std::time::Instant> = None;
        while let Ok(mut job) = rx.recv() {
            if let Some(remaining) =
                last_run.and_then(|t| ANALYTICS_MIN_INTERVAL.checked_sub(t.elapsed()))
            {
                std::thread::sleep(remaining);
            }
            while let Ok(latest) = rx.try_recv() {
                job = latest;
            }
            last_run = Some(std::time::Instant::now());

            let histogram_data = image_processing::calculate_histogram(&job.frame).ok();

            let waveform_data = if job.compute_waveform {
                image_processing::calculate_waveform(
                    &job.frame,
                    job.active_waveform_channel.as_deref(),
                )
                .ok()
            } else {
                None
            };

            if histogram_data.is_some() || waveform_data.is_some() {
                let state = app_handle.state::<AppState>();
                let channel = state.analytics_channel.lock().unwrap().clone();
                if let Some(channel) = channel {
                    let packet = image_processing::encode_analytics_packet(
                        &job.path,
                        histogram_data.as_ref(),
                        waveform_data.as_ref(),
                    );
                    (channel)(packet);
                }
            }
        }
    });
}

#[tauri::command]
fn subscribe_analytics(channel: Channel, state: tauri::State<AppState>) {
    let packet_tx = move |packet: Vec<u8>| {
        let _ = channel.send(InvokeResponseBody::Raw(packet));
    };
    *state.analytics_channel.lock().unwrap() = Some(Arc::new(packet_tx));
}

fn start_preview_worker(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let (tx, rx): (Sender<PreviewJob>, Receiver<PreviewJob>) = mpsc::channel();

    *state.preview_worker_tx.lock().unwrap() = Some(tx);

    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let mut latest: [Option<PreviewJob>; 2] = [None, None];
            let mut next = Some(job);
            while let Some(job) = next {
                let slot = job.layer as usize;
                latest[slot] = Some(job);
                next = rx.try_recv().ok();
            }

            for job in latest.into_iter().flatten() {
                let state = app_handle.state::<AppState>();
                let responder = job.responder;
                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    process_preview_job(
                        &app_handle,
                        state,
                        job.adjustments,
                        job.layer,
                        job.is_interactive,
                        job.target_resolution,
                        job.roi,
                        job.request_analytics,
                        job.compute_waveform,
                        job.active_waveform_channel.as_deref(),
                    )
                }))
                .unwrap_or_else(|_| Err("Preview rendering panicked".to_string()));
                match result {
                    Ok(bytes) => {
                        let _ = responder.send(bytes);
                    }
                    Err(e) => {
                        log::error!("Preview worker error: {}", e);
                    }
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn apply_adjustments(
    js_adjustments: serde_json::Value,
    original: bool,
    is_interactive: bool,
    target_resolution: Option<u32>,
    roi: Option<(f32, f32, f32, f32)>,
    request_analytics: bool,
    compute_waveform: bool,
    active_waveform_channel: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Response, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    {
        let tx_guard = state.preview_worker_tx.lock().unwrap();
        if let Some(worker_tx) = tx_guard.as_ref() {
            let job = PreviewJob {
                adjustments: js_adjustments,
                layer: if original {
                    DisplayLayer::Original
                } else {
                    DisplayLayer::Edited
                },
                is_interactive,
                target_resolution,
                roi,
                request_analytics,
                compute_waveform,
                active_waveform_channel,
                responder: tx,
            };
            worker_tx
                .send(job)
                .map_err(|e| format!("Failed to send to preview worker: {}", e))?;
        } else {
            return Err("Preview worker not running".to_string());
        }
    }

    match rx.await {
        Ok(bytes) => Ok(Response::new(bytes)),
        Err(_) => Err("Superseded or worker failed".to_string()),
    }
}

#[tauri::command]
async fn generate_uncropped_preview(
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Response, String> {
    let context =
        get_or_init_gpu_context(&state, &*crate::host::TauriHost::host(app_handle.clone()))
            .map_err(|e| e.to_string())?;
    let mut adjustments_clone = js_adjustments.clone();
    hydrate_adjustments(&state, &mut adjustments_clone);

    let loaded_image = state
        .original_image
        .lock()
        .unwrap()
        .clone()
        .ok_or("No original image loaded")?;

    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let path = loaded_image.path.clone();
        let is_raw = loaded_image.is_raw;
        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        let target_dim = settings.editor_preview_resolution.unwrap_or(1920);

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        crate::cache_utils::calculate_patch_hash(&adjustments_clone).hash(&mut hasher);
        let patched_key = hasher.finish();

        let mut hasher = DefaultHasher::new();
        patched_key.hash(&mut hasher);
        crate::cache_utils::calculate_effects_hash(&adjustments_clone).hash(&mut hasher);
        target_dim.hash(&mut hasher);
        let base_key = hasher.finish();

        let cached_base = state.geometry_cache.lock().unwrap().get(&base_key).cloned();
        let pre_geometry_base = match cached_base {
            Some(cached) => cached,
            None => {
                let patched = get_or_compute_stage(&state.patched_cache, patched_key, || {
                    compute_patched(&loaded_image, &adjustments_clone)
                })?;
                let downscaled = downscale_f32_image(&patched, target_dim, target_dim);
                drop(patched);

                let relit_image =
                    crate::relight::apply_relight(Cow::Owned(downscaled), &adjustments_clone);
                let fogged_image = crate::fog::apply_fog(relit_image, &adjustments_clone);
                let base = crate::lens_blur::apply_lens_blur(fogged_image, &adjustments_clone)
                    .into_owned();

                let mut cache = state.geometry_cache.lock().unwrap();
                if cache.len() > 2 {
                    cache.clear();
                }
                cache.insert(base_key, base.clone());
                base
            }
        };

        let scale_for_gpu = if loaded_image.image.width() > 0 {
            pre_geometry_base.width() as f32 / loaded_image.image.width() as f32
        } else {
            1.0
        };

        let params = crate::image_processing::get_geometry_params_from_json(&adjustments_clone);
        let mut adjusted_params = params;
        adjusted_params.lens_vignette_amount *= if is_raw { 0.4 } else { 0.8 };

        let warped_image = warp_image_geometry(&pre_geometry_base, adjusted_params);
        let orientation_steps = adjustments_clone["orientationSteps"].as_u64().unwrap_or(0) as u8;
        let flipped_image = apply_flip(
            apply_coarse_rotation(Cow::Owned(warped_image), orientation_steps),
            adjustments_clone["flipHorizontal"]
                .as_bool()
                .unwrap_or(false),
            adjustments_clone["flipVertical"].as_bool().unwrap_or(false),
        )
        .into_owned();

        let (preview_width, preview_height) = flipped_image.dimensions();

        let mask_definitions: Vec<MaskDefinition> = adjustments_clone
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let mask_bitmaps = get_cached_or_generate_masks(
            &state,
            &path,
            &mask_definitions,
            preview_width,
            preview_height,
            scale_for_gpu,
            (0.0, 0.0),
            &adjustments_clone,
        )
        .bitmaps;

        let render = resolve_render_options(
            &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
            is_raw,
        );
        let mut uncropped_adjustments =
            get_all_adjustments_from_json(&adjustments_clone, is_raw, loaded_image.as_shot, render);
        uncropped_adjustments.global.show_clipping = 0;
        let lut_path = adjustments_clone["lutPath"].as_str();
        let lut = lut_path.and_then(|p| lut_processing::get_or_load_lut(&state, p).ok());

        let full_hash = calculate_full_job_hash(&path, &adjustments_clone);

        if let Ok(processed_image) = process_and_get_dynamic_image(
            &context,
            &state,
            &flipped_image,
            full_hash,
            RenderRequest {
                adjustments: uncropped_adjustments,
                mask_bitmaps: &mask_bitmaps,
                mask_cache_key: None,
                lut,
                roi: None,
            },
            "generate_uncropped_preview",
        ) {
            let (width, height) = processed_image.dimensions();
            let rgb_pixels = processed_image.to_rgb8().into_vec();

            if let Ok(bytes) = Encoder::new(Preset::BaselineFastest)
                .quality(80)
                .encode_rgb(&rgb_pixels, width, height)
            {
                return Ok(Response::new(bytes));
            }
        }

        Err("Failed to process uncropped preview".to_string())
    })
    .await
    .map_err(|e| format!("Task execution failed: {}", e))?
}

pub fn get_original_image(
    state: &tauri::State<AppState>,
) -> Result<(std::sync::Arc<image::DynamicImage>, bool), String> {
    let original_image_lock = state.original_image.lock().unwrap();
    let loaded_image = original_image_lock
        .as_ref()
        .ok_or("No original image loaded")?;
    Ok((
        std::sync::Arc::clone(&loaded_image.image),
        loaded_image.is_raw,
    ))
}

#[tauri::command]
async fn save_temp_file(bytes: Vec<u8>) -> Result<String, String> {
    let mut temp_file = NamedTempFile::new().map_err(|e| e.to_string())?;
    temp_file.write_all(&bytes).map_err(|e| e.to_string())?;
    let (_file, path) = temp_file.keep().map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn generate_preview_for_path(
    path: String,
    js_adjustments: Value,
    app_handle: tauri::AppHandle,
) -> Result<Response, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let context =
            get_or_init_gpu_context(&state, &*crate::host::TauriHost::host(app_handle.clone()))
                .map_err(|e| e.to_string())?;
        let (source_path, _) = parse_virtual_path(&path);
        let source_path_str = source_path.to_string_lossy().to_string();
        let is_raw = is_raw_file(&source_path_str);
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let (pristine, _exif) =
            crate::image_loader::get_or_decode_pristine(&state, &source_path, &settings, None)?;
        let base_image = prepare_base_image_cow(&pristine, &source_path_str, &js_adjustments)
            .map_err(|e| e.to_string())?;
        let (transformed_image, unscaled_crop_offset) =
            apply_all_transformations(Cow::Borrowed(base_image.as_ref()), &js_adjustments);
        let (img_w, img_h) = transformed_image.dimensions();
        let mask_definitions: Vec<MaskDefinition> = js_adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let warped_image =
            build_warped_image_for_masks(&base_image, is_raw, &js_adjustments, &mask_definitions);
        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
            .iter()
            .filter_map(|def| {
                generate_mask_bitmap(
                    def,
                    img_w,
                    img_h,
                    1.0,
                    unscaled_crop_offset,
                    warped_image.as_deref(),
                )
            })
            .collect();

        let render = resolve_render_options(&settings, is_raw);
        let mut all_adjustments = get_all_adjustments_from_json(
            &js_adjustments,
            is_raw,
            crate::as_shot::as_shot(&source_path_str),
            render,
        );
        all_adjustments.global.show_clipping = 0;
        let lut_path = js_adjustments["lutPath"].as_str();
        let lut = lut_path.and_then(|p| lut_processing::get_or_load_lut(&state, p).ok());
        let unique_hash = calculate_full_job_hash(&source_path_str, &js_adjustments);

        let final_image = process_and_get_dynamic_image(
            &context,
            &state,
            transformed_image.as_ref(),
            unique_hash,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                mask_cache_key: None,
                lut,
                roi: None,
            },
            "generate_preview_for_path",
        )
        .map_err(|e| e.to_string())?;

        let (width, height) = final_image.dimensions();
        let rgb_pixels = final_image.to_rgb8().into_vec();

        let bytes = Encoder::new(Preset::BaselineFastest)
            .quality(92)
            .encode_rgb(&rgb_pixels, width, height)
            .map_err(|e| format!("Failed to encode with mozjpeg-rs: {}", e))?;

        Ok(Response::new(bytes))
    })
    .await
    .map_err(|e| format!("Task execution failed: {}", e))?
}

fn setup_logging(app_handle: &tauri::AppHandle) {
    let log_dir = match app_handle.path().app_log_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Failed to get app log directory: {}", e);
            return;
        }
    };

    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("Failed to create log directory at {:?}: {}", log_dir, e);
    }

    let log_file_path = log_dir.join("app.log");

    let log_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&log_file_path)
        .ok();

    let var = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let level: log::LevelFilter = var.parse().unwrap_or(log::LevelFilter::Info);

    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(level)
        .chain(fern::Output::call(|record| {
            let _ = writeln!(std::io::stderr(), "{}", record.args());
        }));

    if let Some(file) = log_file {
        dispatch = dispatch.chain(file);
    } else {
        eprintln!(
            "Failed to open log file at {:?}. Logging to console only.",
            log_file_path
        );
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("Failed to apply logger configuration: {}", e);
    }

    panic::set_hook(Box::new(|info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&'static str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", info.payload())
        };
        let location = info.location().map_or_else(
            || "at an unknown location".to_string(),
            |loc| format!("at {}:{}:{}", loc.file(), loc.line(), loc.column()),
        );
        log::error!("PANIC! {} - {}", location, message.trim());
    }));

    log::info!(
        "Logger initialized successfully. Log file at: {:?}",
        log_file_path
    );
}

#[tauri::command]
fn get_log_file_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let log_dir = app_handle.path().app_log_dir().map_err(|e| e.to_string())?;
    let log_file_path = log_dir.join("app.log");
    Ok(log_file_path.to_string_lossy().to_string())
}

#[tauri::command]
fn frontend_log(level: String, message: String) -> Result<(), String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let log_line = |line: &str| match level.to_lowercase().as_str() {
        "error" => log::error!("[frontend] {}", line),
        "warn" => log::warn!("[frontend] {}", line),
        "debug" => log::debug!("[frontend] {}", line),
        "trace" => log::trace!("[frontend] {}", line),
        _ => log::info!("[frontend] {}", line),
    };

    for line in trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        log_line(line);
    }

    Ok(())
}

#[derive(Clone, Copy, Debug)]
#[cfg(not(target_os = "android"))]
struct MonitorBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(not(target_os = "android"))]
fn available_monitor_bounds(window: &tauri::WebviewWindow) -> Vec<MonitorBounds> {
    window
        .available_monitors()
        .map(|monitors| {
            monitors
                .into_iter()
                .map(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    MonitorBounds {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
fn saved_window_state_is_usable(
    state: &WindowState,
    monitors: &[MonitorBounds],
    scale_factor: f64,
) -> bool {
    let min_w = (800.0 * scale_factor) as u32;
    let min_h = (600.0 * scale_factor) as u32;

    if state.width < min_w || state.height < min_h {
        return false;
    }

    if monitors.is_empty() {
        return true;
    }

    let window_left = state.x as i64;
    let window_top = state.y as i64;
    let window_right = window_left + state.width as i64;
    let window_bottom = window_top + state.height as i64;

    monitors.iter().any(|monitor| {
        let monitor_left = monitor.x as i64;
        let monitor_top = monitor.y as i64;
        let monitor_right = monitor_left + monitor.width as i64;
        let monitor_bottom = monitor_top + monitor.height as i64;

        let overlap_width = window_right.min(monitor_right) - window_left.max(monitor_left);
        let overlap_height = window_bottom.min(monitor_bottom) - window_top.max(monitor_top);

        overlap_width >= 100 && overlap_height >= 100
    })
}

#[tauri::command]
fn frontend_ready(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    state: tauri::State<AppState>,
) -> Result<LaunchPayload, String> {
    let is_first_run = !state
        .window_setup_complete
        .swap(true, std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_os = "android")]
    let _ = (is_first_run, &window, &app_handle);

    #[cfg(not(target_os = "android"))]
    {
        #[cfg(any(windows, target_os = "linux"))]
        let mut should_maximize = false;
        #[cfg(any(windows, target_os = "linux"))]
        let mut should_fullscreen = false;
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = (&app_handle, is_first_run);

        #[cfg(any(windows, target_os = "linux"))]
        if is_first_run && let Ok(config_dir) = app_handle.path().app_config_dir() {
            let path = config_dir.join("window_state.json");

            if let Ok(contents) = std::fs::read_to_string(&path)
                && let Ok(saved_state) = serde_json::from_str::<WindowState>(&contents)
            {
                should_maximize = saved_state.maximized;
                should_fullscreen = saved_state.fullscreen;
            }
        }

        if let Err(e) = window.show() {
            log::error!("Failed to show window: {}", e);
        }
        if let Err(e) = window.set_focus() {
            log::error!("Failed to focus window: {}", e);
        }
        #[cfg(any(windows, target_os = "linux"))]
        if is_first_run {
            if should_maximize {
                let _ = window.maximize();
            }
            if should_fullscreen {
                let _ = window.set_fullscreen(true);
            }
        }
    }

    let open_with_file = state.initial_file_path.lock().unwrap().take();
    let edit_session = state.pending_edit_session.lock().unwrap().take();
    if let Some(path) = &open_with_file {
        log::info!("Frontend is ready, returning initial path: {}", path);
    }
    if let Some(session) = &edit_session {
        log::info!(
            "Frontend is ready, returning external edit session for: {}",
            session.source
        );
    }
    Ok(LaunchPayload {
        open_with_file,
        edit_session,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rayon::ThreadPoolBuilder::new()
        .stack_size(8 * 1024 * 1024)
        .build_global();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch_req = parse_launch_args(&args);

    let mut builder = tauri::Builder::default();

    #[cfg(target_os = "linux")]
    {
        builder = builder.plugin(tauri_plugin_wayland_nvidia_quirk::init());
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            log::info!(
                "New instance launched with args: {:?}. Focusing main window.",
                argv
            );
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.unminimize() {
                    log::error!("Failed to unminimize window: {}", e);
                }
                if let Err(e) = window.set_focus() {
                    log::error!("Failed to set focus on window: {}", e);
                }
            }

            let forwarded_args = argv.get(1..).unwrap_or(&[]);
            emit_launch_request(app, parse_launch_args(forwarded_args));
        }));
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_store::Builder::new().build());
    }

    builder
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(PinchZoomDisablePlugin)
        .on_window_event(|window, event| if let tauri::WindowEvent::Resized(size) = event {
            let state = window.state::<AppState>();
            if let Some(ctx) = state.gpu_context.lock().unwrap().as_ref()
                && let Ok(mut display_lock) = ctx.display.try_lock()
                    && let Some(display) = display_lock.as_mut() {
                        display.config.width = size.width.max(1);
                        display.config.height = size.height.max(1);
                        display.surface.configure(&ctx.device, &display.config);
                        display.render(&ctx.device, &ctx.queue);
                    }
        })
        .setup(move |app| {

            let state = app.state::<AppState>();

            #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
            {
                match launch_req.clone() {
                    LaunchRequest::EditSession(session) => {
                        log::info!("Initial launch with external edit session for: {}", session.source);
                        *state.pending_edit_session.lock().unwrap() = Some(session);
                    }
                    LaunchRequest::OpenFile(path) => {
                        log::info!("Initial open: Storing path {} for later.", path);
                        *state.initial_file_path.lock().unwrap() = Some(path);
                    }
                    _ => {}
                }
            }

            let app_handle = app.handle().clone();

            if let Ok(cache_dir) = app_handle.path().app_cache_dir() {
                crate::ai_denoise::initialize_cache_dir(cache_dir.clone());
                crate::effect_maps::initialize_cache_dir(cache_dir.clone());
                crate::exif_processing::initialize_cache_dir(cache_dir.clone());
                        }

            {
                let disks_app_handle = app_handle.clone();
                std::thread::spawn(move || {
                    let disks = sysinfo::Disks::new_with_refreshed_list();
                    let state = disks_app_handle.state::<AppState>();
                    *state.disks_cache.lock().unwrap() = Some(disks);
                });
            }

            let config_dir = app_handle.path().app_config_dir().expect("Failed to get config dir");
            let crash_flag_path = config_dir.join(".gpu_init_crash_flag");

            {
                let state = app.state::<AppState>();
                *state.gpu_crash_flag_path.lock().unwrap() = Some(crash_flag_path.clone());
            }

            let mut settings: AppSettings = load_settings(app_handle.clone()).unwrap_or_default();

            {
                let state = app.state::<AppState>();
                let cache_size = settings.image_cache_size.unwrap_or(5) as usize;
                state.decoded_image_cache.lock().unwrap().set_capacity(cache_size);
            }
            crate::storage::apply_cache_limits(&settings);

            if crash_flag_path.exists() {
                log::warn!("GPU Driver crash detected on last run! Falling back to OpenGL backend.");
                settings.processing_backend = Some("gl".to_string());
                let _ = crate::save_settings(settings.clone(), app_handle.clone());
                let _ = std::fs::remove_file(&crash_flag_path);
            }

            let lens_db = lens_correction::load_lensfun_db(
                &app_handle.path().resource_dir().unwrap_or_default(),
            );
            {
                let state = app.state::<AppState>();
                *state.lens_db.lock().unwrap() = Some(Arc::new(lens_db));
            }

            unsafe {
                if let Some(backend) = &settings.processing_backend
                    && backend != "auto" {
                        std::env::set_var("WGPU_BACKEND", backend);
                    }

                #[cfg(target_os = "linux")]
                {
                    if settings.linux_gpu_optimization.unwrap_or(false) {
                        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
                        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
                        std::env::set_var("NODEVICE_SELECT", "1");
                    }
                }

                #[cfg(not(target_os = "android"))]
                {
                    let resource_path = app_handle
                        .path()
                        .resolve("resources", tauri::path::BaseDirectory::Resource)
                        .expect("failed to resolve resource directory");

                    let ort_library_name = {
                        #[cfg(target_os = "windows")]
                        { "onnxruntime.dll" }
                        #[cfg(target_os = "linux")]
                        { "libonnxruntime.so" }
                        #[cfg(target_os = "macos")]
                        { "libonnxruntime.dylib" }
                        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
                        { "libonnxruntime.so" }
                    };
                    let ort_library_path = resource_path.join(ort_library_name);
                    std::env::set_var("ORT_DYLIB_PATH", &ort_library_path);
                    cli_println!("Set ORT_DYLIB_PATH to: {}", ort_library_path.display());
                }
            }

            setup_logging(&app_handle);

            if let Some(backend) = &settings.processing_backend
                && backend != "auto" {
                    log::info!("Applied processing backend setting: {}", backend);
                }

            #[cfg(target_os = "linux")]
            if settings.linux_gpu_optimization.unwrap_or(false) {
                log::info!("Applied Linux Compatibility Mode (forced software compositing).");
            } else {
                log::info!(
                    "Wayland Nvidia quirk status: {:?}",
                    tauri_plugin_wayland_nvidia_quirk::status()
                );
            }

            start_preview_worker(app_handle.clone());
            start_analytics_worker(app_handle.clone());
            jxl_oxide::integration::register_image_decoding_hook();

            let window_cfg = app.config().app.windows.first().unwrap().clone();
            let decorations = settings.decorations.unwrap_or(window_cfg.decorations);
            #[cfg(target_os = "android")]
            let _ = decorations;

            let main_window_cfg = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("Main window config not found")
                .clone();

            let mut window_builder =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &main_window_cfg)
                    .unwrap();

            #[cfg(not(target_os = "android"))]
            {
                window_builder = window_builder.decorations(decorations).visible(false);
            }

            let _window = window_builder.build().expect("Failed to build window");

            #[cfg(target_os = "android")]
            let _ = &_window;

            #[cfg(not(target_os = "android"))]
            let window = &_window;

            #[cfg(not(target_os = "android"))]
            {
                let app_state = app.state::<AppState>();
                if let Err(error) = get_or_init_gpu_context(&app_state, &*crate::host::TauriHost::host(app.handle().clone())) {
                    log::warn!(
                        "GPU pre-initialization failed (editing and thumbnails may be degraded): {}",
                        error
                    );
                }

                let scale = window.scale_factor().unwrap_or(1.0);
                let default_w = (1280.0 * scale) as u32;
                let default_h = (720.0 * scale) as u32;

                let mut initial_normal_rect = (default_w, default_h, 100i32, 100i32);
                let mut state_loaded = false;

                if let Ok(config_dir) = app.path().app_config_dir() {
                    let path = config_dir.join("window_state.json");
                    if let Ok(contents) = std::fs::read_to_string(&path)
                        && let Ok(state) = serde_json::from_str::<WindowState>(&contents)
                    {
                        let monitor_bounds = available_monitor_bounds(&window);
                        let monitor_size = window.current_monitor().ok().flatten().map(|m| *m.size());

                        let is_maximized_size = monitor_size.map(|m| {
                            state.width >= m.width.saturating_sub(60) && state.height >= m.height.saturating_sub(60)
                        }).unwrap_or(false);

                        if saved_window_state_is_usable(&state, &monitor_bounds, scale) && !is_maximized_size {
                            initial_normal_rect = (state.width, state.height, state.x, state.y);
                            state_loaded = true;

                            let _ = window.set_size(tauri::Size::Physical(
                                tauri::PhysicalSize::new(state.width, state.height),
                            ));
                            let _ = window.set_position(tauri::Position::Physical(
                                tauri::PhysicalPosition::new(state.x, state.y),
                            ));
                        }
                    }
                }

                if !state_loaded {
                    let (mut w, mut h) = (1280.0_f64, 720.0_f64);
                    if let Some(m) = window.current_monitor().ok().flatten() {
                        let ms = m.size().to_logical::<f64>(m.scale_factor());
                        w = w.min(ms.width - 80.0).max(800.0);
                        h = h.min(ms.height - 120.0).max(600.0);
                    }

                    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(w, h)));
                    let _ = window.center();

                    initial_normal_rect.0 = (w * scale) as u32;
                    initial_normal_rect.1 = (h * scale) as u32;
                    if let Ok(pos) = window.outer_position() {
                        initial_normal_rect.2 = pos.x;
                        initial_normal_rect.3 = pos.y;
                    }
                }

                let window_failsafe = window.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    if let Ok(false) = window_failsafe.is_visible() {
                        log::warn!(
                            "Frontend failed to report ready within timeout. Forcing window visibility."
                        );
                        let _ = window_failsafe.show();
                        let _ = window_failsafe.set_focus();
                    }
                });

                let pending_window_state = Arc::new(Mutex::new(None::<WindowState>));
                let pending_state_for_saver = Arc::clone(&pending_window_state);
                let app_handle_for_saver = app.handle().clone();

                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        let state_to_save = {
                            let mut lock = pending_state_for_saver.lock().unwrap();
                            lock.take()
                        };

                        if let Some(state) = state_to_save
                            && let Ok(config_dir) = app_handle_for_saver.path().app_config_dir()
                        {
                            let path = config_dir.join("window_state.json");
                            let _ = std::fs::create_dir_all(&config_dir);
                            if let Ok(json) = serde_json::to_string(&state) {
                                let _ = std::fs::write(&path, json);
                            }
                        }
                    }
                });

                let window_for_handler = window.clone();
                let pending_state_for_handler = Arc::clone(&pending_window_state);
                let last_normal_rect = Arc::new(Mutex::new(initial_normal_rect));
                let last_normal_rect_handler = Arc::clone(&last_normal_rect);

                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                        #[cfg(any(windows, target_os = "linux"))]
                        let maximized = window_for_handler.is_maximized().unwrap_or(false);
                        #[cfg(not(any(windows, target_os = "linux")))]
                        let maximized = false;

                        #[cfg(any(windows, target_os = "linux"))]
                        let fullscreen = window_for_handler.is_fullscreen().unwrap_or(false);
                        #[cfg(not(any(windows, target_os = "linux")))]
                        let fullscreen = false;

                        if window_for_handler.is_minimized().unwrap_or(false) {
                            return;
                        }

                        let mut normal = last_normal_rect_handler.lock().unwrap();

                        if !maximized && !fullscreen {
                            let (curr_w, curr_h) = window_for_handler
                                .outer_size()
                                .map(|s| (s.width, s.height))
                                .unwrap_or((0, 0));

                            let event_scale = window_for_handler.scale_factor().unwrap_or(1.0);
                            let min_w = (800.0 * event_scale) as u32;
                            let min_h = (600.0 * event_scale) as u32;

                            let is_maximize_in_progress = window_for_handler
                                .current_monitor()
                                .ok()
                                .flatten()
                                .map(|m| {
                                    let ms = m.size();
                                    curr_w >= ms.width.saturating_sub(20) && curr_h >= ms.height.saturating_sub(20)
                                })
                                .unwrap_or(false);

                            if !is_maximize_in_progress && curr_w >= min_w && curr_h >= min_h {
                                normal.0 = curr_w;
                                normal.1 = curr_h;

                                if let Ok(position) = window_for_handler.outer_position() {
                                    normal.2 = position.x;
                                    normal.3 = position.y;
                                }
                            }
                        }

                        let state = WindowState {
                            width: normal.0,
                            height: normal.1,
                            x: normal.2,
                            y: normal.3,
                            maximized,
                            fullscreen,
                        };

                        *pending_state_for_handler.lock().unwrap() = Some(state);
                    }
                    _ => {}
                });
            }

            Ok(())
        })
        .manage(Arc::new(AppState::new()))
        .invoke_handler(tauri::generate_handler![
            apply_adjustments,
            subscribe_analytics,
            generate_preview_for_path,
            generate_uncropped_preview,
            get_log_file_path,
            frontend_log,
            lut_processing::load_and_parse_lut,
            lut_processing::list_luts,
            lut_processing::import_luts,
            lut_processing::remove_lut,
            lut_processing::generate_lut_previews,
            save_temp_file,
            get_image_dimensions,
            frontend_ready,
            update_wgpu_transform,
            release_original_preview,
            cache_utils::clear_session_caches,
            cache_utils::clear_image_caches,
            app_settings::load_settings,
            app_settings::save_settings,
            ai_commands::generate_ai_depth_mask,
            ai_commands::check_ai_connector_status,
            ai_commands::test_ai_connector_connection,
            ai_commands::generate_full_image_depth_map,
            ai_commands::generate_relight_normal_map,
            ai_denoise::clear_ai_denoise_cache,
            ai_commands::cancel_ai_task,
            inpainting::invoke_generative_replace_with_mask_def,
            inpainting::generate_manual_cleanup_patch,
            inpainting::generate_liquify_patch,
            inpainting::generate_retouch_patch,
            focus_stacking::stitch_focus_stack,
            focus_stacking::save_focus_stack,
            image_loader::load_image,
            image_loader::is_image_cached,
            export_processing::export_images,
            export_processing::cancel_export,
            export_processing::estimate_export_sizes,
            image_processing::calculate_auto_adjustments,
            image_processing::sample_white_balance,
            auto_straighten::calculate_auto_straighten,
            file_management::apply_auto_straighten_to_paths,
            mask_generation::generate_mask_overlay,
            file_management::get_supported_file_types,
            file_management::save_metadata_and_update_thumbnail,
            file_management::apply_adjustments_to_paths,
            file_management::load_metadata,
            file_management::reset_adjustments_for_paths,
            file_management::apply_auto_lens_correction_to_paths,
            file_management::apply_auto_adjustments_to_paths,
            file_management::clear_all_sidecars,
            storage::get_storage_usage,
            storage::clear_ai_maps_cache,
            storage::clear_metadata_cache,
            storage::clear_ai_models,
            lens_correction::get_lensfun_makers,
            lens_correction::get_lensfun_lenses_for_maker,
            lens_correction::autodetect_lens,
            lens_correction::get_lens_distortion_params,
            guided_perspective::calculate_guided_perspective,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(#[allow(unused_variables)] |app_handle, event| {
            match event {
                #[cfg(target_os = "macos")]
				tauri::RunEvent::Opened { urls } => {
				    if let Some(url) = urls.first()
				        && let Ok(path) = url.to_file_path()
				        && let Some(path_str) = path.to_str()
				    {
				        let state = app_handle.state::<AppState>();

				        if state
				            .window_setup_complete
				            .load(std::sync::atomic::Ordering::Relaxed)
				        {
				            if let Some(window) = app_handle.get_webview_window("main") {
				                let _ = window.unminimize();
				                let _ = window.show();
				                let _ = window.set_focus();
				            }

				            log::info!("macOS runtime open: Opening {}.", path_str);

				            emit_launch_request(
				                app_handle,
				                LaunchRequest::OpenFile(path_str.to_string()),
				            );
				        } else {
				            *state.initial_file_path.lock().unwrap() =
				                Some(path_str.to_string());

				            log::info!(
				                "macOS initial open: Stored path {} for later.",
				                path_str
				            );
				        }
				    }
				}
                tauri::RunEvent::ExitRequested { api, code, .. } => {
                    api.prevent_exit();
                    let code = resolve_exit_code(code, process_exit_code());

                    #[cfg(target_os = "macos")]
                    unsafe { libc::_exit(code); }

                    #[cfg(not(target_os = "macos"))]
                    std::process::exit(code);
                }
                tauri::RunEvent::Exit => {
                    let code = process_exit_code();

                    #[cfg(target_os = "macos")]
                    unsafe { libc::_exit(code); }

                    #[cfg(not(target_os = "macos"))]
                    std::process::exit(code);
                }
                _ => {}
            }
        });
}
pub use engine::adjustment_utils;
pub use engine::ai_processing;
pub use engine::app_state;
pub use engine::as_shot;
pub use engine::baseline_exposure;

pub use engine::effect_maps;
pub use engine::exif_processing;
pub use engine::fog;
pub use engine::gpu_processing;
pub mod image_loader;
pub use engine::lens_blur;

pub use engine::memory_budget;
pub use engine::negative_conversion;
pub use engine::raw_processing;
pub use engine::relight;
mod storage;
pub use engine::white_balance;
mod launch_request;
pub use engine::preview_cache;
