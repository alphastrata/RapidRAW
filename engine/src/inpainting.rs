use std::io::Cursor;

use anyhow::bail;
use base64::{Engine as _, engine::general_purpose};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, GrayImage, Rgb, RgbImage, Rgba, RgbaImage};
use rayon::prelude::*;
use serde_json::Value;

pub type MaskBounds = (u32, u32, u32, u32);

#[derive(Debug)]
pub struct PatchRegion {
    pub color: RgbImage,
    pub mask: GrayImage,
    pub min_x: u32,
    pub min_y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn mask_bounds(mask: &GrayImage) -> anyhow::Result<MaskBounds> {
    let (img_w, img_h) = mask.dimensions();
    let width = img_w as usize;
    let height = img_h as usize;
    if width == 0 || height == 0 {
        bail!("Mask is empty.");
    }
    let raw = mask.as_raw();
    let non_empty: Vec<usize> = (0..height)
        .filter(|&y| raw[y * width..(y + 1) * width].iter().any(|&p| p > 0))
        .collect();
    let Some(&min_y) = non_empty.first() else {
        bail!("Mask is empty.");
    };
    let max_y = non_empty.last().copied().unwrap_or(min_y);
    let (min_x, max_x) = non_empty.iter().fold((width, 0), |(mn, mx), &y| {
        let row = &raw[y * width..(y + 1) * width];
        let mn = row.iter().position(|&p| p > 0).map_or(mn, |p| mn.min(p));
        let mx = row.iter().rposition(|&p| p > 0).map_or(mx, |p| mx.max(p));
        (mn, mx)
    });
    let conv = |v: usize| u32::try_from(v).unwrap_or_default();
    Ok((conv(min_x), conv(max_x), conv(min_y), conv(max_y)))
}

pub fn encode_patch_result(
    color: &RgbImage,
    mask: &GrayImage,
    offset_x: u32,
    offset_y: u32,
    width: u32,
    height: u32,
    is_srgb: bool,
    quality: u8,
    mask_as_png: bool,
) -> anyhow::Result<String> {
    let mut color_buf = Cursor::new(Vec::with_capacity(32_768));
    color.write_with_encoder(JpegEncoder::new_with_quality(&mut color_buf, quality))?;
    let color_base64 = general_purpose::STANDARD.encode(color_buf.get_ref());

    let mut mask_buf = Cursor::new(Vec::with_capacity(32_768));
    if mask_as_png {
        mask.write_to(&mut mask_buf, image::ImageFormat::Png)?;
    } else {
        mask.write_with_encoder(JpegEncoder::new_with_quality(&mut mask_buf, quality))?;
    }
    let mask_base64 = general_purpose::STANDARD.encode(mask_buf.get_ref());

    Ok(serde_json::json!({
        "color": color_base64,
        "mask": mask_base64,
        "offsetX": offset_x,
        "offsetY": offset_y,
        "width": width,
        "height": height,
        "isSrgbEncoded": is_srgb
    })
    .to_string())
}

#[must_use]
pub fn is_heal_patch(sub_masks: &Value, patch_name: &str) -> bool {
    let name_heal = patch_name.to_lowercase().contains("heal");
    let sub_heal = sub_masks.as_array().is_some_and(|arr| {
        arr.iter()
            .filter_map(|sm| sm.get("type").and_then(Value::as_str))
            .any(|t| t.eq_ignore_ascii_case("heal"))
    });
    name_heal || sub_heal
}

fn point_pairs(vals: &[Value]) -> Vec<(f32, f32)> {
    vals.iter()
        .filter_map(|pv| {
            pv.get("x")
                .and_then(Value::as_f64)
                .zip(pv.get("y").and_then(Value::as_f64))
                .map(|(x, y)| (x as f32, y as f32))
        })
        .collect()
}

fn line_points(line: &Value) -> Vec<(f32, f32)> {
    line.get("points")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |pts| point_pairs(pts))
}

// Liquify

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquifyMode {
    Push,
    Pinch,
    Expand,
    Twirl,
    Eraser,
}

#[derive(Clone)]
struct LiquifyStroke {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    radius: f32,
    radius_sq: f32,
    feather: f32,
    pressure: f32,
    mode: LiquifyMode,
    aabb_min_x: f32,
    aabb_max_x: f32,
    aabb_min_y: f32,
    aabb_max_y: f32,
}

fn point_stroke(
    x: f32,
    y: f32,
    radius: f32,
    feather: f32,
    pressure: f32,
    mode: LiquifyMode,
) -> LiquifyStroke {
    segment_stroke(x, y, x, y, radius, feather, pressure, mode)
}

