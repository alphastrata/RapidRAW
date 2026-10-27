use crate::host::TauriHost;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::task::JoinHandle;
use walkdir::WalkDir;
use rayon::prelude::*;

use crate::file_management;
use crate::formats::is_supported_image_file;
use crate::image_processing::ImageMetadata;
use crate::{AppState, exif_processing};
use engine::paths::parse_virtual_path;

pub use engine::tagging::{COLOR_TAG_PREFIX, USER_TAG_PREFIX, generate_tags_with_clip};
use engine::tagging::generate_tags_with_clip;

fn needs_ai_tags(path: &Path) -> bool {
    let (_, sidecar_path) = parse_virtual_path(&path.to_string_lossy());
    match exif_processing::load_sidecar(&sidecar_path).tags {
        None => true,
        Some(tags) => !tags
            .iter()
            .any(|tag| !tag.starts_with(COLOR_TAG_PREFIX) && !tag.starts_with(USER_TAG_PREFIX)),
    }
}

#[tauri::command]
pub async fn start_background_indexing(
    folder_path: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(handle) = state.indexing_task_handle.lock().unwrap().take() {
        println!("Cancelling previous indexing task.");
        handle.abort();
        let _ = app_handle.emit("indexing-cancelled", ());
    }

    let settings = crate::load_settings(app_handle.clone())?;
    if !settings.enable_ai_tagging.unwrap_or(false) {
        return Ok(());
    }

    let max_concurrent_tasks = settings.tagging_thread_count.unwrap_or(3).max(1) as usize;
    let custom_ai_tags = settings.custom_ai_tags.clone();
    let ai_tag_count = settings.ai_tag_count.unwrap_or(10) as usize;

    let clip_models = crate::ai_processing::get_or_init_clip_models(
        &*TauriHost::host(app_handle.clone()),
        &state.ai_state,
        &state.ai_init_lock,
    )
    .await
    .map_err(|e| e.to_string())?;

    let app_handle_clone = app_handle.clone();

    let task: JoinHandle<()> = tokio::spawn(async move {
        let image_paths: Vec<PathBuf> = match fs::read_dir(&folder_path) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && is_supported_image_file(path.to_string_lossy().as_ref())
                        && needs_ai_tags(path)
                })
                .collect(),
            Err(e) => {
                eprintln!("Failed to read directory '{}': {}", folder_path, e);
                let _ = app_handle_clone
                    .emit("indexing-error", format!("Failed to read directory: {}", e));
                *app_handle_clone
                    .state::<AppState>()
                    .indexing_task_handle
                    .lock()
                    .unwrap() = None;
                return;
            }
        };

        let total_images = image_paths.len();
        if total_images == 0 {
            *app_handle_clone
                .state::<AppState>()
                .indexing_task_handle
                .lock()
                .unwrap() = None;
            return;
        }

        let _ = app_handle_clone.emit(
            "indexing-started",
            serde_json::json!({ "current": 0, "total": total_images }),
        );
        println!(
            "Starting background indexing of {} images in: {}",
            total_images, folder_path
        );
        println!(
            "Using {} concurrent threads for AI tagging.",
            max_concurrent_tasks
        );

        let state_clone = app_handle_clone.state::<AppState>();
        let gpu_context = crate::gpu_processing::get_or_init_gpu_context(
            &state_clone,
            &*crate::host::TauriHost::host(app_handle.clone()),
        )
        .ok();

        let processed_count = Arc::new(Mutex::new(0));
        let custom_ai_tags_shared = Arc::new(custom_ai_tags);

        stream::iter(image_paths)
            .for_each_concurrent(max_concurrent_tasks, |path| {
                let app_handle_inner = app_handle_clone.clone();
                let clip_models_inner = clip_models.clone();
                let gpu_context_inner = gpu_context.clone();
                let processed_count_inner = Arc::clone(&processed_count);
                let tags_inner = Arc::clone(&custom_ai_tags_shared);

                async move {
                    let path_str = path.to_string_lossy().to_string();
                    let (source_path, sidecar_path) = parse_virtual_path(&path_str);

                    match file_management::get_cached_or_generate_thumbnail_image(
                        &path_str,
                        &app_handle_inner,
                        gpu_context_inner.as_ref(),
                    ) {
                        Ok(image) => {
                            if let Ok(ai_tags) = generate_tags_with_clip(
                                &image,
                                &clip_models_inner.model,
                                &clip_models_inner.tokenizer,
                                (*tags_inner).clone(),
                                ai_tag_count,
                            ) {
                                println!("Found AI tags for {}: {:?}", path_str, ai_tags);

                                let _ = exif_processing::update_sidecar(
                                    &sidecar_path,
                                    &source_path,
                                    |metadata| {
                                        let mut existing_tags: HashSet<String> = metadata
                                            .tags
                                            .take()
                                            .unwrap_or_default()
                                            .into_iter()
                                            .collect();

                                        existing_tags.extend(ai_tags);

                                        let mut final_tags: Vec<String> =
                                            existing_tags.into_iter().collect();
                                        final_tags.sort_unstable();

                                        metadata.tags = Some(final_tags);
                                    },
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Could not get or generate image for tagging {}: {}",
                                path_str, e
                            );
                        }
                    }

                    let mut count = processed_count_inner.lock().unwrap();
                    *count += 1;
                    let _ = app_handle_inner.emit(
                        "indexing-progress",
                        serde_json::json!({
                            "current": *count,
                            "total": total_images
                        }),
                    );
                }
            })
            .await;

        println!("Background indexing finished for: {}", folder_path);
        let _ = app_handle_clone.emit("indexing-finished", ());

        *app_handle_clone
            .state::<AppState>()
            .indexing_task_handle
            .lock()
            .unwrap() = None;
    });

    *state.indexing_task_handle.lock().unwrap() = Some(task);

    Ok(())
}

