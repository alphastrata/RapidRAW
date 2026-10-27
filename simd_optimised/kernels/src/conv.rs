use core::simd::prelude::*;
use core::simd::{f32x8, i64x8, Simd};

use alloc::vec;
use alloc::vec::Vec;

type I64x8 = i64x8;

struct RowScan<'k> {
    vertical: bool,
    y: usize,
    kernel: &'k [f32],
    w: usize,
    h: usize,
}

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
    Simd::<f32, 8>::gather_select(data, Mask::<isize, 8>::splat(true), gather_idx, f32x8::splat(0.0))
        * f32x8::splat(k)
}

fn row_out(scan: &RowScan<'_>, src: &[f32], dst_row: &mut [f32]) {
    let w = scan.w;
    let (max_coord, coord_stride, fixed) = if scan.vertical {
        (scan.h as i64 - 1, w, splat_i64(scan.y as i64))
    } else {
        (w as i64 - 1, 1usize, splat_i64((scan.y * w) as i64))
    };
    let r = (scan.kernel.len() / 2) as i64;

    for chunk_start in (0..w).step_by(8) {
        let lane_count = (w - chunk_start).min(8);
        let xs = I64x8::from_array(core::array::from_fn(|i| (chunk_start + i) as i64));

        let mut acc = f32x8::splat(0.0);
        for (t, &k) in scan.kernel.iter().enumerate() {
            let coord = if scan.vertical { fixed } else { xs } + splat_i64(t as i64 - r);
            let clamped = simd_clamp_i64(coord, max_coord) * splat_i64(coord_stride as i64);
            let idx = if scan.vertical { clamped + xs } else { clamped + fixed };
            acc += tap(src, idx, k);
        }

        let values: [f32; 8] = acc.to_array();
        dst_row[chunk_start..chunk_start + lane_count].copy_from_slice(&values[..lane_count]);
    }
}

pub fn conv_row_horizontal(src: &[f32], w: usize, kernel: &[f32], y: usize, dst_row: &mut [f32]) {
    row_out(&RowScan { kernel, w, h: 0, vertical: false, y }, src, dst_row);
}

pub fn conv_row_vertical(src: &[f32], w: usize, h: usize, kernel: &[f32], y: usize, dst_row: &mut [f32]) {
    row_out(&RowScan { kernel, w, h, vertical: true, y }, src, dst_row);
}

pub fn convolve_separable(src: &[f32], w: usize, h: usize, kernel: &[f32]) -> Vec<f32> {
    let mut tmp = vec![0.0; w * h];
    let mut out = vec![0.0; w * h];
    for (y, row) in tmp.chunks_mut(w).enumerate() {
        row_out(&RowScan { kernel, w, h, vertical: false, y }, src, row);
    }
    for (y, row) in out.chunks_mut(w).enumerate() {
        row_out(&RowScan { kernel, w, h, vertical: true, y }, &tmp, row);
    }
    out
}