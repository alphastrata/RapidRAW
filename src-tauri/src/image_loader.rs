pub use engine::image_loader::*;

use engine::app_state::AppState;
use engine::cache_utils::DecodeFingerprint;
pub use engine::image_loader::{LoadImageResult, get_or_decode_pristine};
use engine::image_processing::ImageMetadata;
use std::sync::atomic::Ordering;

use image::GenericImageView;
use tauri::Manager;

use engine::app_state::LoadedImage;
use engine::baseline_exposure::BaselineOptions;
use engine::formats::is_raw_file;
use engine::image_processing::effective_baseline_exposure;

use engine::paths::parse_virtual_path;

#[tauri::command]
pub fn is_image_cached(
    path: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> bool {
    let (source_path, _) = parse_virtual_path(&path);
    let settings = crate::app_settings::load_settings(app_handle).unwrap_or_default();
    let Some(fingerprint) = DecodeFingerprint::for_file(&source_path, &settings) else {
        return false;
    };
    state
        .decoded_image_cache
        .lock()
        .unwrap()
        .contains(&source_path.to_string_lossy(), &fingerprint)
}

#[tauri::command]
pub async fn load_image(
    path: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<LoadImageResult, String> {
    let my_generation = state.load_image_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let cancel_token = Some((state.load_image_generation.clone(), my_generation));

    {
        *state
            .original_image
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .cached_preview
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .gpu_image_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .full_warped_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        engine::cache_utils::clear_preview_stage_caches(&state);

        state
            .mask_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        state
            .patch_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        state
            .geometry_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    let (source_path, sidecar_path) = parse_virtual_path(&path);
    let source_path_str = source_path.to_string_lossy().to_string();

    let metadata: ImageMetadata = crate::exif_processing::load_sidecar(&sidecar_path);

    let settings = crate::app_settings::load_settings(app_handle.clone()).unwrap_or_default();
    let baseline_options = BaselineOptions::from_settings(&settings);

    state
        .decoded_image_cache
        .lock()
        .unwrap()
        .compact_except(&source_path_str);

    let (pristine_arc, exif_data) = {
        let app_handle = app_handle.clone();
        let source_path = source_path.clone();
        tokio::task::spawn_blocking(move || {
            let state = app_handle.state::<AppState>();
            get_or_decode_pristine(&state, &source_path, &settings, cancel_token)
        })
        .await
        .map_err(|e| e.to_string())??
    };

    if state.load_image_generation.load(Ordering::SeqCst) != my_generation {
        return Err("Load cancelled".to_string());
    }

    let is_raw = is_raw_file(&source_path_str);

    if state.load_image_generation.load(Ordering::SeqCst) != my_generation {
        return Err("Load cancelled".to_string());
    }

    let (orig_width, orig_height) = pristine_arc.dimensions();
    let as_shot = crate::as_shot::as_shot(&source_path_str);
    if is_raw {
        log::info!(
            "Baseline exposure for '{}': {:+.2} EV (DNG baseline {:+.2} EV, highlight preservation {:+.2} EV)",
            source_path_str,
            effective_baseline_exposure(&metadata.adjustments, as_shot, baseline_options),
            as_shot.exposure.dng_baseline,
            as_shot.exposure.highlight_preservation
        );
    }

    *state.original_image.lock().unwrap() = Some(LoadedImage {
        path: path.clone(),
        image: pristine_arc,
        is_raw,
        as_shot,
    });

    Ok(LoadImageResult {
        width: orig_width,
        height: orig_height,
        metadata,
        exif: exif_data,
        is_raw,
        as_shot_white_balance: as_shot.white_balance,
    })
}
