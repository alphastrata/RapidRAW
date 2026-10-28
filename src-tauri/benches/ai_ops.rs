use std::hint::black_box;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use divan::Bencher;
use image::{DynamicImage, GenericImageView, GrayImage, Luma, Rgb, RgbImage};
use ort::session::Session;
use rapidraw_lib::ai_processing;
use rapidraw_lib::ai_processing::ImageEmbeddings;
use tokenizers::Tokenizer;

const MODELS_DIR: &str = "RAPIDRAW_MODELS_DIR";
const DEFAULT_DIR: &str = ".local/share/io.github.CyberTimon.RapidRAW/models";

fn models_dir() -> PathBuf {
    std::env::var_os(MODELS_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(DEFAULT_DIR))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME not set; set RAPIDRAW_MODELS_DIR to a directory of RapidRAW models")
}

fn load(model_file: &str) -> Mutex<Session> {
    let path = models_dir().join(model_file);
    let _ = ort::init().commit();
    let session = Session::builder()
        .expect("session builder")
        .commit_from_file(path)
        .expect("load model");
    Mutex::new(session)
}

fn image(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let v = ((x * 7 + y * 13) % 231) as u8;
        Rgb([v, (v * 3) % 255, 255 - v])
    }))
}

fn mask(w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        let inside = x % 3 == 0 && y % 3 == 0;
        Luma([if inside { 255 } else { 0 }])
    })
}

static SIZE: u32 = 512;

static LAMA: LazyLock<Mutex<Session>> = LazyLock::new(|| load("lama_fp16.onnx"));
static DEPTH: LazyLock<Mutex<Session>> = LazyLock::new(|| load("depth_anything_v2_vits.onnx"));
static NORMAL: LazyLock<Mutex<Session>> = LazyLock::new(|| load("moge-2-vits-normal-onnx.onnx"));
static DENOISE: LazyLock<Mutex<Session>> = LazyLock::new(|| load("nind_denoise_utnet_684.onnx"));
static SAM_ENCODER: LazyLock<Mutex<Session>> =
    LazyLock::new(|| load("sam_vit_b_01ec64_encoder.onnx"));
static SAM_DECODER: LazyLock<Mutex<Session>> =
    LazyLock::new(|| load("sam_vit_b_01ec64_decoder.onnx"));
static CLIP: LazyLock<Mutex<Session>> = LazyLock::new(|| load("clip_model.onnx"));
static CLIP_TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    Tokenizer::from_file(models_dir().join("clip_tokenizer.json")).expect("load tokenizer")
});

#[divan::bench]
fn lama_inpaint_512(b: Bencher) {
    let img = image(SIZE, SIZE);
    let m = mask(SIZE, SIZE);
    b.bench(|| {
        ai_processing::run_lama_inpainting(black_box(&img), black_box(&m), black_box(&LAMA))
            .unwrap()
    });
}

#[divan::bench]
fn depth_anything_512(b: Bencher) {
    let img = image(SIZE, SIZE);
    b.bench(|| {
        ai_processing::run_depth_anything_model(black_box(&img), black_box(&DEPTH)).unwrap()
    });
}

#[divan::bench]
fn normal_moge_512(b: Bencher) {
    let img = image(SIZE, SIZE);
    b.bench(|| ai_processing::run_normal_model(black_box(&img), black_box(&NORMAL)).unwrap());
}

#[divan::bench]
fn denoise_nind_256(b: Bencher) {
    let img = image(256, 256).to_rgb32f();
    b.bench(|| {
        ai_processing::run_ai_denoise(black_box(&img), black_box(&DENOISE), black_box(&|_| {}))
            .unwrap()
    });
}

#[divan::bench]
fn sam_encoder_512(b: Bencher) {
    let img = image(SIZE, SIZE);
    b.bench(|| {
        ai_processing::generate_image_embeddings(black_box(&img), black_box(&SAM_ENCODER)).unwrap()
    });
}

#[divan::bench]
fn sam_decoder_point(b: Bencher) {
    let img = image(SIZE, SIZE);
    let embeddings = ai_processing::generate_image_embeddings(&img, &SAM_ENCODER).unwrap();
    let (rw, rh) = img.dimensions();
    let target = ImageEmbeddings {
        path_hash: "bench".into(),
        embeddings: embeddings.embeddings,
        original_size: (rw, rh),
    };
    b.bench(|| {
        ai_processing::run_sam_decoder(
            black_box(&SAM_DECODER),
            black_box(&target),
            black_box((10.0, 10.0)),
            black_box((400.0, 500.0)),
            black_box(Some(&img)),
        )
        .unwrap()
    });
}



fn main() {
    println!("models dir: {}", models_dir().display());
    divan::main();
}
