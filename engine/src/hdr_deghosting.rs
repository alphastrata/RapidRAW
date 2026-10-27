use crate::image_processing::{apply_cpu_default_raw_processing, apply_linear_to_srgb};
use crate::panorama as pano;
use anyhow::{Result, anyhow};
use image::{DynamicImage, GenericImageView, Rgb32FImage};
use nalgebra::{Matrix2, Matrix3};

pub use pano::{Descriptor, Feature, KeyPoint, Match};

const DEGHOST_FAST_THRESHOLD: u8 = 8;
const DEGHOST_NON_MAXIMA_SUPPRESSION_RADIUS: f32 = 8.0;
const DEGHOST_MAX_PROCESSING_DIMENSION: u32 = 3200;
const DEGHOST_IDENTITY_MAX_DISPLACEMENT: f64 = 1.0;
const DEGHOST_MIN_RIGID_INLIERS: usize = 2;

pub struct FrameDetection {
    pub keypoints: Vec<KeyPoint>,
    pub features: Vec<Feature>,
    pub scale_factor: f64,
}

pub enum AlignmentOutcome {
    Warped(Rgb32FImage),
    AlreadyAligned,
    Failed,
}

pub fn detect_frame_features(image: &DynamicImage, source_is_raw: bool) -> FrameDetection {
    let detection_proxy = if source_is_raw {
        let mut proxy = image.clone();
        apply_cpu_default_raw_processing(&mut proxy);
        proxy
    } else {
        apply_linear_to_srgb(image.clone())
    };
    let gray_full = image::imageops::colorops::grayscale(&detection_proxy.to_rgb8());
    let (width, height) = gray_full.dimensions();

    let (small_width, small_height, scale_factor) = pano::calculate_downscale_dimensions_capped(
        width,
        height,
        DEGHOST_MAX_PROCESSING_DIMENSION,
    );
    let gray_small = image::imageops::resize(
        &gray_full,
        small_width,
        small_height,
        image::imageops::FilterType::Triangle,
    );
    let normalized = pano::normalize_grayscale(&gray_small);

    let brief_pairs = pano::generate_brief_pairs();
    let features = pano::find_features_tuned(
        &normalized,
        &brief_pairs,
        DEGHOST_FAST_THRESHOLD,
        DEGHOST_NON_MAXIMA_SUPPRESSION_RADIUS,
    );
    let keypoints = features.iter().map(|feature| feature.keypoint).collect();
    FrameDetection {
        keypoints,
        features,
        scale_factor,
    }
}

pub fn align_frame_to_reference(
    frame_image: &DynamicImage,
    frame: &FrameDetection,
    reference: &FrameDetection,
) -> Result<AlignmentOutcome> {
    let matches = pano::match_features(&reference.features, &frame.features);
    if matches.len() < pano::MIN_INLIERS_FOR_CONNECTION {
        return Ok(AlignmentOutcome::Failed);
    }
    let Some(inliers) =
        pano::find_homography_ransac(&matches, &reference.keypoints, &frame.keypoints)
            .map(|(_, inliers)| inliers)
    else {
        return Ok(AlignmentOutcome::Failed);
    };
    let rigid_full = estimate_rigid_transform(&inliers, reference, frame)?;
    let (width, height) = frame_image.dimensions();
    let displacement = max_corner_displacement(&rigid_full, width, height);

    if displacement < DEGHOST_IDENTITY_MAX_DISPLACEMENT {
        return Ok(AlignmentOutcome::AlreadyAligned);
    }
    let warped = pano::warp_image_homography(&frame_image.to_rgb32f(), &rigid_full, width, height)?;
    Ok(AlignmentOutcome::Warped(warped))
}

