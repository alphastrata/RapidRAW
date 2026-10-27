use std::hint::black_box;
use std::sync::LazyLock;

use divan::Bencher;
use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage, codecs::jpeg::JpegEncoder};
use rapidraw_lib::cache_utils;
use rapidraw_lib::effect_maps;
use rapidraw_lib::guided_perspective;
use rapidraw_lib::guided_perspective::{GuideLine, GuideOrientation, GuidePoint};
use rapidraw_lib::image_loader;
use rapidraw_lib::lut_processing;
use rapidraw_lib::mask_generation::{self, MaskDefinition, SubMask};
use rapidraw_lib::export_processing::{self, encode_image_to_bytes};
use serde_json::json;

const W: u32 = 1600;
const H: u32 = 1200;

static SAMPLE: LazyLock<DynamicImage> = LazyLock::new(|| {
    DynamicImage::ImageRgb8(RgbImage::from_fn(W, H, |x, y| {
        Rgb([pattern(x, y, 1), pattern(x, y, 2), pattern(x, y, 3)])
    }))
});

fn pattern(x: u32, y: u32, s: u32) -> u8 {
    ((x.wrapping_mul(1_664_525u32.wrapping_add(s)) ^ y.wrapping_mul(65539u32.wrapping_add(s))) % 255) as u8
}

fn sub_mask(mask_type: &str, parameters: serde_json::Value) -> SubMask {
    serde_json::from_value(json!({
        "id": "bench",
        "type": mask_type,
        "visible": true,
        "invert": false,
        "mode": "additive",
        "parameters": parameters,
    }))
    .expect("sub mask")
}

fn mask_def(sub: SubMask) -> MaskDefinition {
    serde_json::from_value(json!({
        "id": "bench",
        "name": "bench",
        "visible": true,
        "invert": false,
        "adjustments": {},
        "subMasks": [sub],
    }))
    .expect("mask definition")
}

fn generate_mask(mask_type: &str, parameters: serde_json::Value) {
    let def = mask_def(sub_mask(mask_type, parameters));
    let _ = mask_generation::generate_mask_bitmap(&def, W, H, 1.0, (0.0, 0.0), Some(&SAMPLE));
}

#[divan::bench]
fn mask_radial(b: Bencher) {
    b.bench(|| {
        generate_mask(
            "radial",
            json!({"centerX": 0.5, "centerY": 0.5, "radiusX": 0.3, "radiusY": 0.3, "rotation": 0.0, "feather": 0.5}),
        )
    });
}

#[divan::bench]
fn mask_linear(b: Bencher) {
    b.bench(|| generate_mask("linear", json!({"startX": 0.1, "startY": 0.1, "endX": 0.9, "endY": 0.9})));
}

#[divan::bench]
fn mask_brush(b: Bencher) {
    let line = json!({
        "tool": "brush",
        "brushSize": 120.0,
        "feather": 0.5,
        "points": (0..200)
            .map(|i| json!({"x": (i as f64 / 199.0) * 0.9 + 0.05, "y": 0.5}))
            .collect::<Vec<_>>(),
    });
    b.bench(|| generate_mask("brush", json!({"lines": [line]})));
}

#[divan::bench]
fn mask_parametric_color(b: Bencher) {
    b.bench(|| generate_mask("color", json!({"targetX": 800.0, "targetY": 600.0, "tolerance": 30.0})));
}

#[divan::bench]
fn guided_homography(b: Bencher) {
    let lines = vec![
        GuideLine {
            id: "v".into(),
            orientation: GuideOrientation::Vertical,
            p1: GuidePoint { x: 0.2, y: 0.0 },
            p2: GuidePoint { x: 0.25, y: 1.0 },
        },
        GuideLine {
            id: "h".into(),
            orientation: GuideOrientation::Horizontal,
            p1: GuidePoint { x: 0.0, y: 0.3 },
            p2: GuidePoint { x: 1.0, y: 0.35 },
        },
    ];
    b.bench(|| {
        let result = guided_perspective::compute_guided_homography(black_box(&lines), W as f64, H as f64);
        if let Some(result) = result {
            guided_perspective::compute_max_inscribed_crop(black_box(&result.forward_h), W as f64, H as f64);
        }
    });
}

