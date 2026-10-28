pub use engine::paths::{parse_virtual_path, read_file_mapped};

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use walkdir::WalkDir;

use crate::AppState;
use crate::app_settings::{AppSettings, load_settings};
use crate::as_shot::as_shot;
use crate::baseline_exposure::BaselineOptions;

use crate::image_loader;
use crate::image_processing::{
    AutoAdjustMode, ImageMetadata, auto_results_to_json, default_adjustments,
    effective_baseline_exposure, perform_auto_analysis,
};

pub fn resolve_lens_params_in_adjustments(
    adjustments: &mut Value,
    exif_data: &Option<HashMap<String, String>>,
    lens_db: Option<&crate::lens_correction::LensDatabase>,
) {
    if let Some(map) = adjustments.as_object_mut() {
        let mode = map
            .get("lensCorrectionMode")
            .and_then(|v| v.as_str())
            .unwrap_or("manual");

        if mode != "auto" {
            return;
        }

        if let Some(exif) = exif_data {
            let exif_maker = exif.get("Make").map(|s| s.as_str()).unwrap_or("");
            let exif_model = exif.get("LensModel").map(|s| s.as_str()).unwrap_or("");
            let exif_camera_model = exif.get("Model").map(|s| s.as_str()).unwrap_or("");
            if let Some(db) = lens_db {
                if let Some((detected_maker, detected_model)) =
                    crate::lens_correction::find_best_lens_match(
                        db,
                        exif_maker,
                        exif_model,
                        exif_camera_model,
                    )
                {
                    map.insert(
                        "lensMaker".to_string(),
                        serde_json::to_value(&detected_maker).unwrap(),
                    );
                    map.insert(
                        "lensModel".to_string(),
                        serde_json::to_value(&detected_model).unwrap(),
                    );
                } else {
                    map.remove("lensMaker");
                    map.remove("lensModel");
                }
            }
        } else {
            map.remove("lensMaker");
            map.remove("lensModel");
        }

        if let Some(db) = lens_db {
            let has_valid_lens = match (
                map.get("lensMaker").and_then(|v| v.as_str()),
                map.get("lensModel").and_then(|v| v.as_str()),
            ) {
                (Some(maker), Some(model)) if !maker.is_empty() && !model.is_empty() => {
                    let mut focal_length = 50.0;
                    let mut aperture = None;
                    let mut distance = None;

                    if let Some(exif) = exif_data {
                        if let Some(fl_str) = exif
                            .get("FocalLength")
                            .or(exif.get("FocalLengthIn35mmFilm"))
                            && let Ok(fl) = fl_str.replace(" mm", "").trim().parse::<f32>()
                        {
                            focal_length = fl;
                        }
                        if let Some(ap_str) = exif.get("ApertureValue").or(exif.get("FNumber"))
                            && let Ok(ap) = ap_str.replace("f/", "").trim().parse::<f32>()
                        {
                            aperture = Some(ap);
                        }
                        if let Some(dist_str) = exif.get("SubjectDistance")
                            && let Ok(dist) = dist_str.replace(" m", "").trim().parse::<f32>()
                        {
                            distance = Some(dist);
                        }
                    }

                    if let Some(params) = crate::lens_correction::resolve_lens_params(
                        db,
                        maker,
                        model,
                        focal_length,
                        aperture,
                        distance,
                    ) {
                        map.insert(
                            "lensDistortionParams".to_string(),
                            serde_json::to_value(params).unwrap(),
                        );
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if !has_valid_lens {
                map.remove("lensDistortionParams");
            }
        }
    }
}

#[tauri::command]
pub fn get_supported_file_types() -> Result<serde_json::Value, String> {
    let raw_extensions: Vec<&str> = crate::formats::RAW_EXTENSIONS
        .iter()
        .map(|(ext, _)| *ext)
        .collect();
    let non_raw_extensions: Vec<&str> = crate::formats::NON_RAW_EXTENSIONS.to_vec();

    Ok(serde_json::json!({
        "raw": raw_extensions,
        "nonRaw": non_raw_extensions
    }))
}

fn update_sidecars_for_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
    update: impl Fn(&str, &Path, &mut ImageMetadata) + Sync,
) {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
    let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

    paths.par_iter().for_each(|path| {
        let (source_path, sidecar_path) = parse_virtual_path(path);

        match crate::exif_processing::update_sidecar(&sidecar_path, &source_path, |metadata| {
            update(path, &source_path, metadata)
        }) {
            Ok(metadata) => {
                if enable_xmp_sync {
                    sync_metadata_to_xmp(&source_path, &metadata, create_xmp_if_missing);
                }
            }
            Err(e) => log::error!("Failed to update sidecar for {}: {}", path, e),
        }
    });
}

#[tauri::command]
pub async fn apply_adjustments_to_paths(
    paths: Vec<String>,
    adjustments: Value,
    app_handle: AppHandle,
) -> Result<(), String> {
    let lens_db = app_handle
        .state::<AppState>()
        .lens_db
        .lock()
        .unwrap()
        .clone();

    let regenerates_ai = engine::ai_results::has_ai_results(&adjustments);

    tauri::async_runtime::spawn_blocking(move || {
        update_sidecars_for_paths(paths, app_handle, move |_path, source_path, metadata| {
            crate::exif_processing::fill_missing_exif(metadata, source_path);

            if metadata.adjustments.is_null() {
                metadata.adjustments = default_adjustments();
            }

            let mut pasted = adjustments.clone();
            let source_id = regenerates_ai
                .then(|| engine::ai_results::source_id(source_path))
                .flatten();
            if let Some(source_id) = &source_id {
                engine::ai_results::reconcile_ai_results(
                    &mut pasted,
                    &metadata.adjustments,
                    source_id,
                    false,
                );
            }

            if let (Some(new_map), Some(pasted_map)) =
                (metadata.adjustments.as_object_mut(), pasted.as_object())
            {
                for (k, v) in pasted_map {
                    new_map.insert(k.clone(), v.clone());
                }
            }

            resolve_lens_params_in_adjustments(
                &mut metadata.adjustments,
                &metadata.exif,
                lens_db.as_deref(),
            );
        });
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reset_adjustments_for_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        update_sidecars_for_paths(paths, app_handle, |_, _, metadata| {
            metadata.adjustments = default_adjustments();
        });
    });

    Ok(())
}

const RAM_PER_BATCH_DECODE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn run_batch_workers<T: Send>(items: impl Iterator<Item = T> + Send, work: impl Fn(T) + Sync) {
    static WORKERS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let workers = *WORKERS.get_or_init(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let ram_limit = (sys.total_memory() / RAM_PER_BATCH_DECODE_BYTES).max(1) as usize;
        rayon::current_num_threads().min(ram_limit).max(1)
    });

    let items = std::sync::Mutex::new(items);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let next = items.lock().unwrap_or_else(|e| e.into_inner()).next();
                    match next {
                        Some(item) => work(item),
                        None => break,
                    }
                }
            });
        }
    });
}