fn modify_tags_for_path(
    path_str: &str,
    app_handle: &AppHandle,
    modify_fn: impl Fn(&mut Vec<String>),
) -> Result<(), String> {
    let (source_path, sidecar_path) = parse_virtual_path(path_str);

    let metadata = exif_processing::update_sidecar(&sidecar_path, &source_path, |metadata| {
        let mut tags = metadata.tags.take().unwrap_or_default();
        modify_fn(&mut tags);

        tags.sort_unstable();
        tags.dedup();

        if !tags.is_empty() {
            metadata.tags = Some(tags);
        }
    })
    .map_err(|e| e.to_string())?;

    if let Ok(settings) = crate::load_settings(app_handle.clone())
        && settings.enable_xmp_sync.unwrap_or(false)
    {
        let create_if_missing = settings.create_xmp_if_missing.unwrap_or(false);
        file_management::sync_metadata_to_xmp(&source_path, &metadata, create_if_missing);
    }

    Ok(())
}

#[tauri::command]
pub fn add_tag_for_paths(
    paths: Vec<String>,
    tag: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    paths.par_iter().for_each(|path| {
        let tag_clone = tag.clone();
        if let Err(e) = modify_tags_for_path(path, &app_handle, |tags| {
            if !tags.contains(&tag_clone) {
                tags.push(tag_clone.clone());
            }
        }) {
            eprintln!("Failed to add tag to {}: {}", path, e);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn remove_tag_for_paths(
    paths: Vec<String>,
    tag: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    paths.par_iter().for_each(|path| {
        let tag_clone = tag.clone();
        if let Err(e) = modify_tags_for_path(path, &app_handle, |tags| {
            tags.retain(|t| t != &tag_clone);
        }) {
            eprintln!("Failed to remove tag from {}: {}", path, e);
        }
    });
    Ok(())
}

fn rrdata_source_path(rrdata: &Path) -> Option<PathBuf> {
    let name = rrdata.file_name()?.to_str()?;
    let (source_filename, _) = file_management::parse_sidecar_name(name)?;

    Some(rrdata.with_file_name(source_filename))
}

fn sync_xmp_for_rrdata(
    rrdata_path: &Path,
    metadata: &ImageMetadata,
    enable_xmp_sync: bool,
    create_xmp_if_missing: bool,
) {
    if !enable_xmp_sync {
        return;
    }
    if let Some(source_path) = rrdata_source_path(rrdata_path) {
        file_management::sync_metadata_to_xmp(&source_path, metadata, create_xmp_if_missing);
    }
}

fn retain_tags_under(
    root_path: &str,
    app_handle: &AppHandle,
    keep: impl Fn(&str) -> bool,
) -> Result<usize, String> {
    if !Path::new(root_path).exists() {
        return Err(format!("Root path does not exist: {}", root_path));
    }

    let settings = crate::load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
    let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

    let mut updated_count = 0;
    let walker = WalkDir::new(root_path).into_iter();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("rrdata") {
            continue;
        }
        let Some(source_path) = rrdata_source_path(path) else {
            continue;
        };

        let has_removable_tags = exif_processing::load_sidecar(path)
            .tags
            .is_some_and(|tags| tags.iter().any(|tag| !keep(tag)));
        if !has_removable_tags {
            continue;
        }

        let updated = exif_processing::update_sidecar(path, &source_path, |metadata| {
            if let Some(tags) = &mut metadata.tags {
                tags.retain(|tag| keep(tag));
                if tags.is_empty() {
                    metadata.tags = None;
                }
            }
        });
        if let Ok(metadata) = updated {
            updated_count += 1;
            sync_xmp_for_rrdata(path, &metadata, enable_xmp_sync, create_xmp_if_missing);
        }
    }
    Ok(updated_count)
}

#[tauri::command]
pub fn clear_ai_tags(root_path: String, app_handle: AppHandle) -> Result<usize, String> {
    // Keep color tags and user tags, remove others (AI tags)
    retain_tags_under(&root_path, &app_handle, |tag| {
        tag.starts_with(COLOR_TAG_PREFIX) || tag.starts_with(USER_TAG_PREFIX)
    })
}

#[tauri::command]
pub fn clear_all_tags(root_path: String, app_handle: AppHandle) -> Result<usize, String> {
    // Keep only color tags, remove AI and user tags
    retain_tags_under(&root_path, &app_handle, |tag| {
        tag.starts_with(COLOR_TAG_PREFIX)
    })
}