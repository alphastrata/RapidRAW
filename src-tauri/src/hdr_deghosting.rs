use crate::app_settings::AppSettings;
use crate::exif_processing::{read_exposure_time_secs, read_iso};
use crate::formats::is_raw_file;
use crate::image_loader::load_base_image_from_bytes;
use crate::image_processing::apply_srgb_to_linear;
use engine::hdr_deghosting as deghost;
use image::DynamicImage;
use std::{fs, path::Path, time::Duration};
use tauri::{AppHandle, Emitter};

pub type HdrFrame = (String, DynamicImage, Duration, f32);

pub fn load_hdr_frames(
    paths: &[String],
    app_handle: &AppHandle,
    settings: &AppSettings,
) -> Result<Vec<HdrFrame>, String> {
    assert!(paths.len() >= 2, "hdr merge requires at least two paths");
    paths
        .iter()
        .map(|path| {
            let _ = app_handle.emit(
                "hdr-progress",
                format!(
                    "Processing '{}'",
                    Path::new(path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            );
            let file_bytes =
                fs::read(path).map_err(|e| format!("Failed to read image {}: {}", path, e))?;
            let mut dynamic_image =
                load_base_image_from_bytes(&file_bytes, path, false, settings, None)
                    .map_err(|e| format!("Failed to load image {}: {}", path, e))?;
            if !is_raw_file(path) {
                dynamic_image = apply_srgb_to_linear(dynamic_image);
            }
            let gains = match read_iso(path, &file_bytes) {
                None => return Err(format!("Image {} is missing ISO/Sensitivity data", path)),
                Some(gains) => gains as f32,
            };
            let exposure = match read_exposure_time_secs(path, &file_bytes) {
                None => return Err(format!("Image {} is missing ExposureTime data", path)),
                Some(exp) => Duration::from_secs_f32(exp),
            };
            Ok((path.clone(), dynamic_image, exposure, gains))
        })
        .collect()
}

pub fn assert_uniform_dimensions(frames: &[HdrFrame]) -> Result<(), String> {
    assert!(
        !frames.is_empty(),
        "dimension check requires at least one frame"
    );
    let (first_path, first_image, _, _) = &frames[0];
    let width = first_image.width();
    let height = first_image.height();
    let mismatch = frames
        .iter()
        .skip(1)
        .find(|(_, image, _, _)| image.width() != width || image.height() != height);
    match mismatch {
        None => Ok(()),
        Some((path, image, _, _)) => Err(format!(
            "Dimension mismatch detected.\n\nBase image ({}): {}x{}\nTarget image ({}): {}x{}\n\nHDR merge requires all images to be exactly the same size.",
            Path::new(first_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            width,
            height,
            Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            image.width(),
            image.height()
        )),
    }
}

pub fn align_hdr_frames(frames: &mut [HdrFrame], app_handle: &AppHandle) {
    assert!(!frames.is_empty(), "alignment requires at least one frame");
    let _ = app_handle.emit("hdr-progress", "Deghosting...");
    let reference_index = frames.len() / 2;
    let detections: Vec<deghost::FrameDetection> = frames
        .iter()
        .map(|frame| deghost::detect_frame_features(&frame.1, is_raw_file(&frame.0)))
        .collect();
    frames
        .iter_mut()
        .enumerate()
        .filter(|(index, _)| *index != reference_index)
        .for_each(|(index, frame)| {
            let file_name = Path::new(&frame.0)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let _ = app_handle.emit("hdr-progress", format!("Aligning '{}'...", file_name));
            let aligned = match deghost::align_frame_to_reference(
                &frame.1,
                &detections[index],
                &detections[reference_index],
            ) {
                Ok(deghost::AlignmentOutcome::Warped(warped)) => {
                    frame.1 = DynamicImage::ImageRgb32F(warped);
                    true
                }
                Ok(deghost::AlignmentOutcome::AlreadyAligned) => true,
                Ok(deghost::AlignmentOutcome::Failed) | Err(_) => false,
            };
            if !aligned {
                let _ = app_handle.emit(
                    "hdr-progress",
                    format!("Could not align '{}', using as-is", file_name),
                );
            }
        });
}
