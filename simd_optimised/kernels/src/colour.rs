use core::simd::prelude::*;
use core::simd::{f32x8, Simd};

use crate::{splat, LANES};

type F32x8 = Simd<f32, LANES>;
type U32x8 = Simd<u32, LANES>;

const MAGIC: f32 = 12582912.0;
const MANT_OFFSET: u32 = 4_194_177;

fn horner10(r: F32x8) -> F32x8 {
    const C: [f32; 10] = [
        1.01694696e-7,
        1.321_544_8e-6,
        1.525_036_6e-5,
        1.540_353e-4,
        1.333_355_8e-3,
        9.618_129e-3,
        5.550_411e-2,
        2.402_265e-1,
        core::f32::consts::LN_2,
        1.0,
    ];
    let mut p = splat(C[0]);
    for &c in C[1..].iter() {
        p = p * r + splat(c);
    }
    p
}

fn series(t2: F32x8) -> F32x8 {
    const K: [f32; 6] = [1.0 / 11.0, 1.0 / 9.0, 1.0 / 7.0, 1.0 / 5.0, 1.0 / 3.0, 1.0];
    let mut a = splat(K[0]);
    for &c in K[1..].iter() {
        a = a * t2 + splat(c);
    }
    a
}

#[inline]
fn exp2(x: F32x8) -> F32x8 {
    let x = x.simd_clamp(splat(-126.0), splat(127.0));
    let r = x + splat(MAGIC);
    let n_f = r - splat(MAGIC);
    let mant = (r.to_bits() & U32x8::splat(0x007f_ffff)) - U32x8::splat(MANT_OFFSET);
    let scale = f32x8::from_bits(mant << U32x8::splat(23));
    scale * horner10(x - n_f)
}

#[inline]
fn log2(x: F32x8) -> F32x8 {
    let bits = x.to_bits();
    let e = (bits >> U32x8::splat(23u32)).cast::<f32>() - splat(127.0);
    let m = f32x8::from_bits((bits & U32x8::splat(0x007f_ffff)) | U32x8::splat(0x3f80_0000));
    let t = (m - splat(1.0)) / (m + splat(1.0));
    let t2 = t * t;
    e + splat(2.885_39) * t * series(t2)
}

#[inline]
fn pow2(x: F32x8, e: F32x8) -> F32x8 {
    exp2(e * log2(x))
}

pub fn srgb_to_linear(buf: &mut [f32]) {
    chunks_mut(buf, |l| {
        let c = l.simd_max(splat(0.0));
        let above = pow2((c + splat(0.055)) * splat(1.0 / 1.055), splat(2.4));
        c.simd_le(splat(0.04045))
            .select(c * splat(1.0 / 12.92), above)
    })
}

pub fn linear_to_srgb(buf: &mut [f32]) {
    chunks_mut(buf, |l| {
        let c = l.simd_max(splat(0.0));
        let above = splat(1.055) * pow2(c, splat(1.0 / 2.4)) - splat(0.055);
        c.simd_le(splat(0.0031308)).select(c * splat(12.92), above)
    })
}

fn chunks_mut(buf: &mut [f32], f: impl Fn(F32x8) -> F32x8) {
    let (chunks, tail) = buf.as_chunks_mut::<LANES>();
    for chunk in chunks {
        *chunk = f(Simd::from_array(*chunk)).to_array();
    }
    if tail.is_empty() {
        return;
    }
    let mut lanes = splat(0.0);
    for (i, v) in tail.iter().enumerate() {
        lanes[i] = *v;
    }
    let out = f(lanes);
    for (i, v) in tail.iter_mut().enumerate() {
        *v = out[i];
    }
}
