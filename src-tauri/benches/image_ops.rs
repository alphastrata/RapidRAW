use std::hint::black_box;
use std::sync::LazyLock;

use divan::Bencher;
use image::{DynamicImage, GrayImage, Rgb, RgbImage};
use rapidraw_lib::focus_stacking::{LensWarp, PlanarRgb, Plane};
use rapidraw_lib::{ai_processing, focus_stacking, fog, image_processing};
use serde_json::json;

const W: u32 = 1600;
const H: u32 = 1200;

fn pattern(x: u32, y: u32, seed: u32) -> u8 {
    let u = x as f32 / W as f32;
    let v = y as f32 / H as f32;
    let wave = (2.0 * std::f32::consts::PI * (3.0 * u + 5.0 * v)).sin();
    let mut s = seed ^ x.wrapping_mul(19_349_663) ^ y.wrapping_mul(842_502_083);
    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    ((wave * 64.0 + 128.0) as u32 + (s >> 24) % 32) as u8
}

static SAMPLE: LazyLock<DynamicImage> =
    LazyLock::new(|| DynamicImage::ImageRgb8(RgbImage::from_fn(W, H, |x, y| {
        Rgb([pattern(x, y, 0x1F), pattern(x, y, 0x2F), pattern(x, y, 0x3F)])
    })));

static SAMPLE_LUMA: LazyLock<GrayImage> =
    LazyLock::new(|| GrayImage::from_fn(W, H, |x, y| image::Luma([pattern(x, y, 0x7F)])));

static LOW_RES_LUMA: LazyLock<GrayImage> =
    LazyLock::new(|| GrayImage::from_fn(W / 4, H / 4, |x, y| image::Luma([pattern(x, y, 0x4F)])));

static SAMPLE_PLANE: LazyLock<Plane> = LazyLock::new(|| {
    Plane {
        w: W as usize,
        h: H as usize,
        data: (0..(W as usize * H as usize))
            .map(|i| (i % 255) as f32 / 255.0)
            .collect(),
    }
});

static SAMPLE_PLANAR: LazyLock<PlanarRgb> = LazyLock::new(|| PlanarRgb::from_rgb32f(&SAMPLE.to_rgb32f()));

fn fog_adjustments() -> serde_json::Value {
    let depth = image::RgbaImage::from_fn(160, 120, |x, _| {
        let v = ((x as f32 / 160.0) * 255.0) as u8;
        image::Rgba([v, v, v, 255])
    });
    let mut png = Vec::new();
    DynamicImage::ImageRgba8(depth)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode depth map");
    use base64::Engine as _;
    let data_url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png));
    json!({
        "fogEnabled": true,
        "fogDepthMap": data_url,
        "fogAmount": 60.0,
        "fogStart": 10.0,
        "fogDensity": 50.0,
        "fogHeight": 0.0,
        "fogVariation": 25.0,
        "fogGlow": 25.0,
        "fogTemperature": 20.0,
        "fogTint": 0.0,
        "orientationSteps": 0,
        "flipHorizontal": false,
        "flipVertical": false,
    })
}

fn geometry() -> image_processing::GeometryParams {
    image_processing::GeometryParams {
        distortion: 0.2,
        vertical: 10.0,
        horizontal: -5.0,
        rotate: 2.5,
        aspect: 1.1,
        scale: 0.95,
        lens_distortion_amount: 30.0,
        lens_distortion_enabled: true,
        lens_model: 1,
        ..Default::default()
    }
}

#[divan::bench]
fn srgb_to_linear(b: Bencher) {
    b.bench(|| image_processing::apply_srgb_to_linear(black_box((*SAMPLE).clone())));
}

#[divan::bench]
fn linear_to_srgb(b: Bencher) {
    b.bench(|| image_processing::apply_linear_to_srgb(black_box(DynamicImage::ImageRgb32F(SAMPLE.to_rgb32f()))));
}

#[divan::bench]
fn apply_exposure_plus_2ev(b: Bencher) {
    b.bench(|| image_processing::apply_exposure(black_box((*SAMPLE).clone()), 2.0));
}

#[divan::bench]
fn apply_flip(b: Bencher) {
    b.bench(|| image_processing::apply_flip(black_box((*SAMPLE).clone()), true, false));
}

#[divan::bench]
fn apply_coarse_rotation(b: Bencher) {
    b.bench(|| image_processing::apply_coarse_rotation(black_box((*SAMPLE).clone()), 3));
}

