pub mod processing;
pub mod stitching;

pub use processing::{
    calculate_downscale_dimensions, calculate_downscale_dimensions_capped, compute_homography,
    find_features, find_features_tuned, find_homography_ransac, generate_brief_pairs,
    generate_low_detail_mask, match_features, normalize_grayscale, prepare_image,
};
pub use stitching::{
    build_stitching_order, find_pairwise_matches, progressive_seam_stitcher, warp_image_homography,
};

use image::{GrayImage, Rgb32FImage};
use nalgebra::{Matrix3, Point2};

pub const BRIEF_DESCRIPTOR_SIZE: usize = 256;
pub const MIN_INLIERS_FOR_CONNECTION: usize = 15;

pub type Descriptor = [u8; BRIEF_DESCRIPTOR_SIZE / 8];
pub type BriefPairs = Vec<(Point2<i32>, Point2<i32>)>;

#[derive(Debug, Clone, Copy)]
pub struct KeyPoint {
    pub x: u32,
    pub y: u32,
}

#[derive(Clone)]
pub struct Feature {
    pub keypoint: KeyPoint,
    pub descriptor: Descriptor,
}

#[derive(Debug, Clone, Copy)]
pub struct Match {
    pub index1: usize,
    pub index2: usize,
}

pub struct ImageInfo {
    pub image: Rgb32FImage,
    pub low_detail_mask: GrayImage,
    pub scale_factor: f64,
    pub features: Vec<Feature>,
}

#[derive(Clone)]
pub struct MatchInfo {
    pub homography: Matrix3<f64>,
    pub inliers: usize,
}
