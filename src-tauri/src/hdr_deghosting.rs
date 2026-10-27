use crate::app_settings::AppSettings;
use crate::exif_processing::{read_exposure_time_secs, read_iso};
use crate::formats::is_raw_file;
use crate::image_loader::load_base_image_from_bytes;
use crate::image_processing::{
    apply_cpu_default_raw_processing, apply_linear_to_srgb, apply_srgb_to_linear,
};
use crate::panorama_stitching::{self, Feature as ShellFeature, KeyPoint as ShellKeyPoint};
use crate::panorama_utils::{processing, stitching};
use engine::hdr_deghosting as deghost;
use image::{DynamicImage, GenericImageView, GrayImage, Rgb32FImage};
use nalgebra::{Matrix3, Point2};
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
    let brief_pairs = processing::generate_brief_pairs();
    let prepare_view = |image: &DynamicImage, source_is_raw: bool| -> DynamicImage {
        if source_is_raw {
            let mut proxy = image.clone();
            apply_cpu_default_raw_processing(&mut proxy);
            proxy
        } else {
            apply_linear_to_srgb(image.clone())
        }
    };
    let downscale_dims = |width: u32, height: u32, max_dimension: u32| -> (u32, u32, f64) {
        processing::calculate_downscale_dimensions_capped(width, height, max_dimension)
    };
    let normalize = |img: &GrayImage| -> GrayImage { processing::normalize_grayscale(img) };
    let detect_features =
        |img: &GrayImage, fast_threshold: u8, suppression_radius: f32| -> Vec<deghost::Feature> {
            processing::find_features_tuned(img, &brief_pairs, fast_threshold, suppression_radius)
                .into_iter()
                .map(|feature| deghost::Feature {
                    keypoint: deghost::KeyPoint {
                        x: feature.keypoint.x,
                        y: feature.keypoint.y,
                    },
                    descriptor: feature.descriptor,
                })
                .collect()
        };
    let find_matches =
        |reference: &[deghost::Feature], frame: &[deghost::Feature]| -> Vec<deghost::Match> {
            processing::match_features(&shell_features(reference), &shell_features(frame))
                .iter()
                .map(|m| deghost::Match {
                    index1: m.index1,
                    index2: m.index2,
                })
                .collect()
        };
    let ransac_inliers = |matches: &[deghost::Match],
                          reference: &[deghost::KeyPoint],
                          frame: &[deghost::KeyPoint]|
     -> Option<Vec<deghost::Match>> {
        processing::find_homography_ransac(
            &shell_matches(matches),
            &shell_keypoints(reference),
            &shell_keypoints(frame),
        )
        .map(|(_, inliers)| {
            inliers
                .iter()
                .map(|m| deghost::Match {
                    index1: m.index1,
                    index2: m.index2,
                })
                .collect::<Vec<_>>()
        })
    };
    let warp =
        |source: &Rgb32FImage, transform: &Matrix3<f64>, width: u32, height: u32| -> Rgb32FImage {
            stitching::warp_image_homography(source, transform, width, height)
        };
    let pipeline = deghost::FeaturePipeline {
        prepare_view: &prepare_view,
        downscale_dims: &downscale_dims,
        normalize_grayscale: &normalize,
        detect_features: &detect_features,
        min_inliers: processing::MIN_INLIERS_FOR_CONNECTION,
        find_matches: &find_matches,
        ransac_inliers: &ransac_inliers,
        warp_homography: &warp,
    };
    let reference_index = frames.len() / 2;
    let detections: Vec<deghost::FrameDetection> = frames
        .iter()
        .map(|frame| deghost::detect_frame_features(&frame.1, is_raw_file(&frame.0), &pipeline))
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
                &pipeline,
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

fn shell_keypoints(keypoints: &[deghost::KeyPoint]) -> Vec<ShellKeyPoint> {
    keypoints
        .iter()
        .map(|kp| ShellKeyPoint { x: kp.x, y: kp.y })
        .collect()
}

fn shell_features(features: &[deghost::Feature]) -> Vec<ShellFeature> {
    features
        .iter()
        .map(|feature| ShellFeature {
            keypoint: ShellKeyPoint {
                x: feature.keypoint.x,
                y: feature.keypoint.y,
            },
            descriptor: feature.descriptor,
        })
        .collect()
}

fn shell_matches(matches: &[deghost::Match]) -> Vec<panorama_stitching::Match> {
    matches
        .iter()
        .map(|m| panorama_stitching::Match {
            index1: m.index1,
            index2: m.index2,
        })
        .collect()
}