#[divan::bench]
fn apply_crop(b: Bencher) {
    let crop = json!({ "x": 100.0, "y": 80.0, "width": 900.0, "height": 700.0 });
    b.bench(|| image_processing::apply_crop(black_box((*SAMPLE).clone()), black_box(&crop)));
}

#[divan::bench]
fn warp_image_geometry(b: Bencher) {
    b.bench(|| image_processing::warp_image_geometry(black_box(&*SAMPLE), black_box(geometry())));
}

#[divan::bench]
fn downscale_f32(b: Bencher) {
    b.bench(|| image_processing::downscale_f32_image(black_box(&*SAMPLE), W / 2, H / 2));
}

#[divan::bench]
fn cpu_agx_tonemap(b: Bencher) {
    b.bench(|| {
        let mut img = DynamicImage::ImageRgb32F(SAMPLE.to_rgb32f());
        image_processing::apply_cpu_agx_tonemap(black_box(&mut img));
        img
    });
}

#[divan::bench]
fn cpu_default_raw_processing(b: Bencher) {
    b.bench(|| {
        let mut img = black_box((*SAMPLE).clone());
        image_processing::apply_cpu_default_raw_processing(black_box(&mut img));
        img
    });
}

#[divan::bench]
fn remove_raw_artifacts_enhance(b: Bencher) {
    b.bench(|| {
        let mut img = black_box((*SAMPLE).clone());
        image_processing::remove_raw_artifacts_and_enhance(black_box(&mut img), 1.5, 0.5);
        img
    });
}

#[divan::bench]
fn histogram(b: Bencher) {
    b.bench(|| image_processing::calculate_histogram_from_image(black_box(&*SAMPLE)).unwrap());
}

#[divan::bench]
fn waveform(b: Bencher) {
    b.bench(|| image_processing::calculate_waveform_from_image(black_box(&*SAMPLE), black_box(None)).unwrap());
}

#[divan::bench]
fn auto_analysis(b: Bencher) {
    b.bench(|| image_processing::perform_auto_analysis(black_box(&*SAMPLE), black_box(-0.7)));
}

#[divan::bench]
fn apply_fog(b: Bencher) {
    let adjustments = fog_adjustments();
    b.bench(|| {
        let adj = adjustments.clone();
        fog::apply_fog(black_box(std::borrow::Cow::Owned((*SAMPLE).clone())), black_box(&adj))
    });
}

#[divan::bench]
fn fast_guided_filter(b: Bencher) {
    b.bench(|| {
        ai_processing::fast_guided_filter(black_box(&*SAMPLE_LUMA), black_box(&*LOW_RES_LUMA), 32, 0.01)
    });
}

#[divan::bench]
fn gaussian_blur_sigma3(b: Bencher) {
    b.bench(|| focus_stacking::gaussian_blur(black_box(&*SAMPLE_PLANE), 3.0));
}

#[divan::bench]
fn box_filter_r4(b: Bencher) {
    b.bench(|| focus_stacking::box_filter(black_box(&*SAMPLE_PLANE), 4));
}

#[divan::bench]
fn guided_filter_focus(b: Bencher) {
    b.bench(|| focus_stacking::guided_filter(black_box(&*SAMPLE_PLANE), black_box(&*SAMPLE_PLANE), 32, 0.01));
}

#[divan::bench]
fn pyramid_downsample_upsample(b: Bencher) {
    b.bench(|| {
        let down = focus_stacking::downsample(black_box(&*SAMPLE_PLANE));
        focus_stacking::upsample_to(black_box(&down), SAMPLE_PLANE.w, SAMPLE_PLANE.h)
    });
}

#[divan::bench]
fn focus_scores(b: Bencher) {
    b.bench(|| {
        focus_stacking::focus_scores(black_box(&*SAMPLE_PLANE), 4, black_box(&focus_stacking::FusionConfig::default()))
    });
}

#[divan::bench]
fn laplacian_pyramid_collapse(b: Bencher) {
    b.bench(|| {
        let lp = focus_stacking::laplacian_pyramid(black_box(&*SAMPLE_PLANE), 4);
        focus_stacking::collapse_pyramid(black_box(&lp))
    });
}

#[divan::bench]
fn warp_frame_identity(b: Bencher) {
    b.bench(|| {
        let warp = LensWarp::identity(SAMPLE_PLANAR.w, SAMPLE_PLANAR.h);
        focus_stacking::warp_frame(black_box(&*SAMPLE_PLANAR), black_box(&warp), 1.0, 0.0, SAMPLE_PLANAR.w, SAMPLE_PLANAR.h)
    });
}

fn main() {
    divan::main();
}