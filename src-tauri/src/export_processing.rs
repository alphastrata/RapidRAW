use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::codecs::png::PngEncoder;
use image::{
    DynamicImage, GenericImageView, GrayImage, ImageBuffer, ImageEncoder, ImageFormat, Luma,
    imageops,
};
use jxl_encoder::{
    LosslessConfig, LossyConfig, PixelLayout,
    api::{calibrated_jxl_quality, quality_to_distance},
};
use serde_json::Value;
use tauri::Emitter;
use tauri::Manager;
use tiff::encoder::{Rational, TiffEncoder, TiffValue, colortype};
use tiff::tags::ResolutionUnit;

use crate::AppState;
use crate::baseline_exposure::BaselineOptions;
use crate::exif_processing;
use crate::file_management::{
    generate_filename_from_template, parse_virtual_path, read_file_mapped,
};
use crate::formats::is_raw_file;
use crate::image_loader::{load_and_composite, load_base_image_from_bytes, prepare_base_image_cow};
use crate::image_processing::{
    AllAdjustments, Crop, GpuContext, RenderOptions, RenderOutputPrecision, RenderRequest,
    downscale_f32_image, get_all_adjustments_from_json, get_or_init_gpu_context,
    process_and_get_dynamic_image, process_and_get_dynamic_image_with_precision,
    resolve_render_options,
};
use crate::lut_processing::{
    convert_image_to_cube_lut, generate_identity_lut_image, get_or_load_lut,
};
use crate::mask_generation::{MaskDefinition, build_warped_image_for_masks, generate_mask_bitmap};

use crate::as_shot::as_shot;
use crate::cache_utils::{calculate_full_job_hash, calculate_transform_hash};
use crate::{
    apply_all_transformations, generate_transformed_preview, get_cached_or_generate_masks,
    hydrate_adjustments, load_settings,
};

use engine::export_geometry as export_geom;
pub use engine::export_geometry::{
    BorderBasis, BorderOptions, ExportGeometry, ExportSettings, PadOptions, ResizeMode,
    ResizeOptions, WatermarkAnchor, WatermarkSettings,
};
pub use engine::launch_request::TiffBitDepth;

#[derive(Clone)]
pub(crate) enum ExportAdjustmentsMode {
    UseSidecars {
        active_path: Option<String>,
        active_adjustments: Option<Value>,
    },
    GlobalOverride(Value),
}

struct WatermarkCacheEntry {
    path: String,
    modified: Option<std::time::SystemTime>,
    len: Option<u64>,
    image: Arc<DynamicImage>,
    scaled: Option<((u32, u32), Arc<DynamicImage>)>,
}

static WATERMARK_CACHE: Mutex<Option<WatermarkCacheEntry>> = Mutex::new(None);

struct WatermarkCacheGuard;

