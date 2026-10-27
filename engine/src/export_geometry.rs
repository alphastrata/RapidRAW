use image::{DynamicImage, GenericImageView, ImageBuffer, imageops};
use serde::{Deserialize, Serialize};

use crate::launch_request::TiffBitDepth;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ResizeMode {
    LongEdge,
    ShortEdge,
    Width,
    Height,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResizeOptions {
    pub mode: ResizeMode,
    pub value: u32,
    pub dont_enlarge: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum BorderBasis {
    #[default]
    LongEdge,
    ShortEdge,
    EachEdge,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BorderOptions {
    #[serde(default)]
    pub basis: BorderBasis,
    pub horizontal_percent: f32,
    pub vertical_percent: f32,
    pub color: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PadOptions {
    pub ratio_width: f32,
    pub ratio_height: f32,
    pub color: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum WatermarkAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkSettings {
    pub path: String,
    pub anchor: WatermarkAnchor,
    pub scale: f32,
    pub spacing: f32,
    pub opacity: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    pub jpeg_quality: u8,
    #[serde(default)]
    pub tiff_bit_depth: TiffBitDepth,
    pub resize: Option<ResizeOptions>,
    #[serde(default)]
    pub border: Option<BorderOptions>,
    #[serde(default)]
    pub pad: Option<PadOptions>,
    pub keep_metadata: bool,
    #[serde(default)]
    pub preserve_timestamps: bool,
    pub strip_gps: bool,
    pub filename_template: Option<String>,
    pub watermark: Option<WatermarkSettings>,
    #[serde(default)]
    pub export_masks: bool,
    #[serde(default)]
    pub preserve_folders: bool,
    #[serde(default)]
    pub destination_type: Option<String>,
    #[serde(default)]
    pub subfolder: Option<String>,
    #[serde(default)]
    pub dpi: Option<u16>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportGeometry {
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub photo_w: u32,
    pub photo_h: u32,
    pub offset_x: u32,
    pub offset_y: u32,
    pub border_x: u32,
    pub border_y: u32,
}

impl ExportGeometry {
    pub const fn identity(canvas_w: u32, canvas_h: u32) -> Self {
        Self {
            canvas_w,
            canvas_h,
            photo_w: canvas_w,
            photo_h: canvas_h,
            offset_x: 0,
            offset_y: 0,
            border_x: 0,
            border_y: 0,
        }
    }

    pub const fn has_border(&self) -> bool {
        self.border_x > 0 || self.border_y > 0
    }

    pub const fn is_identity(&self) -> bool {
        self.canvas_w == self.photo_w && self.canvas_h == self.photo_h
    }
}

const PAD_RATIO_EPSILON: f64 = 0.001;
const MAX_PAD_DIMENSION: u32 = 65_535;
const MAX_BORDER_PERCENT: f64 = 100.0;

pub fn calculate_resize_target(
    current_w: u32,
    current_h: u32,
    resize_opts: &ResizeOptions,
) -> (u32, u32) {
    if resize_opts.dont_enlarge {
        let exceeds = match resize_opts.mode {
            ResizeMode::LongEdge => current_w.max(current_h) > resize_opts.value,
            ResizeMode::ShortEdge => current_w.min(current_h) > resize_opts.value,
            ResizeMode::Width => current_w > resize_opts.value,
            ResizeMode::Height => current_h > resize_opts.value,
        };
        if !exceeds {
            return (current_w, current_h);
        }
    }

    let fix_width = match resize_opts.mode {
        ResizeMode::LongEdge => current_w >= current_h,
        ResizeMode::ShortEdge => current_w <= current_h,
        ResizeMode::Width => true,
        ResizeMode::Height => false,
    };

    let value_f = resize_opts.value as f32;
    let w_f = current_w as f32;
    let h_f = current_h as f32;
    if fix_width {
        let h = (value_f * (h_f / w_f)).round() as u32;
        (resize_opts.value, h)
    } else {
        let w = (value_f * (w_f / h_f)).round() as u32;
        (w, resize_opts.value)
    }
}

pub fn resize_target(width: u32, height: u32, resize_opts: Option<&ResizeOptions>) -> (u32, u32) {
    match resize_opts {
        Some(opts) => calculate_resize_target(width, height, opts),
        None => (width, height),
    }
}

pub fn parse_hex_color(hex: &str) -> Option<[u8; 3]> {
    let raw = hex.trim().trim_start_matches('#');
    let normalized: String = match raw.len() {
        3 => raw.chars().flat_map(|c| [c, c]).collect(),
        6 => raw.to_string(),
        8 => raw[..6].to_string(),
        _ => return None,
    };
    Some([
        u8::from_str_radix(&normalized[0..2], 16).ok()?,
        u8::from_str_radix(&normalized[2..4], 16).ok()?,
        u8::from_str_radix(&normalized[4..6], 16).ok()?,
    ])
}

pub fn fill_color(hex: Option<&str>) -> [u8; 3] {
    hex.and_then(parse_hex_color).unwrap_or([0, 0, 0])
}

pub const fn bordered_size(width: u32, height: u32, border_x: u32, border_y: u32) -> (u32, u32) {
    (
        width.saturating_add(border_x.saturating_mul(2)),
        height.saturating_add(border_y.saturating_mul(2)),
    )
}

fn exceeds_max_dimension(label: &str, width: u32, height: u32) -> bool {
    let exceeds = width > MAX_PAD_DIMENSION || height > MAX_PAD_DIMENSION;
    if exceeds {
        log::warn!(
            "{} {}x{} exceeds the maximum supported dimension; skipping.",
            label,
            width,
            height
        );
    }
    exceeds
}

pub fn calculate_pad_target(
    current_w: u32,
    current_h: u32,
    pad: &PadOptions,
) -> Option<(u32, u32)> {
    let ratio_w = f64::from(pad.ratio_width);
    let ratio_h = f64::from(pad.ratio_height);
    if !ratio_w.is_finite() || !ratio_h.is_finite() || ratio_w <= 0.0 || ratio_h <= 0.0 {
        return None;
    }

    let target_ratio = ratio_w / ratio_h;
    let original_ratio = f64::from(current_w) / f64::from(current_h);
    if (original_ratio - target_ratio).abs() < PAD_RATIO_EPSILON {
        return None;
    }

    let (new_w, new_h) = if original_ratio > target_ratio {
        (
            current_w,
            (f64::from(current_w) / target_ratio).round() as u32,
        )
    } else {
        (
            (f64::from(current_h) * target_ratio).round() as u32,
            current_h,
        )
    };

    let new_w = new_w.max(current_w);
    let new_h = new_h.max(current_h);

    if exceeds_max_dimension("Pad target", new_w, new_h)
        || (new_w == current_w && new_h == current_h)
    {
        return None;
    }

    Some((new_w, new_h))
}

pub fn calculate_border_thickness(
    src_w: u32,
    src_h: u32,
    border: &BorderOptions,
) -> Option<(u32, u32)> {
    let horizontal = f64::from(border.horizontal_percent);
    let vertical = f64::from(border.vertical_percent);
    if !horizontal.is_finite() || !vertical.is_finite() {
        return None;
    }

    if horizontal < 0.0
        || vertical < 0.0
        || horizontal > MAX_BORDER_PERCENT
        || vertical > MAX_BORDER_PERCENT
    {
        log::warn!(
            "Border percentages {}/{} are outside 0..={}; skipping border.",
            horizontal,
            vertical,
            MAX_BORDER_PERCENT
        );
        return None;
    }

    let (basis_x, basis_y) = match border.basis {
        BorderBasis::LongEdge => {
            let edge = f64::from(src_w.max(src_h));
            (edge, edge)
        }
        BorderBasis::ShortEdge => {
            let edge = f64::from(src_w.min(src_h));
            (edge, edge)
        }
        BorderBasis::EachEdge => (f64::from(src_w), f64::from(src_h)),
    };

    let border_x = (basis_x * horizontal / 100.0).round() as u32;
    let border_y = (basis_y * vertical / 100.0).round() as u32;

    if border_x == 0 && border_y == 0 {
        return None;
    }

    let (bordered_w, bordered_h) = bordered_size(src_w, src_h, border_x, border_y);
    if exceeds_max_dimension("Bordered size", bordered_w, bordered_h) {
        return None;
    }

    Some((border_x, border_y))
}

pub fn compute_export_geometry(
    src_w: u32,
    src_h: u32,
    settings: &ExportSettings,
) -> Option<ExportGeometry> {
    if src_w == 0 || src_h == 0 {
        return None;
    }

    let border = settings
        .border
        .as_ref()
        .and_then(|border| calculate_border_thickness(src_w, src_h, border));
    let border_src = border.unwrap_or((0, 0));
    let (bordered_w, bordered_h) = bordered_size(src_w, src_h, border_src.0, border_src.1);

    let pad_target = settings
        .pad
        .as_ref()
        .and_then(|pad| calculate_pad_target(bordered_w, bordered_h, pad));

    if border.is_none() && pad_target.is_none() {
        return None;
    }

    let canvas = pad_target.unwrap_or((bordered_w, bordered_h));

    Some(compute_fused_geometry(
        (src_w, src_h),
        border_src,
        canvas,
        settings.resize.as_ref(),
    ))
}

fn compute_fused_geometry(
    src: (u32, u32),
    border_src: (u32, u32),
    canvas: (u32, u32),
    resize_opts: Option<&ResizeOptions>,
) -> ExportGeometry {
    let (canvas_w, canvas_h) = resize_target(canvas.0, canvas.1, resize_opts);
    let (canvas_w, canvas_h) = (canvas_w.max(1), canvas_h.max(1));

    let src_wf = f64::from(src.0);
    let src_hf = f64::from(src.1);
    let canvas_wf = f64::from(canvas.0);
    let canvas_hf = f64::from(canvas.1);
    let scale_x = f64::from(canvas_w) / canvas_wf;
    let scale_y = f64::from(canvas_h) / canvas_hf;

    let photo_w = ((src_wf * scale_x).round() as u32).clamp(1, canvas_w);
    let photo_h = ((src_hf * scale_y).round() as u32).clamp(1, canvas_h);

    let border_x = ((f64::from(border_src.0) * scale_x).round() as u32)
        .min(canvas_w.saturating_sub(photo_w) / 2);
    let border_y = ((f64::from(border_src.1) * scale_y).round() as u32)
        .min(canvas_h.saturating_sub(photo_h) / 2);

    ExportGeometry {
        canvas_w,
        canvas_h,
        photo_w,
        photo_h,
        offset_x: (canvas_w - photo_w) / 2,
        offset_y: (canvas_h - photo_h) / 2,
        border_x,
        border_y,
    }
}

pub fn compute_export_output_size(src_w: u32, src_h: u32, settings: &ExportSettings) -> (u32, u32) {
    match compute_export_geometry(src_w, src_h, settings) {
        Some(geometry) => (geometry.canvas_w, geometry.canvas_h),
        None => resize_target(src_w, src_h, settings.resize.as_ref()),
    }
}

pub fn watermark_scaled_dims(
    base_w: u32,
    base_h: u32,
    watermark_w: u32,
    watermark_h: u32,
    scale_pct: f32,
) -> (u32, u32) {
    let wm_w_f = watermark_w as f32;
    let wm_h_f = watermark_h as f32;
    let base_min_dim = base_w.min(base_h) as f32;
    let factor = (base_min_dim * (scale_pct / 100.0)) / wm_w_f.max(1.0);
    let new_w = (wm_w_f * factor).round() as u32;
    let new_h = (wm_h_f * factor).round() as u32;
    (new_w, new_h)
}

fn watermark_offset(
    base_w: u32,
    base_h: u32,
    watermark_w: u32,
    watermark_h: u32,
    anchor: &WatermarkAnchor,
    spacing_pixels: i64,
) -> (i64, i64) {
    let base_wide = i64::from(base_w);
    let base_high = i64::from(base_h);
    let wm_wide = i64::from(watermark_w);
    let wm_high = i64::from(watermark_h);

    let x = match anchor {
        WatermarkAnchor::TopLeft | WatermarkAnchor::CenterLeft | WatermarkAnchor::BottomLeft => {
            spacing_pixels
        }
        WatermarkAnchor::TopCenter | WatermarkAnchor::Center | WatermarkAnchor::BottomCenter => {
            (base_wide - wm_wide) / 2
        }
        WatermarkAnchor::TopRight | WatermarkAnchor::CenterRight | WatermarkAnchor::BottomRight => {
            base_wide - wm_wide - spacing_pixels
        }
    };

    let y = match anchor {
        WatermarkAnchor::TopLeft | WatermarkAnchor::TopCenter | WatermarkAnchor::TopRight => {
            spacing_pixels
        }
        WatermarkAnchor::CenterLeft | WatermarkAnchor::Center | WatermarkAnchor::CenterRight => {
            (base_high - wm_high) / 2
        }
        WatermarkAnchor::BottomLeft
        | WatermarkAnchor::BottomCenter
        | WatermarkAnchor::BottomRight => base_high - wm_high - spacing_pixels,
    };

    (x, y)
}

/// Alpha-attenuates and overlays `watermark` (already scaled) onto `base`.
pub fn composite_watermark(
    base: &mut DynamicImage,
    watermark: &DynamicImage,
    anchor: &WatermarkAnchor,
    spacing_pct: f32,
    opacity_pct: f32,
) {
    let (base_w, base_h) = base.dimensions();
    let (wm_w, wm_h) = watermark.dimensions();
    let opacity_factor = (opacity_pct / 100.0).clamp(0.0, 1.0);
    let spacing_pixels = (f64::from(base_w.min(base_h)) * f64::from(spacing_pct) / 100.0) as i64;
    let (x, y) = watermark_offset(base_w, base_h, wm_w, wm_h, anchor, spacing_pixels);

    if matches!(
        base,
        DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_)
    ) {
        let mut base_rgba = base.to_rgba16();
        let mut watermark_rgba = watermark.to_rgba16();
        watermark_rgba.pixels_mut().for_each(|pixel| {
            let alpha = f32::from(pixel[3]) * opacity_factor;
            pixel[3] = alpha.round() as u16;
        });
        image::imageops::overlay(&mut base_rgba, &watermark_rgba, x, y);
        *base = DynamicImage::ImageRgba16(base_rgba);
    } else if matches!(
        base,
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_)
    ) {
        let mut base_rgba = base.to_rgba32f();
        let mut watermark_rgba = watermark.to_rgba32f();
        watermark_rgba
            .pixels_mut()
            .for_each(|pixel| pixel[3] *= opacity_factor);
        image::imageops::overlay(&mut base_rgba, &watermark_rgba, x, y);
        *base = DynamicImage::ImageRgba32F(base_rgba);
    } else {
        let mut watermark_rgba = watermark.to_rgba8();
        watermark_rgba.pixels_mut().for_each(|pixel| {
            let alpha = f32::from(pixel[3]) * opacity_factor;
            pixel[3] = alpha.round() as u8;
        });
        if let DynamicImage::ImageRgba8(base_rgba) = base {
            image::imageops::overlay(base_rgba, &watermark_rgba, x, y);
        } else {
            let mut base_rgba = base.to_rgba8();
            image::imageops::overlay(&mut base_rgba, &watermark_rgba, x, y);
            *base = DynamicImage::ImageRgba8(base_rgba);
        }
    }
}

trait FillChannel: image::Primitive {
    fn from_u8(value: u8) -> Self;
}

impl FillChannel for u8 {
    fn from_u8(value: u8) -> Self {
        value
    }
}

impl FillChannel for u16 {
    fn from_u8(value: u8) -> Self {
        u16::from(value).saturating_mul(257)
    }
}

impl FillChannel for f32 {
    fn from_u8(value: u8) -> Self {
        f32::from(value) / 255.0
    }
}

trait SolidFill: image::Pixel + 'static {
    fn from_solid(color: [u8; 3]) -> Self;
}

impl<T: FillChannel> SolidFill for image::Rgb<T>
where
    image::Rgb<T>: image::Pixel<Subpixel = T> + 'static,
{
    fn from_solid([r, g, b]: [u8; 3]) -> Self {
        image::Rgb([T::from_u8(r), T::from_u8(g), T::from_u8(b)])
    }
}

impl<T: FillChannel> SolidFill for image::Rgba<T>
where
    image::Rgba<T>: image::Pixel<Subpixel = T> + 'static,
{
    fn from_solid([r, g, b]: [u8; 3]) -> Self {
        image::Rgba([
            T::from_u8(r),
            T::from_u8(g),
            T::from_u8(b),
            T::DEFAULT_MAX_VALUE,
        ])
    }
}

pub fn fill_border_frame<P>(
    canvas: &mut ImageBuffer<P, Vec<P::Subpixel>>,
    geometry: &ExportGeometry,
    fill: P,
) where
    P: image::Pixel + 'static,
    P::Subpixel: 'static,
{
    let box_x = geometry.offset_x.saturating_sub(geometry.border_x);
    let box_y = geometry.offset_y.saturating_sub(geometry.border_y);
    let box_w = geometry
        .photo_w
        .saturating_add(geometry.border_x.saturating_mul(2));
    let below_photo = geometry.offset_y.saturating_add(geometry.photo_h);
    let right_of_photo = geometry.offset_x.saturating_add(geometry.photo_w);

    let strips = [
        (box_x, box_y, box_w, geometry.border_y),
        (box_x, below_photo, box_w, geometry.border_y),
        (
            box_x,
            geometry.offset_y,
            geometry.border_x,
            geometry.photo_h,
        ),
        (
            right_of_photo,
            geometry.offset_y,
            geometry.border_x,
            geometry.photo_h,
        ),
    ];

    strips.iter().for_each(|(x, y, width, height)| {
        let x_end = x.saturating_add(*width).min(canvas.width());
        let y_end = y.saturating_add(*height).min(canvas.height());
        (*y..y_end).for_each(|row| {
            (*x..x_end).for_each(|col| canvas.put_pixel(col, row, fill));
        });
    });
}

pub fn place_photo_on_canvas<P>(
    src: &ImageBuffer<P, Vec<P::Subpixel>>,
    geometry: &ExportGeometry,
    outer_fill: P,
    border_fill: Option<P>,
) -> ImageBuffer<P, Vec<P::Subpixel>>
where
    P: image::Pixel + 'static,
    P::Subpixel: 'static,
{
    let mut canvas = ImageBuffer::from_pixel(geometry.canvas_w, geometry.canvas_h, outer_fill);

    if let Some(fill) = border_fill
        && geometry.has_border()
    {
        fill_border_frame(&mut canvas, geometry, fill);
    }

    imageops::replace(
        &mut canvas,
        src,
        i64::from(geometry.offset_x),
        i64::from(geometry.offset_y),
    );
    canvas
}

pub fn compose_image_on_canvas(
    image: &DynamicImage,
    geometry: &ExportGeometry,
    pad_color: [u8; 3],
    border_color: [u8; 3],
) -> DynamicImage {
    fn compose<P: SolidFill>(
        src: &ImageBuffer<P, Vec<P::Subpixel>>,
        geometry: &ExportGeometry,
        pad_color: [u8; 3],
        border_color: [u8; 3],
    ) -> ImageBuffer<P, Vec<P::Subpixel>>
    where
        P::Subpixel: 'static,
    {
        place_photo_on_canvas(
            src,
            geometry,
            P::from_solid(pad_color),
            Some(P::from_solid(border_color)),
        )
    }

    match image {
        DynamicImage::ImageRgb8(buffer) => {
            DynamicImage::ImageRgb8(compose(buffer, geometry, pad_color, border_color))
        }
        DynamicImage::ImageRgba8(buffer) => {
            DynamicImage::ImageRgba8(compose(buffer, geometry, pad_color, border_color))
        }
        DynamicImage::ImageRgb16(buffer) => {
            DynamicImage::ImageRgb16(compose(buffer, geometry, pad_color, border_color))
        }
        DynamicImage::ImageRgba16(buffer) => {
            DynamicImage::ImageRgba16(compose(buffer, geometry, pad_color, border_color))
        }
        DynamicImage::ImageRgb32F(buffer) => {
            DynamicImage::ImageRgb32F(compose(buffer, geometry, pad_color, border_color))
        }
        DynamicImage::ImageRgba32F(buffer) => {
            DynamicImage::ImageRgba32F(compose(buffer, geometry, pad_color, border_color))
        }
        other => DynamicImage::ImageRgba8(compose(
            &other.to_rgba8(),
            geometry,
            pad_color,
            border_color,
        )),
    }
}

pub fn apply_export_geometry(
    mut image: DynamicImage,
    export_settings: &ExportSettings,
) -> anyhow::Result<(DynamicImage, ExportGeometry)> {
    let (src_w, src_h) = image.dimensions();

    match compute_export_geometry(src_w, src_h, export_settings) {
        None => {
            let (target_w, target_h) = resize_target(src_w, src_h, export_settings.resize.as_ref());
            if target_w != src_w || target_h != src_h {
                image = image.resize(target_w, target_h, imageops::FilterType::Lanczos3);
            }

            let (width, height) = image.dimensions();
            Ok((image, ExportGeometry::identity(width, height)))
        }
        Some(geometry) => {
            if geometry.photo_w != src_w || geometry.photo_h != src_h {
                image = image.resize_exact(
                    geometry.photo_w,
                    geometry.photo_h,
                    imageops::FilterType::Lanczos3,
                );
            }

            let pad_color = fill_color(export_settings.pad.as_ref().map(|pad| pad.color.as_str()));
            let border_color = fill_color(
                export_settings
                    .border
                    .as_ref()
                    .map(|border| border.color.as_str()),
            );

            Ok((
                compose_image_on_canvas(&image, &geometry, pad_color, border_color),
                geometry,
            ))
        }
    }
}
