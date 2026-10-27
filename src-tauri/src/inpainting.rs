use crate::host::TauriHost;

use image::{DynamicImage, GenericImageView, RgbaImage};
use serde_json::Value;

use crate::ai_connector;
use crate::ai_processing;
use crate::app_settings::load_settings;
use crate::app_state::AppState;
use crate::image_loader::prepare_base_image_cow;
use crate::image_processing::{
    apply_linear_to_srgb, inverse_transform_mask, inverse_transform_point,
};
use crate::mask_generation::{AiPatchDefinition, MaskDefinition, generate_mask_bitmap};
use crate::resolve_warped_image_for_masks;

use engine::inpainting::{
    PatchRegion, cleanup_patch, composite_patch, encode_patch_result, is_heal_patch, liquify,
    mask_bounds, retouch,
};

fn prepare_source_image(
    patch_id: &str,
    current_adjustments: &Value,
    state: &tauri::State<'_, AppState>,
) -> Result<(DynamicImage, bool), String> {
    let mut source_image_adjustments = current_adjustments.clone();
    if let Some(patches) = source_image_adjustments
        .get_mut("aiPatches")
        .and_then(|v| v.as_array_mut())
    {
        patches.retain(|p| p.get("id").and_then(|id| id.as_str()) != Some(patch_id));
    }

    let loaded_image = state
        .original_image
        .lock()
        .unwrap()
        .clone()
        .ok_or("No original image loaded")?;
    let is_raw = crate::negative_conversion::effective_is_raw(
        loaded_image.is_raw,
        &source_image_adjustments,
    );

    let composited = prepare_base_image_cow(
        &loaded_image.image,
        &loaded_image.path,
        &source_image_adjustments,
    )
    .map_err(|e| format!("Failed to prepare source image: {}", e))?
    .into_owned();

    let source_image = if is_raw {
        apply_linear_to_srgb(composited)
    } else {
        composited
    };

    Ok((source_image, is_raw))
}

struct PatchInput {
    source: DynamicImage,
    mask: image::GrayImage,
    trans_dims: (u32, u32),
    is_raw: bool,
}

fn build_patch_input(
    patch_definition: &AiPatchDefinition,
    current_adjustments: &Value,
    state: &tauri::State<'_, AppState>,
) -> Result<PatchInput, String> {
    let (source, is_raw) = prepare_source_image(&patch_definition.id, current_adjustments, state)?;
    let (img_w, img_h) = source.dimensions();

    let orientation_steps = current_adjustments
        .get("orientationSteps")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u8;
    let trans_dims = if orientation_steps % 2 == 1 {
        (img_h, img_w)
    } else {
        (img_w, img_h)
    };

    let mask_def_for_generation = MaskDefinition {
        id: patch_definition.id.clone(),
        name: patch_definition.name.clone(),
        visible: patch_definition.visible,
        invert: patch_definition.invert,
        opacity: 100.0,
        adjustments: serde_json::Value::Null,
        sub_masks: patch_definition.sub_masks.clone(),
    };

    let warped_image = resolve_warped_image_for_masks(
        state,
        current_adjustments,
        std::slice::from_ref(&mask_def_for_generation),
    );

    let generation_mask = generate_mask_bitmap(
        &mask_def_for_generation,
        trans_dims.0,
        trans_dims.1,
        1.0,
        (0.0, 0.0),
        warped_image.as_deref(),
    )
    .ok_or("Failed to generate mask bitmap for AI replace")?;

    let mask = inverse_transform_mask(generation_mask, current_adjustments);

    Ok(PatchInput {
        source,
        mask,
        trans_dims,
        is_raw,
    })
}

fn encode_region(
    region: PatchRegion,
    is_raw: bool,
    quality: u8,
    mask_as_png: bool,
) -> Result<String, String> {
    encode_patch_result(
        &region.color,
        &region.mask,
        region.min_x,
        region.min_y,
        region.width,
        region.height,
        is_raw,
        quality,
        mask_as_png,
    )
    .map_err(|e| e.to_string())
}

fn encode_region_err(
    r: std::result::Result<PatchRegion, anyhow::Error>,
    is_raw: bool,
    quality: u8,
    mask_as_png: bool,
) -> Result<String, String> {
    r.map_err(|e| e.to_string())
        .and_then(|region| encode_region(region, is_raw, quality, mask_as_png))
}