fn segment_stroke(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    radius: f32,
    feather: f32,
    pressure: f32,
    mode: LiquifyMode,
) -> LiquifyStroke {
    LiquifyStroke {
        x1,
        y1,
        x2,
        y2,
        radius,
        radius_sq: radius * radius,
        feather,
        pressure,
        mode,
        aabb_min_x: x1.min(x2) - radius,
        aabb_max_x: x1.max(x2) + radius,
        aabb_min_y: y1.min(y2) - radius,
        aabb_max_y: y1.max(y2) + radius,
    }
}

fn parse_liquify(sub_masks: &Value) -> (Vec<LiquifyStroke>, Vec<(f32, f32, f32)>) {
    let Some(masks) = sub_masks.as_array() else {
        return (Vec::new(), Vec::new());
    };
    let per_line = masks
        .iter()
        .filter(|sm| {
            sm.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.eq_ignore_ascii_case("liquify"))
        })
        .filter_map(|sm| {
            let params = sm.get("parameters")?;
            let pressure = params
                .get("pressure")
                .and_then(Value::as_f64)
                .unwrap_or(50.0) as f32;
            let force = (pressure / 100.0).clamp(0.01, 1.0);
            let mode = match params.get("liquifyMode").and_then(Value::as_str) {
                Some("pinch") => LiquifyMode::Pinch,
                Some("expand") => LiquifyMode::Expand,
                Some("twirl") => LiquifyMode::Twirl,
                _ => LiquifyMode::Push,
            };
            params
                .get("lines")
                .and_then(Value::as_array)
                .map(|lines| (lines, force, mode))
        })
        .flat_map(|(lines, force, mode)| lines.iter().map(move |line| (line, force, mode)))
        .filter_map(|(line, force, mode)| {
            let radius = line
                .get("brushSize")
                .and_then(Value::as_f64)
                .unwrap_or(50.0) as f32
                / 2.0;
            let feather = line.get("feather").and_then(Value::as_f64).unwrap_or(0.5) as f32;
            let mode = if line.get("tool").and_then(Value::as_str) == Some("eraser") {
                LiquifyMode::Eraser
            } else {
                mode
            };
            Some((line_points(line), radius, feather, force, mode))
        });

    per_line.fold(
        (Vec::new(), Vec::new()),
        |(mut strokes, mut points), entry| {
            let (pts, radius, feather, force, mode) = entry;
            if pts.len() == 1 {
                let (xf, yf) = pts[0];
                points.push((xf, yf, radius));
                strokes.push(point_stroke(xf, yf, radius, feather, force, mode));
                return (strokes, points);
            }
            let segs = pts
                .iter()
                .fold((Vec::new(), None), |(mut segs, prev), &(xf, yf)| {
                    if let Some((px, py)) = prev {
                        segs.push(segment_stroke(px, py, xf, yf, radius, feather, force, mode));
                    }
                    (segs, Some((xf, yf)))
                });
            strokes.extend(segs.0);
            points.extend(pts.iter().map(|&(x, y)| (x, y, radius)));
            (strokes, points)
        },
    )
}

fn point_to_segment_dist_sq(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> (f32, f32) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return ((px - x1) * (px - x1) + (py - y1) * (py - y1), 0.0);
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / l2;
    let t = t.clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    (
        (px - proj_x) * (px - proj_x) + (py - proj_y) * (py - proj_y),
        t,
    )
}

fn bilinear_sample(img: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let w = i32::try_from(img.width()).unwrap_or(1);
    let h = i32::try_from(img.height()).unwrap_or(1);
    let x_floor = x.floor() as i32;
    let y_floor = y.floor() as i32;
    let x_frac = x - x_floor as f32;
    let y_frac = y - y_floor as f32;
    let x0 = x_floor.clamp(0, w - 1) as u32;
    let y0 = y_floor.clamp(0, h - 1) as u32;
    let x1 = (x_floor + 1).clamp(0, w - 1) as u32;
    let y1 = (y_floor + 1).clamp(0, h - 1) as u32;

    let (p00, p10, p01, p11) = (
        img.get_pixel(x0, y0),
        img.get_pixel(x1, y0),
        img.get_pixel(x0, y1),
        img.get_pixel(x1, y1),
    );
    let out = std::array::from_fn(|c| {
        let val = f32::from(p00[c]) * (1.0 - x_frac) * (1.0 - y_frac)
            + f32::from(p10[c]) * x_frac * (1.0 - y_frac)
            + f32::from(p01[c]) * (1.0 - x_frac) * y_frac
            + f32::from(p11[c]) * x_frac * y_frac;
        val.clamp(0.0, 255.0) as u8
    });
    Rgb(out)
}

