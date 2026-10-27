use crate::panorama::{
    BRIEF_DESCRIPTOR_SIZE, BriefPairs, Descriptor, Feature, ImageInfo, KeyPoint,
    MIN_INLIERS_FOR_CONNECTION, Match,
};
use image::{DynamicImage, GrayImage, ImageBuffer, Luma};
use imageproc::corners::{Corner, corners_fast9};
use nalgebra::{Matrix3, Point2, RowDVector, SVD};
use rayon::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PROCESSING_DIMENSION: u32 = 1600;
const FAST_THRESHOLD: u8 = 15;
const NON_MAXIMA_SUPPRESSION_RADIUS: f32 = 15.0;
const DETECTION_BLUR_SIGMA: f32 = 1.5;
const DESCRIPTOR_BLUR_SIGMA: f32 = 2.0;
const BRIEF_PATCH_SIZE: u32 = 32;
const BRIEF_HALF_PATCH: i32 = 16;
const BRIEF_COORD_RANGE: u64 = 32;
const BRIEF_PAIR_SEED: u64 = 0x12_34_56_78_9a_bc_de_f0;
const MATCH_RATIO_THRESHOLD: f32 = 0.8;
const RANSAC_ITERATIONS: usize = 2500;
const RANSAC_INLIER_THRESHOLD: f64 = 5.0;
const LOW_DETAIL_WINDOW_RADIUS: i64 = 16;
const LOW_DETAIL_VARIANCE_THRESHOLD: f64 = 60.0;

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E_37_79_B9_7F_4A_7C_15);
    let mixed = *state;
    let mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF_58_47_6D_1C_E4_E5_B9);
    let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94_D0_49_BB_13_31_11_EB);
    mixed ^ (mixed >> 31)
}

fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

fn u64_to_f64(value: u64) -> f64 {
    value as f64
}

fn u128_to_f64(value: u128) -> f64 {
    value as f64
}

fn entropy_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(BRIEF_PAIR_SEED, |elapsed| elapsed.as_nanos() as u64)
}

fn sample_below(state: &mut u64, range: u64) -> i32 {
    (splitmix64(state) % range) as i32
}

pub fn calculate_downscale_dimensions(width: u32, height: u32) -> (u32, u32, f64) {
    calculate_downscale_dimensions_capped(width, height, MAX_PROCESSING_DIMENSION)
}

pub fn calculate_downscale_dimensions_capped(
    width: u32,
    height: u32,
    max_dimension: u32,
) -> (u32, u32, f64) {
    let long_side = width.max(height);
    if long_side <= max_dimension {
        return (width, height, 1.0);
    }

    let scale_factor = f64::from(long_side) / f64::from(max_dimension);
    let new_width = (f64::from(width) / scale_factor).round() as u32;
    let new_height = (f64::from(height) / scale_factor).round() as u32;
    (new_width, new_height, scale_factor)
}

pub fn normalize_grayscale(img: &GrayImage) -> GrayImage {
    let (minimum, maximum) = img.pixels().fold((u8::MAX, u8::MIN), |(lo, hi), pixel| {
        let value = pixel[0];
        (lo.min(value), hi.max(value))
    });
    if maximum <= minimum {
        return img.clone();
    }
    let span = f32::from(maximum - minimum);
    let (width, height) = img.dimensions();
    GrayImage::from_fn(width, height, |x, y| {
        let value = img.get_pixel(x, y)[0];
        let stretched = f32::from(value - minimum) / span * 255.0;
        Luma([stretched.round() as u8])
    })
}

pub fn prepare_image(image: &DynamicImage, brief_pairs: &BriefPairs) -> ImageInfo {
    let image_f32 = image.to_rgb32f();
    let color_full_u8 = image.to_rgb8();
    let gray_full = image::imageops::colorops::grayscale(&color_full_u8);

    let (width, height) = gray_full.dimensions();
    let (new_width, new_height, scale_factor) = calculate_downscale_dimensions(width, height);

    let gray_small = image::imageops::resize(
        &gray_full,
        new_width,
        new_height,
        image::imageops::FilterType::Triangle,
    );

    let features = find_features(&gray_small, brief_pairs);
    let low_detail_mask =
        generate_low_detail_mask(&gray_full).unwrap_or_else(|| GrayImage::new(width, height));

    ImageInfo {
        image: image_f32,
        low_detail_mask,
        scale_factor,
        features,
    }
}

