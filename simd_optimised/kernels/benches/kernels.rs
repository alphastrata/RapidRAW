use divan::Bencher;

use simd_kernels::{colour, conv, ops};

fn main() {
    divan::main();
}

const W: usize = 2048;
const H: usize = 512;

fn rgb_buf() -> Vec<f32> {
    (0..3 * W * H).map(|i| (i % 255) as f32 / 255.0).collect()
}

fn grey_buf() -> Vec<f32> {
    (0..W * H).map(|i| (i % 255) as f32 / 255.0).collect()
}

#[divan::bench]
fn srgb_to_linear(bencher: Bencher) {
    let v = std::sync::Mutex::new(rgb_buf());
    bencher.bench(|| colour::srgb_to_linear(&mut v.lock().unwrap()));
}

#[divan::bench]
fn linear_to_srgb(bencher: Bencher) {
    let v = std::sync::Mutex::new(rgb_buf());
    bencher.bench(|| colour::linear_to_srgb(&mut v.lock().unwrap()));
}

#[divan::bench]
fn scale(bencher: Bencher) {
    let v = std::sync::Mutex::new(rgb_buf());
    bencher.bench(|| ops::scale(&mut v.lock().unwrap(), 1.5));
}

#[divan::bench]
fn sum(bencher: Bencher) {
    let v = rgb_buf();
    bencher.bench(|| ops::sum(&v));
}

#[divan::bench]
fn blur_sigma3(bencher: Bencher) {
    let v = grey_buf();
    let kernel: Vec<f32> = [1.0 / 16.0, 0.25, 6.0 / 16.0, 0.25, 1.0 / 16.0].to_vec();
    bencher.bench(|| conv::convolve_separable(&v, W, H, &kernel));
}