#[tauri::command]
pub async fn apply_auto_lens_correction_to_paths(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let lens_db = app_handle
        .state::<AppState>()
        .lens_db
        .lock()
        .unwrap()
        .clone();

    tauri::async_runtime::spawn_blocking(move || {
        update_sidecars_for_paths(paths, app_handle, move |_, source_path, metadata| {
            crate::exif_processing::fill_missing_exif(metadata, source_path);

            if metadata.adjustments.is_null() {
                metadata.adjustments = default_adjustments();
            }

            if let Some(obj) = metadata.adjustments.as_object_mut() {
                obj.insert("lensCorrectionMode".to_string(), serde_json::json!("auto"));
                obj.insert("lensDistortionEnabled".to_string(), serde_json::json!(true));
                obj.insert("lensTcaEnabled".to_string(), serde_json::json!(true));
                obj.insert("lensVignetteEnabled".to_string(), serde_json::json!(true));
            }

            resolve_lens_params_in_adjustments(
                &mut metadata.adjustments,
                &metadata.exif,
                lens_db.as_deref(),
            );
        });
    })
    .await
    .map_err(|e| e.to_string())
}

async fn update_sidecars_from_images(
    paths: Vec<String>,
    app_handle: AppHandle,
    operation: &'static str,
    fast_decode: bool,
    update: impl Fn(&image::DynamicImage, &str, &AppSettings, &mut Value) + Send + Sync + 'static,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
        let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

        let process = |path: &String| {
            let (source_path, sidecar_path) = parse_virtual_path(path);
            let source_path_str = source_path.to_string_lossy().to_string();

            let apply = || -> Result<(), String> {
                let file_bytes = read_file_mapped(&source_path).map_err(|e| e.to_string())?;
                let image = image_loader::load_base_image_from_bytes(
                    &file_bytes,
                    &source_path_str,
                    fast_decode,
                    &settings,
                    None,
                )
                .map_err(|e| e.to_string())?;

                let updated_metadata = crate::exif_processing::update_sidecar(
                    &sidecar_path,
                    &source_path,
                    |metadata| {
                        if metadata.adjustments.is_null() {
                            metadata.adjustments = default_adjustments();
                        }
                        update(
                            &image,
                            &source_path_str,
                            &settings,
                            &mut metadata.adjustments,
                        );
                    },
                )
                .map_err(|e| e.to_string())?;

                if enable_xmp_sync {
                    sync_metadata_to_xmp(&source_path, &updated_metadata, create_xmp_if_missing);
                }
                Ok(())
            };

            if let Err(e) = apply() {
                eprintln!("Failed to apply {} to {}: {}", operation, path, e);
            }
        };

        if fast_decode {
            run_batch_workers(paths.iter(), process);
        } else if let Ok(pool) = rayon::ThreadPoolBuilder::new().num_threads(2).build() {
            pool.install(|| paths.par_iter().for_each(process));
        } else {
            paths.iter().for_each(process);
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_auto_adjustments_to_paths(
    paths: Vec<String>,
    mode: Option<AutoAdjustMode>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let mode = mode.unwrap_or_default();
    update_sidecars_from_images(
        paths,
        app_handle,
        "auto adjustments",
        true,
        move |image, source_path, settings, adjustments| {
            let auto_results = perform_auto_analysis(
                image,
                effective_baseline_exposure(
                    adjustments,
                    as_shot(source_path),
                    BaselineOptions::from_settings(settings),
                ),
            );
            let auto_adjustments_json = auto_results_to_json(&auto_results, mode);

            let mut active_tools = crate::image_processing::active_tools(adjustments);

            if let (Some(existing_map), Some(auto_map)) = (
                adjustments.as_object_mut(),
                auto_adjustments_json.as_object(),
            ) {
                for (k, v) in auto_map {
                    if k == "activeTools" {
                        if let Some(auto_tools) = v.as_object() {
                            active_tools.extend(auto_tools.clone());
                        }
                        existing_map.insert(
                            k.clone(),
                            serde_json::Value::Object(std::mem::take(&mut active_tools)),
                        );
                    } else {
                        existing_map.insert(k.clone(), v.clone());
                    }
                }
            }
        },
    )
    .await
}

#[tauri::command]
pub async fn apply_auto_straighten_to_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    update_sidecars_from_images(
        paths,
        app_handle,
        "auto straighten",
        false,
        |image, source_path, _, adjustments| {
            engine::auto_straighten::straighten_adjustments(image, source_path, adjustments)
        },
    )
    .await
}

#[tauri::command]
pub async fn load_metadata(path: String, app_handle: AppHandle) -> Result<ImageMetadata, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(app_handle).unwrap_or_default();
        let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);

        let (source_path, sidecar_path) = parse_virtual_path(&path);
        let mut metadata = crate::exif_processing::load_sidecar(&sidecar_path);

        if enable_xmp_sync && sync_metadata_from_xmp(&source_path, &mut metadata) {
            let _ =
                crate::exif_processing::update_sidecar(&sidecar_path, &source_path, |current| {
                    sync_metadata_from_xmp(&source_path, current);
                });
        }

        if engine::ai_results::has_ai_results(&metadata.adjustments)
            && let Some(source_id) = engine::ai_results::source_id(&source_path)
        {
            engine::ai_results::reconcile_ai_results(
                &mut metadata.adjustments,
                &Value::Null,
                &source_id,
                true,
            );
        }

        metadata
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))
}