pub fn find_features(img: &GrayImage, brief_pairs: &BriefPairs) -> Vec<Feature> {
    find_features_tuned(
        img,
        brief_pairs,
        FAST_THRESHOLD,
        NON_MAXIMA_SUPPRESSION_RADIUS,
    )
}

pub fn find_features_tuned(
    img: &GrayImage,
    brief_pairs: &BriefPairs,
    fast_threshold: u8,
    non_maxima_suppression_radius: f32,
) -> Vec<Feature> {
    let blurred_img_u8 = imageproc::filter::gaussian_blur_f32(img, DETECTION_BLUR_SIGMA);
    let corners = corners_fast9(&blurred_img_u8, fast_threshold);
    let keypoints = non_maximal_suppression(&corners, non_maxima_suppression_radius);
    let blurred_img_f32 =
        imageproc::filter::gaussian_blur_f32(&convert_gray_u8_to_f32(img), DESCRIPTOR_BLUR_SIGMA);
    keypoints
        .into_par_iter()
        .filter_map(|kp| {
            compute_brief_descriptor(&blurred_img_f32, &kp, BRIEF_PATCH_SIZE, brief_pairs).map(
                |descriptor| Feature {
                    keypoint: kp,
                    descriptor,
                },
            )
        })
        .collect()
}

fn non_maximal_suppression(corners: &[Corner], radius: f32) -> Vec<KeyPoint> {
    let radius_sq = radius * radius;
    let mut sorted_corners: Vec<Corner> = corners.to_vec();
    sorted_corners.sort_by(|a, b| b.score.total_cmp(&a.score));

    sorted_corners
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            sorted_corners[..*index]
                .iter()
                .all(|kept| !keypoints_conflict(kept, candidate, radius_sq))
        })
        .map(|(_, corner)| KeyPoint {
            x: corner.x,
            y: corner.y,
        })
        .collect()
}

fn keypoints_conflict(kept: &Corner, candidate: &Corner, radius: f32) -> bool {
    let radius_sq = f64::from(radius) * f64::from(radius);
    let dx = f64::from(kept.x) - f64::from(candidate.x);
    let dy = f64::from(kept.y) - f64::from(candidate.y);
    dx.mul_add(dx, dy * dy) < radius_sq
}

pub fn generate_brief_pairs() -> BriefPairs {
    let mut state = BRIEF_PAIR_SEED;
    (0..BRIEF_DESCRIPTOR_SIZE)
        .map(|_| (brief_sample(&mut state), brief_sample(&mut state)))
        .collect()
}

fn brief_sample(state: &mut u64) -> Point2<i32> {
    Point2::new(
        sample_below(state, BRIEF_COORD_RANGE) - BRIEF_HALF_PATCH,
        sample_below(state, BRIEF_COORD_RANGE) - BRIEF_HALF_PATCH,
    )
}

fn compute_brief_descriptor(
    img: &ImageBuffer<Luma<f32>, Vec<f32>>,
    kp: &KeyPoint,
    patch_size: u32,
    pairs: &BriefPairs,
) -> Option<Descriptor> {
    let (width, height) = img.dimensions();
    let half_patch_size = patch_size / 2;
    let in_bounds = kp.x >= half_patch_size
        && kp.x < width - half_patch_size
        && kp.y >= half_patch_size
        && kp.y < height - half_patch_size;
    if !in_bounds {
        return None;
    }
    Some(pairs.iter().enumerate().fold(
        [0u8; BRIEF_DESCRIPTOR_SIZE / 8],
        |mut descriptor, (pair_index, pair)| {
            let (Some(first_x), Some(first_y), Some(second_x), Some(second_y)) = (
                kp.x.checked_add_signed(pair.0.x),
                kp.y.checked_add_signed(pair.0.y),
                kp.x.checked_add_signed(pair.1.x),
                kp.y.checked_add_signed(pair.1.y),
            ) else {
                return descriptor;
            };
            if img.get_pixel(first_x, first_y)[0] < img.get_pixel(second_x, second_y)[0] {
                descriptor[pair_index / 8] |= 1 << (pair_index % 8);
            }
            descriptor
        },
    ))
}

fn hamming_distance(d1: &Descriptor, d2: &Descriptor) -> u32 {
    d1.iter()
        .zip(d2.iter())
        .map(|(b1, b2)| (b1 ^ b2).count_ones())
        .sum()
}

