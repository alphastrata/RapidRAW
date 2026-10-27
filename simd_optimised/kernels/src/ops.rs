use core::simd::Simd;
use core::simd::prelude::*;

use crate::{LANES, splat};

pub fn scale(buf: &mut [f32], gain: f32) {
    let g = splat(gain);
    let (chunks, tail) = buf.as_chunks_mut::<LANES>();
    chunks
        .iter_mut()
        .for_each(|chunk| *chunk = (Simd::from_array(*chunk) * g).to_array());
    tail.iter_mut().for_each(|v| *v *= gain);
}

pub fn sub_into(dst: &mut [f32], a: &[f32], b: &[f32]) {
    let n = dst.len().min(a.len()).min(b.len());
    let (d_chunks, d_tail) = dst[..n].as_chunks_mut::<LANES>();
    let (a_chunks, _) = a[..n].as_chunks::<LANES>();
    let (b_chunks, _) = b[..n].as_chunks::<LANES>();
    d_chunks
        .iter_mut()
        .zip(a_chunks)
        .zip(b_chunks)
        .for_each(|((d, x), y)| *d = (Simd::from_array(*x) - Simd::from_array(*y)).to_array());
    let start = n - d_tail.len();
    d_tail
        .iter_mut()
        .enumerate()
        .for_each(|(i, d)| *d = a[start + i] - b[start + i]);
}

#[must_use]
pub fn sum(buf: &[f32]) -> f64 {
    let (chunks, tail) = buf.as_chunks::<LANES>();
    let mut acc = splat(0.0);
    chunks.iter().for_each(|c| acc += Simd::from_array(*c));
    f64::from(acc.reduce_sum()) + tail.iter().map(|&v| f64::from(v)).sum::<f64>()
}
