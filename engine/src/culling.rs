use image::{DynamicImage, GenericImageView, GrayImage, imageops};
use image_hasher::ImageHash;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::VecDeque;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CullingSettings {
    pub similarity_threshold: u32,
    pub blur_threshold: f64,
    pub group_similar: bool,
    pub filter_blurry: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImageAnalysisResult {
    pub path: String,
    pub quality_score: f64,
    pub sharpness_metric: f64,
    pub center_focus_metric: f64,
    pub exposure_metric: f64,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CullGroup {
    pub representative: ImageAnalysisResult,
    pub duplicates: Vec<ImageAnalysisResult>,
}

#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CullingSuggestions {
    pub similar_groups: Vec<CullGroup>,
    pub blurry_images: Vec<ImageAnalysisResult>,
    pub failed_paths: Vec<String>,
}

pub struct ImageAnalysisData {
    pub hash: ImageHash,
    pub result: ImageAnalysisResult,
}

const WEIGHT_SHARPNESS: f64 = 0.40;
const WEIGHT_CENTER_FOCUS: f64 = 0.35;
const WEIGHT_EXPOSURE: f64 = 0.25;
const MIN_LAPLACIAN_DIM: u32 = 3;
const LAPLACIAN_CENTRE_WEIGHT: i32 = 4;
const CROP_QUARTER: u32 = 4;
const CROP_HALF: u32 = 2;
const EXPOSURE_CLIP_DARK: usize = 5;
const EXPOSURE_CLIP_BRIGHT: usize = 250;
const EXPOSURE_CHANNEL_LEN: usize = 256;
const EXPOSURE_PENALTY_SCALE: f64 = 5.0;
const QUALITY_LOG_SCALE: f64 = 3.5;
const MIN_GROUP_SIZE: usize = 2;
const VARIANCE_POWER: i32 = 2;

fn calculate_laplacian_variance(image: &GrayImage) -> f64 {
    let (width, height) = image.dimensions();

    if width < MIN_LAPLACIAN_DIM || height < MIN_LAPLACIAN_DIM {
        return 0.0;
    }

    let laplacian_values: Vec<f64> = (1..height - 1)
        .flat_map(|y| (1..width - 1).map(move |x| (y, x)))
        .map(|(y, x)| {
            let p_center = i32::from(image.get_pixel(x, y)[0]);
            let p_north = i32::from(image.get_pixel(x, y - 1)[0]);
            let p_south = i32::from(image.get_pixel(x, y + 1)[0]);
            let p_west = i32::from(image.get_pixel(x - 1, y)[0]);
            let p_east = i32::from(image.get_pixel(x + 1, y)[0]);

            f64::from(p_north + p_south + p_west + p_east - LAPLACIAN_CENTRE_WEIGHT * p_center)
        })
        .collect();

    if laplacian_values.is_empty() {
        return 0.0;
    }

    let sample_count = laplacian_values.len() as f64;
    let mean = laplacian_values.iter().sum::<f64>() / sample_count;

    laplacian_values
        .iter()
        .map(|v| (v - mean).powi(VARIANCE_POWER))
        .sum::<f64>()
        / sample_count
}

fn calculate_exposure_metric(image: &GrayImage) -> f64 {
    let histogram = imageproc::stats::histogram(image);

    let Some(channel) = histogram.channels.first() else {
        return 0.0;
    };

    let total_pixels = f64::from(image.width() * image.height());

    if total_pixels == 0.0 {
        return 0.0;
    }

    let dark_pixels = f64::from(channel[0..EXPOSURE_CLIP_DARK].iter().sum::<u32>());
    let bright_pixels = f64::from(
        channel[EXPOSURE_CLIP_BRIGHT..EXPOSURE_CHANNEL_LEN]
            .iter()
            .sum::<u32>(),
    );

    let dark_clip_ratio = dark_pixels / total_pixels;
    let bright_clip_ratio = bright_pixels / total_pixels;

    let penalty =
        (dark_clip_ratio * EXPOSURE_PENALTY_SCALE) + (bright_clip_ratio * EXPOSURE_PENALTY_SCALE);

    (1.0f64 - penalty).max(0.0)
}

pub fn analyze_image(
    path: &str,
    image: &DynamicImage,
    hasher: &image_hasher::Hasher,
) -> ImageAnalysisData {
    const ANALYSIS_DIM: u32 = 720;

    let (width, height) = image.dimensions();

    let thumbnail = image.thumbnail(ANALYSIS_DIM, ANALYSIS_DIM);
    let gray_thumbnail = thumbnail.to_luma8();

    let sharpness_metric = calculate_laplacian_variance(&gray_thumbnail);
    let exposure_metric = calculate_exposure_metric(&gray_thumbnail);

    let (thumb_w, thumb_h) = gray_thumbnail.dimensions();

    let center_crop = imageops::crop_imm(
        &gray_thumbnail,
        thumb_w / CROP_QUARTER,
        thumb_h / CROP_QUARTER,
        thumb_w / CROP_HALF,
        thumb_h / CROP_HALF,
    )
    .to_image();

    let center_focus_metric = calculate_laplacian_variance(&center_crop);

    let normalized_sharpness = ((sharpness_metric + 1.0).log10() / QUALITY_LOG_SCALE).min(1.0);
    let normalized_center_focus =
        ((center_focus_metric + 1.0).log10() / QUALITY_LOG_SCALE).min(1.0);

    let quality_score = (normalized_sharpness * WEIGHT_SHARPNESS)
        + (normalized_center_focus * WEIGHT_CENTER_FOCUS)
        + (exposure_metric * WEIGHT_EXPOSURE);

    let hash = hasher.hash_image(&thumbnail);

    ImageAnalysisData {
        hash,
        result: ImageAnalysisResult {
            path: path.to_string(),
            quality_score,
            sharpness_metric,
            center_focus_metric,
            exposure_metric,
            width,
            height,
        },
    }
}

fn collect_group(
    start: usize,
    items: &[Option<ImageAnalysisData>],
    processed: &mut [bool],
    threshold: u32,
) -> Vec<usize> {
    processed[start] = true;
    let mut group = vec![start];
    let mut queue = VecDeque::from([start]);

    while let Some(current_idx) = queue.pop_front() {
        let members: Vec<usize> = (current_idx + 1..items.len())
            .filter(|&j| !processed[j])
            .filter(|&j| {
                items[j]
                    .as_ref()
                    .zip(items[current_idx].as_ref())
                    .is_some_and(|(data, source)| data.hash.dist(&source.hash) <= threshold)
            })
            .collect();

        members.into_iter().for_each(|j| {
            processed[j] = true;
            group.push(j);
            queue.push_back(j);
        });
    }

    group
}

fn sort_group_indices(indices: &mut [usize], items: &[Option<ImageAnalysisData>]) {
    indices.sort_by(|&a, &b| {
        let ordering = items[a]
            .as_ref()
            .zip(items[b].as_ref())
            .and_then(|(x, y)| x.result.quality_score.partial_cmp(&y.result.quality_score));

        ordering.unwrap_or(Ordering::Equal)
    });
}

fn extract_group(indices: &[usize], items: &mut [Option<ImageAnalysisData>]) -> Option<CullGroup> {
    let (&representative_idx, duplicate_indices) = indices.split_first()?;
    let representative = items[representative_idx].take()?;

    let duplicates = duplicate_indices
        .iter()
        .filter_map(|&idx| items[idx].take().map(|data| data.result))
        .collect();

    Some(CullGroup {
        representative: representative.result,
        duplicates,
    })
}

pub fn build_suggestions(
    analyses: Vec<ImageAnalysisData>,
    settings: &CullingSettings,
) -> CullingSuggestions {
    let mut items: Vec<Option<ImageAnalysisData>> = analyses.into_iter().map(Some).collect();
    let mut processed = vec![false; items.len()];

    let similar_groups = if settings.group_similar {
        let mut group_indices: Vec<Vec<usize>> = (0..items.len())
            .filter_map(|i| {
                if processed[i] {
                    return None;
                }

                Some(collect_group(
                    i,
                    &items,
                    &mut processed,
                    settings.similarity_threshold,
                ))
            })
            .filter(|group| group.len() >= MIN_GROUP_SIZE)
            .collect();

        group_indices
            .iter_mut()
            .for_each(|group| sort_group_indices(group, &items));

        group_indices
            .into_iter()
            .filter_map(|group| extract_group(&group, &mut items))
            .collect()
    } else {
        Vec::new()
    };

    let blurry_images = if settings.filter_blurry {
        let mut blurry: Vec<ImageAnalysisResult> = processed
            .iter()
            .zip(items.iter_mut())
            .filter(|(done, _)| !**done)
            .filter_map(|(_, item)| item.take().map(|data| data.result))
            .filter(|result| result.sharpness_metric < settings.blur_threshold)
            .collect();

        blurry.sort_by(|a, b| {
            a.sharpness_metric
                .partial_cmp(&b.sharpness_metric)
                .unwrap_or(Ordering::Equal)
        });

        blurry
    } else {
        Vec::new()
    };

    CullingSuggestions {
        similar_groups,
        blurry_images,
        failed_paths: Vec::new(),
    }
}