#[tauri::command]
pub fn save_metadata_and_update_thumbnail(
    path: String,
    adjustments: Value,
    app_handle: AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let (source_path, sidecar_path) = parse_virtual_path(&path);
    let lens_db = state.lens_db.lock().unwrap().clone();
    let source_id = engine::ai_results::source_id(&source_path);
    let metadata =
        crate::exif_processing::update_sidecar(&sidecar_path, &source_path, |metadata| {
            crate::exif_processing::fill_missing_exif(metadata, &source_path);
            let mut final_adjustments = adjustments;
            if let Some(source_id) = &source_id {
                engine::ai_results::reconcile_ai_results(
                    &mut final_adjustments,
                    &metadata.adjustments,
                    source_id,
                    true,
                );
            }
            resolve_lens_params_in_adjustments(
                &mut final_adjustments,
                &metadata.exif,
                lens_db.as_deref(),
            );
            metadata.adjustments = final_adjustments;
        })
        .map_err(|e| e.to_string())?;

    if let Ok(settings) = load_settings(app_handle.clone())
        && settings.enable_xmp_sync.unwrap_or(false)
    {
        let create_if_missing = settings.create_xmp_if_missing.unwrap_or(false);
        sync_metadata_to_xmp(&source_path, &metadata, create_if_missing);
    }

    Ok(())
}

