use std::borrow::Cow;
use std::fs;
use std::io::{Cursor, Seek, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

pub use engine::launch_request::TiffBitDepth;

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    pub filename_template: Option<String>,
    pub jpeg_quality: u8,
    pub tiff_bit_depth: TiffBitDepth,
    pub keep_metadata: bool,
    pub preserve_timestamps: bool,
    pub export_masks: bool,
    pub strip_gps: bool,
    pub destination_type: Option<String>,
    pub subfolder: Option<String>,
    pub dpi: Option<u16>,
}

#[derive(Clone)]
pub(crate) struct CurrentEditAdjustments {
    pub active_path: Option<String>,
    pub active_adjustments: Option<Value>,
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
    task_token: Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>,
    cancellation_token: Arc<AtomicBool>,
    app_handle: Option<tauri::AppHandle>,
}

impl ExportTaskGuard {
    fn with_app_handle(
        task_token: Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>,
        cancellation_token: Arc<AtomicBool>,
        app_handle: tauri::AppHandle,
    ) -> Self {
        Self {
            task_token,
            cancellation_token,
            app_handle: Some(app_handle),
        }
    }
}

fn register_export_task(
    task_token: &std::sync::Mutex<Option<Arc<AtomicBool>>>,
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
    task_token: &std::sync::Mutex<Option<Arc<AtomicBool>>>,
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
    task_token: &std::sync::Mutex<Option<Arc<AtomicBool>>>,
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
    process_image_for_export_pipeline(
        path,
        base_image,
        js_adjustments,
        context,
        state,
        is_raw,
        "process_image_for_export",
        app_handle,
        render_output_precision(output_format, export_settings),
    )
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

        for (i, mask_bitmap) in mask_bitmaps.iter().enumerate() {
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

            let (final_w, final_h) = processed.dimensions();
            let alpha_resized: GrayImage =
                if (mask_bitmap.width(), mask_bitmap.height()) == (final_w, final_h) {
                    mask_bitmap.clone()
                } else {
                    imageops::resize(
                        mask_bitmap,
                        final_w,
                        final_h,
                        imageops::FilterType::Lanczos3,
                    )
                };
            ensure_export_not_cancelled(cancellation_token)?;

            let mask_image_path =
                output_dir.join(format!("{}_mask_{}_image.{}", stem, i, extension));
            let mask_alpha_path = output_dir.join(format!("{}_mask_{}_alpha.png", stem, i));

            save_image_with_metadata(
                &processed,
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

fn estimate_export_memory(
    source_path: &str,
    is_raw: bool,
    loaded_dimensions: Option<(u32, u32)>,
    mask_count: usize,
) -> u64 {
    const FALLBACK_EXPORT_MEMORY: u64 = 4 * 1024 * 1024 * 1024;

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
pub(crate) async fn export_images_impl(
    paths: Vec<String>,
    output_folder_or_file: String,
    is_explicit_file_path: bool,
    export_settings: ExportSettings,
    output_format: String,
    adjustments_mode: CurrentEditAdjustments,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    completion_tx: Option<tokio::sync::oneshot::Sender<Result<(), usize>>>,
) -> Result<(), String> {
    let cancellation_token = register_export_task(&state.export_task_token)?;
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

    let _export_task = tokio::spawn(async move {
        let _task_guard = task_guard;
        let output_folder_path = std::path::Path::new(&output_folder_or_file);
        let total_paths = paths.len();
        let progress_counter = Arc::new(AtomicUsize::new(0));
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let mut export_errors: Vec<String> = Vec::new();

        for (global_index, image_path_str) in paths.into_iter().enumerate() {
            if cancellation_token.load(Ordering::SeqCst) {
                break;
            }

            let app_handle_clone = app_handle.clone();
            let context_clone = Arc::clone(&context);
            let output_folder_path = output_folder_path.to_path_buf();
            let export_settings = export_settings.clone();
            let output_format = output_format.clone();
            let cancellation_token_clone = Arc::clone(&cancellation_token);
            let adjustments_mode = adjustments_mode.clone();
            let progress_counter_clone = Arc::clone(&progress_counter);
            let settings = settings.clone();
            let memory_budget = crate::memory_budget::MemoryBudget::new(u64::MAX);

            let result = tokio::task::spawn_blocking(move || {
                ensure_export_not_cancelled(&cancellation_token_clone)?;

                let state = app_handle_clone.state::<AppState>();
                let (source_path, sidecar_path) = parse_virtual_path(&image_path_str);
                let source_path_str = source_path.to_string_lossy().to_string();

                let active_path = &adjustments_mode.active_path;
                let is_current_edit = Some(&source_path_str) == active_path.as_ref();

                let sidecar = crate::exif_processing::load_sidecar(&sidecar_path);
                let export_xmp = crate::exif_processing::ExportXmp::from_metadata(&sidecar);

                let mut js_adjustments = if is_current_edit {
                    adjustments_mode
                        .active_adjustments
                        .clone()
                        .unwrap_or(sidecar.adjustments)
                } else {
                    sidecar.adjustments
                };

                hydrate_adjustments(&state, &mut js_adjustments);
                let is_raw = is_raw_file(&source_path_str);
                let original_path = std::path::Path::new(&source_path_str);
                let file_date = exif_processing::get_creation_date_from_path(original_path);

                let filename_template = export_settings
                    .filename_template
                    .as_deref()
                    .unwrap_or("{original_filename}_edited");

                let new_stem = generate_filename_from_template(
                    filename_template,
                    original_path,
                    global_index + 1,
                    total_paths,
                    &file_date,
                );

                let new_filename = format!("{}.{}", new_stem, output_format);

                let mut output_path =
                    if export_settings.destination_type.as_deref() == Some("originalFolder") {
                        let mut dir = source_path
                            .parent()
                            .unwrap_or(std::path::Path::new(""))
                            .to_path_buf();
                        if let Some(sub) = &export_settings.subfolder {
                            let trimmed = sub.trim().trim_start_matches(['/', '\\']);
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
                    } else {
                        output_folder_path.join(&new_filename)
                    };

                let extension = output_format.to_lowercase();

                if !(is_explicit_file_path && total_paths == 1) {
                    let parent_dir = output_path
                        .parent()
                        .unwrap_or(std::path::Path::new(""))
                        .to_path_buf();
                    let mut counter = 1;

                    while output_path.exists() {
                        let incremented_filename =
                            format!("{}_{}.{}", new_stem, counter, extension);
                        output_path = parent_dir.join(&incremented_filename);
                        counter += 1;
                    }
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
                    let Some(_app_reservation) = state
                        .memory_budget
                        .reserve(estimated_memory, Some(&cancellation_token_clone))
                    else {
                        return Err("Export cancelled".to_string());
                    };
                    let Some(_export_reservation) =
                        memory_budget.reserve(estimated_memory, Some(&cancellation_token_clone))
                    else {
                        return Err("Export cancelled".to_string());
                    };

                    let base_image: Cow<DynamicImage> = if let Some(loaded_image) = &loaded_image {
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
                        base_image.as_ref(),
                        &main_export_adjustments,
                        &export_settings,
                        &context_clone,
                        &state,
                        is_raw,
                        &app_handle_clone,
                        actual_output_format,
                    )?;
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

                    if export_settings.export_masks {
                        export_masks_for_image(
                            base_image.as_ref(),
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

                ensure_export_not_cancelled(&cancellation_token_clone)?;

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

                result
            })
            .await;

            match result {
                Ok(res) => {
                    if let Err(e) = res {
                        export_errors.push(e);
                    }
                }
                Err(e) => export_errors.push(format!("Export worker failed: {}", e)),
            }
        }

        let error_count = export_errors.len();
        let finalized = finish_export_task(
            &app_handle.state::<AppState>().export_task_token,
            &cancellation_token,
            |cancelled| {
                if cancelled {
                    log::info!("Export cancelled and worker cleanup completed");
                    let _ = app_handle.emit("export-cancelled", ());
                    return;
                }

                for error in &export_errors {
                    log::error!("Export error: {}", error);
                    let _ = app_handle.emit("export-error", error.clone());
                }

                if error_count == 0 {
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

#[tauri::command]
pub async fn export_images(
    paths: Vec<String>,
    output_folder_or_file: String,
    is_explicit_file_path: bool,
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
        export_settings,
        output_format,
        CurrentEditAdjustments {
            active_path: current_edit_path,
            active_adjustments: current_edit_adjustments,
        },
        state,
        app_handle,
        None,
    )
    .await
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

        let (processed_preview_w, processed_preview_h) = processed_preview.dimensions();
        let pixel_ratio = if processed_preview_w > 0 && processed_preview_h > 0 {
            (full_w as f64 * full_h as f64)
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

        let (processed_preview_w, processed_preview_h) = processed_preview.dimensions();
        let pixel_ratio = if processed_preview_w > 0 && processed_preview_h > 0 {
            (full_w as f64 * full_h as f64)
                / (processed_preview_w as f64 * processed_preview_h as f64)
        } else {
            1.0
        };

        (single_image_estimated_size as f64 * pixel_ratio) as usize
    };

    Ok(single_image_extrapolated_size * paths.len())
}