pub fn liquify(source: &RgbImage, sub_masks: &Value) -> anyhow::Result<PatchRegion> {
    let (strokes, points) = parse_liquify(sub_masks);
    if strokes.is_empty() {
        bail!("No brush strokes found for Liquify.");
    }

    let (img_w, img_h) = source.dimensions();
    let img_w_i = i32::try_from(img_w).unwrap_or(1);
    let img_h_i = i32::try_from(img_h).unwrap_or(1);

    let max_brush_radius = points.iter().map(|&(_, _, r)| r).fold(0.0_f32, f32::max);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = points.iter().fold(
        (img_w as f32, img_h as f32, 0.0_f32, 0.0_f32),
        |(mnx, mny, mxx, mxy), &(x, y, r)| {
            (
                mnx.min(x - r),
                mny.min(y - r),
                mxx.max(x + r),
                mxy.max(y + r),
            )
        },
    );

    let margin = (max_brush_radius * 2.5).clamp(50.0, 300.0);
    min_x -= margin;
    min_y -= margin;
    max_x += margin;
    max_y += margin;

    let min_x_u32 = clamp_u32(min_x, img_w_i - 1);
    let min_y_u32 = clamp_u32(min_y, img_h_i - 1);
    let max_x_u32 = clamp_u32(max_x, img_w_i - 1);
    let max_y_u32 = clamp_u32(max_y, img_h_i - 1);

    let crop_w = max_x_u32 - min_x_u32 + 1;
    let crop_h = max_y_u32 - min_y_u32 + 1;

    let crop_w_us = crop_w as usize;
    let row_stride = crop_w_us * 3;
    let mut color_pixels = vec![0u8; row_stride * crop_h as usize];
    let mut mask_pixels = vec![0u8; crop_w_us * crop_h as usize];

    let min_x_f = min_x_u32 as f32;
    let min_y_f = min_y_u32 as f32;

    color_pixels
        .par_chunks_mut(row_stride)
        .zip(mask_pixels.par_chunks_mut(crop_w_us))
        .enumerate()
        .for_each(|(y, (color_row, mask_row))| {
            let orig_y = y as f32 + min_y_f;
            (0..crop_w_us).for_each(|x| {
                let orig_x = x as f32 + min_x_f;
                let (disp_x, disp_y) = strokes.iter().fold((0.0_f32, 0.0_f32), |acc, stroke| {
                    if orig_x < stroke.aabb_min_x
                        || orig_x > stroke.aabb_max_x
                        || orig_y < stroke.aabb_min_y
                        || orig_y > stroke.aabb_max_y
                    {
                        return acc;
                    }
                    let (dist_sq, t) = point_to_segment_dist_sq(
                        orig_x, orig_y, stroke.x1, stroke.y1, stroke.x2, stroke.y2,
                    );
                    if dist_sq >= stroke.radius_sq {
                        return acc;
                    }
                    let dist = dist_sq.sqrt();
                    let feather = stroke.feather.clamp(0.01, 1.0);
                    let inner_radius = stroke.radius * (1.0 - feather);
                    let elastic_falloff = if dist <= inner_radius {
                        1.0
                    } else {
                        let falloff_t = (dist - inner_radius) / (stroke.radius - inner_radius);
                        let cos_val = f32::midpoint(1.0, (falloff_t * std::f32::consts::PI).cos());
                        cos_val * cos_val
                    };
                    let dx = stroke.x2 - stroke.x1;
                    let dy = stroke.y2 - stroke.y1;
                    let step_dist = (dx * dx + dy * dy).sqrt();
                    let step_factor = if step_dist < 0.001 {
                        0.45
                    } else {
                        (step_dist / stroke.radius).clamp(0.05, 0.6)
                    };
                    match stroke.mode {
                        LiquifyMode::Push => {
                            let k = elastic_falloff * stroke.pressure * 0.6;
                            (acc.0 - dx * k, acc.1 - dy * k)
                        }
                        LiquifyMode::Pinch => {
                            let strength = elastic_falloff * stroke.pressure * step_factor * 0.45;
                            let rx = orig_x - (stroke.x1 + t * dx);
                            let ry = orig_y - (stroke.y1 + t * dy);
                            (acc.0 + rx * strength, acc.1 + ry * strength)
                        }
                        LiquifyMode::Expand => {
                            let strength = elastic_falloff * stroke.pressure * step_factor * 0.30;
                            let rx = orig_x - (stroke.x1 + t * dx);
                            let ry = orig_y - (stroke.y1 + t * dy);
                            (acc.0 - rx * strength, acc.1 - ry * strength)
                        }
                        LiquifyMode::Twirl => {
                            let angle = elastic_falloff * stroke.pressure * step_factor * 1.4;
                            let (cos_a, sin_a) = (angle.cos(), angle.sin());
                            let rx = orig_x - (stroke.x1 + t * dx);
                            let ry = orig_y - (stroke.y1 + t * dy);
                            (
                                acc.0 + (rx * cos_a - ry * sin_a) - rx,
                                acc.1 + (rx * sin_a + ry * cos_a) - ry,
                            )
                        }
                        LiquifyMode::Eraser => {
                            let erase_strength =
                                (elastic_falloff * stroke.pressure * step_factor * 5.0)
                                    .clamp(0.0, 1.0);
                            let retain = 1.0 - erase_strength;
                            (acc.0 * retain, acc.1 * retain)
                        }
                    }
                });

                if disp_x * disp_x + disp_y * disp_y > 0.001 {
                    let disp_sq = disp_x * disp_x + disp_y * disp_y;
                    let mask_val = (disp_sq.sqrt() * 255.0).clamp(0.0, 255.0) as u8;
                    let px = bilinear_sample(source, orig_x + disp_x, orig_y + disp_y);
                    color_row[x * 3] = px[0];
                    color_row[x * 3 + 1] = px[1];
                    color_row[x * 3 + 2] = px[2];
                    mask_row[x] = mask_val;
                } else {
                    let px = source.get_pixel(orig_x as u32, orig_y as u32);
                    color_row[x * 3] = px[0];
                    color_row[x * 3 + 1] = px[1];
                    color_row[x * 3 + 2] = px[2];
                    mask_row[x] = 0;
                }
            });
        });

    let color = RgbImage::from_raw(crop_w, crop_h, color_pixels)
        .ok_or_else(|| anyhow::anyhow!("Liquify buffer size mismatch"))?;
    let mask = GrayImage::from_raw(crop_w, crop_h, mask_pixels)
        .ok_or_else(|| anyhow::anyhow!("Liquify mask buffer size mismatch"))?;

    Ok(PatchRegion {
        color,
        mask,
        min_x: min_x_u32,
        min_y: min_y_u32,
        width: crop_w,
        height: crop_h,
    })
}

