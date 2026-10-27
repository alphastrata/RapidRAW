use simd_kernels::{colour, conv, ops};

struct Lcg(u32);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 9) as f32 / (1u32 << 23) as f32
    }
    fn next_plane(&mut self, n: usize) -> Vec<f32> {
        (0..n).map(|_| self.next_f32()).collect()
    }
}

fn srgb_scalar(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_inv_scalar(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn convolve_scalar_reference(src: &[f32], w: usize, h: usize, kernel: &[f32]) -> Vec<f32> {
    let (w_i, h_i) = (w as i64, h as i64);
    let mut tmp = vec![0.0f32; w * h];
    let mut out = vec![0.0f32; w * h];
    let r_kern = (kernel.len().max(1) / 2) as i64;
    for y in 0..h {
        for x in 0..w {
            let x_i = x as i64;
            let mut acc = 0.0f32;
            for (&k, i) in kernel.iter().zip(0i64..) {
                let sx = (x_i + i - r_kern).clamp(0, w_i - 1) as usize;
                acc += k * src[y * w + sx];
            }
            tmp[y * w + x] = acc;
        }
    }
    for y in 0..h {
        let y_i = y as i64;
        for x in 0..w {
            let mut acc = 0.0f32;
            for (&k, i) in kernel.iter().zip(0i64..) {
                let sy = (y_i + i - r_kern).clamp(0, h_i - 1) as usize;
                acc += k * tmp[sy * w + x];
            }
            out[y * w + x] = acc;
        }
    }
    out
}

#[test]
fn srgb_to_linear_accuracy() {
    let mut rng = Lcg(0xC0FFEE);
    let samples: Vec<f32> = (0..4096)
        .map(|_| rng.next_f32() * 2.0 - 0.1)
        .chain([0.0, 0.04045, 0.04046, 1.0, 1.4])
        .collect();
    let mut got = samples.clone();
    colour::srgb_to_linear(&mut got);
    for (&input, &output) in samples.iter().zip(&got) {
        let expected = srgb_scalar(input.max(0.0));
        let error = (output - expected).abs() / expected.abs().max(1e-6);
        assert!(
            error < 5e-5,
            "srgb_to_linear({input}) = {output}, expected {expected}"
        );
    }
}

#[test]
fn linear_to_srgb_accuracy() {
    let mut rng = Lcg(0xBADF00D);
    let samples: Vec<f32> = (0..4096)
        .map(|_| rng.next_f32() * 2.0 - 0.1)
        .chain([0.0, 0.0031308, 0.003131, 1.0, 1.4])
        .collect();
    let mut got = samples.clone();
    colour::linear_to_srgb(&mut got);
    for (&input, &output) in samples.iter().zip(&got) {
        let expected = srgb_inv_scalar(input.max(0.0));
        let error = (output - expected).abs();
        assert!(
            error < 2e-5,
            "linear_to_srgb({input}) = {output}, expected {expected}"
        );
    }
}

#[test]
fn convolve_separable_parity() {
    let (w, h) = (97, 63);
    let mut rng = Lcg(0x5EED);
    let src = rng.next_plane(w * h);
    for taps in [3usize, 5, 9, 19, 25] {
        let kernel: Vec<f32> = (0..taps).map(|i| (i as f32 + 1.0).sqrt()).collect();
        let expected = convolve_scalar_reference(&src, w, h, &kernel);
        let got = conv::convolve_separable(&src, w, h, &kernel);
        for (a, b) in expected.iter().zip(&got) {
            assert_eq!(a.to_bits(), b.to_bits(), "taps {taps}");
        }
    }
}

#[test]
fn ops_parity() {
    let mut rng = Lcg(0xABCDEF);
    let a = rng.next_plane(1003);
    let b = rng.next_plane(1003);

    let mut scaled = a.clone();
    ops::scale(&mut scaled, 2.19);
    for (o, &x) in scaled.iter().zip(&a) {
        assert_eq!(*o, x * 2.19);
    }

    let mut diff = a.clone();
    ops::sub_into(&mut diff, &a, &b);
    for ((d, x), y) in diff.iter().zip(&a).zip(&b) {
        assert_eq!(*d, x - y);
    }

    let expected: f64 = a.iter().map(|&v| f64::from(v)).sum();
    assert!((ops::sum(&a) - expected).abs() / expected < 1e-5);
}

#[test]
fn odd_length_tails() {
    let mut got: Vec<f32> = (0..13).map(|i| i as f32 + 1.0).collect();
    let expected: Vec<f32> = got.iter().map(|&x| srgb_scalar(x.max(0.0))).collect();
    colour::srgb_to_linear(&mut got);
    for (o, e) in got.iter().zip(expected) {
        assert!((o - e).abs() / e < 5e-5);
    }
}
