use crate::host::TauriHost;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::sync::Mutex;

use base64::{Engine as _, engine::general_purpose};
use image::{DynamicImage, GenericImageView, GrayImage, ImageFormat};

use crate::ai_connector;
use crate::ai_processing::{
    AiDepthMaskParameters, AiMaskModel, AiModels, AiState, CachedDepthMap, get_or_init_ai_models,
    run_depth_anything_model,
};
use crate::app_settings::load_settings;
use crate::app_state::{AiTaskGuard, AppState};
use crate::cache_utils::GEOMETRY_KEYS;
use crate::effect_maps::EffectMapKind;

#[tauri::command]
pub fn cancel_ai_task(task_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let tasks = state.active_ai_tasks.lock().unwrap();
    if let Some(token) = tasks.get(&task_id) {
        token.cancel();
    }
    Ok(())
}

fn encode_to_base64_png(image: &GrayImage) -> Result<String, String> {
    let mut buf = Cursor::new(Vec::new());
    image
        .write_to(&mut buf, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let base64_str = general_purpose::STANDARD.encode(buf.get_ref());
    Ok(format!("data:image/png;base64,{}", base64_str))
}

use engine::mask_generation::MaskTransform;

pub fn ai_cache_key(path: &str, adjustments: &serde_json::Value) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.as_bytes());
    let mut geo_hasher = DefaultHasher::new();
    for key in GEOMETRY_KEYS {
        if let Some(val) = adjustments.get(key) {
            key.hash(&mut geo_hasher);
            val.to_string().hash(&mut geo_hasher);
        }
    }
    hasher.update(&geo_hasher.finish().to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

fn loaded_source_id(state: &tauri::State<'_, AppState>) -> Option<String> {
    let loaded = state.original_image.lock().unwrap().clone()?;
    engine::ai_results::source_id_for(&loaded.path)
}

fn check_cancelled(guard: &Option<AiTaskGuard>) -> Result<(), String> {
    if guard.as_ref().is_some_and(|g| g.token.is_cancelled()) {
        return Err("Task cancelled".to_string());
    }
    Ok(())
}

fn cached_depth_estimate(
    ai_state: &Mutex<Option<AiState>>,
    models: &AiModels,
    cache_key: &str,
    warped_image: &DynamicImage,
) -> Result<CachedDepthMap, String> {
    if let Some(cached) = ai_state
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|s| s.depth_map.clone())
        .filter(|cached| cached.path_hash == cache_key)
    {
        return Ok(cached);
    }

    let depth_image = run_depth_anything_model(
        warped_image,
        &*models.depth_anything().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let depth = CachedDepthMap {
        path_hash: cache_key.to_string(),
        depth_image,
        original_size: warped_image.dimensions(),
    };
    if let Some(state) = ai_state.lock().unwrap().as_mut() {
        state.depth_map = Some(depth.clone());
    }
    Ok(depth)
}

#[allow(clippy::too_many_arguments)]
fn generate_depth_mask(
    ai_state: &Mutex<Option<AiState>>,
    models: &AiModels,
    cache_key: &str,
    warped_image: &DynamicImage,
    params: AiDepthMaskParameters,
    transform: MaskTransform,
    ai_source: Option<String>,
) -> Result<AiDepthMaskParameters, String> {
    let depth = cached_depth_estimate(ai_state, models, cache_key, warped_image)?;
    let raw_depth_fullres = image::imageops::resize(
        &depth.depth_image,
        depth.original_size.0,
        depth.original_size.1,
        image::imageops::FilterType::Triangle,
    );

    Ok(AiDepthMaskParameters {
        mask_data_base64: Some(encode_to_base64_png(&raw_depth_fullres)?),
        rotation: Some(transform.rotation),
        flip_horizontal: Some(transform.flip_horizontal),
        flip_vertical: Some(transform.flip_vertical),
        orientation_steps: Some(transform.orientation_steps),
        ai_source,
        ..params
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn generate_ai_depth_mask(
    js_adjustments: serde_json::Value,
    path: String,
    min_depth: f32,
    max_depth: f32,
    min_fade: f32,
    max_fade: f32,
    feather: f32,
    rotation: f32,
    flip_horizontal: bool,
    flip_vertical: bool,
    orientation_steps: u8,
    task_id: Option<String>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<AiDepthMaskParameters, String> {
    let guard = task_id.map(|id| AiTaskGuard::new(&state.active_ai_tasks, id));
    check_cancelled(&guard)?;

    let models = get_or_init_ai_models(
        &*TauriHost::host(app_handle.clone()),
        &state.ai_state,
        &state.ai_init_lock,
        AiMaskModel::Depth,
    )
    .await
    .map_err(|e| e.to_string())?;
    check_cancelled(&guard)?;

    let warped_image =
        engine::preview_cache::get_cached_full_warped_image(&state, &js_adjustments)?;
    check_cancelled(&guard)?;

    let transform = MaskTransform {
        rotation,
        flip_horizontal,
        flip_vertical,
        orientation_steps,
    };
    let params = AiDepthMaskParameters {
        min_depth,
        max_depth,
        min_fade,
        max_fade,
        feather,
        ..Default::default()
    };
    let params = generate_depth_mask(
        &state.ai_state,
        &models,
        &ai_cache_key(&path, &js_adjustments),
        &warped_image,
        params,
        transform,
        loaded_source_id(&state),
    )?;
    check_cancelled(&guard)?;
    Ok(params)
}

pub async fn generate_effect_map(
    app_handle: &tauri::AppHandle,
    kind: EffectMapKind,
    source_image: &DynamicImage,
    source_id: &str,
) -> Result<String, String> {
    let state = tauri::Manager::state::<AppState>(app_handle);
    let map = match kind {
        EffectMapKind::Depth => {
            let models = get_or_init_ai_models(
                &*TauriHost::host(app_handle.clone()),
                &state.ai_state,
                &state.ai_init_lock,
                AiMaskModel::Depth,
            )
            .await
            .map_err(|e| e.to_string())?;
            let session = models.depth_anything().map_err(|e| e.to_string())?;
            DynamicImage::ImageLuma8(
                run_depth_anything_model(source_image, &session).map_err(|e| e.to_string())?,
            )
        }
        EffectMapKind::Normal => {
            let session = crate::ai_processing::get_or_init_normal_model(
                &*TauriHost::host(app_handle.clone()),
                &state.ai_state,
                &state.ai_init_lock,
            )
            .await
            .map_err(|e| e.to_string())?;
            DynamicImage::ImageRgba8(
                crate::ai_processing::run_normal_model(source_image, &session)
                    .map_err(|e| e.to_string())?,
            )
        }
    };

    crate::effect_maps::store_source_map(kind, source_id, &map).map_err(|e| e.to_string())
}

async fn generate_loaded_effect_map(
    kind: EffectMapKind,
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let loaded_image = get_loaded_image(&state)?;
    let source_id = engine::ai_results::source_id_for(&loaded_image.path)
        .ok_or("Unable to identify the loaded image")?;
    let source_image = crate::mask_generation::build_converted_source_image(
        &loaded_image.image,
        loaded_image.is_raw,
        &js_adjustments,
    );
    generate_effect_map(&app_handle, kind, &source_image, &source_id).await
}

#[tauri::command]
pub async fn generate_full_image_depth_map(
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    generate_loaded_effect_map(EffectMapKind::Depth, js_adjustments, state, app_handle).await
}

#[tauri::command]
pub async fn generate_relight_normal_map(
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    generate_loaded_effect_map(EffectMapKind::Normal, js_adjustments, state, app_handle).await
}

fn get_loaded_image(state: &tauri::State<'_, AppState>) -> Result<crate::LoadedImage, String> {
    state
        .original_image
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| "No original image loaded".to_string())
}

#[tauri::command]
pub async fn check_ai_connector_status(app_handle: tauri::AppHandle) {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let is_connected = if let Some(address) = settings.ai_connector_address {
        ai_connector::check_status(&address).await.unwrap_or(false)
    } else {
        false
    };
    use tauri::Emitter;
    let _ = app_handle.emit(
        "ai-connector-status-update",
        serde_json::json!({ "connected": is_connected }),
    );
}

#[tauri::command]
pub async fn test_ai_connector_connection(address: String) -> Result<(), String> {
    match ai_connector::check_status(&address).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("Server reachable but returned bad health status".to_string()),
        Err(e) => Err(e.to_string()),
    }
}