// Manual cleanup (clone stamp / Poisson heal)

fn clamp_cast_u32(v: i32) -> u32 {
    if v <= 0 {
        0
    } else {
        u32::try_from(v).unwrap_or_default()
    }
}

fn clamp_u32(v: f32, max: i32) -> u32 {
    clamp_cast_u32((v as i32).clamp(0, max))
}

fn clone_fill(
    source: &DynamicImage,
    mask: &GrayImage,
    bounds: MaskBounds,
    offset_x: i32,
    offset_y: i32,
) -> RgbImage {
    let (min_x, max_x, min_y, max_y) = bounds;
    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    let (img_w, img_h) = source.dimensions();
    let img_w_i = i32::try_from(img_w).unwrap_or(1) - 1;
    let img_h_i = i32::try_from(img_h).unwrap_or(1) - 1;

    let mut color = RgbImage::new(crop_w, crop_h);
    (min_y..=max_y).for_each(|y| {
        (min_x..=max_x)
            .filter(|&x| mask.get_pixel(x, y)[0] > 0)
            .for_each(|x| {
                let x_i = i32::try_from(x).unwrap_or(i32::MAX);
                let y_i = i32::try_from(y).unwrap_or(i32::MAX);
                let src_x = (x_i + offset_x).clamp(0, img_w_i);
                let src_y = (y_i + offset_y).clamp(0, img_h_i);
                let src_px = source.get_pixel(clamp_cast_u32(src_x), clamp_cast_u32(src_y));
                color.put_pixel(x - min_x, y - min_y, Rgb([src_px[0], src_px[1], src_px[2]]));
            });
    });
    color
}