fn encoded_jpeg() -> Vec<u8> {
    let rgb = SAMPLE.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&rgb)
        .expect("encode jpeg");
    out
}

#[divan::bench]
fn decode_jpeg(b: Bencher) {
    let encoded = encoded_jpeg();
    let settings: rapidraw_lib::AppSettings = serde_json::from_value(json!({})).unwrap();
    b.bench(|| {
        image_loader::load_base_image_from_bytes(black_box(&encoded), "bench.jpg", false, black_box(&settings), None)
            .unwrap()
    });
}

#[divan::bench]
fn encode_jpeg_q85(b: Bencher) {
    b.bench(|| encode(&SAMPLE, "jpeg"));
}

#[divan::bench]
fn encode_webp_q85(b: Bencher) {
    b.bench(|| encode(&SAMPLE, "webp"));
}

#[divan::bench]
fn encode_jxl_q85(b: Bencher) {
    b.bench(|| encode(&SAMPLE, "jxl"));
}

#[divan::bench]
fn encode_png(b: Bencher) {
    b.bench(|| encode(&SAMPLE, "png"));
}

fn encode(image: &DynamicImage, format: &str) -> Vec<u8> {
    let settings: export_processing::ExportSettings =
        serde_json::from_value(json!({"jpegQuality": 85, "keepMetadata": false, "stripGps": true, "resize": null, "filenameTemplate": null, "watermark": null})).unwrap();
    encode_image_to_bytes(image, format, &settings).unwrap()
}

#[divan::bench]
fn lut_identity_33(b: Bencher) {
    b.bench(|| lut_processing::generate_identity_lut_image(33));
}

#[divan::bench]
fn lut_image_to_cube_33(b: Bencher) {
    b.bench(|| lut_processing::convert_image_to_cube_lut(black_box(&*SAMPLE), 33).unwrap());
}

struct TempLut {
    path: std::path::PathBuf,
}

impl TempLut {
    fn cube(size: u32) -> TempLut {
        let mut cube = format!("TITLE bench\nDOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\nLUT_3D_SIZE {size}\n");
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    cube += &format!(
                        "{:.5} {:.5} {:.5}\n",
                        r as f32 / (size - 1) as f32,
                        g as f32 / (size - 1) as f32,
                        b as f32 / (size - 1) as f32
                    );
                }
            }
        }
        let path = std::env::temp_dir().join(format!("bench_lut_{}.cube", std::process::id()));
        std::fs::write(&path, cube).expect("write lut");
        TempLut { path }
    }
}

impl Drop for TempLut {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[divan::bench]
fn lut_parse_cube_33(b: Bencher) {
    let lut = TempLut::cube(33);
    b.bench(|| lut_processing::parse_lut_file(lut.path.to_str().unwrap()).unwrap());
}

#[divan::bench]
fn visual_hash(b: Bencher) {
    let adjustments = json!({"exposure": 0.3, "contrast": 12.0});
    b.bench(|| cache_utils::calculate_visual_hash(black_box("IMG_0001.CR2"), black_box(&adjustments)));
}

#[divan::bench]
fn patch_and_effects_hash(b: Bencher) {
    let adjustments = json!({"exposure": 0.3, "contrast": 12.0, "crop": {"x": 10, "y": 10, "width": 100, "height": 100}});
    b.bench(|| {
        cache_utils::calculate_patch_hash(black_box(&adjustments))
            + cache_utils::calculate_effects_hash(black_box(&adjustments))
            + cache_utils::calculate_geometry_hash(black_box(&adjustments))
    });
}

#[divan::bench]
fn effect_map_resolve(b: Bencher) {
    let map: GrayImage = GrayImage::from_fn(320, 240, |x, _| Luma([(x * 255 / 320) as u8]));
    let mut png = Vec::new();
    map
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    use base64::Engine as _;
    let data_url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png));
    let adjustments = json!({"orientationSteps": 2u64});
    b.bench(|| effect_maps::resolve_luma_map(black_box(&data_url), black_box(&adjustments)));
}

fn main() {
    divan::main();
}