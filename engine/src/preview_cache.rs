use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use image::{DynamicImage, GenericImageView};

use crate::app_state::{AppState, LoadedImage};
use crate::image_loader::prepare_base_image_cow;
use crate::cache_utils::{calculate_transform_hash, calculate_geometry_hash};
use crate::image_processing::{Crop, downscale_f32_image, get_geometry_params_from_json, is_geometry_identity, apply_geometry_warp};
use std::borrow::Cow;

use crate::mask_generation::build_full_warped_image_rgb8;
use crate::paths::parse_virtual_path;
pub type TransformedPreview = (Arc<DynamicImage>, f32, (f32, f32));

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
        (target_scale * 2.0).min(1.0)
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

pub fn build_ai_source_image(
    image: &DynamicImage,
    is_raw: bool,
    js_adjustments: &serde_json::Value,
) -> DynamicImage {
    let source =
        crate::negative_conversion::apply_negative_conversion(Cow::Borrowed(image), js_adjustments);
    build_full_warped_image_rgb8(
        &source,
        crate::negative_conversion::effective_is_raw(is_raw, js_adjustments),
        js_adjustments,
    )
}

pub fn get_cached_full_warped_image(
    state: &tauri::State<AppState>,
    js_adjustments: &serde_json::Value,
) -> Result<Arc<DynamicImage>, String> {
    get_cached_full_warped_image_for_path(state, None, js_adjustments)
}

pub fn get_cached_full_warped_image_for_path(
    state: &tauri::State<AppState>,
    path: Option<&str>,
    js_adjustments: &serde_json::Value,
) -> Result<Arc<DynamicImage>, String> {
    let loaded_image = state
        .original_image
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or("No original image loaded")?;

    if let Some(path) = path
        && parse_virtual_path(path).0 != parse_virtual_path(&loaded_image.path).0
    {
        return Err(format!("'{}' is not the loaded image", path));
    }

    let mut hasher = DefaultHasher::new();
    loaded_image.path.hash(&mut hasher);
    calculate_geometry_hash(js_adjustments).hash(&mut hasher);
    let cache_key = hasher.finish();

    let cached = || {
        let mut cache_lock = state
            .full_warped_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match cache_lock.as_ref() {
            Some((hash, img)) if *hash == cache_key => Some(Arc::clone(img)),
            _ => {
                *cache_lock = None;
                None
            }
        }
    };

    if let Some(img) = cached() {
        return Ok(img);
    }

    let _build_guard = rayon::current_thread_index().is_none().then(|| {
        state
            .full_warped_build_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    });
    if let Some(img) = cached() {
        return Ok(img);
    }

    let warped_arc = Arc::new(build_ai_source_image(
        &loaded_image.image,
        loaded_image.is_raw,
        js_adjustments,
    ));
    *state
        .full_warped_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some((cache_key, Arc::clone(&warped_arc)));

    Ok(warped_arc)
}