fn poisson_fill(
    source: &DynamicImage,
    mask: &GrayImage,
    bounds: MaskBounds,
    offset_x: i32,
    offset_y: i32,
) -> anyhow::Result<RgbImage> {
    let (min_x, max_x, min_y, max_y) = bounds;
    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    let bw = crop_w + 2;
    let bh = crop_h + 2;
    let bw_us = usize::try_from(bw)?;
    let bh_us = usize::try_from(bh)?;
    let (img_w, img_h) = source.dimensions();
    let img_w_i = i32::try_from(img_w).unwrap_or(1);
    let img_h_i = i32::try_from(img_h).unwrap_or(1);
    let min_x_i = i32::try_from(min_x)?;
    let min_y_i = i32::try_from(min_y)?;

    let mut region = vec![0u8; bw_us * bh_us];
    (0..bh_us).for_each(|y| {
        (0..bw_us).for_each(|x| {
            let img_x = i32::try_from(x).unwrap_or(0) + min_x_i - 1;
            let img_y = i32::try_from(y).unwrap_or(0) + min_y_i - 1;
            if img_x >= 0
                && img_x < img_w_i
                && img_y >= 0
                && img_y < img_h_i
                && mask.get_pixel(clamp_cast_u32(img_x), clamp_cast_u32(img_y))[0] > 0
            {
                region[y * bw_us + x] = 1;
            }
        });
    });

    let interior = || (1..bh_us - 1).flat_map(move |y| (1..bw_us - 1).map(move |x| (x, y)));
    let ring: Vec<(usize, usize)> = interior()
        .filter(|&(x, y)| {
            let idx = y * bw_us + x;
            region[idx] == 0
                && (region[idx - bw_us] == 1
                    || region[idx + bw_us] == 1
                    || region[idx - 1] == 1
                    || region[idx + 1] == 1)
        })
        .collect();
    ring.iter().for_each(|&(x, y)| region[y * bw_us + x] = 2);

    let omega: Vec<(usize, usize)> = interior()
        .filter(|&(x, y)| region[y * bw_us + x] == 1)
        .collect();

    let mut v_r = vec![0.0_f32; bw_us * bh_us];
    let mut v_g = vec![0.0_f32; bw_us * bh_us];
    let mut v_b = vec![0.0_f32; bw_us * bh_us];

    ring.iter().for_each(|&(x, y)| {
        let idx = y * bw_us + x;
        let img_x = i32::try_from(x).unwrap_or(0) + min_x_i - 1;
        let img_y = i32::try_from(y).unwrap_or(0) + min_y_i - 1;
        let img_x_u = clamp_cast_u32(img_x);
        let img_y_u = clamp_cast_u32(img_y);
        let src_x = (img_x + offset_x).clamp(0, img_w_i - 1);
        let src_y = (img_y + offset_y).clamp(0, img_h_i - 1);
        if img_x_u < img_w && img_y_u < img_h && mask.get_pixel(img_x_u, img_y_u)[0] > 0 {
            let dest_px = source.get_pixel(img_x_u, img_y_u);
            let src_px = source.get_pixel(clamp_cast_u32(src_x), clamp_cast_u32(src_y));
            v_r[idx] = f32::from(dest_px[0]) - f32::from(src_px[0]);
            v_g[idx] = f32::from(dest_px[1]) - f32::from(src_px[1]);
            v_b[idx] = f32::from(dest_px[2]) - f32::from(src_px[2]);
        }
    });

    let sor = 1.6_f32;
    (0..400).for_each(|_| {
        omega.iter().for_each(|&(x, y)| {
            let idx = y * bw_us + x;
            let sum_r = v_r[idx - bw_us] + v_r[idx + bw_us] + v_r[idx - 1] + v_r[idx + 1];
            let sum_g = v_g[idx - bw_us] + v_g[idx + bw_us] + v_g[idx - 1] + v_g[idx + 1];
            let sum_b = v_b[idx - bw_us] + v_b[idx + bw_us] + v_b[idx - 1] + v_b[idx + 1];
            v_r[idx] = (1.0 - sor) * v_r[idx] + sor * 0.25 * sum_r;
            v_g[idx] = (1.0 - sor) * v_g[idx] + sor * 0.25 * sum_g;
            v_b[idx] = (1.0 - sor) * v_b[idx] + sor * 0.25 * sum_b;
        });
    });

    let mut color = RgbImage::new(crop_w, crop_h);
    omega.iter().for_each(|&(x, y)| {
        let idx = y * bw_us + x;
        let img_x = i32::try_from(x).unwrap_or(0) + min_x_i - 1;
        let img_y = i32::try_from(y).unwrap_or(0) + min_y_i - 1;
        let src_x = (img_x + offset_x).clamp(0, img_w_i - 1);
        let src_y = (img_y + offset_y).clamp(0, img_h_i - 1);
        let src_px = source.get_pixel(clamp_cast_u32(src_x), clamp_cast_u32(src_y));
        let out_r = (f32::from(src_px[0]) + v_r[idx]).clamp(0.0, 255.0) as u8;
        let out_g = (f32::from(src_px[1]) + v_g[idx]).clamp(0.0, 255.0) as u8;
        let out_b = (f32::from(src_px[2]) + v_b[idx]).clamp(0.0, 255.0) as u8;
        color.put_pixel(
            u32::try_from(x - 1).unwrap_or_default(),
            u32::try_from(y - 1).unwrap_or_default(),
            Rgb([out_r, out_g, out_b]),
        );
    });
    Ok(color)
}