fn estimate_rigid_transform(
    inliers: &[Match],
    reference: &FrameDetection,
    frame: &FrameDetection,
) -> Result<Matrix3<f64>> {
    if inliers.len() < DEGHOST_MIN_RIGID_INLIERS {
        return Err(anyhow!("rigid estimate requires at least two inliers"));
    }

    let pairs: Vec<((f64, f64), (f64, f64))> = inliers
        .iter()
        .map(|m| {
            let reference_point = reference
                .keypoints
                .get(m.index1)
                .ok_or_else(|| anyhow!("inlier index out of range"))?;
            let frame_point = frame
                .keypoints
                .get(m.index2)
                .ok_or_else(|| anyhow!("inlier index out of range"))?;
            Ok((
                (f64::from(reference_point.x), f64::from(reference_point.y)),
                (f64::from(frame_point.x), f64::from(frame_point.y)),
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    let count = pairs.len() as f64;

    let reference_centroid = centroid(pairs.iter().map(|(r, _)| *r), count)?;
    let frame_centroid = centroid(pairs.iter().map(|(_, f)| *f), count)?;

    let (h00, h01, h10, h11) = pairs.iter().fold(
        (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64),
        |(sum_h00, sum_h01, sum_h10, sum_h11), ((rx, ry), (fx, fy))| {
            let (ax, ay) = (rx - reference_centroid.0, ry - reference_centroid.1);
            let (bx, by) = (fx - frame_centroid.0, fy - frame_centroid.1);
            (
                sum_h00 + ax * bx,
                sum_h01 + ax * by,
                sum_h10 + ay * bx,
                sum_h11 + ay * by,
            )
        },
    );

    let covariance = Matrix2::new(h00, h01, h10, h11);
    let svd = covariance.svd(true, true);
    let u = svd.u.ok_or_else(|| anyhow!("svd failed to produce u"))?;
    let v = svd
        .v_t
        .ok_or_else(|| anyhow!("svd failed to produce v_t"))?
        .transpose();
    let rotation = rigid_rotation(&u, &v);

    let tx = frame_centroid.0
        - (rotation[(0, 0)] * reference_centroid.0 + rotation[(0, 1)] * reference_centroid.1);
    let ty = frame_centroid.1
        - (rotation[(1, 0)] * reference_centroid.0 + rotation[(1, 1)] * reference_centroid.1);

    let (scale_x, scale_y) = (frame.scale_factor, frame.scale_factor);
    Ok(Matrix3::new(
        rotation[(0, 0)],
        rotation[(0, 1)],
        tx * scale_x,
        rotation[(1, 0)],
        rotation[(1, 1)],
        ty * scale_y,
        0.0,
        0.0,
        1.0,
    ))
}

fn rigid_rotation(u: &Matrix2<f64>, v: &Matrix2<f64>) -> Matrix2<f64> {
    let mut rotation = v * u.transpose();

    if rotation.determinant() < 0.0 {
        let mut corrected = *v;
        corrected[(0, 1)] = -corrected[(0, 1)];
        corrected[(1, 1)] = -corrected[(1, 1)];
        rotation = corrected * u.transpose();
    }
    rotation
}

fn centroid(points: impl Iterator<Item = (f64, f64)>, count: f64) -> Result<(f64, f64)> {
    if count <= 0.0 {
        return Err(anyhow!("centroid requires a positive count"));
    }
    let (sum_x, sum_y) = points.fold((0.0_f64, 0.0_f64), |sum, (x, y)| (sum.0 + x, sum.1 + y));
    Ok((sum_x / count, sum_y / count))
}

fn max_corner_displacement(transform: &Matrix3<f64>, width: u32, height: u32) -> f64 {
    let (width_f, height_f) = (f64::from(width), f64::from(height));
    let (m11, m12, m13) = (transform.m11, transform.m12, transform.m13);
    let (m21, m22, m23) = (transform.m21, transform.m22, transform.m23);

    [
        (0.0, 0.0),
        (width_f, 0.0),
        (0.0, height_f),
        (width_f, height_f),
    ]
    .into_iter()
    .fold(0.0_f64, |max_displacement, (x, y)| {
        let mapped_x = m11 * x + m12 * y + m13;
        let mapped_y = m21 * x + m22 * y + m23;
        let displacement =
            ((mapped_x - x) * (mapped_x - x) + (mapped_y - y) * (mapped_y - y)).sqrt();
        max_displacement.max(displacement)
    })
}
