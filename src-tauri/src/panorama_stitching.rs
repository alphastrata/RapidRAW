use crate::app_settings::load_settings;
use crate::app_state::AppState;
use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose};
use engine::panorama::{self as pano, ImageInfo};
use engine::paths::parse_virtual_path;
use image::{DynamicImage, GenericImageView, ImageFormat, Rgb32FImage};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::formats::is_raw_file;
use crate::image_processing::apply_cpu_default_raw_processing;

#[tauri::command]
pub async fn stitch_panorama(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if paths.len() < 2 {
        return Err("Please select at least two images to stitch.".to_string());
    }

    let source_paths: Vec<String> = paths
        .iter()
        .map(|p| parse_virtual_path(p).0.to_string_lossy().into_owned())
        .collect();

    let panorama_result_handle = state.panorama_result.clone();

    let task = tokio::task::spawn_blocking(move || {
        let panorama_result = stitch_images(source_paths, app_handle.clone());

        match panorama_result {
            Ok(panorama_image) => {
                let _ = app_handle.emit("panorama-progress", "Creating preview...");

                let (w, h) = panorama_image.dimensions();
                let (w_f, h_f) = (w as f32, h as f32);
                let (new_w, new_h) = if w > h {
                    (800, (800.0 * h_f / w_f).round() as u32)
                } else {
                    ((800.0 * w_f / h_f).round() as u32, 800)
                };

                let preview_f32 =
                    crate::image_processing::downscale_f32_image(&panorama_image, new_w, new_h);

                let preview_u8 = preview_f32.to_rgb8();

                let mut buf = Cursor::new(Vec::new());

                if let Err(e) = preview_u8.write_to(&mut buf, ImageFormat::Png) {
                    return Err(format!("Failed to encode panorama preview: {}", e));
                }

                let base64_str = general_purpose::STANDARD.encode(buf.get_ref());
                let final_base64 = format!("data:image/png;base64,{}", base64_str);

                *panorama_result_handle
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(panorama_image);

                let _ = app_handle.emit(
                    "panorama-complete",
                    serde_json::json!({
                        "base64": final_base64,
                    }),
                );
                Ok(())
            }
            Err(e) => {
                let _ = app_handle.emit("panorama-error", e.to_string());
                Err(e.to_string())
            }
        }
    });

    match task.await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(join_err) => Err(format!("Panorama task failed: {}", join_err)),
    }
}

#[tauri::command]
pub async fn save_panorama(
    first_path_str: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let panorama_image = state
        .panorama_result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .ok_or_else(|| {
            "No panorama image found in memory to save. It might have already been saved."
                .to_string()
        })?;

    let (first_path, _) = parse_virtual_path(&first_path_str);
    let parent_dir = first_path
        .parent()
        .ok_or_else(|| "Could not determine parent directory of the first image.".to_string())?;
    let stem = first_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("panorama");

    let (output_filename, image_to_save): (String, DynamicImage) =
        if panorama_image.color().has_alpha() {
            (
                format!("{}_Pano.png", stem),
                DynamicImage::ImageRgba8(panorama_image.to_rgba8()),
            )
        } else if panorama_image.as_rgb32f().is_some() {
            (format!("{}_Pano.tiff", stem), panorama_image)
        } else {
            (
                format!("{}_Pano.png", stem),
                DynamicImage::ImageRgb8(panorama_image.to_rgb8()),
            )
        };

    let output_path = parent_dir.join(output_filename);

    image_to_save
        .save(&output_path)
        .map_err(|e| format!("Failed to save panorama image: {}", e))?;

    let (real_path, _) = engine::paths::parse_virtual_path(&first_path_str);
    let _ =
        crate::exif_processing::write_rrexif_sidecar(&real_path.to_string_lossy(), &output_path);

    Ok(output_path.to_string_lossy().to_string())
}