pub fn cleanup_patch(
    source: &DynamicImage,
    mask: &GrayImage,
    bounds: MaskBounds,
    offset_x: i32,
    offset_y: i32,
    heal: bool,
) -> anyhow::Result<PatchRegion> {
    let (min_x, max_x, min_y, max_y) = bounds;
    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    let color = if heal {
        poisson_fill(source, mask, bounds, offset_x, offset_y)?
    } else {
        clone_fill(source, mask, bounds, offset_x, offset_y)
    };
    let mask_out = image::imageops::crop_imm(mask, min_x, min_y, crop_w, crop_h).to_image();
    Ok(PatchRegion {
        color,
        mask: mask_out,
        min_x,
        min_y,
        width: crop_w,
        height: crop_h,
    })
}

// Generative replace compositing

#[must_use]
pub fn composite_patch(
    source: &DynamicImage,
    patch: &RgbaImage,
    patch_x: u32,
    patch_y: u32,
    mask: &GrayImage,
    bounds: MaskBounds,
) -> PatchRegion {
    let (min_x, max_x, min_y, max_y) = bounds;
    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    let mut color = RgbImage::new(crop_w, crop_h);
    (min_x..=max_x).for_each(|x| {
        (min_y..=max_y).for_each(|y| {
            let px = if mask.get_pixel(x, y)[0] > 0 {
                patch
                    .get_pixel_checked(x.wrapping_sub(patch_x), y.wrapping_sub(patch_y))
                    .copied()
                    .unwrap_or(Rgba([0, 0, 0, 0]))
            } else {
                source.get_pixel(x, y)
            };
            color.put_pixel(x - min_x, y - min_y, Rgb([px[0], px[1], px[2]]));
        });
    });
    let mask_out = image::imageops::crop_imm(mask, min_x, min_y, crop_w, crop_h).to_image();
    PatchRegion {
        color,
        mask: mask_out,
        min_x,
        min_y,
        width: crop_w,
        height: crop_h,
    }
}

// Retouch (guided bilateral-style blend)

struct RetouchStroke {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    radius: f32,
    feather: f32,
    eraser: bool,
}

struct RetouchPlan {
    strokes: Vec<RetouchStroke>,
    intensity: f32,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    max_radius: f32,
}

fn parse_retouch(sub_masks: &Value) -> RetouchPlan {
    let Some(masks) = sub_masks.as_array() else {
        return RetouchPlan {
            strokes: Vec::new(),
            intensity: 50.0,
            min_x: 0.0,
            min_y: 0.0,
            max_x: -1.0,
            max_y: -1.0,
            max_radius: 0.0,
        };
    };
    let per_line = masks
        .iter()
        .filter(|sm| sm.get("type").and_then(Value::as_str) == Some("retouch"))
        .filter_map(|sm| sm.get("parameters"))
        .filter_map(|params| {
            let intensity = params
                .get("intensity")
                .and_then(Value::as_f64)
                .unwrap_or(50.0) as f32;
            params
                .get("lines")
                .and_then(Value::as_array)
                .map(|lines| (lines, intensity))
        })
        .flat_map(|(lines, intensity)| lines.iter().map(move |line| (line, intensity)));

    per_line.fold(
        RetouchPlan {
            strokes: Vec::new(),
            intensity: 50.0,
            min_x: f32::MAX,
            min_y: f32::MAX,
            max_x: f32::MIN,
            max_y: f32::MIN,
            max_radius: 0.0,
        },
        |mut plan, (line, intensity)| {
            plan.intensity = intensity;
            let radius = line
                .get("brushSize")
                .and_then(Value::as_f64)
                .unwrap_or(50.0) as f32
                / 2.0;
            let feather = line.get("feather").and_then(Value::as_f64).unwrap_or(0.5) as f32;
            let eraser = line.get("tool").and_then(Value::as_str) == Some("eraser");
            plan.max_radius = plan.max_radius.max(radius);
            let pts = line_points(line);
            if pts.len() == 1 {
                let (xf, yf) = pts[0];
                plan.min_x = plan.min_x.min(xf - radius);
                plan.min_y = plan.min_y.min(yf - radius);
                plan.max_x = plan.max_x.max(xf + radius);
                plan.max_y = plan.max_y.max(yf + radius);
                plan.strokes.push(RetouchStroke {
                    x1: xf,
                    y1: yf,
                    x2: xf,
                    y2: yf,
                    radius,
                    feather,
                    eraser,
                });
            } else {
                pts.iter().for_each(|&(xf, yf)| {
                    plan.min_x = plan.min_x.min(xf - radius);
                    plan.min_y = plan.min_y.min(yf - radius);
                    plan.max_x = plan.max_x.max(xf + radius);
                    plan.max_y = plan.max_y.max(yf + radius);
                });
                let segs = pts.iter().zip(pts.iter().skip(1));
                plan.strokes
                    .extend(segs.map(|(&(px, py), &(qx, qy))| RetouchStroke {
                        x1: px,
                        y1: py,
                        x2: qx,
                        y2: qy,
                        radius,
                        feather,
                        eraser,
                    }));
            }
            plan
        },
    )
}

