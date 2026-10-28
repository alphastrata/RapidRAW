use crate::ai_processing::{
    AiDepthMaskParameters, AiForegroundMaskParameters, AiSkyMaskParameters, AiSubjectMaskParameters,
};
use base64::{Engine as _, engine::general_purpose};
use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
use image::{DynamicImage, ExtendedColorType, GenericImageView, GrayImage, ImageEncoder, Luma};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::app_state::AppState;
use crate::image_processing::{apply_cpu_default_raw_processing, apply_geometry_warp};
use crate::{get_cached_full_warped_image, get_cached_full_warped_image_for_path};

const MASK_CACHE_LIMIT_BYTES: usize = 128 * 1024 * 1024;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(crate = "serde")]
#[serde(rename_all = "camelCase")]
pub enum SubMaskMode {
    Additive,
    Subtractive,
    Intersect,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(crate = "serde")]
#[serde(rename_all = "camelCase")]
pub struct SubMask {
    pub id: String,
    #[serde(rename = "type")]
    pub mask_type: String,
    pub visible: bool,
    #[serde(default)]
    pub invert: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    pub mode: SubMaskMode,
    pub parameters: Value,
}

fn default_opacity() -> f32 {
    100.0
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(crate = "serde")]
#[serde(rename_all = "camelCase")]
pub struct MaskDefinition {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub invert: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    pub adjustments: Value,
    pub sub_masks: Vec<SubMask>,
}

