#![no_std]
#![feature(portable_simd)]

extern crate alloc;

use core::simd::f32x8;

pub mod colour;
pub mod conv;
pub mod ops;

pub fn map_lanes(buf: &mut [f32], f: impl Fn(f32x8) -> f32x8) {
    let (chunks, tail) = buf.as_chunks_mut::<LANES>();
    for chunk in chunks {
        *chunk = f(core::simd::Simd::from_array(*chunk)).to_array();
    }
    if !tail.is_empty() {
        let mut lanes = f32x8::splat(0.0);
        for (i, v) in tail.iter().enumerate() {
            lanes[i] = *v;
        }
        let out = f(lanes);
        for (i, v) in tail.iter_mut().enumerate() {
            *v = out[i];
        }
    }
}

pub const LANES: usize = 8;

#[inline]
pub fn splat(v: f32) -> f32x8 {
    f32x8::splat(v)
}