#[tauri::command]
pub fn clear_all_sidecars(root_path: String) -> Result<usize, String> {
    if !Path::new(&root_path).exists() {
        return Err(format!("Root path does not exist: {}", root_path));
    }

    let mut deleted_count = 0;
    let walker = WalkDir::new(root_path).into_iter();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && let Some(extension) = path.extension()
            && (extension == "rrdata" || extension == "rrexif")
        {
            if fs::remove_file(path).is_ok() {
                deleted_count += 1;
            } else {
                eprintln!("Failed to delete sidecar file: {:?}", path);
            }
        }
    }

    Ok(deleted_count)
}

pub fn generate_filename_from_template(
    template: &str,
    original_path: &std::path::Path,
    sequence: usize,
    total: usize,
    file_date: &DateTime<Utc>,
) -> String {
    let stem = original_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let sequence_str = format!(
        "{:0width$}",
        sequence,
        width = total.to_string().len().max(1)
    );
    let local_date = file_date.with_timezone(&chrono::Local);
    let stem_prefix = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    let filename_number = &stem[stem_prefix.len()..];

    let mut result = template.to_string();
    result = result.replace("{original_filename}", stem);
    result = result.replace("{original_filename_number}", filename_number);
    result = result.replace("{sequence}", &sequence_str);
    result = result.replace("{YYYY}", &local_date.format("%Y").to_string());
    result = result.replace("{MM}", &local_date.format("%m").to_string());
    result = result.replace("{DD}", &local_date.format("%d").to_string());
    result = result.replace("{hh}", &local_date.format("%H").to_string());
    result = result.replace("{mm}", &local_date.format("%M").to_string());

    result
}

pub fn xmp_subject_bag<S: AsRef<str>>(items: &[S]) -> String {
    let mut bag = String::from("<dc:subject>\n    <rdf:Bag>\n");
    for item in items {
        bag.push_str(&format!("     <rdf:li>{}</rdf:li>\n", item.as_ref()));
    }
    bag.push_str("    </rdf:Bag>\n   </dc:subject>");
    bag
}