fn stitch_images(image_paths: Vec<String>, app_handle: AppHandle) -> anyhow::Result<DynamicImage> {
    if image_paths.len() < 2 {
        bail!("At least two images are required for a panorama.");
    }

    let _ = app_handle.emit("panorama-progress", "Starting panorama process...");
    println!(
        "Starting panorama stitching process for {} images...",
        image_paths.len()
    );

    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let start_time = Instant::now();
    let _ = app_handle.emit("panorama-progress", "Loading and preparing images...");
    println!("Loading and preparing images (in parallel)...");
    let brief_pairs = pano::generate_brief_pairs();

    let image_data: Vec<ImageInfo> = image_paths
        .par_iter()
        .enumerate()
        .map(|(_i, filename)| {
            let _ = app_handle.emit(
                "panorama-progress",
                format!(
                    "Processing '{}'",
                    Path::new(filename)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            );
            println!("  - Processing '{}'", filename);

            let file_bytes =
                fs::read(filename).with_context(|| format!("Failed to read image {filename}"))?;

            let mut dynamic_image = crate::image_loader::load_base_image_from_bytes(
                &file_bytes,
                filename,
                false,
                &settings,
                None,
            )
            .with_context(|| format!("Failed to load image {filename}"))?;

            if is_raw_file(filename) {
                apply_cpu_default_raw_processing(&mut dynamic_image);
            }

            let info = pano::prepare_image(&dynamic_image, &brief_pairs);
            println!(
                "    Found {} features in '{}'",
                info.features.len(),
                filename
            );

            Ok(info)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    println!(
        "Image loading and feature detection completed in {:.2?}\n",
        start_time.elapsed()
    );

    let start_time = Instant::now();
    let _ = app_handle.emit("panorama-progress", "Finding image matches...");
    println!("Finding all pairwise matches (in parallel)...");
    let pairwise_matches: HashMap<(usize, usize), pano::MatchInfo> =
        pano::find_pairwise_matches(&image_data);
    println!(
        "Pairwise matching completed in {:.2?}\n",
        start_time.elapsed()
    );

    if pairwise_matches.is_empty() {
        bail!("No suitable matches found between any pair of images. Cannot create a panorama.");
    }

    let start_time = Instant::now();
    let _ = app_handle.emit("panorama-progress", "Determining stitching order...");
    println!("Determining stitching order...");
    let (ordered_indices, global_homographies) =
        pano::build_stitching_order(image_data.len(), &pairwise_matches).map_err(|e| {
            e.context("Could not determine a connected stitching order for these images.")
        })?;

    if ordered_indices.len() < 2 {
        bail!("Could not find a connected sequence of at least two images.");
    }

    let ordered_filenames: Vec<String> = ordered_indices
        .iter()
        .map(|&i| {
            Path::new(&image_paths[i])
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    println!("Stitching order determined: {:?}", ordered_filenames);
    let _ = app_handle.emit(
        "panorama-progress",
        format!("Stitching order: {}", ordered_filenames.join(" -> ")),
    );

    let unstitched_count = image_data.len() - ordered_indices.len();
    if unstitched_count > 0 {
        let warning_msg = format!(
            "Warning: {} image(s) could not be matched and will be excluded.",
            unstitched_count
        );
        println!("{}", warning_msg);
        let _ = app_handle.emit("panorama-warning", warning_msg);
    }
    println!(
        "Global homography calculation completed in {:.2?}\n",
        start_time.elapsed()
    );

    let start_time = Instant::now();
    let _ = app_handle.emit("panorama-progress", "Warping and blending images...");
    println!("Warping and blending full-resolution images with progressive optimal seams...");

    let stitched_images_info: Vec<&ImageInfo> =
        ordered_indices.iter().map(|&i| &image_data[i]).collect();
    let ordered_names = ordered_filenames.clone();
    let progress = |index: usize, total: usize| {
        let name = ordered_names
            .get(index + 1)
            .map_or_else(String::new, |name| name.clone());
        let _ = app_handle.emit(
            "panorama-progress",
            format!("Stitching image {} of {}: {}", index + 2, total, name),
        );
    };
    let panorama: Rgb32FImage =
        pano::progressive_seam_stitcher(&stitched_images_info, &global_homographies, &progress)?;

    println!("Stitching completed in {:.2?}\n", start_time.elapsed());

    let _ = app_handle.emit("panorama-progress", "Finalizing panorama...");

    Ok(DynamicImage::ImageRgb32F(panorama))
}