fn draw_stroke_to_mask(
    mask: &mut [u8],
    img_w: u32,
    img_h: u32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    radius: f32,
    feather: f32,
    is_eraser: bool,
) {
    let radius_sq = radius * radius;
    let inner_radius = radius * (1.0 - feather.clamp(0.0, 1.0));
    let img_w_i = i32::try_from(img_w).unwrap_or(1);
    let img_h_i = i32::try_from(img_h).unwrap_or(1);
    let img_w_us = img_w as usize;

    let min_x = ((x1.min(x2) - radius).floor() as i32).clamp(0, img_w_i - 1);
    let min_y = ((y1.min(y2) - radius).floor() as i32).clamp(0, img_h_i - 1);
    let max_x = ((x1.max(x2) + radius).ceil() as i32).clamp(0, img_w_i - 1);
    let max_y = ((y1.max(y2) + radius).ceil() as i32).clamp(0, img_h_i - 1);

    (min_y..=max_y).for_each(|y| {
        (min_x..=max_x).for_each(|x| {
            let (dist_sq, _) = point_to_segment_dist_sq(x as f32, y as f32, x1, y1, x2, y2);
            if dist_sq <= radius_sq {
                let dist = dist_sq.sqrt();
                let alpha = if dist <= inner_radius {
                    1.0
                } else {
                    let t = (dist - inner_radius) / (radius - inner_radius);
                    let cos_val = f32::midpoint(1.0, (t * std::f32::consts::PI).cos());
                    cos_val * cos_val
                };
                let added_val = (alpha * 255.0) as u8;
                let idx =
                    (usize::try_from(y).unwrap_or(0) * img_w_us) + usize::try_from(x).unwrap_or(0);
                if is_eraser {
                    mask[idx] = mask[idx].saturating_sub(added_val);
                } else {
                    mask[idx] = mask[idx].saturating_add(added_val);
                }
            }
        });
    });
}