impl Drop for WatermarkCacheGuard {
    fn drop(&mut self) {
        *WATERMARK_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn load_watermark_image(path: &str) -> Result<Arc<DynamicImage>, String> {
    let metadata = fs::metadata(path).ok();
    let modified = metadata.as_ref().and_then(|m| m.modified().ok());
    let len = metadata.as_ref().map(|m| m.len());
    let mut cache = WATERMARK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = cache.as_ref()
        && entry.path == path
        && entry.modified.is_some()
        && entry.modified == modified
        && entry.len == len
    {
        return Ok(Arc::clone(&entry.image));
    }
    let image =
        Arc::new(image::open(path).map_err(|e| format!("Failed to open watermark image: {}", e))?);
    *cache = Some(WatermarkCacheEntry {
        path: path.to_string(),
        modified,
        len,
        image: Arc::clone(&image),
        scaled: None,
    });
    Ok(image)
}

fn scaled_watermark_image(
    path: &str,
    watermark_img: &Arc<DynamicImage>,
    new_wm_w: u32,
    new_wm_h: u32,
) -> Arc<DynamicImage> {
    {
        let cache = WATERMARK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.as_ref()
            && entry.path == path
            && Arc::ptr_eq(&entry.image, watermark_img)
            && let Some((size, scaled)) = &entry.scaled
            && *size == (new_wm_w, new_wm_h)
        {
            return Arc::clone(scaled);
        }
    }
    let scaled = Arc::new(watermark_img.resize_exact(
        new_wm_w,
        new_wm_h,
        image::imageops::FilterType::Lanczos3,
    ));
    let mut cache = WATERMARK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = cache.as_mut()
        && entry.path == path
        && Arc::ptr_eq(&entry.image, watermark_img)
    {
        entry.scaled = Some(((new_wm_w, new_wm_h), Arc::clone(&scaled)));
    }
    scaled
}

fn apply_watermark(
    base_image: &mut DynamicImage,
    watermark_settings: &WatermarkSettings,
) -> Result<(), String> {
    let watermark_img = load_watermark_image(&watermark_settings.path)?;

    let (base_w, base_h) = base_image.dimensions();
    let (new_wm_w, new_wm_h) = export_geom::watermark_scaled_dims(
        base_w,
        base_h,
        watermark_img.width(),
        watermark_img.height(),
        watermark_settings.scale,
    );

    if new_wm_w == 0 || new_wm_h == 0 {
        return Ok(());
    }

    let scaled_watermark =
        scaled_watermark_image(&watermark_settings.path, &watermark_img, new_wm_w, new_wm_h);
    export_geom::composite_watermark(
        base_image,
        &scaled_watermark,
        &watermark_settings.anchor,
        watermark_settings.spacing,
        watermark_settings.opacity,
    );

    Ok(())
}

fn relative_dir_is_safe(rel_dir: &Path) -> bool {
    rel_dir.components().all(|component| {
        matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    })
}

#[cfg(windows)]
fn component_matches(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(not(windows))]
fn component_matches(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left == right
}

fn strip_prefix_preserving_source_case(source_path: &Path, base_path: &Path) -> Option<PathBuf> {
    let source_components: Vec<_> = source_path.components().collect();
    let base_components: Vec<_> = base_path.components().collect();

    if base_components.len() > source_components.len() {
        return None;
    }

    if !source_components
        .iter()
        .zip(base_components.iter())
        .all(|(source, base)| component_matches(*source, *base))
    {
        return None;
    }

    Some(source_components[base_components.len()..].iter().collect())
}

fn relative_export_dir_for_preserved_folders(
    source_path: &Path,
    base_origin_folders: &[String],
) -> Option<PathBuf> {
    base_origin_folders
        .iter()
        .filter_map(|base| {
            let base_path = Path::new(base);
            strip_prefix_preserving_source_case(source_path, base_path)
                .map(|rel_path| (base_path.components().count(), rel_path))
        })
        .max_by_key(|(component_count, _)| *component_count)
        .and_then(|(_, rel_path)| {
            let rel_dir = rel_path.parent().unwrap_or_else(|| Path::new(""));
            if relative_dir_is_safe(rel_dir) {
                Some(rel_dir.to_path_buf())
            } else {
                None
            }
        })
}

fn apply_export_geometry(
    image: DynamicImage,
    export_settings: &ExportSettings,
) -> Result<(DynamicImage, ExportGeometry), String> {
    export_geom::apply_export_geometry(image, export_settings).map_err(|e| e.to_string())
}

fn apply_export_geometry_and_watermark(
    image: DynamicImage,
    export_settings: &ExportSettings,
) -> Result<(DynamicImage, ExportGeometry), String> {
    let (mut image, geometry) = apply_export_geometry(image, export_settings)?;

    if let Some(watermark_settings) = &export_settings.watermark {
        apply_watermark(&mut image, watermark_settings)?;
    }
    Ok((image, geometry))
}

const FALLBACK_EXPORT_MEMORY: u64 = 4 * 1024 * 1024 * 1024;

fn estimate_export_memory(
    source_path: &str,
    is_raw: bool,
    loaded_dimensions: Option<(u32, u32)>,
    mask_count: usize,
) -> u64 {
    let pixels = loaded_dimensions
        .map(|(w, h)| w as u64 * h as u64)
        .or_else(|| crate::memory_budget::image_pixel_count(Path::new(source_path), is_raw));

    let Some(pixels) = pixels else {
        return FALLBACK_EXPORT_MEMORY;
    };

    let bytes_per_pixel = if is_raw { 48 } else { 40 }
        + 6 * mask_count.min(crate::image_processing::MAX_MASKS) as u64;
    pixels * bytes_per_pixel
}

fn ensure_export_not_cancelled(cancellation_token: &AtomicBool) -> Result<(), String> {
    if cancellation_token.load(Ordering::SeqCst) {
        Err("Export cancelled".to_string())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportCancellationRequest {
    Requested,
    AlreadyRequested,
    NoActiveTask,
}

struct ExportTaskGuard {
    task_token: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    cancellation_token: Arc<AtomicBool>,
    app_handle: Option<tauri::AppHandle>,
}

impl ExportTaskGuard {
    fn new(
        task_token: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        cancellation_token: Arc<AtomicBool>,
    ) -> Self {
        Self {
            task_token,
            cancellation_token,
            app_handle: None,
        }
    }

    fn with_app_handle(
        task_token: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        cancellation_token: Arc<AtomicBool>,
        app_handle: tauri::AppHandle,
    ) -> Self {
        let mut guard = Self::new(task_token, cancellation_token);
        guard.app_handle = Some(app_handle);
        guard
    }
}

fn register_export_task(
    task_token: &Mutex<Option<Arc<AtomicBool>>>,
) -> Result<Arc<AtomicBool>, String> {
    let mut active_token = task_token.lock().unwrap();
    if active_token.is_some() {
        return Err("An export is already in progress.".to_string());
    }

    let cancellation_token = Arc::new(AtomicBool::new(false));
    *active_token = Some(Arc::clone(&cancellation_token));
    Ok(cancellation_token)
}

fn request_export_cancellation<F>(
    task_token: &Mutex<Option<Arc<AtomicBool>>>,
    on_requested: F,
) -> ExportCancellationRequest
where
    F: FnOnce(),
{
    let active_token = task_token.lock().unwrap();
    let Some(cancellation_token) = active_token.as_ref() else {
        return ExportCancellationRequest::NoActiveTask;
    };

    if cancellation_token.swap(true, Ordering::SeqCst) {
        ExportCancellationRequest::AlreadyRequested
    } else {
        on_requested();
        ExportCancellationRequest::Requested
    }
}

fn finish_export_task<F>(
    task_token: &Mutex<Option<Arc<AtomicBool>>>,
    cancellation_token: &Arc<AtomicBool>,
    on_finish: F,
) -> bool
where
    F: FnOnce(bool),
{
    let mut active_token = task_token.lock().unwrap();
    let Some(current_token) = active_token.as_ref() else {
        return false;
    };
    if !Arc::ptr_eq(current_token, cancellation_token) {
        return false;
    }

    let cancelled = cancellation_token.load(Ordering::SeqCst);
    *active_token = None;

    on_finish(cancelled);
    true
}

impl Drop for ExportTaskGuard {
    fn drop(&mut self) {
        let app_handle = self.app_handle.clone();
        let _ = finish_export_task(
            &self.task_token,
            &self.cancellation_token,
            |cancelled| match (cancelled, app_handle) {
                (true, Some(app_handle)) => {
                    let _ = app_handle.emit("export-cancelled", ());
                }
                (false, Some(app_handle)) => {
                    let _ = app_handle.emit("export-error", "Export task terminated unexpectedly");
                }
                _ => {}
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn process_image_for_export_pipeline(
    path: &str,
    base_image: &DynamicImage,
    js_adjustments: &Value,
    context: &GpuContext,
    state: &tauri::State<AppState>,
    is_raw: bool,
    debug_tag: &str,
    app_handle: &tauri::AppHandle,
    output_precision: RenderOutputPrecision,
) -> Result<DynamicImage, String> {
    let (transformed_image, unscaled_crop_offset) =
        apply_all_transformations(Cow::Borrowed(base_image), js_adjustments);
    let (img_w, img_h) = transformed_image.dimensions();

    let mask_definitions: Vec<MaskDefinition> = js_adjustments
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();

    let warped_image =
        build_warped_image_for_masks(base_image, is_raw, js_adjustments, &mask_definitions);
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

    let render = resolve_render_options(
        &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
        is_raw,
    );
    let mut all_adjustments =
        get_all_adjustments_from_json(js_adjustments, is_raw, as_shot(path), render);
    all_adjustments.global.show_clipping = 0;

    let lut_path = js_adjustments["lutPath"].as_str();
    let lut = lut_path.and_then(|p| get_or_load_lut(state, p).ok());

    let unique_hash = calculate_full_job_hash(path, js_adjustments);

    process_and_get_dynamic_image_with_precision(
        context,
        state,
        transformed_image.as_ref(),
        unique_hash,
        RenderRequest {
            adjustments: all_adjustments,
            mask_bitmaps: &mask_bitmaps,
            mask_cache_key: None,
            lut,
            roi: None,
        },
        debug_tag,
        output_precision,
    )
    .map_err(|e| e.to_string())
}

fn render_output_precision(
    output_format: &str,
    export_settings: &ExportSettings,
) -> RenderOutputPrecision {
    if matches!(output_format.to_lowercase().as_str(), "tif" | "tiff")
        && export_settings.tiff_bit_depth == TiffBitDepth::Sixteen
    {
        RenderOutputPrecision::SixteenBit
    } else {
        RenderOutputPrecision::EightBit
    }
}

fn set_timestamps_from_exif(src: &Path, dst: &Path) {
    let capture_dt = exif_processing::get_creation_date_from_path(src);

    if capture_dt.timestamp() <= 0 {
        let now = filetime::FileTime::now();
        if let Err(e) = filetime::set_file_times(dst, now, now) {
            log::warn!("Could not set timestamps on '{}': {}", dst.display(), e);
        }
        return;
    }

    let ft = filetime::FileTime::from_unix_time(
        capture_dt.timestamp(),
        capture_dt.timestamp_subsec_nanos(),
    );
    if let Err(e) = filetime::set_file_times(dst, ft, ft) {
        log::warn!("Could not set timestamps on '{}': {}", dst.display(), e);
    }
}

fn save_image_with_metadata(
    image: &DynamicImage,
    output_path: &std::path::Path,
    source_path_str: &str,
    export_xmp: &exif_processing::ExportXmp,
    export_settings: &ExportSettings,
) -> Result<(), String> {
    let extension = output_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mut image_bytes = encode_image_to_bytes(image, &extension, export_settings)?;

    exif_processing::write_image_with_metadata(
        &mut image_bytes,
        source_path_str,
        &extension,
        export_settings.keep_metadata,
        export_settings.strip_gps,
        export_xmp,
        export_settings.dpi,
    )
    .map_err(|e| e.to_string())?;

    fs::write(output_path, image_bytes)
        .map_err(|e| format!("Failed to write file to '{}': {}", output_path.display(), e))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_image_for_export(
    path: &str,
    base_image: &DynamicImage,
    js_adjustments: &Value,
    export_settings: &ExportSettings,
    context: &GpuContext,
    state: &tauri::State<AppState>,
    is_raw: bool,
    app_handle: &tauri::AppHandle,
    output_format: &str,
) -> Result<DynamicImage, String> {
    let processed_image = process_image_for_export_pipeline(
        path,
        base_image,
        js_adjustments,
        context,
        state,
        is_raw,
        "process_image_for_export",
        app_handle,
        render_output_precision(output_format, export_settings),
    )?;

    apply_export_geometry_and_watermark(processed_image, export_settings).map(|(image, _)| image)
}

fn build_single_mask_adjustments(all: &AllAdjustments, mask_index: usize) -> AllAdjustments {
    let mut single = AllAdjustments {
        global: all.global,
        mask_adjustments: all.mask_adjustments,
        mask_count: 1,
        tile_offset_x: all.tile_offset_x,
        tile_offset_y: all.tile_offset_y,
        mask_atlas_cols: all.mask_atlas_cols,
    };
    single.mask_adjustments[0] = all.mask_adjustments[mask_index];
    for i in 1..single.mask_adjustments.len() {
        single.mask_adjustments[i] = Default::default();
    }
    single
}

fn encode_grayscale_to_png(bitmap: &GrayImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    bitmap
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

const SRGB_ICC_PROFILE: &[u8] = include_bytes!("../icc/sRGB-v2-magic.icc");

pub fn encode_image_to_bytes(
    image: &DynamicImage,
    output_format: &str,
    export_settings: &ExportSettings,
) -> Result<Vec<u8>, String> {
    let jpeg_quality = export_settings.jpeg_quality;
    let mut image_bytes = Vec::new();
    let mut cursor = Cursor::new(&mut image_bytes);

    match output_format.to_lowercase().as_str() {
        "jxl" => {
            let (width, height) = image.dimensions();
            let has_alpha = image.color().has_alpha();

            let jxl_data = if jpeg_quality == 100 {
                if has_alpha {
                    let rgba = image.to_rgba8();
                    LosslessConfig::new()
                        .encode(rgba.as_raw(), width, height, PixelLayout::Rgba8)
                        .map_err(|e| format!("Failed to encode lossless JXL: {}", e))?
                } else {
                    let rgb = image.to_rgb8();
                    LosslessConfig::new()
                        .encode(rgb.as_raw(), width, height, PixelLayout::Rgb8)
                        .map_err(|e| format!("Failed to encode lossless JXL: {}", e))?
                }
            } else {
                let jxl_quality = calibrated_jxl_quality(jpeg_quality as f32);
                let distance = quality_to_distance(jxl_quality);

                if has_alpha {
                    let rgba = image.to_rgba8();
                    LossyConfig::new(distance)
                        .encode(rgba.as_raw(), width, height, PixelLayout::Rgba8)
                        .map_err(|e| format!("Failed to encode lossy JXL: {}", e))?
                } else {
                    let rgb = image.to_rgb8();
                    LossyConfig::new(distance)
                        .encode(rgb.as_raw(), width, height, PixelLayout::Rgb8)
                        .map_err(|e| format!("Failed to encode lossy JXL: {}", e))?
                }
            };

            return Ok(jxl_data);
        }
        "webp" => {
            let encoder = webp::Encoder::from_image(image)
                .map_err(|_| "Failed to create WebP encoder".to_string())?;
            let webp_mem = encoder.encode(jpeg_quality as f32);
            return Ok(webp_mem.to_vec());
        }
        "jpg" | "jpeg" => {
            let rgb_image = image
                .as_rgb8()
                .map_or_else(|| Cow::Owned(image.to_rgb8()), Cow::Borrowed);
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, jpeg_quality);
            encoder
                .set_icc_profile(SRGB_ICC_PROFILE.to_vec())
                .map_err(|e| e.to_string())?;
            if let Some(dpi) = export_settings.dpi {
                encoder.set_pixel_density(PixelDensity::dpi(dpi));
            }
            rgb_image
                .write_with_encoder(encoder)
                .map_err(|e| e.to_string())?;
        }
        "png" => {
            let converted;
            let image_to_encode = if image.as_rgb32f().is_some() {
                converted = DynamicImage::ImageRgb16(image.to_rgb16());
                &converted
            } else {
                image
            };

            let mut encoder = PngEncoder::new(&mut cursor);
            encoder
                .set_icc_profile(SRGB_ICC_PROFILE.to_vec())
                .map_err(|e| e.to_string())?;
            image_to_encode
                .write_with_encoder(encoder)
                .map_err(|e| e.to_string())?;
            if let Some(dpi) = export_settings.dpi {
                insert_png_phys_chunk(&mut image_bytes, dpi);
            }
        }
        "tif" | "tiff" => {
            let (width, height) = image.dimensions();
            let mut encoder = TiffEncoder::new(&mut cursor).map_err(|e| e.to_string())?;
            match export_settings.tiff_bit_depth {
                TiffBitDepth::Eight => write_tiff::<colortype::RGB8, _>(
                    &mut encoder,
                    width,
                    height,
                    image
                        .as_rgb8()
                        .map_or_else(|| Cow::Owned(image.to_rgb8()), Cow::Borrowed)
                        .as_raw(),
                    export_settings.dpi,
                )?,
                TiffBitDepth::Sixteen => write_tiff::<colortype::RGB16, _>(
                    &mut encoder,
                    width,
                    height,
                    image
                        .as_rgb16()
                        .map_or_else(|| Cow::Owned(image.to_rgb16()), Cow::Borrowed)
                        .as_raw(),
                    export_settings.dpi,
                )?,
            }
        }
        "avif" => {
            image
                .write_to(&mut cursor, image::ImageFormat::Avif)
                .map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("Unsupported file format: {}", output_format)),
    };
    Ok(image_bytes)
}

fn write_tiff<C: colortype::ColorType, W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    width: u32,
    height: u32,
    data: &[C::Inner],
    dpi: Option<u16>,
) -> Result<(), String>
where
    [C::Inner]: TiffValue,
{
    let mut image = encoder
        .new_image::<C>(width, height)
        .map_err(|e| e.to_string())?;
    if let Some(dpi) = dpi {
        image.resolution(
            ResolutionUnit::Inch,
            Rational {
                n: dpi.into(),
                d: 1,
            },
        );
    }
    image.write_data(data).map_err(|e| e.to_string())
}

fn insert_png_phys_chunk(image_bytes: &mut Vec<u8>, dpi: u16) {
    let pixels_per_meter = (f64::from(dpi) / 0.0254).round() as u32;
    let mut data = Vec::with_capacity(9);
    data.extend_from_slice(&pixels_per_meter.to_be_bytes());
    data.extend_from_slice(&pixels_per_meter.to_be_bytes());
    data.push(1);
    exif_processing::insert_png_chunk(image_bytes, b"pHYs", &data);
}

#[allow(clippy::too_many_arguments)]
fn export_masks_for_image(
    base_image: &DynamicImage,
    js_adjustments: &Value,
    export_settings: &ExportSettings,
    output_path_obj: &std::path::Path,
    source_path_str: &str,
    export_xmp: &exif_processing::ExportXmp,
    context: &Arc<GpuContext>,
    state: &tauri::State<AppState>,
    is_raw: bool,
    app_handle: &tauri::AppHandle,
    cancellation_token: &AtomicBool,
) -> Result<(), String> {
    ensure_export_not_cancelled(cancellation_token)?;
    let (transformed_image, unscaled_crop_offset) =
        apply_all_transformations(Cow::Borrowed(base_image), js_adjustments);
    ensure_export_not_cancelled(cancellation_token)?;
    let (img_w, img_h) = transformed_image.dimensions();
    let mask_definitions: Vec<MaskDefinition> = js_adjustments
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();

    let warped_image =
        build_warped_image_for_masks(base_image, is_raw, js_adjustments, &mask_definitions);
    let mut mask_bitmaps = Vec::with_capacity(mask_definitions.len());
    for definition in &mask_definitions {
        ensure_export_not_cancelled(cancellation_token)?;
        if let Some(bitmap) = generate_mask_bitmap(
            definition,
            img_w,
            img_h,
            1.0,
            unscaled_crop_offset,
            warped_image.as_deref(),
        ) {
            mask_bitmaps.push(bitmap);
        }
        ensure_export_not_cancelled(cancellation_token)?;
    }

    if !mask_bitmaps.is_empty() {
        let render = resolve_render_options(
            &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
            is_raw,
        );
        let all_adjustments =
            get_all_adjustments_from_json(js_adjustments, is_raw, as_shot(source_path_str), render);
        let lut_path = js_adjustments["lutPath"].as_str();
        let lut = lut_path.and_then(|p| get_or_load_lut(state, p).ok());
        let unique_hash = calculate_full_job_hash(source_path_str, js_adjustments);
        let output_dir = output_path_obj.parent().unwrap_or(output_path_obj);
        let stem = output_path_obj
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("export");
        let extension = output_path_obj
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("jpg");

        for (i, _) in mask_bitmaps.iter().enumerate() {
            ensure_export_not_cancelled(cancellation_token)?;
            let single_adjustments = build_single_mask_adjustments(&all_adjustments, i);
            let full_white_mask = ImageBuffer::from_fn(img_w, img_h, |_, _| Luma([255u8]));
            let single_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = vec![full_white_mask];

            let processed = process_and_get_dynamic_image_with_precision(
                context,
                state,
                transformed_image.as_ref(),
                unique_hash,
                RenderRequest {
                    adjustments: single_adjustments,
                    mask_bitmaps: &single_bitmaps,
                    mask_cache_key: None,
                    lut: lut.clone(),
                    roi: None,
                },
                "export_mask_image",
                render_output_precision(extension, export_settings),
            )
            .map_err(|e| e.to_string())?;
            ensure_export_not_cancelled(cancellation_token)?;

            let (with_options, geometry) =
                apply_export_geometry_and_watermark(processed, export_settings)?;

            let alpha_scaled = imageops::resize(
                &mask_bitmaps[i],
                geometry.photo_w,
                geometry.photo_h,
                imageops::FilterType::Lanczos3,
            );

            let alpha_resized: GrayImage = if geometry.is_identity() {
                alpha_scaled
            } else {
                export_geom::place_photo_on_canvas(&alpha_scaled, &geometry, Luma([0u8]), None)
            };
            ensure_export_not_cancelled(cancellation_token)?;

            let mask_image_path =
                output_dir.join(format!("{}_mask_{}_image.{}", stem, i, extension));
            let mask_alpha_path = output_dir.join(format!("{}_mask_{}_alpha.png", stem, i));

            save_image_with_metadata(
                &with_options,
                &mask_image_path,
                source_path_str,
                export_xmp,
                export_settings,
            )?;
            ensure_export_not_cancelled(cancellation_token)?;

            if export_settings.preserve_timestamps {
                set_timestamps_from_exif(Path::new(source_path_str), &mask_image_path);
            }

            let alpha_bytes = encode_grayscale_to_png(&alpha_resized)?;
            ensure_export_not_cancelled(cancellation_token)?;
            fs::write(&mask_alpha_path, alpha_bytes).map_err(|e| e.to_string())?;
            ensure_export_not_cancelled(cancellation_token)?;
        }
    }
    Ok(())
}

fn export_adjustments_as_lut(
    js_adjustments: &Value,
    source_path_str: &str,
    context: &Arc<GpuContext>,
    state: &tauri::State<AppState>,
    app_handle: &tauri::AppHandle,
    cancellation_token: &AtomicBool,
) -> Result<Vec<u8>, String> {
    ensure_export_not_cancelled(cancellation_token)?;
    let lut_size = 33;
    let identity_image = generate_identity_lut_image(lut_size);

    let render = RenderOptions {
        baseline: BaselineOptions::default(),
        ..resolve_render_options(
            &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
            false,
        )
    };
    let mut all_adjustments =
        get_all_adjustments_from_json(js_adjustments, false, as_shot(source_path_str), render);

    all_adjustments.global.show_clipping = 0;
    all_adjustments.global.vignette_amount = 0.0;
    all_adjustments.global.grain_amount = 0.0;
    all_adjustments.global.sharpness = 0.0;
    all_adjustments.global.clarity = 0.0;
    all_adjustments.global.dehaze = 0.0;
    all_adjustments.global.structure = 0.0;
    all_adjustments.global.centré = 0.0;
    all_adjustments.global.glow_amount = 0.0;
    all_adjustments.global.halation_amount = 0.0;
    all_adjustments.global.flare_amount = 0.0;
    all_adjustments.global.luma_noise_reduction = 0.0;
    all_adjustments.global.color_noise_reduction = 0.0;
    all_adjustments.global.chromatic_aberration_red_cyan = 0.0;
    all_adjustments.global.chromatic_aberration_blue_yellow = 0.0;

    let lut_path = js_adjustments["lutPath"].as_str();
    let lut = lut_path.and_then(|p| get_or_load_lut(state, p).ok());
    let unique_hash = calculate_full_job_hash(source_path_str, js_adjustments);

    let processed_lut = process_and_get_dynamic_image(
        context,
        state,
        &identity_image,
        unique_hash,
        RenderRequest {
            adjustments: all_adjustments,
            mask_bitmaps: &[],
            mask_cache_key: None,
            lut,
            roi: None,
        },
        "export_lut",
    )
    .map_err(|e| e.to_string())?;
    ensure_export_not_cancelled(cancellation_token)?;

    let cube_lut = convert_image_to_cube_lut(&processed_lut, lut_size)?;
    ensure_export_not_cancelled(cancellation_token)?;
    Ok(cube_lut)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn export_images_impl(
    paths: Vec<String>,
    output_folder_or_file: String,
    is_explicit_file_path: bool,
    base_origin_folders: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    adjustments_mode: ExportAdjustmentsMode,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    completion_tx: Option<tokio::sync::oneshot::Sender<Result<(), usize>>>,
) -> Result<(), String> {
    let cancellation_token = register_export_task(&state.export_task_token)?;
    *WATERMARK_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let task_guard = ExportTaskGuard::with_app_handle(
        Arc::clone(&state.export_task_token),
        Arc::clone(&cancellation_token),
        app_handle.clone(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    if cancellation_token.load(Ordering::SeqCst) {
        return Ok(());
    }

    let context =
        match get_or_init_gpu_context(&state, &*crate::host::TauriHost::host(app_handle.clone())) {
            Ok(context) => context,
            Err(_) if cancellation_token.load(Ordering::SeqCst) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };

    if cancellation_token.load(Ordering::SeqCst) {
        return Ok(());
    }

    let context = Arc::new(context);
    let progress_counter = Arc::new(AtomicUsize::new(0));

    let available_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let mut sys = sysinfo::System::new();
    sys.refresh_memory();

    let app_settings = load_settings(app_handle.clone()).unwrap_or_default();
    let memory_limit = app_settings.export_memory_limit.unwrap_or(80).clamp(25, 95);

    let available_ram = sys.available_memory();
    let available_ram_gb = available_ram as f64 / 1024.0 / 1024.0 / 1024.0;
    let memory_budget = crate::memory_budget::MemoryBudget::new(
        (available_ram as f64 * memory_limit as f64 / 100.0) as u64,
    );

    let num_threads = if paths.len() == 1 {
        1
    } else {
        match app_settings.export_worker_threads {
            Some(threads) if threads > 0 => (threads as usize).clamp(1, 8),
            _ => available_cores.clamp(1, 4),
        }
    };

    log::info!(
        "Batch Export: {} cores, {:.1} GB free RAM -> up to {} threads",
        available_cores,
        available_ram_gb,
        num_threads
    );

    let _export_task = tokio::spawn(async move {
        let _watermark_cache_guard = WatermarkCacheGuard;
        let _task_guard = task_guard;
        let output_folder_path = std::path::Path::new(&output_folder_or_file);
        let total_paths = paths.len();
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let mut base_path_counts: HashMap<String, usize> = HashMap::new();
        let mut export_items = Vec::with_capacity(total_paths);

        for (i, path_str) in paths.into_iter().enumerate() {
            let (source_path, _) = parse_virtual_path(&path_str);
            let source_str = source_path.to_string_lossy().to_string();
            let count = base_path_counts.entry(source_str.clone()).or_insert(0);
            *count += 1;

            let mut explicit_vc = None;
            if let Some(idx) = path_str.rfind("vc=") {
                let id_str = path_str[idx + 3..].split('&').next().unwrap_or("");
                if let Ok(id) = id_str.parse::<u32>() {
                    explicit_vc = Some(id);
                }
            }
            if explicit_vc.is_none() {
                let lower = path_str.to_lowercase();
                if let Some(idx) = lower.rfind("_vc") {
                    let id_str: String = lower[idx + 3..]
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if let Ok(id) = id_str.parse::<u32>() {
                        explicit_vc = Some(id);
                    }
                }
            }
            export_items.push((i, path_str, *count, explicit_vc));
        }

        let used_paths = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let semaphore = Arc::new(tokio::sync::Semaphore::new(num_threads));
        let mut join_handles = Vec::new();

        for (global_index, image_path_str, appearance_count, explicit_vc) in export_items {
            if cancellation_token.load(Ordering::SeqCst) {
                break;
            }
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            if cancellation_token.load(Ordering::SeqCst) {
                drop(permit);
                break;
            }

            let app_handle_clone = app_handle.clone();
            let context_clone = Arc::clone(&context);
            let progress_counter_clone = Arc::clone(&progress_counter);
            let output_folder_path = output_folder_path.to_path_buf();
            let base_origin_folders = base_origin_folders.clone();
            let export_settings = export_settings.clone();
            let output_format = output_format.clone();
            let settings = settings.clone();
            let cancellation_token_clone = Arc::clone(&cancellation_token);
            let adjustments_mode = adjustments_mode.clone();
            let used_paths_clone = Arc::clone(&used_paths);
            let memory_budget = Arc::clone(&memory_budget);

            let handle = tokio::task::spawn_blocking(move || {
                ensure_export_not_cancelled(&cancellation_token_clone)?;

                let state = app_handle_clone.state::<AppState>();
                let (source_path, sidecar_path) = parse_virtual_path(&image_path_str);
                let source_path_str = source_path.to_string_lossy().to_string();

                let is_current_edit = match &adjustments_mode {
                    ExportAdjustmentsMode::UseSidecars { active_path, .. } => {
                        Some(&source_path_str) == active_path.as_ref()
                    }
                    ExportAdjustmentsMode::GlobalOverride(_) => false,
                };

                let sidecar = crate::exif_processing::load_sidecar(&sidecar_path);
                let export_xmp = crate::exif_processing::ExportXmp::from_metadata(&sidecar);

                let mut js_adjustments = match &adjustments_mode {
                    ExportAdjustmentsMode::UseSidecars {
                        active_adjustments, ..
                    } => {
                        if is_current_edit {
                            if let Some(adj) = active_adjustments {
                                adj.clone()
                            } else {
                                sidecar.adjustments
                            }
                        } else {
                            sidecar.adjustments
                        }
                    }
                    ExportAdjustmentsMode::GlobalOverride(adj) => adj.clone(),
                };

                hydrate_adjustments(&state, &mut js_adjustments);
                let is_raw = is_raw_file(&source_path_str);
                let original_path = std::path::Path::new(&source_path_str);
                let file_date = exif_processing::get_creation_date_from_path(original_path);

                let filename_template = export_settings
                    .filename_template
                    .as_deref()
                    .unwrap_or("{original_filename}_edited");

                let mut new_stem = generate_filename_from_template(
                    filename_template,
                    original_path,
                    global_index + 1,
                    total_paths,
                    &file_date,
                );

                if let Some(vc_id) = explicit_vc {
                    new_stem = format!("{}_VC{:02}", new_stem, vc_id);
                } else if appearance_count > 1 {
                    new_stem = format!("{}_VC{:02}", new_stem, appearance_count - 1);
                }

                let new_filename = format!("{}.{}", new_stem, output_format);

                let mut output_path =
                    if export_settings.destination_type.as_deref() == Some("originalFolder") {
                        let mut dir = source_path
                            .parent()
                            .unwrap_or(std::path::Path::new(""))
                            .to_path_buf();
                        if let Some(sub) = &export_settings.subfolder {
                            let mut trimmed = sub.trim();

                            while trimmed.starts_with('/') || trimmed.starts_with('\\') {
                                trimmed = &trimmed[1..];
                            }

                            if !trimmed.is_empty() {
                                dir = dir.join(trimmed);
                            }
                        }

                        if let Err(e) = std::fs::create_dir_all(&dir) {
                            return Err(format!(
                                "Failed to create export subdirectory '{}': {}",
                                dir.display(),
                                e
                            ));
                        }

                        dir.join(&new_filename)
                    } else if is_explicit_file_path && total_paths == 1 {
                        output_folder_path.clone()
                    } else if export_settings.preserve_folders {
                        if let Some(rel_dir) = relative_export_dir_for_preserved_folders(
                            source_path.as_path(),
                            &base_origin_folders,
                        ) {
                            let full_dir = output_folder_path.join(rel_dir);
                            if let Err(e) = std::fs::create_dir_all(&full_dir) {
                                return Err(format!(
                                    "Failed to create export subdirectory '{}': {}",
                                    full_dir.display(),
                                    e
                                ));
                            }
                            full_dir.join(&new_filename)
                        } else {
                            output_folder_path.join(&new_filename)
                        }
                    } else {
                        output_folder_path.join(&new_filename)
                    };

                let extension = output_format.to_lowercase();

                if !(is_explicit_file_path && total_paths == 1) {
                    let mut used = used_paths_clone.lock().unwrap();
                    let parent_dir = output_path
                        .parent()
                        .unwrap_or(std::path::Path::new(""))
                        .to_path_buf();
                    let mut counter = 1;

                    while output_path.exists() || used.contains(&output_path) {
                        let incremented_filename =
                            format!("{}_{}.{}", new_stem, counter, extension);
                        output_path = parent_dir.join(&incremented_filename);
                        counter += 1;
                    }
                    used.insert(output_path.clone());
                }

                let result: Result<(), String> = (|| {
                    if extension == "cube" {
                        let cube_bytes = export_adjustments_as_lut(
                            &js_adjustments,
                            &source_path_str,
                            &context_clone,
                            &state,
                            &app_handle_clone,
                            &cancellation_token_clone,
                        )?;
                        ensure_export_not_cancelled(&cancellation_token_clone)?;
                        fs::write(&output_path, cube_bytes).map_err(|e| e.to_string())?;
                        ensure_export_not_cancelled(&cancellation_token_clone)?;
                        return Ok(());
                    }

                    let loaded_image = if is_current_edit {
                        state
                            .original_image
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone()
                            .filter(|loaded| {
                                parse_virtual_path(&loaded.path).0
                                    == parse_virtual_path(&source_path_str).0
                            })
                    } else {
                        None
                    };

                    let estimated_memory = estimate_export_memory(
                        &source_path_str,
                        is_raw,
                        loaded_image
                            .as_ref()
                            .map(|loaded| (loaded.image.width(), loaded.image.height())),
                        js_adjustments["masks"].as_array().map_or(0, |m| m.len()),
                    );
                    let Some(_export_reservation) =
                        memory_budget.reserve(estimated_memory, Some(&cancellation_token_clone))
                    else {
                        return Err("Export cancelled".to_string());
                    };
                    let Some(_app_reservation) = state
                        .memory_budget
                        .reserve(estimated_memory, Some(&cancellation_token_clone))
                    else {
                        return Err("Export cancelled".to_string());
                    };

                    let cached_patched = loaded_image.as_ref().and_then(|loaded_image| {
                        crate::cached_patched_image(&state, &loaded_image.image, &js_adjustments)
                    });

                    let base_image: Cow<DynamicImage> = if let Some(patched) = &cached_patched {
                        Cow::Borrowed(patched)
                    } else if let Some(loaded_image) = &loaded_image {
                        prepare_base_image_cow(
                            &loaded_image.image,
                            &loaded_image.path,
                            &js_adjustments,
                        )
                        .map_err(|e| format!("Failed to composite AI patches: {}", e))?
                    } else {
                        Cow::Owned(match read_file_mapped(Path::new(&source_path_str)) {
                            Ok(mmap) => load_and_composite(
                                &mmap,
                                &source_path_str,
                                &js_adjustments,
                                false,
                                &settings,
                                None,
                            )
                            .map_err(|e| format!("Failed to load from mmap: {}", e))?,
                            Err(_) => {
                                let bytes =
                                    fs::read(&source_path_str).map_err(|e| e.to_string())?;
                                load_and_composite(
                                    &bytes,
                                    &source_path_str,
                                    &js_adjustments,
                                    false,
                                    &settings,
                                    None,
                                )
                                .map_err(|e| format!("Failed to load from bytes: {}", e))?
                            }
                        })
                    };
                    let mut base_image = Some(base_image);
                    ensure_export_not_cancelled(&cancellation_token_clone)?;

                    let mut main_export_adjustments = js_adjustments.clone();
                    if export_settings.export_masks
                        && let Some(obj) = main_export_adjustments.as_object_mut()
                    {
                        obj.insert("masks".to_string(), serde_json::json!([]));
                    }

                    let actual_output_format = output_path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .unwrap_or(output_format.as_str());

                    let final_image = process_image_for_export(
                        &source_path_str,
                        base_image.as_ref().unwrap(),
                        &main_export_adjustments,
                        &export_settings,
                        &context_clone,
                        &state,
                        is_raw,
                        &app_handle_clone,
                        actual_output_format,
                    )?;
                    if !export_settings.export_masks {
                        base_image = None;
                    }
                    ensure_export_not_cancelled(&cancellation_token_clone)?;
                    save_image_with_metadata(
                        &final_image,
                        &output_path,
                        &source_path_str,
                        &export_xmp,
                        &export_settings,
                    )?;
                    ensure_export_not_cancelled(&cancellation_token_clone)?;

                    if export_settings.preserve_timestamps {
                        set_timestamps_from_exif(Path::new(&source_path_str), &output_path);
                    }
                    ensure_export_not_cancelled(&cancellation_token_clone)?;

                    if export_settings.export_masks
                        && let Some(base_image) = &base_image
                    {
                        export_masks_for_image(
                            base_image,
                            &js_adjustments,
                            &export_settings,
                            &output_path,
                            &source_path_str,
                            &export_xmp,
                            &context_clone,
                            &state,
                            is_raw,
                            &app_handle_clone,
                            &cancellation_token_clone,
                        )?;
                    }

                    Ok(())
                })();

                if !cancellation_token_clone.load(Ordering::SeqCst) {
                    let current_progress =
                        progress_counter_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = app_handle_clone.emit(
                        "batch-export-progress",
                        serde_json::json!({
                            "current": current_progress,
                            "total": total_paths,
                            "path": &image_path_str
                        }),
                    );
                }

                drop(permit);
                if cancellation_token_clone.load(Ordering::SeqCst) {
                    Err("Export cancelled".to_string())
                } else {
                    result
                }
            });

            join_handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in join_handles {
            match handle.await {
                Ok(res) => results.push(res),
                Err(e) => results.push(Err(format!("Thread crashed: {}", e))),
            }
        }

        let errors: Vec<String> = results.into_iter().filter_map(Result::err).collect();
        let error_count = errors.len();
        let export_state = app_handle.state::<AppState>();
        let finalized = finish_export_task(
            &export_state.export_task_token,
            &cancellation_token,
            |cancelled| {
                if cancelled {
                    log::info!("Batch export cancelled and worker cleanup completed");
                    let _ = app_handle.emit("export-cancelled", ());
                    return;
                }

                for error in &errors {
                    log::error!("Export error: {}", error);
                    if total_paths == 1 {
                        let _ = app_handle.emit("export-error", error.clone());
                    }
                }

                if error_count > 0 && total_paths > 1 {
                    let _ = app_handle.emit(
                        "export-error",
                        format!("{error_count} of {total_paths} exports failed"),
                    );
                } else if error_count == 0 {
                    let _ = app_handle.emit(
                        "batch-export-progress",
                        serde_json::json!({ "current": total_paths, "total": total_paths, "path": "" }),
                    );
                    let _ = app_handle.emit("export-complete", ());
                }
            },
        );

        if !finalized {
            log::warn!("Ignoring terminal events from a stale export task");
        }

        if let Some(tx) = completion_tx {
            if error_count > 0 {
                let _ = tx.send(Err(error_count));
            } else {
                let _ = tx.send(Ok(()));
            }
        }
    });

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_images(
    paths: Vec<String>,
    output_folder_or_file: String,
    is_explicit_file_path: bool,
    base_origin_folders: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    current_edit_path: Option<String>,
    current_edit_adjustments: Option<Value>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    export_images_impl(
        paths,
        output_folder_or_file,
        is_explicit_file_path,
        base_origin_folders,
        export_settings,
        output_format,
        ExportAdjustmentsMode::UseSidecars {
            active_path: current_edit_path,
            active_adjustments: current_edit_adjustments,
        },
        state,
        app_handle,
        None,
    )
    .await
}

pub async fn run_headless_export(
    session: crate::launch_request::HeadlessExportSession,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    cli_println!("Starting headless export...");
    let state = app_handle.state::<crate::AppState>();

    let source_path = std::path::Path::new(&session.source);
    if !source_path.exists() {
        return Err(format!("Source path does not exist: {}", session.source));
    }

    let mut paths = Vec::new();
    if source_path.is_dir() {
        paths = crate::file_management::list_supported_images_recursive(session.source.clone());
    } else {
        paths.push(session.source.clone());
    }

    if paths.is_empty() {
        return Err("No supported images found at the source path.".to_string());
    }

    let output_path = std::path::Path::new(&session.output);
    let is_explicit_file_path =
        paths.len() == 1 && output_path.extension().is_some() && !output_path.is_dir();

    if is_explicit_file_path {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create output parent directory: {}", e))?;
        }
    } else {
        std::fs::create_dir_all(&session.output)
            .map_err(|e| format!("Failed to create output directory: {}", e))?;
    }

    cli_println!("Found {} images to export. Processing...", paths.len());

    let export_settings = ExportSettings {
        jpeg_quality: session.quality,
        tiff_bit_depth: session.tiff_bit_depth,
        resize: None,
        border: None,
        pad: None,
        keep_metadata: session.keep_metadata,
        preserve_timestamps: true,
        strip_gps: false,
        filename_template: None,
        watermark: None,
        export_masks: false,
        preserve_folders: true,
        destination_type: None,
        subfolder: None,
        dpi: None,
    };

    let mut custom_adjustments = None;
    if let Some(adj_path) = &session.adjustments_override {
        let content = std::fs::read_to_string(adj_path)
            .map_err(|e| format!("Failed to read adjustments file: {}", e))?;
        let json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse adjustments JSON: {}", e))?;
        custom_adjustments = Some(json);
        cli_println!(
            "Loaded custom adjustments to override sidecars from: {}",
            adj_path
        );
    }

    let (tx, rx) = tokio::sync::oneshot::channel();

    let mode = if let Some(adj) = custom_adjustments {
        ExportAdjustmentsMode::GlobalOverride(adj)
    } else {
        ExportAdjustmentsMode::UseSidecars {
            active_path: None,
            active_adjustments: None,
        }
    };

    export_images_impl(
        paths,
        session.output,
        is_explicit_file_path,
        vec![session.source],
        export_settings,
        session.format,
        mode,
        state.clone(),
        app_handle.clone(),
        Some(tx),
    )
    .await?;

    match rx.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(errors)) => Err(format!("Export completed with {} errors.", errors)),
        Err(_) => Err("Export task panicked or was cancelled.".to_string()),
    }
}

#[tauri::command]
pub fn cancel_export(
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    match request_export_cancellation(&state.export_task_token, || {
        let _ = app_handle.emit("export-cancelling", ());
    }) {
        ExportCancellationRequest::Requested => {
            log::info!("Export cancellation requested; workers will stop at the next checkpoint");
        }
        ExportCancellationRequest::AlreadyRequested => {
            log::info!("Export cancellation was already requested");
        }
        ExportCancellationRequest::NoActiveTask => {
            return Err("No export task is currently running.".to_string());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn estimate_export_sizes(
    paths: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    current_edit_path: Option<String>,
    current_edit_adjustments: Option<Value>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<usize, String> {
    if output_format.to_lowercase() == "cube" {
        return Ok(1_050_000 * paths.len());
    }

    if paths.is_empty() {
        return Ok(0);
    }

    let first_path = &paths[0];
    let (source_path, sidecar_path) = parse_virtual_path(first_path);
    let source_path_str = source_path.to_string_lossy().to_string();

    let context =
        get_or_init_gpu_context(&state, &*crate::host::TauriHost::host(app_handle.clone()))
            .map_err(|e| e.to_string())?;
    let is_current_edit = Some(&source_path_str) == current_edit_path.as_ref();
    let is_raw = is_raw_file(&source_path_str);
    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let single_image_extrapolated_size: usize = if is_current_edit
        && current_edit_adjustments.is_some()
    {
        let loaded_image = state
            .original_image
            .lock()
            .unwrap()
            .clone()
            .ok_or("No original image loaded")?;
        let mut adjustments_clone = current_edit_adjustments.clone().unwrap();
        hydrate_adjustments(&state, &mut adjustments_clone);

        let new_transform_hash = calculate_transform_hash(&adjustments_clone);
        let cached_preview_lock = state.cached_preview.lock().unwrap();
        let preview_dim = settings.editor_preview_resolution.unwrap_or(1920);

        let (preview_image, scale, unscaled_crop_offset) = if let Some(cached) =
            &*cached_preview_lock
        {
            if cached.transform_hash == new_transform_hash && cached.preview_dim == preview_dim {
                let img = Arc::clone(&cached.image);
                let s = cached.scale;
                let offset = cached.unscaled_crop_offset;
                drop(cached_preview_lock);
                (img, s, offset)
            } else {
                drop(cached_preview_lock);
                generate_transformed_preview(
                    &state,
                    &loaded_image,
                    &adjustments_clone,
                    preview_dim,
                )?
            }
        } else {
            drop(cached_preview_lock);
            generate_transformed_preview(&state, &loaded_image, &adjustments_clone, preview_dim)
                .map_err(|e| e.to_string())?
        };

        let (img_w, img_h) = preview_image.dimensions();
        let mask_definitions: Vec<MaskDefinition> = adjustments_clone
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let scaled_crop_offset = (
            unscaled_crop_offset.0 * scale,
            unscaled_crop_offset.1 * scale,
        );

        let mask_bitmaps = get_cached_or_generate_masks(
            &state,
            &loaded_image.path,
            &mask_definitions,
            img_w,
            img_h,
            scale,
            scaled_crop_offset,
            &adjustments_clone,
        )
        .bitmaps;

        let render = resolve_render_options(
            &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
            is_raw,
        );
        let mut all_adjustments =
            get_all_adjustments_from_json(&adjustments_clone, is_raw, loaded_image.as_shot, render);
        all_adjustments.global.show_clipping = 0;

        let lut = adjustments_clone["lutPath"]
            .as_str()
            .and_then(|p| get_or_load_lut(&state, p).ok());
        let unique_hash =
            calculate_full_job_hash(&loaded_image.path, &adjustments_clone).wrapping_add(1);

        let processed_preview = process_and_get_dynamic_image_with_precision(
            &context,
            &state,
            &preview_image,
            unique_hash,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                mask_cache_key: None,
                lut,
                roi: None,
            },
            "estimate_export_size",
            render_output_precision(&output_format, &export_settings),
        )
        .map_err(|e| e.to_string())?;

        let preview_bytes =
            encode_image_to_bytes(&processed_preview, &output_format, &export_settings)?;
        let preview_byte_size = preview_bytes.len();

        let (full_w, full_h) = crate::adjustment_utils::transformed_dimensions(
            loaded_image.image.width(),
            loaded_image.image.height(),
            &adjustments_clone,
        );

        let (final_full_w, final_full_h) =
            export_geom::compute_export_output_size(full_w, full_h, &export_settings);

        let (processed_preview_w, processed_preview_h) = processed_preview.dimensions();
        let pixel_ratio = if processed_preview_w > 0 && processed_preview_h > 0 {
            (final_full_w as f64 * final_full_h as f64)
                / (processed_preview_w as f64 * processed_preview_h as f64)
        } else {
            1.0
        };

        (preview_byte_size as f64 * pixel_ratio) as usize
    } else {
        let metadata = crate::exif_processing::load_sidecar(&sidecar_path);
        let mut js_adjustments = metadata.adjustments;

        const ESTIMATE_DIM: u32 = 1280;

        let file_slice: Vec<u8>;
        let mmap_guard;
        let file_data: &[u8] = match read_file_mapped(Path::new(&source_path_str)) {
            Ok(mmap) => {
                mmap_guard = Some(mmap);
                mmap_guard.as_ref().unwrap()
            }
            Err(_) => {
                file_slice = fs::read(&source_path_str).map_err(|io_err| io_err.to_string())?;
                &file_slice
            }
        };

        let original_image =
            load_base_image_from_bytes(file_data, &source_path_str, true, &settings, None)
                .map_err(|e| e.to_string())?;

        let raw_scale_factor = if is_raw {
            crate::raw_processing::get_fast_demosaic_scale_factor(
                file_data,
                original_image.width(),
                original_image.height(),
            )
        } else {
            1.0
        };

        if let Some(crop_val) = js_adjustments.get_mut("crop")
            && let Ok(c) = serde_json::from_value::<Crop>(crop_val.clone())
        {
            *crop_val = serde_json::to_value(Crop {
                x: c.x * raw_scale_factor as f64,
                y: c.y * raw_scale_factor as f64,
                width: c.width * raw_scale_factor as f64,
                height: c.height * raw_scale_factor as f64,
            })
            .unwrap_or(serde_json::Value::Null);
        }

        let (transformed_shrunk_res, unscaled_crop_offset) =
            apply_all_transformations(Cow::Borrowed(&original_image), &js_adjustments);
        let (shrunk_w, shrunk_h) = transformed_shrunk_res.dimensions();

        let preview_base = if shrunk_w > ESTIMATE_DIM || shrunk_h > ESTIMATE_DIM {
            downscale_f32_image(transformed_shrunk_res.as_ref(), ESTIMATE_DIM, ESTIMATE_DIM)
        } else {
            transformed_shrunk_res.into_owned()
        };

        let (preview_w, preview_h) = preview_base.dimensions();
        let gpu_scale = if shrunk_w > 0 {
            preview_w as f32 / shrunk_w as f32
        } else {
            1.0
        };
        let total_scale = gpu_scale * raw_scale_factor;

        let mask_definitions: Vec<MaskDefinition> = js_adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();
        let scaled_crop_offset = (
            unscaled_crop_offset.0 * gpu_scale,
            unscaled_crop_offset.1 * gpu_scale,
        );

        let mask_bitmaps = get_cached_or_generate_masks(
            &state,
            &source_path_str,
            &mask_definitions,
            preview_w,
            preview_h,
            total_scale,
            scaled_crop_offset,
            &js_adjustments,
        )
        .bitmaps;

        let render = resolve_render_options(
            &crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default(),
            is_raw,
        );
        let mut all_adjustments = get_all_adjustments_from_json(
            &js_adjustments,
            is_raw,
            as_shot(&source_path_str),
            render,
        );
        all_adjustments.global.show_clipping = 0;

        let lut = js_adjustments["lutPath"]
            .as_str()
            .and_then(|p| get_or_load_lut(&state, p).ok());
        let unique_hash =
            calculate_full_job_hash(&source_path_str, &js_adjustments).wrapping_add(1);

        let processed_preview = process_and_get_dynamic_image_with_precision(
            &context,
            &state,
            &preview_base,
            unique_hash,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                mask_cache_key: None,
                lut,
                roi: None,
            },
            "estimate_batch_export_size",
            render_output_precision(&output_format, &export_settings),
        )
        .map_err(|e| e.to_string())?;

        let preview_bytes =
            encode_image_to_bytes(&processed_preview, &output_format, &export_settings)?;
        let single_image_estimated_size = preview_bytes.len();

        let full_w = (shrunk_w as f32 / raw_scale_factor).round() as u32;
        let full_h = (shrunk_h as f32 / raw_scale_factor).round() as u32;

        let (final_full_w, final_full_h) =
            export_geom::compute_export_output_size(full_w, full_h, &export_settings);

        let (processed_preview_w, processed_preview_h) = processed_preview.dimensions();
        let pixel_ratio = if processed_preview_w > 0 && processed_preview_h > 0 {
            (final_full_w as f64 * final_full_h as f64)
                / (processed_preview_w as f64 * processed_preview_h as f64)
        } else {
            1.0
        };

        (single_image_estimated_size as f64 * pixel_ratio) as usize
    };

    Ok(single_image_extrapolated_size * paths.len())
}