pub fn match_features(features1: &[Feature], features2: &[Feature]) -> Vec<Match> {
    features1
        .par_iter()
        .enumerate()
        .filter_map(|(i1, f1)| {
            let (best_dist, second_best_dist, best_idx) = features2.iter().enumerate().fold(
                (u32::MAX, u32::MAX, 0),
                |(best, second, best_idx), (i2, f2)| {
                    let dist = hamming_distance(&f1.descriptor, &f2.descriptor);
                    if dist < best {
                        (dist, best, i2)
                    } else if dist < second {
                        (best, dist, best_idx)
                    } else {
                        (best, second, best_idx)
                    }
                },
            );

            let ratio = best_dist as f32 / second_best_dist as f32;
            (second_best_dist > 0 && ratio < MATCH_RATIO_THRESHOLD).then_some(Match {
                index1: i1,
                index2: best_idx,
            })
        })
        .collect()
}

pub fn find_homography_ransac(
    matches: &[Match],
    keypoints1: &[KeyPoint],
    keypoints2: &[KeyPoint],
) -> Option<(Matrix3<f64>, Vec<Match>)> {
    let points: Vec<(Point2<f64>, Point2<f64>)> = matches
        .iter()
        .map(|m| {
            let p1 = keypoints1.get(m.index1)?;
            let p2 = keypoints2.get(m.index2)?;
            Some((
                Point2::new(f64::from(p1.x), f64::from(p1.y)),
                Point2::new(f64::from(p2.x), f64::from(p2.y)),
            ))
        })
        .collect::<Option<Vec<_>>>()?;

    if points.len() < 4 {
        return None;
    }

    let ransac_inlier_threshold_sq = RANSAC_INLIER_THRESHOLD.powi(2);
    let mut state = entropy_seed();

    let (best_inliers, best_h) = (0..RANSAC_ITERATIONS)
        .filter_map(|_| {
            let sample_points = sample_points(&mut state, &points);
            let degenerate = DEGENERATE_TRIPLES.iter().any(|&(a, b, c)| {
                are_points_collinear(sample_points[a].0, sample_points[b].0, sample_points[c].0)
            });
            if degenerate {
                return None;
            }
            let h = compute_homography(&sample_points)?;
            Some((
                inlier_matches(&h, matches, &points, ransac_inlier_threshold_sq),
                h,
            ))
        })
        .max_by_key(|(inliers, _)| inliers.len())?;

    (best_inliers.len() >= MIN_INLIERS_FOR_CONNECTION).then_some((best_h, best_inliers))
}

const DEGENERATE_TRIPLES: [(usize, usize, usize); 4] = [(0, 1, 2), (0, 1, 3), (0, 2, 3), (1, 2, 3)];

fn sample_points(
    state: &mut u64,
    points: &[(Point2<f64>, Point2<f64>)],
) -> [(Point2<f64>, Point2<f64>); 4] {
    let len_u = points.len() as u64;
    let indices: [usize; 4] = core::array::from_fn(|_| (splitmix64(state) % len_u) as usize);
    indices.map(|index| points[index])
}

fn inlier_matches(
    h: &Matrix3<f64>,
    matches: &[Match],
    points: &[(Point2<f64>, Point2<f64>)],
    threshold_sq: f64,
) -> Vec<Match> {
    matches
        .par_iter()
        .zip(points.par_iter())
        .filter_map(|(m, (p1, p2))| {
            let transformed = h * nalgebra::Point3::new(p1.x, p1.y, 1.0);
            if transformed.z.abs() < 1e-8 {
                return None;
            }
            let projected =
                Point2::new(transformed.x / transformed.z, transformed.y / transformed.z);
            let dist_sq = (p2.x - projected.x).powi(2) + (p2.y - projected.y).powi(2);
            (dist_sq < threshold_sq).then_some(*m)
        })
        .collect()
}

fn are_points_collinear(p1: Point2<f64>, p2: Point2<f64>, p3: Point2<f64>) -> bool {
    let area = p1.x * (p2.y - p3.y) + p2.x * (p3.y - p1.y) + p3.x * (p1.y - p2.y);
    area.abs() < 1e-6
}

