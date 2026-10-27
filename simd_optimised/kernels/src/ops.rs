use core::simd::prelude::*;
use core::simd::Simd;

use crate::{splat, LANES};

pub fn scale(buf: &mut [f32], gain: f32) {
    let g = splat(gain);
    let (chunks, tail) = buf.as_chunks_mut::<LANES>();
    for chunk in chunks {
        *chunk = (Simd::from_array(*chunk) * g).to_array();
    }
    for v in tail.iter_mut() {
        *v *= gain;
    }
}

pub fn sub_into(dst: &mut [f32], a: &[f32], b: &[f32]) {
    let n = dst.len().min(a.len()).min(b.len());
    let (d_chunks, d_tail) = dst[..n].as_chunks_mut::<LANES>();
    let (a_chunks, _) = a[..n].as_chunks::<LANES>();
    let (b_chunks, _) = b[..n].as_chunks::<LANES>();
    for ((d, x), y) in d_chunks.iter_mut().zip(a_chunks).zip(b_chunks) {
        *d = (Simd::from_array(*x) - Simd::from_array(*y)).to_array();
    }
    let start = n - d_tail.len();
    for i in 0..d_tail.len() {
        d_tail[i] = a[start + i] - b[start + i];
    }
}

pub fn sum(buf: &[f32]) -> f64 {
    let (chunks, tail) = buf.as_chunks::<LANES>();
    let mut acc = splat(0.0);
    for c in chunks.iter() {
        acc += Simd::from_array(*c);
    }
    acc.reduce_sum() as f64 + tail.iter().map(|&v| v as f64).sum::<f64>()
}