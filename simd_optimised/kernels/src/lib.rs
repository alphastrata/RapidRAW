#![no_std]
#![feature(portable_simd)]

extern crate alloc;

use core::simd::f32x8;

pub mod colour;
pub mod conv;
pub mod ops;

pub const LANES: usize = 8;

#[inline]
pub fn splat(v: f32) -> f32x8 {
    f32x8::splat(v)
}