#[tauri::command]
pub async fn generate_manual_cleanup_patch(
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    source_point: (f64, f64),
    _task_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let input = build_patch_input(&patch_definition, &current_adjustments, &state)?;
    let (min_x, max_x, min_y, max_y) = mask_bounds(&input.mask).map_err(|e| e.to_string())?;

    let center_x = f64::from(min_x + max_x) / 2.0;
    let center_y = f64::from(min_y + max_y) / 2.0;

    let source_point_untransformed = inverse_transform_point(
        source_point.0,
        source_point.1,
        f64::from(input.trans_dims.0),
        f64::from(input.trans_dims.1),
        &current_adjustments,
    );

    let offset_x = (source_point_untransformed.0 - center_x).round() as i32;
    let offset_y = (source_point_untransformed.1 - center_y).round() as i32;

    let sub_masks_val = serde_json::to_value(&patch_definition.sub_masks).unwrap_or(Value::Null);
    let heal = is_heal_patch(&sub_masks_val, &patch_definition.name);

    let region = cleanup_patch(
        &input.source,
        &input.mask,
        (min_x, max_x, min_y, max_y),
        offset_x,
        offset_y,
        heal,
    );
    encode_region_err(region, input.is_raw, 100, false)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn invoke_generative_replace_with_mask_def(
    path: String,
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    use_fast_inpaint: bool,
    task_id: Option<String>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let guard = task_id
        .as_ref()
        .map(|id| crate::app_state::AiTaskGuard::new(&state.active_ai_tasks, id.clone()));
    let cancel_flag = guard.as_ref().map(|g| &g.token);

    if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }

    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let input = build_patch_input(&patch_definition, &current_adjustments, &state)?;
    let (img_w, img_h) = input.source.dimensions();

    let (min_x, max_x, min_y, max_y) = mask_bounds(&input.mask).map_err(|e| e.to_string())?;

    if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }

    let (final_patch, patch_x, patch_y) = if use_fast_inpaint {
        let lama_model = ai_processing::get_or_init_lama_model(
            &*TauriHost::host(app_handle.clone()),
            &state.ai_state,
            &state.ai_init_lock,
        )
        .await
        .map_err(|e| e.to_string())?;

        if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
            return Err("Task cancelled".to_string());
        }

        ai_processing::run_lama_inpainting(&input.source, &input.mask, &lama_model)
            .map_err(|e| e.to_string())?
    } else if settings.ai_provider.as_deref() == Some("ai-connector")
        && let Some(address) = settings.ai_connector_address
    {
        let base_url = format!("http://{}", address);

        let mut rgba_mask = RgbaImage::new(img_w, img_h);
        for (src_val, dst_chunk) in input.mask.as_raw().iter().zip(rgba_mask.chunks_mut(4)) {
            let intensity = *src_val;
            dst_chunk[0] = intensity;
            dst_chunk[1] = intensity;
            dst_chunk[2] = intensity;
            dst_chunk[3] = 255;
        }
        let mask_image_dynamic = DynamicImage::ImageRgba8(rgba_mask);

        let (real_path_buf, _) = engine::paths::parse_virtual_path(&path);
        let real_path_str = real_path_buf.to_string_lossy().to_string();

        let inpaint_fut = ai_connector::process_inpainting(
            &base_url,
            &real_path_str,
            &input.source,
            &mask_image_dynamic,
            patch_definition.prompt,
        );

        let inpainted = if let Some(token) = cancel_flag.as_ref() {
            tokio::select! {
                res = inpaint_fut => res.map_err(|e| e.to_string())?,
                _ = token.wait_for_cancel() => {
                    return Err("Task cancelled".to_string());
                }
            }
        } else {
            inpaint_fut.await.map_err(|e| e.to_string())?
        };
        let inpainted_rgba = match inpainted.dimensions() {
            (w, h) if (w, h) != (img_w, img_h) => image::imageops::resize(
                &inpainted,
                img_w,
                img_h,
                image::imageops::FilterType::Lanczos3,
            ),
            _ => inpainted,
        };
        (inpainted_rgba, 0, 0)
    } else {
        return Err(
            "No generative backend configured or connection invalid. Please check your AI settings."
                .to_string(),
        );
    };

    if cancel_flag.as_ref().is_some_and(|t| t.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }

    let region = composite_patch(
        &input.source,
        &final_patch,
        patch_x,
        patch_y,
        &input.mask,
        (min_x, max_x, min_y, max_y),
    );
    encode_region(region, input.is_raw, 95, true)
}

#[tauri::command]
pub async fn generate_liquify_patch(
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    _source_point: (f64, f64),
    _task_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (source_image, is_raw) =
        prepare_source_image(&patch_definition.id, &current_adjustments, &state)?;
    let source_rgb = source_image.into_rgb8();
    let sub_masks_val = serde_json::to_value(&patch_definition.sub_masks).unwrap_or(Value::Null);

    let region = liquify(&source_rgb, &sub_masks_val);
    encode_region_err(region, is_raw, 100, false)
}

#[tauri::command]
pub async fn generate_retouch_patch(
    patch_definition: AiPatchDefinition,
    current_adjustments: Value,
    _task_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (source_image, is_raw) =
        prepare_source_image(&patch_definition.id, &current_adjustments, &state)?;
    let source_rgb = source_image.into_rgb8();
    let sub_masks_val = serde_json::to_value(&patch_definition.sub_masks).unwrap_or(Value::Null);

    let region = retouch(&source_rgb, &sub_masks_val);
    encode_region_err(region, is_raw, 100, false)
}
