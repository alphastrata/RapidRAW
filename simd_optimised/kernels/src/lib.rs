#![no_std]
#![feature(portable_simd)]

extern crate alloc;

use core::simd::f32x8;

pub mod colour;
pub mod conv;
pub mod ops;

pub fn map_lanes(buf: &mut [f32], f: impl Fn(f32x8) -> f32x8) {
    let (chunks, tail) = buf.as_chunks_mut::<LANES>();
    chunks
        .iter_mut()
        .for_each(|chunk| *chunk = f(core::simd::Simd::from_array(*chunk)).to_array());
    if tail.is_empty() {
        return;
    }

    let mut lanes = f32x8::splat(0.0);
    lanes.as_mut_array()[..tail.len()].copy_from_slice(tail);
    f(lanes)
        .to_array()
        .iter()
        .zip(tail)
        .for_each(|(o, v)| *v = *o);
}

pub const LANES: usize = 8;

#[inline]
#[must_use]
pub fn splat(v: f32) -> f32x8 {
    f32x8::splat(v)
}