pub fn compute_homography(points: &[(Point2<f64>, Point2<f64>)]) -> Option<Matrix3<f64>> {
    if points.len() < 4 {
        return None;
    }
    let a_rows: Vec<RowDVector<f64>> = points
        .iter()
        .flat_map(|(p1, p2)| {
            let (x, y) = (p1.x, p1.y);
            let (xp, yp) = (p2.x, p2.y);
            [
                RowDVector::from_vec(vec![-x, -y, -1.0, 0.0, 0.0, 0.0, x * xp, y * xp, xp]),
                RowDVector::from_vec(vec![0.0, 0.0, 0.0, -x, -y, -1.0, x * yp, y * yp, yp]),
            ]
        })
        .collect();
    let a = nalgebra::DMatrix::from_rows(&a_rows);
    let svd = SVD::new(a, true, true);
    let v_t = svd.v_t?;
    let h_vec = v_t.row(v_t.nrows() - 1).transpose();
    Some(Matrix3::from_iterator(h_vec.iter().cloned()).transpose())
}

fn convert_gray_u8_to_f32(img: &GrayImage) -> ImageBuffer<Luma<f32>, Vec<f32>> {
    let (width, height) = img.dimensions();
    ImageBuffer::from_fn(width, height, |x, y| {
        Luma([f32::from(img.get_pixel(x, y)[0]) / 255.0])
    })
}

fn build_integral_images(gray: &GrayImage) -> (Vec<u64>, Vec<u128>) {
    let raw = gray.as_raw();
    let width = usize::try_from(gray.width()).unwrap_or(0);
    if width == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut sat = Vec::with_capacity(raw.len());
    let mut sat_sq = Vec::with_capacity(raw.len());

    raw.chunks_exact(width).enumerate().for_each(|(y, row)| {
        let base_index = y * width;
        row.iter()
            .copied()
            .enumerate()
            .fold((0u64, 0u128), |(row_sum, row_sum_sq), (x, value)| {
                let value_u = u64::from(value);
                let value_sq = u128::from(value_u) * u128::from(value_u);
                let row_sum = row_sum + value_u;
                let row_sum_sq = row_sum_sq + value_sq;
                let index = base_index + x;
                let (above_sum, above_sq) = if y > 0 {
                    (sat[index - width], sat_sq[index - width])
                } else {
                    (0, 0)
                };
                sat.push(row_sum + above_sum);
                sat_sq.push(row_sum_sq + above_sq);
                (row_sum, row_sum_sq)
            });
    });
    (sat, sat_sq)
}

pub fn generate_low_detail_mask(gray_full: &GrayImage) -> Option<GrayImage> {
    let (width, height) = gray_full.dimensions();
    let width_i = i64::from(width);
    let height_i = i64::from(height);
    let (sat, sat_sq) = build_integral_images(gray_full);
    let radius = LOW_DETAIL_WINDOW_RADIUS;

    let sat_at = |s: &[u64], x: i64, y: i64| -> u64 {
        if x < 0 || y < 0 {
            return 0;
        }
        s[usize::try_from(y * width_i + x).unwrap_or(usize::MAX)]
    };
    let sat_sq_at = |s: &[u128], x: i64, y: i64| -> u128 {
        if x < 0 || y < 0 {
            return 0;
        }
        s[usize::try_from(y * width_i + x).unwrap_or(usize::MAX)]
    };

    let mask: Vec<u8> = (0..height_i)
        .into_par_iter()
        .flat_map_iter(|y| {
            (0..width_i)
                .map(|x| {
                    let (x1, y1) = (x - radius - 1, y - radius - 1);
                    let (x2, y2) = (
                        (x + radius).min(width_i - 1),
                        (y + radius).min(height_i - 1),
                    );

                    let n = i64_to_f64((x2 - x1) * (y2 - y1));
                    if n < 1.0 {
                        return 0;
                    }

                    let sum = sat_at(&sat, x2, y2) + sat_at(&sat, x1, y1)
                        - sat_at(&sat, x2, y1)
                        - sat_at(&sat, x1, y2);
                    let sum_sq = sat_sq_at(&sat_sq, x2, y2) + sat_sq_at(&sat_sq, x1, y1)
                        - sat_sq_at(&sat_sq, x2, y1)
                        - sat_sq_at(&sat_sq, x1, y2);

                    let mean = u64_to_f64(sum) / n;
                    let variance = u128_to_f64(sum_sq) / n - mean.powi(2);
                    if variance < LOW_DETAIL_VARIANCE_THRESHOLD {
                        255
                    } else {
                        0
                    }
                })
                .collect::<Vec<u8>>()
        })
        .collect();

    GrayImage::from_raw(width, height, mask)
}