pub fn retouch(source: &RgbImage, sub_masks: &Value) -> anyhow::Result<PatchRegion> {
    let plan = parse_retouch(sub_masks);
    if plan.min_x > plan.max_x || plan.min_y > plan.max_y {
        bail!("No brush strokes found for Retouch.");
    }

    let (img_w, img_h) = source.dimensions();
    let img_w_us = img_w as usize;
    let img_w_f = img_w as f32;
    let img_h_f = img_h as f32;

    let mut mask_canvas = vec![0u8; img_w_us * img_h as usize];
    plan.strokes.iter().for_each(|s| {
        draw_stroke_to_mask(
            &mut mask_canvas,
            img_w,
            img_h,
            s.x1,
            s.y1,
            s.x2,
            s.y2,
            s.radius,
            s.feather,
            s.eraser,
        );
    });

    let pad = (plan.max_radius * 3.0 + 30.0).ceil();
    let min_x = (plan.min_x - pad).clamp(0.0, img_w_f - 1.0) as u32;
    let min_y = (plan.min_y - pad).clamp(0.0, img_h_f - 1.0) as u32;
    let max_x = (plan.max_x + pad).clamp(0.0, img_w_f - 1.0) as u32;
    let max_y = (plan.max_y + pad).clamp(0.0, img_h_f - 1.0) as u32;
    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    let crop_w_us = crop_w as usize;
    let crop_h_us = crop_h as usize;

    let orig_crop = image::imageops::crop_imm(source, min_x, min_y, crop_w, crop_h).to_image();
    let orig_raw = orig_crop.as_raw();

    let min_x_us = min_x as usize;
    let min_y_us = min_y as usize;
    let mask_pixels: Vec<u8> = (0..crop_h_us)
        .flat_map(move |y| (0..crop_w_us).map(move |x| (x, y)))
        .map(|(x, y)| mask_canvas[(y + min_y_us) * img_w_us + (x + min_x_us)])
        .collect();

    let norm_intensity = (plan.intensity / 100.0).clamp(0.05, 1.0);
    let sig_s = (1.0 + norm_intensity * 6.0).clamp(1.2, 8.0);
    let sig_c = (8.0 + norm_intensity * 20.0).clamp(8.0, 28.0);
    let sig_s_sq = sig_s * sig_s;
    let sig_c_sq = sig_c * sig_c;

    let r = (sig_s * 2.0).ceil() as i32;
    let window_size = (2 * r + 1) as usize;

    let spatial_weights: Vec<f32> = (-r..=r)
        .flat_map(move |dy| (-r..=r).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| {
            let dist_s = (dx * dx + dy * dy) as f32;
            (-dist_s / (2.0 * sig_s_sq)).exp()
        })
        .collect();

    let color_weights: Vec<f32> = (0..=3 * 255 * 255)
        .map(|i| (-(i as f32) / (2.0 * sig_c_sq)).exp())
        .collect();

    let crop_w_i = i32::try_from(crop_w).unwrap_or(1);
    let crop_h_i = i32::try_from(crop_h).unwrap_or(1);
    let mut color_pixels = vec![0u8; crop_w_us * crop_h_us * 3];

    color_pixels
        .par_chunks_mut(crop_w_us * 3)
        .enumerate()
        .for_each(|(y, row_out)| {
            let y_i = i32::try_from(y).unwrap_or(0);
            (0..crop_w_us).for_each(|x| {
                let px_idx = (y * crop_w_us + x) * 3;
                let m_val = mask_pixels[y * crop_w_us + x];
                if m_val == 0 {
                    row_out[px_idx..px_idx + 3].copy_from_slice(&orig_raw[px_idx..px_idx + 3]);
                    return;
                }

                let m = f32::from(m_val) / 255.0;
                let smooth_m = m * m * (3.0 - 2.0 * m);
                let blend = smooth_m * (0.3 + norm_intensity * 0.7);

                let ctr_r = i32::from(orig_raw[px_idx]);
                let ctr_g = i32::from(orig_raw[px_idx + 1]);
                let ctr_b = i32::from(orig_raw[px_idx + 2]);
                let x_i = i32::try_from(x).unwrap_or(0);

                let y_min = (y_i - r).max(0);
                let y_max = (y_i + r).min(crop_h_i - 1);
                let x_min = (x_i - r).max(0);
                let x_max = (x_i + r).min(crop_w_i - 1);

                let window =
                    (y_min..=y_max).flat_map(move |ny| (x_min..=x_max).map(move |nx| (nx, ny)));
                let sums = window.fold((0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32), |acc, (nx, ny)| {
                    let (sum_w, sum_r, sum_g, sum_b) = acc;
                    let spatial_w = spatial_weights[(usize::try_from(ny - y_i + r).unwrap_or(0)
                        * window_size)
                        + usize::try_from(nx - x_i + r).unwrap_or(0)];
                    let n_idx = (usize::try_from(ny).unwrap_or(0) * crop_w_us
                        + usize::try_from(nx).unwrap_or(0))
                        * 3;
                    let pr = i32::from(orig_raw[n_idx]);
                    let pg = i32::from(orig_raw[n_idx + 1]);
                    let pb = i32::from(orig_raw[n_idx + 2]);
                    let dr = pr - ctr_r;
                    let dg = pg - ctr_g;
                    let db = pb - ctr_b;
                    let color_dist_sq = (dr * dr + dg * dg + db * db) as usize;
                    let w = spatial_w * color_weights[color_dist_sq];
                    (
                        sum_w + w,
                        sum_r + pr as f32 * w,
                        sum_g + pg as f32 * w,
                        sum_b + pb as f32 * w,
                    )
                });

                let base_r = sums.1 / sums.0;
                let base_g = sums.2 / sums.0;
                let base_b = sums.3 / sums.0;
                row_out[px_idx] =
                    (ctr_r as f32 * (1.0 - blend) + base_r * blend).clamp(0.0, 255.0) as u8;
                row_out[px_idx + 1] =
                    (ctr_g as f32 * (1.0 - blend) + base_g * blend).clamp(0.0, 255.0) as u8;
                row_out[px_idx + 2] =
                    (ctr_b as f32 * (1.0 - blend) + base_b * blend).clamp(0.0, 255.0) as u8;
            });
        });

    let color = RgbImage::from_raw(crop_w, crop_h, color_pixels)
        .ok_or_else(|| anyhow::anyhow!("Retouch buffer size mismatch"))?;
    let mask = GrayImage::from_raw(crop_w, crop_h, mask_pixels)
        .ok_or_else(|| anyhow::anyhow!("Retouch mask buffer size mismatch"))?;

    Ok(PatchRegion {
        color,
        mask,
        min_x,
        min_y,
        width: crop_w,
        height: crop_h,
    })
}