impl MaskDefinition {
    #[must_use]
    pub fn requires_warped_image(&self) -> bool {
        self.sub_masks
            .iter()
            .any(|sm| sm.mask_type == "color" || sm.mask_type == "luminance")
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(crate = "serde")]
#[serde(rename_all = "camelCase")]
pub struct PatchData {
    pub color: String,
    pub mask: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(crate = "serde")]
#[serde(rename_all = "camelCase")]
pub struct AiPatchDefinition {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub invert: bool,
    pub prompt: String,
    #[serde(default)]
    pub patch_data: Option<PatchData>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    pub sub_masks: Vec<SubMask>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct GrowFeatherParameters {
    #[serde(default)]
    grow: f32,
    #[serde(default)]
    feather: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RadialMaskParameters {
    center_x: f64,
    center_y: f64,
    radius_x: f64,
    radius_y: f64,
    rotation: f32,
    feather: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct LinearMaskParameters {
    start_x: f64,
    start_y: f64,
    end_x: f64,
    end_y: f64,
    #[serde(default = "default_range")]
    range: f32,
}

fn default_range() -> f32 {
    50.0
}

impl Default for LinearMaskParameters {
    fn default() -> Self {
        Self {
            start_x: 0.0,
            start_y: 0.0,
            end_x: 0.0,
            end_y: 0.0,
            range: default_range(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct BrushLine {
    tool: String,
    brush_size: f32,
    points: Vec<Point>,
    #[serde(default = "default_brush_feather")]
    feather: f32,
}

fn default_brush_feather() -> f32 {
    0.5
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BrushMaskParameters {
    #[serde(default)]
    lines: Vec<BrushLine>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct FlowLine {
    tool: String,
    brush_size: f32,
    points: Vec<Point>,
    #[serde(default = "default_brush_feather")]
    feather: f32,
    #[serde(default = "default_line_flow")]
    flow: f32,
}

fn default_line_flow() -> f32 {
    10.0
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct FlowMaskParameters {
    #[serde(default)]
    lines: Vec<FlowLine>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct ParametricMaskParameters {
    target_x: f64,
    target_y: f64,
    #[serde(default = "default_tolerance")]
    tolerance: f32,
    #[serde(default)]
    grow: f32,
    #[serde(default)]
    feather: f32,
    #[serde(default)]
    rotation: f32,
    #[serde(default)]
    flip_horizontal: bool,
    #[serde(default)]
    flip_vertical: bool,
    #[serde(default)]
    orientation_steps: u8,
}

fn default_tolerance() -> f32 {
    20.0
}

impl Default for ParametricMaskParameters {
    fn default() -> Self {
        Self {
            target_x: 0.0,
            target_y: 0.0,
            tolerance: default_tolerance(),
            grow: 0.0,
            feather: 35.0,
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            orientation_steps: 0,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct MaskTransform {
    pub rotation: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub orientation_steps: u8,
}

impl MaskTransform {
    #[must_use]
    pub fn from_adjustments(adjustments: &serde_json::Value) -> Self {
        Self {
            rotation: adjustments["rotation"].as_f64().unwrap_or(0.0) as f32,
            flip_horizontal: adjustments["flipHorizontal"].as_bool().unwrap_or(false),
            flip_vertical: adjustments["flipVertical"].as_bool().unwrap_or(false),
            orientation_steps: adjustments["orientationSteps"].as_u64().unwrap_or(0) as u8,
        }
    }

    #[must_use]
    pub fn box_space(&self, (width, height): (u32, u32)) -> (u32, u32) {
        if self.orientation_steps % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        }
    }

    #[must_use]
    pub fn unrotate_box(
        &self,
        start_point: (f64, f64),
        end_point: (f64, f64),
        (img_w, img_h): (u32, u32),
    ) -> ((f64, f64), (f64, f64)) {
        let (coarse_rotated_w, coarse_rotated_h) = {
            let (w, h) = self.box_space((img_w, img_h));
            (f64::from(w), f64::from(h))
        };
        let center = (coarse_rotated_w / 2.0, coarse_rotated_h / 2.0);

        let angle_rad = f64::from(self.rotation).to_radians();
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();

        let unrotate = |p: (f64, f64)| {
            let px = p.0 - center.0;
            let py = p.1 - center.1;
            (
                px * cos_a + py * sin_a + center.0,
                -px * sin_a + py * cos_a + center.1,
            )
        };

        let unflip = |p: (f64, f64)| {
            let mut new_px = p.0;
            let mut new_py = p.1;
            if self.flip_horizontal {
                new_px = coarse_rotated_w - p.0;
            }
            if self.flip_vertical {
                new_py = coarse_rotated_h - p.1;
            }
            (new_px, new_py)
        };

        let un_coarse_rotate = |p: (f64, f64)| -> (f64, f64) {
            let w = f64::from(img_w);
            let h = f64::from(img_h);
            match self.orientation_steps {
                1 => (p.1, h - p.0),
                2 => (w - p.0, h - p.1),
                3 => (w - p.1, p.0),
                _ => p,
            }
        };

        let corners = [
            start_point,
            (start_point.0, end_point.1),
            end_point,
            (end_point.0, start_point.1),
        ]
        .map(|p| un_coarse_rotate(unflip(unrotate(p))));

        let min_x = corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let min_y = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_x = corners
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = corners
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max);

        ((min_x, min_y), (max_x, max_y))
    }
}

fn grayscale_dilate(image: &GrayImage, k: u8) -> GrayImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return image.clone();
    }
    let w = width as usize;
    let h = height as usize;
    let r = usize::from(k);
    let src = image.as_raw();

    let mut temp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];

    temp.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(temp_row, src_row)| {
            temp_row.iter_mut().enumerate().for_each(|(x, slot)| {
                let start = x.saturating_sub(r);
                let end = (x + r).min(w - 1);
                let max_val = src_row[start..=end].iter().fold(0, |a, &b| a.max(b));
                *slot = max_val;
            });
        });

    out.par_chunks_mut(w).enumerate().for_each(|(y, out_row)| {
        let start = y.saturating_sub(r);
        let end = (y + r).min(h - 1);
        out_row.copy_from_slice(&temp[start * w..(start + 1) * w]);
        ((start + 1)..=end).for_each(|yi| {
            out_row
                .iter_mut()
                .zip(&temp[yi * w..(yi + 1) * w])
                .for_each(|(o, &t)| {
                    *o = (*o).max(t);
                });
        });
    });

    let Some(image) = GrayImage::from_raw(width, height, out) else {
        unreachable!("out has exactly width * height bytes")
    };
    image
}

fn grayscale_erode(image: &GrayImage, k: u8) -> GrayImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return image.clone();
    }
    let w = width as usize;
    let h = height as usize;
    let r = usize::from(k);
    let src = image.as_raw();

    let mut temp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];

    temp.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(temp_row, src_row)| {
            temp_row.iter_mut().enumerate().for_each(|(x, slot)| {
                let start = x.saturating_sub(r);
                let end = (x + r).min(w - 1);
                let min_val = src_row[start..=end].iter().fold(255, |a, &b| a.min(b));
                *slot = min_val;
            });
        });

    out.par_chunks_mut(w).enumerate().for_each(|(y, out_row)| {
        let start = y.saturating_sub(r);
        let end = (y + r).min(h - 1);
        out_row.copy_from_slice(&temp[start * w..(start + 1) * w]);
        ((start + 1)..=end).for_each(|yi| {
            out_row
                .iter_mut()
                .zip(&temp[yi * w..(yi + 1) * w])
                .for_each(|(o, &t)| {
                    *o = (*o).min(t);
                });
        });
    });

    let Some(image) = GrayImage::from_raw(width, height, out) else {
        unreachable!("out has exactly width * height bytes")
    };
    image
}

fn apply_grow_and_feather(mask: &mut GrayImage, grow: f32, feather: f32, width: u32, height: u32) {
    let base_dimension = width.min(height) as f32;

    if grow.abs() > 0.01 {
        const MAX_GROW_PERCENTAGE: f32 = 0.01;
        let grow_pixels = (grow / 100.0) * base_dimension * MAX_GROW_PERCENTAGE;

        let amount = grow_pixels.abs().round() as u8;

        if amount > 0 {
            if grow_pixels > 0.0 {
                *mask = grayscale_dilate(mask, amount);
            } else {
                *mask = grayscale_erode(mask, amount);
            }
        }
    }

    if feather > 0.0 {
        const MAX_FEATHER_SIGMA_PERCENTAGE: f32 = 0.005;
        let sigma = (feather / 100.0) * base_dimension * MAX_FEATHER_SIGMA_PERCENTAGE;

        if sigma > 0.01 {
            *mask = imageproc::filter::gaussian_blur_f32(mask, sigma);
        }
    }
}

fn stroke_bounds(
    points: &[Point],
    width: u32,
    height: u32,
    radius: f32,
    scale: f32,
    crop_offset: (f32, f32),
) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 || points.is_empty() {
        return None;
    }

    let r_pad = radius.ceil() + 2.0;

    let (min_x, min_y, max_x, max_y) = points.iter().fold(
        (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ),
        |(min_x, min_y, max_x, max_y), p| {
            let px = p.x as f32 * scale - crop_offset.0;
            let py = p.y as f32 * scale - crop_offset.1;

            (
                min_x.min(px - r_pad),
                min_y.min(py - r_pad),
                max_x.max(px + r_pad),
                max_y.max(py + r_pad),
            )
        },
    );

    let width_limit = (width - 1) as f32;
    let height_limit = (height - 1) as f32;

    if max_x < 0.0 || max_y < 0.0 || min_x > width_limit || min_y > height_limit {
        return None;
    }

    let min_x = min_x.floor().max(0.0).min(width_limit) as u32;
    let min_y = min_y.floor().max(0.0).min(height_limit) as u32;
    let max_x = max_x.ceil().max(0.0).min(width_limit) as u32;
    let max_y = max_y.ceil().max(0.0).min(height_limit) as u32;

    if min_x > max_x || min_y > max_y {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    }
}

#[allow(clippy::too_many_arguments)]
fn render_stroke_layer_parallel(
    points: &[Point],
    radius: f32,
    feather: f32,
    scale: f32,
    crop_offset: (f32, f32),
    layer_offset: (f32, f32),
    bb_w: u32,
    bb_h: u32,
) -> GrayImage {
    let mut out_pixels = vec![0u8; (bb_w * bb_h) as usize];
    if points.is_empty() || radius <= 0.0 {
        return match GrayImage::from_raw(bb_w, bb_h, out_pixels) {
            Some(image) => image,
            None => unreachable!("out_pixels has exactly bb_w * bb_h bytes"),
        };
    }

    struct Segment {
        x1: f32,
        y1: f32,
        dx: f32,
        dy: f32,
        len_sq: f32,
        bounds_left: i32,
        bounds_right: i32,
        bounds_top: i32,
        bounds_bottom: i32,
    }

    let bb_w_i = bb_w as i32;
    let bb_h_i = bb_h as i32;

    let segments: Vec<Segment> = points
        .windows(2)
        .filter_map(|pair| {
            let x1 = pair[0].x as f32 * scale - crop_offset.0 - layer_offset.0;
            let y1 = pair[0].y as f32 * scale - crop_offset.1 - layer_offset.1;
            let x2 = pair[1].x as f32 * scale - crop_offset.0 - layer_offset.0;
            let y2 = pair[1].y as f32 * scale - crop_offset.1 - layer_offset.1;

            let left = ((x1.min(x2) - radius).floor() as i32).max(0);
            let right = ((x1.max(x2) + radius).ceil() as i32).min(bb_w_i - 1);
            let top = ((y1.min(y2) - radius).floor() as i32).max(0);
            let bottom = ((y1.max(y2) + radius).ceil() as i32).min(bb_h_i - 1);

            if left > right || top > bottom {
                return None;
            }

            let dx = x2 - x1;
            let dy = y2 - y1;

            Some(Segment {
                x1,
                y1,
                dx,
                dy,
                len_sq: dx * dx + dy * dy,
                bounds_left: left,
                bounds_right: right,
                bounds_top: top,
                bounds_bottom: bottom,
            })
        })
        .collect();

    let mut single_point = None;
    if segments.is_empty() && !points.is_empty() {
        let x1 = points[0].x as f32 * scale - crop_offset.0 - layer_offset.0;
        let y1 = points[0].y as f32 * scale - crop_offset.1 - layer_offset.1;
        let left = ((x1 - radius).floor() as i32).max(0);
        let right = ((x1 + radius).ceil() as i32).min(bb_w_i - 1);
        let top = ((y1 - radius).floor() as i32).max(0);
        let bottom = ((y1 + radius).ceil() as i32).min(bb_h_i - 1);
        if left <= right && top <= bottom {
            single_point = Some((x1, y1, left, right, top, bottom));
        }
    }

    let feather_amount = feather.clamp(0.0, 1.0);
    let inner_radius = radius * (1.0 - feather_amount);
    let feather_range = (radius - inner_radius).max(0.01);
    let radius_sq = radius * radius;
    let inner_radius_sq = inner_radius * inner_radius;

    out_pixels
        .par_chunks_mut(bb_w as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let py = y as f32;
            let y_i32 = y as i32;

            let active_segments: Vec<&Segment> = segments
                .iter()
                .filter(|seg| y_i32 >= seg.bounds_top && y_i32 <= seg.bounds_bottom)
                .collect();

            let is_point_active = single_point
                .as_ref()
                .is_some_and(|pt| y_i32 >= pt.4 && y_i32 <= pt.5);

            if active_segments.is_empty() && !is_point_active {
                return;
            }

            row.iter_mut().enumerate().for_each(|(x, pixel)| {
                let px = x as f32;
                let x_i32 = x as i32;

                let mut min_dist_sq = radius_sq + 1.0;

                active_segments
                    .iter()
                    .filter(|seg| x_i32 >= seg.bounds_left && x_i32 <= seg.bounds_right)
                    .for_each(|seg| {
                        let dist_sq = if seg.len_sq < 0.0001 {
                            (px - seg.x1) * (px - seg.x1) + (py - seg.y1) * (py - seg.y1)
                        } else {
                            let t = (((px - seg.x1) * seg.dx + (py - seg.y1) * seg.dy)
                                / seg.len_sq)
                                .clamp(0.0, 1.0);
                            let proj_x = seg.x1 + t * seg.dx;
                            let proj_y = seg.y1 + t * seg.dy;
                            (px - proj_x) * (px - proj_x) + (py - proj_y) * (py - proj_y)
                        };
                        if dist_sq < min_dist_sq {
                            min_dist_sq = dist_sq;
                        }
                    });

                if is_point_active
                    && let Some(pt) = single_point.as_ref()
                        && x_i32 >= pt.2 && x_i32 <= pt.3 {
                            let dist_sq = (px - pt.0) * (px - pt.0) + (py - pt.1) * (py - pt.1);
                            if dist_sq < min_dist_sq {
                                min_dist_sq = dist_sq;
                            }
                        }

                if min_dist_sq <= radius_sq {
                    let intensity = if min_dist_sq <= inner_radius_sq {
                        1.0
                    } else {
                        let dist = min_dist_sq.sqrt();
                        let t = ((dist - inner_radius) / feather_range).clamp(0.0, 1.0);
                        1.0 - (t * t * (3.0 - 2.0 * t))
                    };
                    *pixel = (intensity * 255.0).round() as u8;
                }
            });
        });

    match GrayImage::from_raw(bb_w, bb_h, out_pixels) {
        Some(image) => image,
        None => unreachable!("out_pixels has exactly bb_w * bb_h bytes"),
    }
}

fn generate_radial_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> GrayImage {
    let params: RadialMaskParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let mut mask = GrayImage::new(width, height);

    let center_x_f = params.center_x as f32 * scale - crop_offset.0;
    let center_y_f = params.center_y as f32 * scale - crop_offset.1;
    let center_x = center_x_f as i32;
    let center_y = center_y_f as i32;
    let radius_x = params.radius_x as f32 * scale;
    let radius_y = params.radius_y as f32 * scale;
    let rotation_rad = params.rotation * PI / 180.0;
    let cos_rot = rotation_rad.cos();
    let sin_rot = rotation_rad.sin();
    let inner_bound = 1.0 - params.feather.clamp(0.0, 1.0);

    let (cx, cy) = (center_x as f32, center_y as f32);

    for_each_mask_row(&mut mask, |y, row| {
        row.iter_mut().enumerate().for_each(|(x, value)| {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;

            let rot_dx = dx * cos_rot + dy * sin_rot;
            let rot_dy = -dx * sin_rot + dy * cos_rot;

            let norm_x = rot_dx / radius_x.max(0.01);
            let norm_y = rot_dy / radius_y.max(0.01);

            let dist = (norm_x.powi(2) + norm_y.powi(2)).sqrt();

            let intensity = 1.0 - (dist - inner_bound) / (1.0 - inner_bound).max(0.01);
            let clamped_intensity = intensity.clamp(0.0, 1.0);

            *value = (clamped_intensity * 255.0) as u8;
        });
    });

    mask
}

fn generate_linear_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> GrayImage {
    let params: LinearMaskParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let mut mask = GrayImage::new(width, height);

    let start_x = params.start_x as f32 * scale - crop_offset.0;
    let start_y = params.start_y as f32 * scale - crop_offset.1;
    let end_x = params.end_x as f32 * scale - crop_offset.0;
    let end_y = params.end_y as f32 * scale - crop_offset.1;
    let range = params.range * scale;

    let line_vec_x = end_x - start_x;
    let line_vec_y = end_y - start_y;

    let len_sq = line_vec_x.powi(2) + line_vec_y.powi(2);

    if len_sq < 0.01 {
        return mask;
    }

    let perp_vec_x = -line_vec_y / len_sq.sqrt();
    let perp_vec_y = line_vec_x / len_sq.sqrt();

    let half_width = range.max(0.01);

    for_each_mask_row(&mut mask, |y_u, row| {
        let y = y_u as f32;
        row.iter_mut().enumerate().for_each(|(x_u, value)| {
            let x = x_u as f32;

            let pixel_vec_x = x - start_x;
            let pixel_vec_y = y - start_y;

            let dist_perp = pixel_vec_x * perp_vec_x + pixel_vec_y * perp_vec_y;

            let t = dist_perp / half_width;

            let intensity = 0.5 - t * 0.5;

            let clamped_intensity = intensity.clamp(0.0, 1.0);

            *value = (clamped_intensity * 255.0) as u8;
        });
    });

    mask
}

fn generate_brush_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> GrayImage {
    let params: BrushMaskParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let mut final_mask = GrayImage::new(width, height);

    params.lines.iter().for_each(|line| {
        if line.points.is_empty() {
            return;
        }

        let is_eraser = line.tool == "eraser";
        let radius = (line.brush_size * scale / 2.0).max(0.0);
        let feather = line.feather.clamp(0.0, 1.0);

        let Some((min_x, min_y, max_x, max_y)) =
            stroke_bounds(&line.points, width, height, radius, scale, crop_offset)
        else {
            return;
        };

        let bb_w = max_x - min_x + 1;
        let bb_h = max_y - min_y + 1;
        let layer_offset = (min_x as f32, min_y as f32);

        let line_mask = render_stroke_layer_parallel(
            &line.points,
            radius,
            feather,
            scale,
            crop_offset,
            layer_offset,
            bb_w,
            bb_h,
        );

        (0..bb_h).for_each(|y| {
            (0..bb_w).for_each(|x| {
                let src_val = f32::from(line_mask.get_pixel(x, y)[0]) / 255.0;
                if src_val <= 0.0 {
                    return;
                }

                let abs_x = min_x + x;
                let abs_y = min_y + y;
                let dst_pixel = final_mask.get_pixel_mut(abs_x, abs_y);
                let dst_val = f32::from(dst_pixel[0]) / 255.0;

                let blended = if is_eraser {
                    dst_val * (1.0 - src_val)
                } else {
                    dst_val + src_val - dst_val * src_val
                };

                dst_pixel[0] = (blended.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
        });
    });

    final_mask
}

fn generate_flow_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> GrayImage {
    let params: FlowMaskParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let mut final_mask = GrayImage::new(width, height);

    params.lines.iter().for_each(|line| {
        if line.points.is_empty() {
            return;
        }

        let is_eraser = line.tool == "eraser";
        let flow_per_stroke = (line.flow.clamp(0.0, 100.0) / 100.0) * 255.0;
        let radius = (line.brush_size * scale / 2.0).max(0.0);
        let feather = line.feather.clamp(0.0, 1.0);

        let Some((min_x, min_y, max_x, max_y)) =
            stroke_bounds(&line.points, width, height, radius, scale, crop_offset)
        else {
            return;
        };

        let bb_w = max_x - min_x + 1;
        let bb_h = max_y - min_y + 1;
        let layer_offset = (min_x as f32, min_y as f32);

        let line_mask = render_stroke_layer_parallel(
            &line.points,
            radius,
            feather,
            scale,
            crop_offset,
            layer_offset,
            bb_w,
            bb_h,
        );

        (0..bb_h).for_each(|y| {
            (0..bb_w).for_each(|x| {
                let stroke_pixel = f32::from(line_mask.get_pixel(x, y)[0]);
                if stroke_pixel <= 0.0 {
                    return;
                }

                let abs_x = min_x + x;
                let abs_y = min_y + y;
                let pixel = final_mask.get_pixel_mut(abs_x, abs_y);

                let c_norm = f32::from(pixel[0]) / 255.0;
                let delta = ((stroke_pixel / 255.0) * flow_per_stroke).round();
                let d_norm = (delta / 255.0).clamp(0.0, 1.0);

                let next = if is_eraser {
                    c_norm * (1.0 - d_norm)
                } else {
                    c_norm + d_norm - c_norm * d_norm
                };

                pixel[0] = (next.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
        });
    });

    final_mask
}

pub struct TransformParams {
    pub rotation: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub orientation_steps: u8,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub crop_offset: (f32, f32),
}

fn generate_ai_bitmap_from_full_mask(
    full_mask_image: &GrayImage,
    tf: &TransformParams,
) -> GrayImage {
    let (full_mask_w, full_mask_h) = full_mask_image.dimensions();
    let mut final_mask = GrayImage::new(tf.width, tf.height);

    let angle_rad = tf.rotation.to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let (coarse_rotated_w, coarse_rotated_h) = if tf.orientation_steps % 2 == 1 {
        (full_mask_h, full_mask_w)
    } else {
        (full_mask_w, full_mask_h)
    };

    let scaled_coarse_rotated_w = coarse_rotated_w as f32 * tf.scale;
    let scaled_coarse_rotated_h = coarse_rotated_h as f32 * tf.scale;
    let center_x = scaled_coarse_rotated_w / 2.0;
    let center_y = scaled_coarse_rotated_h / 2.0;

    for_each_mask_row(&mut final_mask, |y_out, row| {
        row.iter_mut().enumerate().for_each(|(x_out, value)| {
            let x_uncrop = x_out as f32 + tf.crop_offset.0;
            let y_uncrop = y_out as f32 + tf.crop_offset.1;

            let x_centered = x_uncrop - center_x;
            let y_centered = y_uncrop - center_y;

            let x_unrotated = x_centered * cos_a + y_centered * sin_a + center_x;
            let y_unrotated = -x_centered * sin_a + y_centered * cos_a + center_y;

            let x_unflipped = if tf.flip_horizontal {
                scaled_coarse_rotated_w - x_unrotated
            } else {
                x_unrotated
            };
            let y_unflipped = if tf.flip_vertical {
                scaled_coarse_rotated_h - y_unrotated
            } else {
                y_unrotated
            };

            let (x_unrotated_coarse, y_unrotated_coarse) = match tf.orientation_steps {
                0 => (x_unflipped, y_unflipped),
                1 => (y_unflipped, scaled_coarse_rotated_w - x_unflipped),
                2 => (
                    scaled_coarse_rotated_w - x_unflipped,
                    scaled_coarse_rotated_h - y_unflipped,
                ),
                3 => (scaled_coarse_rotated_h - y_unflipped, x_unflipped),
                _ => (x_unflipped, y_unflipped),
            };

            let x_src = x_unrotated_coarse / tf.scale;
            let y_src = y_unrotated_coarse / tf.scale;

            if x_src >= 0.0
                && x_src < full_mask_w as f32
                && y_src >= 0.0
                && y_src < full_mask_h as f32
            {
                *value = full_mask_image.get_pixel(x_src as u32, y_src as u32)[0];
            }
        });
    });

    final_mask
}

#[must_use]
pub fn generate_ai_bitmap_from_base64(data_url: &str, tf: &TransformParams) -> Option<GrayImage> {
    let b64_data = if let Some(idx) = data_url.find(',') {
        &data_url[idx + 1..]
    } else {
        data_url
    };

    let decoded_bytes = general_purpose::STANDARD.decode(b64_data).ok()?;
    let full_mask_image = image::load_from_memory(&decoded_bytes).ok()?.to_luma8();

    Some(generate_ai_bitmap_from_full_mask(&full_mask_image, tf))
}

fn generate_ai_sky_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> Option<GrayImage> {
    let params: AiSkyMaskParameters = serde_json::from_value(params_value.clone()).ok()?;
    let grow_feather: GrowFeatherParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let data_url = params.mask_data_base64?;

    let tf = TransformParams {
        rotation: params.rotation.unwrap_or(0.0),
        flip_horizontal: params.flip_horizontal.unwrap_or(false),
        flip_vertical: params.flip_vertical.unwrap_or(false),
        orientation_steps: params.orientation_steps.unwrap_or(0),
        width,
        height,
        scale,
        crop_offset,
    };
    let mut mask = generate_ai_bitmap_from_base64(&data_url, &tf)?;

    apply_grow_and_feather(
        &mut mask,
        grow_feather.grow,
        grow_feather.feather,
        width,
        height,
    );

    Some(mask)
}

fn generate_ai_depth_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> Option<GrayImage> {
    let params: AiDepthMaskParameters = serde_json::from_value(params_value.clone()).ok()?;
    let grow_feather: GrowFeatherParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let data_url = params.mask_data_base64?;

    let tf = TransformParams {
        rotation: params.rotation.unwrap_or(0.0),
        flip_horizontal: params.flip_horizontal.unwrap_or(false),
        flip_vertical: params.flip_vertical.unwrap_or(false),
        orientation_steps: params.orientation_steps.unwrap_or(0),
        width,
        height,
        scale,
        crop_offset,
    };

    let depth_map = generate_ai_bitmap_from_base64(&data_url, &tf)?;

    let (w, h) = depth_map.dimensions();

    fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
        let t = ((x - edge0) / (edge1 - edge0).max(0.0001)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    let min_fade = params.min_fade;
    let max_fade = params.max_fade;

    let data: Vec<u8> = depth_map
        .as_raw()
        .par_iter()
        .map(|&p| {
            let val_pct = (f32::from(p) / 255.0) * 100.0;

            let lower_bound = smoothstep(params.min_depth - min_fade, params.min_depth, val_pct);
            let upper_bound =
                1.0 - smoothstep(params.max_depth, params.max_depth + max_fade, val_pct);
            let bandpass_weight = lower_bound * upper_bound;

            let depth_intensity = val_pct / 100.0;
            let final_intensity = bandpass_weight * depth_intensity;

            (final_intensity * 255.0) as u8
        })
        .collect();
    let mut mask = GrayImage::from_raw(w, h, data)?;

    if params.feather > 0.0 {
        mask = image::imageops::blur(&mask, params.feather * 0.1);
    }

    apply_grow_and_feather(
        &mut mask,
        grow_feather.grow,
        grow_feather.feather,
        width,
        height,
    );

    Some(mask)
}

fn generate_ai_foreground_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> Option<GrayImage> {
    let params: AiForegroundMaskParameters = serde_json::from_value(params_value.clone()).ok()?;
    let grow_feather: GrowFeatherParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let data_url = params.mask_data_base64?;

    let tf = TransformParams {
        rotation: params.rotation.unwrap_or(0.0),
        flip_horizontal: params.flip_horizontal.unwrap_or(false),
        flip_vertical: params.flip_vertical.unwrap_or(false),
        orientation_steps: params.orientation_steps.unwrap_or(0),
        width,
        height,
        scale,
        crop_offset,
    };
    let mut mask = generate_ai_bitmap_from_base64(&data_url, &tf)?;

    apply_grow_and_feather(
        &mut mask,
        grow_feather.grow,
        grow_feather.feather,
        width,
        height,
    );

    Some(mask)
}

fn generate_ai_subject_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> Option<GrayImage> {
    let params: AiSubjectMaskParameters = serde_json::from_value(params_value.clone()).ok()?;
    let grow_feather: GrowFeatherParameters =
        serde_json::from_value(params_value.clone()).unwrap_or_default();
    let data_url = params.mask_data_base64?;

    let tf = TransformParams {
        rotation: params.rotation.unwrap_or(0.0),
        flip_horizontal: params.flip_horizontal.unwrap_or(false),
        flip_vertical: params.flip_vertical.unwrap_or(false),
        orientation_steps: params.orientation_steps.unwrap_or(0),
        width,
        height,
        scale,
        crop_offset,
    };
    let mut mask = generate_ai_bitmap_from_base64(&data_url, &tf)?;

    apply_grow_and_feather(
        &mut mask,
        grow_feather.grow,
        grow_feather.feather,
        width,
        height,
    );

    Some(mask)
}

fn generate_color_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    warped_image: Option<&image::DynamicImage>,
) -> Option<GrayImage> {
    let params: ParametricMaskParameters = serde_json::from_value(params_value.clone()).ok()?;
    let warped = warped_image?;
    let (full_w, full_h) = warped.dimensions();

    let target_x = params.target_x.round() as i32;
    let target_y = params.target_y.round() as i32;
    let full_w_i = full_w as i32;
    let full_h_i = full_h as i32;
    if target_x < 0 || target_y < 0 || target_x >= full_w_i || target_y >= full_h_i {
        return None;
    }

    let ref_pixel = warped.get_pixel(target_x as u32, target_y as u32);
    let ref_r = f32::from(ref_pixel[0]);
    let ref_g = f32::from(ref_pixel[1]);
    let ref_b = f32::from(ref_pixel[2]);

    let mut mask = GrayImage::new(width, height);

    let angle_rad = params.rotation * PI / 180.0;
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let (coarse_rotated_w, coarse_rotated_h) = if params.orientation_steps % 2 == 1 {
        (full_h, full_w)
    } else {
        (full_w, full_h)
    };

    let scaled_coarse_rotated_w = coarse_rotated_w as f32 * scale;
    let scaled_coarse_rotated_h = coarse_rotated_h as f32 * scale;
    let center_x = scaled_coarse_rotated_w / 2.0;
    let center_y = scaled_coarse_rotated_h / 2.0;

    let tolerance_sq = (params.tolerance * 2.55).max(1.0).powi(2) * 3.0;
    let inv_scale = 1.0 / scale;

    for_each_mask_row(&mut mask, |y_out, row| {
        let y_uncrop = y_out as f32 + crop_offset.1;
        let y_centered = y_uncrop - center_y;
        let y_sin = y_centered * sin_a;
        let y_cos = y_centered * cos_a;

        row.iter_mut().enumerate().for_each(|(x_out, value)| {
            let x_uncrop = x_out as f32 + crop_offset.0;
            let x_centered = x_uncrop - center_x;

            let x_unrotated = x_centered * cos_a + y_sin + center_x;
            let y_unrotated = -x_centered * sin_a + y_cos + center_y;

            let x_unflipped = if params.flip_horizontal {
                scaled_coarse_rotated_w - x_unrotated
            } else {
                x_unrotated
            };
            let y_unflipped = if params.flip_vertical {
                scaled_coarse_rotated_h - y_unrotated
            } else {
                y_unrotated
            };

            let (x_unrotated_coarse, y_unrotated_coarse) = match params.orientation_steps {
                0 => (x_unflipped, y_unflipped),
                1 => (y_unflipped, scaled_coarse_rotated_w - x_unflipped),
                2 => (
                    scaled_coarse_rotated_w - x_unflipped,
                    scaled_coarse_rotated_h - y_unflipped,
                ),
                3 => (scaled_coarse_rotated_h - y_unflipped, x_unflipped),
                _ => (x_unflipped, y_unflipped),
            };

            if x_unrotated_coarse >= 0.0 && y_unrotated_coarse >= 0.0 {
                let x_src = (x_unrotated_coarse * inv_scale) as u32;
                let y_src = (y_unrotated_coarse * inv_scale) as u32;

                if x_src < full_w && y_src < full_h {
                    let pixel = warped.get_pixel(x_src, y_src);
                    let dist_sq = (f32::from(pixel[0]) - ref_r).powi(2)
                        + (f32::from(pixel[1]) - ref_g).powi(2)
                        + (f32::from(pixel[2]) - ref_b).powi(2);

                    if dist_sq <= tolerance_sq {
                        let intensity = 1.0 - (dist_sq.sqrt() / tolerance_sq.sqrt());
                        *value = (intensity * 255.0) as u8;
                    }
                }
            }
        });
    });

    apply_grow_and_feather(&mut mask, params.grow, params.feather, width, height);
    Some(mask)
}

fn generate_luminance_bitmap(
    params_value: &Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    warped_image: Option<&image::DynamicImage>,
) -> Option<GrayImage> {
    let params: ParametricMaskParameters = serde_json::from_value(params_value.clone()).ok()?;
    let warped = warped_image?;
    let (full_w, full_h) = warped.dimensions();

    let target_x = params.target_x.round() as i32;
    let target_y = params.target_y.round() as i32;
    let full_w_i = full_w as i32;
    let full_h_i = full_h as i32;
    if target_x < 0 || target_y < 0 || target_x >= full_w_i || target_y >= full_h_i {
        return None;
    }

    let ref_pixel = warped.get_pixel(target_x as u32, target_y as u32);
    let ref_luma = 0.299 * f32::from(ref_pixel[0])
        + 0.587 * f32::from(ref_pixel[1])
        + 0.114 * f32::from(ref_pixel[2]);

    let mut mask = GrayImage::new(width, height);

    let angle_rad = params.rotation * PI / 180.0;
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let (coarse_rotated_w, coarse_rotated_h) = if params.orientation_steps % 2 == 1 {
        (full_h, full_w)
    } else {
        (full_w, full_h)
    };

    let scaled_coarse_rotated_w = coarse_rotated_w as f32 * scale;
    let scaled_coarse_rotated_h = coarse_rotated_h as f32 * scale;
    let center_x = scaled_coarse_rotated_w / 2.0;
    let center_y = scaled_coarse_rotated_h / 2.0;

    let tolerance_val = (params.tolerance * 2.55).max(1.0);
    let inv_scale = 1.0 / scale;

    for_each_mask_row(&mut mask, |y_out, row| {
        let y_uncrop = y_out as f32 + crop_offset.1;
        let y_centered = y_uncrop - center_y;
        let y_sin = y_centered * sin_a;
        let y_cos = y_centered * cos_a;

        row.iter_mut().enumerate().for_each(|(x_out, value)| {
            let x_uncrop = x_out as f32 + crop_offset.0;
            let x_centered = x_uncrop - center_x;

            let x_unrotated = x_centered * cos_a + y_sin + center_x;
            let y_unrotated = -x_centered * sin_a + y_cos + center_y;

            let x_unflipped = if params.flip_horizontal {
                scaled_coarse_rotated_w - x_unrotated
            } else {
                x_unrotated
            };
            let y_unflipped = if params.flip_vertical {
                scaled_coarse_rotated_h - y_unrotated
            } else {
                y_unrotated
            };

            let (x_unrotated_coarse, y_unrotated_coarse) = match params.orientation_steps {
                0 => (x_unflipped, y_unflipped),
                1 => (y_unflipped, scaled_coarse_rotated_w - x_unflipped),
                2 => (
                    scaled_coarse_rotated_w - x_unflipped,
                    scaled_coarse_rotated_h - y_unflipped,
                ),
                3 => (scaled_coarse_rotated_h - y_unflipped, x_unflipped),
                _ => (x_unflipped, y_unflipped),
            };

            if x_unrotated_coarse >= 0.0 && y_unrotated_coarse >= 0.0 {
                let x_src = (x_unrotated_coarse * inv_scale) as u32;
                let y_src = (y_unrotated_coarse * inv_scale) as u32;

                if x_src < full_w && y_src < full_h {
                    let pixel = warped.get_pixel(x_src, y_src);
                    let luma = 0.299 * f32::from(pixel[0])
                        + 0.587 * f32::from(pixel[1])
                        + 0.114 * f32::from(pixel[2]);
                    let dist = (luma - ref_luma).abs();

                    if dist <= tolerance_val {
                        let intensity = 1.0 - (dist / tolerance_val);
                        *value = (intensity * 255.0) as u8;
                    }
                }
            }
        });
    });

    apply_grow_and_feather(&mut mask, params.grow, params.feather, width, height);
    Some(mask)
}

fn for_each_mask_row(mask: &mut GrayImage, fill_row: impl Fn(u32, &mut [u8]) + Sync) {
    let width = mask.width() as usize;
    if width == 0 {
        return;
    }
    mask.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| fill_row(y as u32, row));
}

fn generate_all_bitmap(width: u32, height: u32) -> GrayImage {
    GrayImage::from_pixel(width, height, Luma([255]))
}

fn generate_sub_mask_bitmap(
    sub_mask: &SubMask,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    warped_image: Option<&DynamicImage>,
) -> Option<GrayImage> {
    if !sub_mask.visible {
        return None;
    }

    match sub_mask.mask_type.as_str() {
        "radial" => Some(generate_radial_bitmap(
            &sub_mask.parameters,
            width,
            height,
            scale,
            crop_offset,
        )),
        "linear" => Some(generate_linear_bitmap(
            &sub_mask.parameters,
            width,
            height,
            scale,
            crop_offset,
        )),
        "brush" | "clone" | "heal" | "liquify" | "retouch" => Some(generate_brush_bitmap(
            &sub_mask.parameters,
            width,
            height,
            scale,
            crop_offset,
        )),
        "flow" => Some(generate_flow_bitmap(
            &sub_mask.parameters,
            width,
            height,
            scale,
            crop_offset,
        )),
        "color" => generate_color_bitmap(
            &sub_mask.parameters,
            width,
            height,
            scale,
            crop_offset,
            warped_image,
        ),
        "luminance" => generate_luminance_bitmap(
            &sub_mask.parameters,
            width,
            height,
            scale,
            crop_offset,
            warped_image,
        ),
        "ai-subject" => {
            generate_ai_subject_bitmap(&sub_mask.parameters, width, height, scale, crop_offset)
        }
        "ai-foreground" => {
            generate_ai_foreground_bitmap(&sub_mask.parameters, width, height, scale, crop_offset)
        }
        "ai-sky" | "ai-text" | "ai-face" => {
            generate_ai_sky_bitmap(&sub_mask.parameters, width, height, scale, crop_offset)
        }
        "ai-depth" => {
            generate_ai_depth_bitmap(&sub_mask.parameters, width, height, scale, crop_offset)
        }
        "quick-eraser" => {
            generate_ai_subject_bitmap(&sub_mask.parameters, width, height, scale, crop_offset)
        }
        "all" => Some(generate_all_bitmap(width, height)),
        _ => None,
    }
}

#[must_use]
pub fn generate_mask_bitmap(
    mask_def: &MaskDefinition,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    warped_image: Option<&DynamicImage>,
) -> Option<GrayImage> {
    if !mask_def.visible || mask_def.sub_masks.is_empty() {
        return None;
    }

    let mut final_mask = GrayImage::new(width, height);

    mask_def
        .sub_masks
        .iter()
        .filter_map(|sub_mask| {
            Some((
                sub_mask,
                generate_sub_mask_bitmap(
                    sub_mask,
                    width,
                    height,
                    scale,
                    crop_offset,
                    warped_image,
                )?,
            ))
        })
        .for_each(|(sub_mask, mut sub_bitmap)| {
            let invert = sub_mask.invert;
            let opacity_multiplier = (sub_mask.opacity / 100.0).clamp(0.0, 1.0);
            let mode = sub_mask.mode;

            final_mask
                .par_iter_mut()
                .zip(sub_bitmap.par_iter_mut())
                .for_each(|(pixel, sub_pixel)| {
                    if invert {
                        *sub_pixel = 255 - *sub_pixel;
                    }
                    if opacity_multiplier < 1.0 {
                        *sub_pixel = (f32::from(*sub_pixel) * opacity_multiplier) as u8;
                    }
                    *pixel = match mode {
                        SubMaskMode::Additive => (*pixel).max(*sub_pixel),
                        SubMaskMode::Subtractive => pixel.saturating_sub(*sub_pixel),
                        SubMaskMode::Intersect => (*pixel).min(*sub_pixel),
                    };
                });
        });

    let invert = mask_def.invert;
    let opacity_multiplier = (mask_def.opacity / 100.0).clamp(0.0, 1.0);
    if invert || opacity_multiplier < 1.0 {
        final_mask.par_iter_mut().for_each(|pixel| {
            if invert {
                *pixel = 255 - *pixel;
            }
            if opacity_multiplier < 1.0 {
                *pixel = (f32::from(*pixel) * opacity_multiplier) as u8;
            }
        });
    }

    Some(final_mask)
}

pub fn generate_mask_overlay(
    mut mask_def: serde_json::Value,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    mut js_adjustments: Option<serde_json::Value>,
    state: &AppState,
) -> Result<Option<Vec<u8>>, String> {
    if let Some(ref mut adj) = js_adjustments {
        crate::adjustment_utils::hydrate_adjustments(state, adj);
    }

    if let Some(sub_masks) = mask_def.get_mut("subMasks").and_then(|v| v.as_array_mut()) {
        let mut cache = state
            .patch_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::adjustment_utils::hydrate_sub_masks(sub_masks, &mut cache);
    }

    let parsed_mask_def: MaskDefinition = serde_json::from_value(mask_def)
        .map_err(|e| format!("Failed to parse hydrated mask_def: {e}"))?;

    let scaled_crop_offset = (crop_offset.0 * scale, crop_offset.1 * scale);

    let warped_image = js_adjustments.as_ref().and_then(|adj| {
        resolve_warped_image_for_masks(state, adj, std::slice::from_ref(&parsed_mask_def))
    });

    if let Some(gray_mask) = generate_mask_bitmap(
        &parsed_mask_def,
        width,
        height,
        scale,
        scaled_crop_offset,
        warped_image.as_deref(),
    ) {
        let rgba_mask: Vec<u8> = gray_mask
            .as_raw()
            .par_iter()
            .flat_map_iter(|&intensity| [255, 0, 0, (f32::from(intensity) * 0.5) as u8])
            .collect();

        let mut buf = Vec::new();
        PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, PngFilterType::Adaptive)
            .write_image(&rgba_mask, width, height, ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;

        Ok(Some(buf))
    } else {
        Ok(None)
    }
}

pub fn resolve_warped_image_for_masks(
    state: &AppState,
    adjustments: &serde_json::Value,
    masks: &[MaskDefinition],
) -> Option<Arc<DynamicImage>> {
    if masks.iter().any(MaskDefinition::requires_warped_image) {
        get_cached_full_warped_image(state, adjustments).ok()
    } else {
        None
    }
}

fn build_full_source_image(base_image: &DynamicImage, is_raw: bool) -> Cow<'_, DynamicImage> {
    let mut image = Cow::Borrowed(base_image);
    if is_raw {
        apply_cpu_default_raw_processing(image.to_mut());
    }
    image
}

#[must_use]
pub fn build_converted_source_image<'a>(
    base_image: &'a DynamicImage,
    is_raw: bool,
    adjustments: &serde_json::Value,
) -> Cow<'a, DynamicImage> {
    let mut image = crate::negative_conversion::apply_negative_conversion(
        Cow::Borrowed(base_image),
        adjustments,
    );
    if crate::negative_conversion::effective_is_raw(is_raw, adjustments) {
        apply_cpu_default_raw_processing(image.to_mut());
    }
    image
}

#[must_use]
pub fn build_full_warped_image<'a>(
    base_image: &'a DynamicImage,
    is_raw: bool,
    adjustments: &serde_json::Value,
) -> Cow<'a, DynamicImage> {
    apply_geometry_warp(build_full_source_image(base_image, is_raw), adjustments)
}

#[must_use]
pub fn build_full_warped_image_rgb8(
    base_image: &DynamicImage,
    is_raw: bool,
    adjustments: &serde_json::Value,
) -> DynamicImage {
    let warped = build_full_warped_image(base_image, is_raw, adjustments);
    let rgb8 = match warped.as_rgb32f() {
        Some(buffer) => {
            let data: Vec<u8> = buffer
                .as_raw()
                .par_iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect();
            let Some(rgb8) = image::RgbImage::from_raw(buffer.width(), buffer.height(), data)
            else {
                unreachable!("data length matches buffer dimensions")
            };
            rgb8
        }
        None => warped.to_rgb8(),
    };
    DynamicImage::ImageRgb8(rgb8)
}

#[must_use]
pub fn build_warped_image_for_masks<'a>(
    base_image: &'a DynamicImage,
    is_raw: bool,
    adjustments: &serde_json::Value,
    masks: &[MaskDefinition],
) -> Option<Cow<'a, DynamicImage>> {
    if masks.iter().any(MaskDefinition::requires_warped_image) {
        Some(Cow::Owned(build_full_warped_image_rgb8(
            base_image,
            crate::negative_conversion::effective_is_raw(is_raw, adjustments),
            adjustments,
        )))
    } else {
        None
    }
}

pub struct GeneratedMasks {
    pub bitmaps: Vec<GrayImage>,
    pub cache_key: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn get_cached_or_generate_masks(
    state: &AppState,
    path: &str,
    definitions: &[MaskDefinition],
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
    adjustments: &serde_json::Value,
) -> GeneratedMasks {
    let mut combined_hasher = DefaultHasher::new();
    let mut bitmaps = Vec::with_capacity(definitions.len());

    definitions.iter().for_each(|def| {
        let mut hasher = DefaultHasher::new();
        let def_json = serde_json::to_string(&(
            &def.id,
            &def.name,
            def.visible,
            def.invert,
            def.opacity,
            &def.sub_masks,
        ))
        .unwrap_or_default();
        def_json.hash(&mut hasher);
        path.hash(&mut hasher);
        if def.requires_warped_image() {
            crate::cache_utils::calculate_geometry_hash(adjustments).hash(&mut hasher);
        }
        width.hash(&mut hasher);
        height.hash(&mut hasher);
        scale.to_bits().hash(&mut hasher);
        crop_offset.0.to_bits().hash(&mut hasher);
        crop_offset.1.to_bits().hash(&mut hasher);
        let key = hasher.finish();

        let cached = state
            .mask_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned();

        let bitmap = cached.or_else(|| {
            let warped_image = def
                .requires_warped_image()
                .then(|| get_cached_full_warped_image_for_path(state, Some(path), adjustments).ok())
                .flatten();
            let generated = generate_mask_bitmap(
                def,
                width,
                height,
                scale,
                crop_offset,
                warped_image.as_deref(),
            )?;

            let mut cache = state
                .mask_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let cached_bytes: usize = cache.values().map(|m| m.as_raw().len()).sum();
            if cache.len() > 50 || cached_bytes > MASK_CACHE_LIMIT_BYTES {
                cache.clear();
            }
            cache.insert(key, generated.clone());
            Some(generated)
        });

        (key, bitmap.is_some()).hash(&mut combined_hasher);
        bitmaps.extend(bitmap);
    });

    GeneratedMasks {
        bitmaps,
        cache_key: combined_hasher.finish(),
    }
}
