use core::simd::prelude::*;
use core::simd::{f32x8, i64x8, Simd};

use alloc::vec;
use alloc::vec::Vec;

type I64x8 = i64x8;

#[inline]
fn splat_i64(v: i64) -> I64x8 {
    I64x8::splat(v)
}

#[inline]
fn simd_clamp_i64(c: I64x8, max: i64) -> I64x8 {
    c.simd_max(splat_i64(0)).simd_min(splat_i64(max))
}

#[inline]
fn tap(data: &[f32], idx: I64x8, k: f32) -> f32x8 {
    let lanes: [i64; 8] = idx.to_array();
    let gather_idx = Simd::<usize, 8>::from_array(lanes.map(|i| i as usize));
    Simd::<f32, 8>::gather_select(data, Mask::<isize, 8>::splat(true), gather_idx, f32x8::splat(0.0)) * f32x8::splat(k)
}

fn pass(dst: &mut [f32], src: &[f32], kernel: &[f32], w: usize, h: usize, horizontal: bool) {
    let r = (kernel.len() / 2) as i64;
    let (max_coord, coord_stride) = if horizontal {
        (w as i64 - 1, 1usize)
    } else {
        (h as i64 - 1, w)
    };

    for (y, out_row) in dst.chunks_mut(w).enumerate() {
        for chunk_start in (0..w).step_by(8) {
            let lane_count = (w - chunk_start).min(8);
            let xs = I64x8::from_array(core::array::from_fn(|i| (chunk_start + i) as i64));

            let mut acc = f32x8::splat(0.0);
            for (t, &k) in kernel.iter().enumerate() {
                let coord = if horizontal { xs } else { splat_i64(y as i64) }
                    + splat_i64(t as i64)
                    - splat_i64(r);
                let clamped = simd_clamp_i64(coord, max_coord) * splat_i64(coord_stride as i64);
                let idx = if horizontal {
                    clamped + splat_i64((y * w) as i64)
                } else {
                    clamped + xs
                };
                acc += tap(src, idx, k);
            }

            let values: [f32; 8] = acc.to_array();
            out_row[chunk_start..chunk_start + lane_count]
                .copy_from_slice(&values[..lane_count]);
        }
    }
}

pub fn convolve_separable(src: &[f32], w: usize, h: usize, kernel: &[f32]) -> Vec<f32> {
    let mut tmp = vec![0.0; w * h];
    let mut out = vec![0.0; w * h];
    pass(&mut tmp, src, kernel, w, h, true);
    pass(&mut out, &tmp, kernel, w, h, false);
    out
}