pub fn extract_xmp_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(start_idx) = content.find("<dc:subject>")
        && let Some(end_idx) = content[start_idx..].find("</dc:subject>")
    {
        let subject_block = &content[start_idx..start_idx + end_idx];
        let mut current_idx = 0;
        while let Some(li_start) = subject_block[current_idx..].find("<rdf:li>") {
            let val_start = current_idx + li_start + 8;
            if let Some(li_end) = subject_block[val_start..].find("</rdf:li>") {
                tags.push(subject_block[val_start..val_start + li_end].to_string());
                current_idx = val_start + li_end + 9;
            } else {
                break;
            }
        }
    }
    tags
}

pub fn resolve_xmp_path(image_path: &Path) -> Option<PathBuf> {
    let xmp_path = image_path.with_extension("xmp");
    let xmp_path_upper = image_path.with_extension("XMP");
    if xmp_path.exists() {
        Some(xmp_path)
    } else if xmp_path_upper.exists() {
        Some(xmp_path_upper)
    } else {
        None
    }
}

pub fn sync_metadata_from_xmp(source_path: &Path, metadata: &mut ImageMetadata) -> bool {
    let mut changed = false;

    if let Some(xmp_file) = resolve_xmp_path(source_path)
        && let Ok(content) = fs::read_to_string(&xmp_file)
    {
        let xmp_tags = extract_xmp_tags(&content);
        let mut current_tags = metadata.tags.clone().unwrap_or_default();
        let original_len = current_tags.len();
        let had_no_tags = metadata.tags.is_none();

        for tag in xmp_tags {
            let tag = quick_xml::escape::unescape(&tag)
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_else(|_| tag.clone());
            if !current_tags.contains(&tag) {
                current_tags.push(tag);
            }
        }

        if current_tags.len() != original_len || (had_no_tags && !current_tags.is_empty()) {
            metadata.tags = Some(current_tags);
            changed = true;
        }
    }
    changed
}

pub fn sync_metadata_to_xmp(source_path: &Path, metadata: &ImageMetadata, create_if_missing: bool) {
    let xmp_path = source_path.with_extension("xmp");
    let xmp_path_upper = source_path.with_extension("XMP");

    let mut actual_xmp = if xmp_path.exists() {
        Some(xmp_path.clone())
    } else if xmp_path_upper.exists() {
        Some(xmp_path_upper.clone())
    } else {
        None
    };

    if actual_xmp.is_none() {
        if !create_if_missing {
            return;
        }
        let skeleton = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="RapidRAW">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
        if let Err(e) = fs::write(&xmp_path, skeleton) {
            log::error!("Failed to create skeleton XMP: {}", e);
            return;
        }
        actual_xmp = Some(xmp_path);
    }

    let Some(xmp_file) = actual_xmp else { return };
    let Ok(mut content) = fs::read_to_string(&xmp_file) else {
        return;
    };

    let re_subject =
        regex::Regex::new(r#"(?s)<dc:subject>\s*<rdf:Bag>.*?</rdf:Bag>\s*</dc:subject>"#).unwrap();
    let tags = metadata.tags.clone().unwrap_or_default();
    if tags.is_empty() {
        content = re_subject.replace_all(&content, "").to_string();
    } else {
        let escaped: Vec<std::borrow::Cow<str>> = tags
            .iter()
            .map(|t| quick_xml::escape::escape(t.as_str()))
            .collect();
        let bag = xmp_subject_bag(&escaped);

        if re_subject.is_match(&content) {
            content = re_subject
                .replace(&content, regex::NoExpand(&bag))
                .to_string();
        } else if let Some(last_index) = content.rfind("</rdf:Description>") {
            let (start, end) = content.split_at(last_index);
            content = format!("{} {}\n  {}", start, bag, end);
        }
    }

    let _ = fs::write(&xmp_file, content);
}
