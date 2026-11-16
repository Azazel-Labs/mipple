//! BC6H (BPTC float) block codec: 16-byte blocks of RGB half floats, for HDR color.
//!
//! Both variants encode and decode: BC6H_UF16 (unsigned) and BC6H_SF16 (signed). Errors are
//! measured on the half-float bit patterns, which grow like the logarithm of the value (1024 steps
//! per octave), so a squared error there is a relative error: a dark texel counts as much as a
//! bright one with the same ratio of error to value. That is also the domain the format
//! interpolates in. Signed halves are measured as their magnitude's bits with their sign
//! ([`half_key`]), the scale signed blocks interpolate on, so -x and x are twice x's bits apart.
//!
//! The bit layouts of the 14 modes ([`LAYOUT`]) were generated from the D3D11 specification's mode
//! tables and are checked against Microsoft's reference decoder (texconv) in
//! `tests/bc6h_reference.rs`.

use crate::bc7::{anchors, subset_of};

mod batch;
pub mod rdo;

/// Half-float bits (R, G, B) of one texel.
pub type Half3 = [u16; 3];
/// The 16 texels of a 4x4 block in raster order.
pub type HdrBlock = [Half3; 16];

/// Largest finite half, 65504.
pub const HALF_MAX: u16 = 0x7bff;

/// `f` as the nearest half float (round to nearest, ties to even).
pub fn f32_to_half(f: f32) -> u16 {
    let x = f.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let a = x & 0x7fff_ffff;
    if a >= 0x7f80_0000 {
        // Infinity, or NaN (kept quiet).
        return sign | 0x7c00 | if a > 0x7f80_0000 { 0x200 } else { 0 };
    }
    if a >= 0x477f_f000 {
        // 65520 and up round to infinity.
        return sign | 0x7c00;
    }
    if a < 0x3880_0000 {
        // Below the smallest normal half, 2^-14: a multiple of 2^-24.
        return sign | (f32::from_bits(a) * 16_777_216.0).round_ties_even() as u16;
    }
    let mut h = (((a >> 23) - 112) << 10) | ((a >> 13) & 0x3ff);
    let rest = a & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && h & 1 == 1) {
        h += 1;
    }
    sign | h as u16
}

/// The value of half float `h`.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let e = u32::from((h >> 10) & 0x1f);
    let m = u32::from(h & 0x3ff);
    let bits = match e {
        0 => return f32::from_bits(sign | (m as f32 * (1.0 / 16_777_216.0)).to_bits()),
        31 => sign | 0x7f80_0000 | (m << 13),
        _ => sign | ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

/// The half an unsigned BC6H block can hold nearest `f`: negative values and NaN become 0, values
/// past [`HALF_MAX`] become it.
pub fn f32_to_uf16(f: f32) -> u16 {
    if f.is_nan() || f <= 0.0 {
        0
    } else {
        f32_to_half(f).min(HALF_MAX)
    }
}

/// The half a signed BC6H block can hold nearest `f`: NaN and zero become 0, magnitudes past
/// [`HALF_MAX`] become it.
pub fn f32_to_sf16(f: f32) -> u16 {
    if f.is_nan() || f == 0.0 {
        0
    } else {
        let h = f32_to_half(f);
        (h & 0x8000) | (h & 0x7fff).min(HALF_MAX)
    }
}

/// [`f32_to_sf16`] when `signed`, else [`f32_to_uf16`].
pub fn f32_to_bc6h(f: f32, signed: bool) -> u16 {
    if signed { f32_to_sf16(f) } else { f32_to_uf16(f) }
}

/// Half `h` on the scale errors are measured on, clamped to [`HALF_MAX`] steps: unsigned, its
/// bits (negative values as 0); signed, its magnitude's bits with its sign.
pub fn half_key(h: u16, signed: bool) -> i32 {
    let m = i32::from((h & 0x7fff).min(HALF_MAX));
    if h & 0x8000 == 0 {
        m
    } else if signed {
        -m
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug)]
struct Mode {
    /// The 2- or 5-bit value at the start of the block.
    value: u8,
    mode_bits: u8,
    /// Endpoints after the first are stored as deltas from it.
    transformed: bool,
    two_regions: bool,
    /// Bits of the first endpoint.
    eb: u8,
    /// Bits of every other endpoint, per channel (equal to `eb` when not transformed).
    db: [u8; 3],
}

impl Mode {
    const fn new(value: u8, transformed: bool, two_regions: bool, eb: u8, db: [u8; 3]) -> Self {
        let mode_bits = if value < 2 { 2 } else { 5 };
        Self {
            value,
            mode_bits,
            transformed,
            two_regions,
            eb,
            db,
        }
    }

    fn regions(self) -> usize {
        if self.two_regions { 2 } else { 1 }
    }
}

/// The 14 modes in D3D order (mode 1 at index 0).
const MODES: [Mode; 14] = [
    Mode::new(0, true, true, 10, [5, 5, 5]),
    Mode::new(1, true, true, 7, [6, 6, 6]),
    Mode::new(2, true, true, 11, [5, 4, 4]),
    Mode::new(6, true, true, 11, [4, 5, 4]),
    Mode::new(10, true, true, 11, [4, 4, 5]),
    Mode::new(14, true, true, 9, [5, 5, 5]),
    Mode::new(18, true, true, 8, [6, 5, 5]),
    Mode::new(22, true, true, 8, [5, 6, 5]),
    Mode::new(26, true, true, 8, [5, 5, 6]),
    Mode::new(30, false, true, 6, [6, 6, 6]),
    Mode::new(3, false, false, 10, [10, 10, 10]),
    Mode::new(7, true, false, 11, [9, 9, 9]),
    Mode::new(11, true, false, 12, [8, 8, 8]),
    Mode::new(15, true, false, 16, [4, 4, 4]),
];

/// Index into [`MODES`] of the mode whose bits open `raw`, or `None` for the four reserved values.
fn mode_index(raw: u128) -> Option<usize> {
    let v = if raw & 2 == 0 { raw & 3 } else { raw & 31 } as u8;
    MODES.iter().position(|m| m.value == v)
}

/// Where each header bit after the mode bits goes, per mode in [`MODES`] order: `field << 4 | bit`
/// with `field = 3 * endpoint + channel`, endpoints w, x, y, z (subset 0's pair, then subset 1's)
/// and channels R, G, B.
#[rustfmt::skip]
const LAYOUT: [&[u8]; 14] = [
    // mode 1 (mode value 0)
    &[
        116, 132, 180, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37,
        38, 39, 40, 41, 48, 49, 50, 51, 52, 164, 112, 113, 114, 115, 64, 65, 66, 67, 68, 176, 160, 161, 162, 163, 80,
        81, 82, 83, 84, 177, 128, 129, 130, 131, 96, 97, 98, 99, 100, 178, 144, 145, 146, 147, 148, 179,
    ],
    // mode 2 (mode value 1)
    &[
        117, 164, 165, 0, 1, 2, 3, 4, 5, 6, 176, 177, 132, 16, 17, 18, 19, 20, 21, 22, 133, 178, 116, 32, 33, 34, 35,
        36, 37, 38, 179, 181, 180, 48, 49, 50, 51, 52, 53, 112, 113, 114, 115, 64, 65, 66, 67, 68, 69, 160, 161, 162,
        163, 80, 81, 82, 83, 84, 85, 128, 129, 130, 131, 96, 97, 98, 99, 100, 101, 144, 145, 146, 147, 148, 149,
    ],
    // mode 3 (mode value 2)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 52, 10, 112, 113, 114, 115, 64, 65, 66, 67, 26, 176, 160, 161, 162, 163, 80, 81, 82, 83, 42,
        177, 128, 129, 130, 131, 96, 97, 98, 99, 100, 178, 144, 145, 146, 147, 148, 179,
    ],
    // mode 4 (mode value 6)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 10, 164, 112, 113, 114, 115, 64, 65, 66, 67, 68, 26, 160, 161, 162, 163, 80, 81, 82, 83, 42,
        177, 128, 129, 130, 131, 96, 97, 98, 99, 176, 178, 144, 145, 146, 147, 116, 179,
    ],
    // mode 5 (mode value 10)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 10, 132, 112, 113, 114, 115, 64, 65, 66, 67, 26, 176, 160, 161, 162, 163, 80, 81, 82, 83, 84,
        42, 128, 129, 130, 131, 96, 97, 98, 99, 177, 178, 144, 145, 146, 147, 180, 179,
    ],
    // mode 6 (mode value 14)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 132, 16, 17, 18, 19, 20, 21, 22, 23, 24, 116, 32, 33, 34, 35, 36, 37, 38, 39, 40,
        180, 48, 49, 50, 51, 52, 164, 112, 113, 114, 115, 64, 65, 66, 67, 68, 176, 160, 161, 162, 163, 80, 81, 82,
        83, 84, 177, 128, 129, 130, 131, 96, 97, 98, 99, 100, 178, 144, 145, 146, 147, 148, 179,
    ],
    // mode 7 (mode value 18)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 164, 132, 16, 17, 18, 19, 20, 21, 22, 23, 178, 116, 32, 33, 34, 35, 36, 37, 38, 39,
        179, 180, 48, 49, 50, 51, 52, 53, 112, 113, 114, 115, 64, 65, 66, 67, 68, 176, 160, 161, 162, 163, 80, 81,
        82, 83, 84, 177, 128, 129, 130, 131, 96, 97, 98, 99, 100, 101, 144, 145, 146, 147, 148, 149,
    ],
    // mode 8 (mode value 22)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 176, 132, 16, 17, 18, 19, 20, 21, 22, 23, 117, 116, 32, 33, 34, 35, 36, 37, 38, 39,
        165, 180, 48, 49, 50, 51, 52, 164, 112, 113, 114, 115, 64, 65, 66, 67, 68, 69, 160, 161, 162, 163, 80, 81,
        82, 83, 84, 177, 128, 129, 130, 131, 96, 97, 98, 99, 100, 178, 144, 145, 146, 147, 148, 179,
    ],
    // mode 9 (mode value 26)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 177, 132, 16, 17, 18, 19, 20, 21, 22, 23, 133, 116, 32, 33, 34, 35, 36, 37, 38, 39,
        181, 180, 48, 49, 50, 51, 52, 164, 112, 113, 114, 115, 64, 65, 66, 67, 68, 176, 160, 161, 162, 163, 80, 81,
        82, 83, 84, 85, 128, 129, 130, 131, 96, 97, 98, 99, 100, 178, 144, 145, 146, 147, 148, 179,
    ],
    // mode 10 (mode value 30)
    &[
        0, 1, 2, 3, 4, 5, 164, 176, 177, 132, 16, 17, 18, 19, 20, 21, 117, 133, 178, 116, 32, 33, 34, 35, 36, 37,
        165, 179, 181, 180, 48, 49, 50, 51, 52, 53, 112, 113, 114, 115, 64, 65, 66, 67, 68, 69, 160, 161, 162, 163,
        80, 81, 82, 83, 84, 85, 128, 129, 130, 131, 96, 97, 98, 99, 100, 101, 144, 145, 146, 147, 148, 149,
    ],
    // mode 11 (mode value 3)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 80, 81, 82, 83, 84, 85, 86,
        87, 88, 89,
    ],
    // mode 12 (mode value 7)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 52, 53, 54, 55, 56, 10, 64, 65, 66, 67, 68, 69, 70, 71, 72, 26, 80, 81, 82, 83, 84, 85, 86,
        87, 88, 42,
    ],
    // mode 13 (mode value 11)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 52, 53, 54, 55, 11, 10, 64, 65, 66, 67, 68, 69, 70, 71, 27, 26, 80, 81, 82, 83, 84, 85, 86,
        87, 43, 42,
    ],
    // mode 14 (mode value 15)
    &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
        48, 49, 50, 51, 15, 14, 13, 12, 11, 10, 64, 65, 66, 67, 31, 30, 29, 28, 27, 26, 80, 81, 82, 83, 47, 46, 45,
        44, 43, 42,
    ],
];

const W3: [i32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [i32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// A run of header bits: `len` bits of field `field` from bit `bit` up, stored consecutively.
#[derive(Clone, Copy)]
struct Run {
    field: u8,
    bit: u8,
    len: u8,
}

/// [`LAYOUT`] as runs, per mode: the runs and their count.
const RUNS: [([Run; 40], usize); 14] = {
    let mut out = [(
        [Run {
            field: 0,
            bit: 0,
            len: 0,
        }; 40],
        0,
    ); 14];
    let mut mi = 0;
    while mi < 14 {
        let l = LAYOUT[mi];
        let mut n = 0;
        let mut i = 0;
        while i < l.len() {
            let (f, b) = (l[i] >> 4, l[i] & 15);
            let mut len = 1;
            while i + len < l.len() && l[i + len] >> 4 == f && (l[i + len] & 15) as usize == b as usize + len {
                len += 1;
            }
            out[mi].0[n] = Run {
                field: f,
                bit: b,
                len: len as u8,
            };
            n += 1;
            i += len;
        }
        out[mi].1 = n;
        mi += 1;
    }
    out
};

/// [`RUNS`] in one place for runtime indexing (a `const` would be copied per use).
static RUNS_STATIC: [([Run; 40], usize); 14] = RUNS;

/// A BC6H block with its fields unpacked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bc6hBlock {
    /// Index of the mode, 0..14 (the D3D mode number less one).
    pub mode: u8,
    /// Partition, 0..32 (two-region modes only).
    pub partition: u8,
    /// Endpoint fields as stored, `[w, x, y, z][channel]`: deltas from w (two's complement in the
    /// field's width) for transformed modes.
    pub fields: [[u16; 3]; 4],
    pub idx: [u8; 16],
}

/// Anchor texel of each region (texel 0, and the second region's from the BC7 table).
fn region_anchors(mode: Mode, partition: u8) -> [usize; 2] {
    if mode.two_regions {
        let a = anchors(2, partition);
        [a[0], a[1]]
    } else {
        [0, usize::MAX]
    }
}

fn region_of(mode: Mode, partition: u8, i: usize) -> usize {
    if mode.two_regions {
        subset_of(2, partition, i)
    } else {
        0
    }
}

/// Unpacks a block, or `None` for a reserved mode.
pub fn unpack(raw: &[u8]) -> Option<Bc6hBlock> {
    let v = u128::from_le_bytes(raw[..16].try_into().expect("16-byte block"));
    let mi = mode_index(v)?;
    let m = MODES[mi];
    let mut b = Bc6hBlock {
        mode: mi as u8,
        ..Default::default()
    };
    let mut pos = u32::from(m.mode_bits);
    let (runs, n) = &RUNS_STATIC[mi];
    for r in &runs[..*n] {
        let f = usize::from(r.field);
        b.fields[f / 3][f % 3] |= (((v >> pos) as u16) & ((1 << r.len) - 1)) << r.bit;
        pos += u32::from(r.len);
    }
    let ib = if m.two_regions {
        b.partition = ((v >> pos) & 31) as u8;
        pos += 5;
        3
    } else {
        4
    };
    let anchor = region_anchors(m, b.partition);
    for (i, x) in b.idx.iter_mut().enumerate() {
        let n = if anchor.contains(&i) { ib - 1 } else { ib };
        *x = ((v >> pos) & ((1 << n) - 1)) as u8;
        pos += n;
    }
    debug_assert_eq!(pos, 128);
    Some(b)
}

/// Packs a block. Anchor indices must have their top bit clear, and fields must fit their widths.
pub fn pack(b: &Bc6hBlock) -> [u8; 16] {
    // One copy per mode, so each mode's header runs unroll into straight-line code.
    match b.mode {
        0 => pack_mode::<0>(b),
        1 => pack_mode::<1>(b),
        2 => pack_mode::<2>(b),
        3 => pack_mode::<3>(b),
        4 => pack_mode::<4>(b),
        5 => pack_mode::<5>(b),
        6 => pack_mode::<6>(b),
        7 => pack_mode::<7>(b),
        8 => pack_mode::<8>(b),
        9 => pack_mode::<9>(b),
        10 => pack_mode::<10>(b),
        11 => pack_mode::<11>(b),
        12 => pack_mode::<12>(b),
        _ => pack_mode::<13>(b),
    }
}

#[inline(always)]
fn pack_mode<const MI: usize>(b: &Bc6hBlock) -> [u8; 16] {
    let m = MODES[MI];
    let mut v = u128::from(m.value);
    let mut pos = u32::from(m.mode_bits);
    let (runs, n) = &RUNS[MI];
    for r in &runs[..*n] {
        let f = usize::from(r.field);
        v |= u128::from((b.fields[f / 3][f % 3] >> r.bit) & ((1 << r.len) - 1)) << pos;
        pos += u32::from(r.len);
    }
    let ib = if m.two_regions {
        v |= u128::from(b.partition & 31) << pos;
        pos += 5;
        3
    } else {
        4
    };
    // The indices fit in 64 bits (63 or 46), built apart and placed with one shift.
    let a1 = region_anchors(m, b.partition)[1];
    let mut bits = 0u64;
    let mut at = 0;
    for (i, &x) in b.idx.iter().enumerate() {
        let n = if i == 0 || i == a1 { ib - 1 } else { ib };
        debug_assert!(u32::from(x) < 1 << n, "index {x} of texel {i} needs more than {n} bits");
        bits |= u64::from(x) << at;
        at += n;
    }
    v |= u128::from(bits) << pos;
    debug_assert_eq!(pos + at, 128);
    v.to_le_bytes()
}

fn sign_extend(v: i32, bits: u8) -> i32 {
    let s = 32 - u32::from(bits);
    (v << s) >> s
}

/// An endpoint component back on the 16-bit scale the format interpolates on.
fn unquantize(v: i32, eb: u8, signed: bool) -> i32 {
    if signed {
        if eb >= 16 {
            return v;
        }
        let a = v.abs();
        let u = if a == 0 {
            0
        } else if a >= (1 << (eb - 1)) - 1 {
            0x7fff
        } else {
            ((a << 15) + 0x4000) >> (eb - 1)
        };
        if v < 0 { -u } else { u }
    } else if eb >= 15 {
        v
    } else if v == 0 {
        0
    } else if v == (1 << eb) - 1 {
        0xffff
    } else {
        ((v << 16) + 0x8000) >> eb
    }
}

/// An interpolated value as half-float bits.
fn finish(v: i32, signed: bool) -> u16 {
    if !signed {
        ((v * 31) >> 6) as u16
    } else if v < 0 {
        ((((-v) * 31) >> 5) as u16) | 0x8000
    } else {
        ((v * 31) >> 5) as u16
    }
}

fn interp(e0: i32, e1: i32, w: i32) -> i32 {
    (e0 * (64 - w) + e1 * w + 32) >> 6
}

/// Endpoint values on the 16-bit scale, `[region][end][channel]`.
fn endpoints(b: &Bc6hBlock, signed: bool) -> [[[i32; 3]; 2]; 2] {
    let m = MODES[usize::from(b.mode)];
    let mut e = [[0i32; 3]; 4];
    for c in 0..3 {
        let mut w = i32::from(b.fields[0][c]);
        if signed {
            w = sign_extend(w, m.eb);
        }
        e[0][c] = w;
        for k in 1..2 * m.regions() {
            let mut v = i32::from(b.fields[k][c]);
            if signed || m.transformed {
                v = sign_extend(v, m.db[c]);
            }
            if m.transformed {
                v = (v + w) & ((1 << m.eb) - 1);
                if signed {
                    v = sign_extend(v, m.eb);
                }
            }
            e[k][c] = v;
        }
    }
    let mut out = [[[0i32; 3]; 2]; 2];
    for (k, ek) in e.iter().enumerate() {
        for c in 0..3 {
            out[k / 2][k % 2][c] = unquantize(ek[c], m.eb, signed);
        }
    }
    out
}

/// Decodes an unpacked block to half-float bits.
pub fn decode_block(b: &Bc6hBlock, signed: bool) -> HdrBlock {
    let m = MODES[usize::from(b.mode)];
    let e = endpoints(b, signed);
    let wt: &[i32] = if m.two_regions { &W3 } else { &W4 };
    let mut out = [[0u16; 3]; 16];
    for (i, o) in out.iter_mut().enumerate() {
        let r = region_of(m, b.partition, i);
        let w = wt[usize::from(b.idx[i])];
        for c in 0..3 {
            o[c] = finish(interp(e[r][0][c], e[r][1][c], w), signed);
        }
    }
    out
}

/// Decodes a 16-byte block to half-float bits. Reserved modes decode to zero, as the D3D
/// specification requires.
pub fn decode(raw: &[u8], signed: bool) -> HdrBlock {
    unpack(raw).map_or([[0; 3]; 16], |b| decode_block(&b, signed))
}

/// Decodes a 16-byte block to 8-bit RGB (alpha 255): what an LDR viewer or
/// [`crate::format::Format::decode_block`] sees. Unsigned values are clamped to [0, 1] and scaled
/// by 255; signed ones are clamped to [-1, 1] and mapped back as [`ldr_block`] stores them,
/// `v` to `(255 v + 255) / 2`.
pub fn decode_ldr(raw: &[u8], out: &mut crate::surface::Block, signed: bool) {
    for (o, h) in out.iter_mut().zip(decode(raw, signed)) {
        for c in 0..3 {
            let v = half_to_f32(h[c]);
            o[c] = if signed {
                ((v.clamp(-1.0, 1.0) * 255.0 + 255.0) * 0.5).round() as u8
            } else {
                (v.clamp(0.0, 1.0) * 255.0).round() as u8
            };
        }
        o[3] = 255;
    }
}

/// The half-float targets for an 8-bit block: `u` read as `u / 255`, or signed, as
/// `(2u - 255) / 255` (the crate's convention for signed formats).
pub fn ldr_block(px: &crate::surface::Block, signed: bool) -> HdrBlock {
    px.map(|p| {
        [0, 1, 2].map(|c| {
            let u = f32::from(p[c]);
            if signed {
                f32_to_sf16((2.0 * u - 255.0) / 255.0)
            } else {
                f32_to_uf16(u / 255.0)
            }
        })
    })
}

/// Sum of squared differences between two blocks' halves, on the [`half_key`] scale (signed when
/// `signed`; unsigned blocks hold their keys as their bits).
pub fn sse(a: &HdrBlock, b: &HdrBlock, signed: bool) -> u64 {
    let key = |h: u16| {
        if signed {
            i64::from(half_key(h, true))
        } else {
            i64::from(h)
        }
    };
    a.iter()
        .zip(b)
        .map(|(x, y)| {
            (0..3)
                .map(|c| {
                    let d = key(x[c]) - key(y[c]);
                    (d * d) as u64
                })
                .sum::<u64>()
        })
        .sum()
}

/// Search settings for an effort level 0..=9.
#[derive(Clone, Copy, Debug)]
struct Search {
    /// Partitions (best estimated fit first) tried with the two-region modes.
    partitions: usize,
    /// One-region modes tried per block: the most precise ones whose deltas fit.
    modes1: usize,
    /// Two-region modes tried per partition, likewise.
    modes2: usize,
    /// Least-squares refits after the first indexing.
    refits: usize,
    /// Whether a refit chooses the indices again (otherwise it only moves the endpoints).
    reindex: bool,
    /// Two-region modes are skipped when the one-region error is at most this multiple of the best
    /// partition's estimated error (0: never skipped).
    skip: f32,
    /// BC6H_SF16 rather than BC6H_UF16.
    signed: bool,
}

fn search(effort: u8, signed: bool) -> Search {
    let e = usize::from(effort.min(crate::format::MAX_EFFORT));
    Search {
        // Effort 0 is one region only (no partition search).
        partitions: [0, 1, 2, 2, 3, 4, 6, 8, 12, 32][e],
        modes1: [1, 1, 1, 2, 2, 2, 2, 3, 4, 4][e],
        modes2: [1, 1, 1, 1, 2, 2, 3, 3, 4, 10][e],
        refits: [1, 1, 1, 2, 2, 2, 2, 3, 3, 4][e],
        skip: [1.0, 1.0, 0.5, 0.35, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0][e],
        // Effort 0 keeps its indices through the refit.
        reindex: e > 0,
        signed,
    }
}

/// One-region modes ([`MODES`] indices), most precise endpoints first.
const ONE_ORDER: [usize; 4] = [13, 12, 11, 10];
/// Two-region modes, most precise endpoints first.
const TWO_ORDER: [usize; 10] = [2, 3, 4, 0, 5, 6, 7, 8, 1, 9];

/// Encodes one block of half floats at effort 0..=9: signed (BC6H_SF16) when `signed`, else
/// unsigned with negative values and NaN as 0.
pub fn encode_block(px: &HdrBlock, effort: u8, signed: bool) -> [u8; 16] {
    batch::encode_batch(std::slice::from_ref(px), effort, signed)[0]
}

/// Encodes an 8-bit block as [`ldr_block`] reads it (what [`crate::format::Format::encode_block`]
/// does for BC6H; HDR sources go through [`encode_block`]).
pub fn encode_ldr(px: &crate::surface::Block, effort: u8, signed: bool) -> [u8; 16] {
    encode_block(&ldr_block(px, signed), effort, signed)
}

/// Encodes every block of an HDR surface, in parallel, raster order: signed (BC6H_SF16) when
/// `signed`.
pub fn encode_surface(surface: &crate::hdr::HdrSurface, effort: u8, signed: bool) -> Vec<u8> {
    // Each block's texels are converted as its lanes are filled (F16C where the CPU has it).
    let px = &surface.pixels;
    let (w, h, bw) = (
        surface.width as usize,
        surface.height as usize,
        surface.blocks_x() as usize,
    );
    #[cfg(target_arch = "x86_64")]
    let f16c = crate::hdr::f16c();
    encode_blocks(
        surface.block_count(),
        |i, l, x| {
            let (x0, y0) = (i as usize % bw * 4, i as usize / bw * 4);
            let interior = x0 + 4 <= w && y0 + 4 <= h;
            #[cfg(target_arch = "x86_64")]
            if f16c && interior {
                let rows: [&[[f32; 3]]; 4] = std::array::from_fn(|ty| &px[(y0 + ty) * w + x0..][..4]);
                // SAFETY: the CPU reports F16C and AVX2.
                unsafe {
                    if signed {
                        fill_f16c::<true>(&rows, l, x);
                    } else {
                        fill_f16c::<false>(&rows, l, x);
                    }
                };
                return;
            }
            if signed {
                batch::set_lane(x, l, &surface.read_block_as(i % bw as u32, i / bw as u32, true), true);
            } else if interior {
                let rows: [&[[f32; 3]]; 4] = std::array::from_fn(|ty| &px[(y0 + ty) * w + x0..][..4]);
                for (ty, row) in rows.iter().enumerate() {
                    for (tx, p) in row.iter().enumerate() {
                        for c in 0..3 {
                            x[(ty * 4 + tx) * 3 + c][l] = f32::from(f32_to_uf16(p[c]));
                        }
                    }
                }
            } else {
                // An edge block: texels past the image repeat its last row and column.
                for t in 0..16 {
                    let p = px[(y0 + t / 4).min(h - 1) * w + (x0 + t % 4).min(w - 1)];
                    for c in 0..3 {
                        x[t * 3 + c][l] = f32::from(f32_to_uf16(p[c]));
                    }
                }
            }
        },
        effort,
        signed,
    )
}

/// Lane `l` of `x` from four rows of four texels, each channel as [`f32_to_uf16`] (or signed,
/// [`f32_to_sf16`] as its [`half_key`]): clamping to [0, 65504] first (`max` returns its second
/// operand, 0, for NaN) leaves finite, non-negative values, which the round-to-nearest-even
/// conversion maps as `f32_to_half` does. Signed, NaN is zeroed first and the clamp is to
/// [-65504, 65504]; -0 lands on key 0 as 0 does.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,f16c")]
unsafe fn fill_f16c<const S: bool>(rows: &[&[[f32; 3]]; 4], l: usize, x: &mut batch::Texels) {
    use std::arch::x86_64::*;
    let top = _mm256_set1_ps(65504.0);
    let low = if S {
        _mm256_set1_ps(-65504.0)
    } else {
        _mm256_setzero_ps()
    };
    for (ty, row) in rows.iter().enumerate() {
        let s = row.as_flattened();
        assert!(s.len() >= 12);
        let mut v = [0f32; 16];
        // SAFETY: `s` holds 12 floats, read as 8 and the last 8 (overlapping); `v` holds 16.
        unsafe {
            for (k, off) in [(0, 0), (8, 4)] {
                let mut a = _mm256_loadu_ps(s.as_ptr().add(off));
                if S {
                    a = _mm256_and_ps(a, _mm256_cmp_ps::<_CMP_ORD_Q>(a, a));
                }
                let a = _mm256_min_ps(_mm256_max_ps(a, low), top);
                let h = _mm256_cvtepu16_epi32(_mm256_cvtps_ph::<_MM_FROUND_TO_NEAREST_INT>(a));
                let key = if S {
                    // Magnitude, negated where the sign bit is set: (m ^ s) - s.
                    let m = _mm256_and_si256(h, _mm256_set1_epi32(0x7fff));
                    let sg = _mm256_srai_epi32::<31>(_mm256_slli_epi32::<16>(h));
                    _mm256_sub_epi32(_mm256_xor_si256(m, sg), sg)
                } else {
                    h
                };
                _mm256_storeu_ps(v.as_mut_ptr().add(k), _mm256_cvtepi32_ps(key));
            }
        }
        // v[0..8] are floats 0..8; v[8..16] are floats 4..12.
        for k in 0..8 {
            x[ty * 12 + k][l] = v[k];
        }
        for k in 8..12 {
            x[ty * 12 + k][l] = v[k + 4];
        }
    }
}

/// Encodes blocks `0..n` eight at a time, in parallel, raster order: `fill(i, l, x)` puts block
/// `i` in lane `l` of `x` (as [`half_key`], signed when `signed`).
fn encode_blocks(n: usize, fill: impl Fn(u32, usize, &mut batch::Texels) + Sync, effort: u8, signed: bool) -> Vec<u8> {
    use rayon::prelude::*;
    const L: usize = crate::simd::LANES;
    let s = search(effort, signed);
    let mut out = vec![0u8; n * 16];
    out.par_chunks_mut(16 * L)
        .enumerate()
        .with_min_len(16)
        .for_each(|(g, dst)| {
            let k = dst.len() / 16;
            let mut x = [[0f32; L]; 48];
            for l in 0..k {
                fill((g * L + l) as u32, l, &mut x);
            }
            batch::pad_lanes(&mut x, k);
            let enc = batch::encode_texels(&x, s);
            dst.copy_from_slice(enc[..k].as_flattened());
        });
    out
}

/// Decodes BC6H blocks (raster order, signed when `signed`) to an HDR surface.
pub fn decode_surface(data: &[u8], width: u32, height: u32, signed: bool) -> crate::hdr::HdrSurface {
    crate::hdr::HdrSurface::from_blocks(
        width,
        height,
        data.as_chunks::<16>().0.iter().map(|b| decode(b, signed)),
    )
}

/// Encodes every block of an 8-bit surface (as [`encode_ldr`]), in parallel, raster order.
pub fn encode_surface_ldr(surface: &crate::surface::Surface, effort: u8, signed: bool) -> Vec<u8> {
    let bx = surface.blocks_x();
    encode_blocks(
        surface.block_count(),
        |i, l, x| batch::set_lane(x, l, &ldr_block(&surface.read_block(i % bx, i / bx), signed), signed),
        effort,
        signed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn half_conversion_round_trips_and_rounds() {
        for h in 0..=0xffffu16 {
            let f = half_to_f32(h);
            if f.is_nan() {
                assert!(f32_to_half(f) & 0x7c00 == 0x7c00 && f32_to_half(f) & 0x3ff != 0);
            } else {
                assert_eq!(f32_to_half(f), h, "{h:#06x} {f}");
            }
        }
        assert_eq!(f32_to_half(1.0), 0x3c00);
        assert_eq!(f32_to_half(65504.0), 0x7bff);
        assert_eq!(f32_to_half(65519.0), 0x7bff);
        assert_eq!(f32_to_half(65520.0), 0x7c00);
        // Halfway between 1 and the next half rounds to even.
        assert_eq!(f32_to_half(1.0 + 1.0 / 2048.0), 0x3c00);
        assert_eq!(f32_to_half(1.0 + 3.0 / 2048.0), 0x3c02);
        assert_eq!(f32_to_half(2f32.powi(-25)), 0);
        assert_eq!(f32_to_half(2f32.powi(-25) * 1.01), 1);
        assert_eq!(f32_to_uf16(-3.0), 0);
        assert_eq!(f32_to_uf16(1e9), HALF_MAX);
    }

    #[test]
    fn layouts_cover_every_bit_once() {
        for (mi, m) in MODES.iter().enumerate() {
            let lay = LAYOUT[mi];
            let mut seen = std::collections::HashSet::new();
            for &e in lay {
                assert!(seen.insert(e), "mode {mi}: field bit {e} twice");
                let (f, bit) = (usize::from(e >> 4), e & 15);
                let width = if f < 3 { m.eb } else { m.db[f % 3] };
                assert!(bit < width, "mode {mi}: bit {bit} of field {f} past width {width}");
                assert!(f < 6 * m.regions());
            }
            let fields: usize =
                3 * usize::from(m.eb) + (2 * m.regions() - 1) * m.db.iter().map(|&d| d as usize).sum::<usize>();
            assert_eq!(lay.len(), fields, "mode {mi}");
            let idx_bits = if m.two_regions { 5 + 46 } else { 63 };
            assert_eq!(usize::from(m.mode_bits) + lay.len() + idx_bits, 128, "mode {mi}");
        }
    }

    #[test]
    fn pack_unpack_round_trips_random_blocks() {
        let mut seed = 0x1234_5678_9abc_def1u64;
        let mut n = 0;
        while n < 20_000 {
            let raw = (u128::from(rng(&mut seed)) << 64 | u128::from(rng(&mut seed))).to_le_bytes();
            let Some(b) = unpack(&raw) else { continue };
            assert_eq!(pack(&b), raw);
            n += 1;
        }
        let reserved = [19u8, 23, 27, 31].map(|v| {
            let mut raw = [0u8; 16];
            raw[0] = v;
            unpack(&raw)
        });
        assert!(reserved.iter().all(Option::is_none));
    }

    fn block_from(f: impl Fn(usize, usize) -> f32) -> HdrBlock {
        std::array::from_fn(|i| [0, 1, 2].map(|c| f32_to_uf16(f(i, c))))
    }

    /// Mean squared error per channel sample, in half-float steps.
    fn mse(px: &HdrBlock, effort: u8) -> f64 {
        mse_as(px, effort, false)
    }

    fn mse_as(px: &HdrBlock, effort: u8, signed: bool) -> f64 {
        let raw = encode_block(px, effort, signed);
        sse(px, &decode(&raw, signed), signed) as f64 / 48.0
    }

    #[test]
    fn flat_and_smooth_blocks_encode_nearly_exactly() {
        let flat = block_from(|_, c| [0.25, 3.0, 1200.0][c]);
        assert!(mse(&flat, 5) < 1.0, "{}", mse(&flat, 5));
        // Within one octave a ramp is a line in half bits: 16 evenly spaced values, which 4-bit
        // indices hold to a few bits (their weights are 64ths, not 15ths).
        let ramp = block_from(|i, c| (1.0 + (i % 4) as f32 * 0.25 + (i / 4) as f32 * 0.0625) * [0.5, 2.0, 8.0][c]);
        for e in [0, 5] {
            assert!(mse(&ramp, e) < 30.0, "{}", mse(&ramp, e));
        }
        // A wide range: four octaves across the block, a curve in half bits (about 1600).
        let wide = block_from(|i, c| 2f32.powf(i as f32 / 4.0) * [1.0, 0.8, 0.5][c]);
        assert!(mse(&wide, 5) < 2000.0, "{}", mse(&wide, 5));
    }

    #[test]
    fn effort_does_not_hurt_and_random_blocks_decode() {
        let mut seed = 77u64;
        let (mut lo, mut hi) = (0.0, 0.0);
        for _ in 0..200 {
            let base = [0, 1, 2].map(|_| (rng(&mut seed) % 2000) as f32 / 100.0);
            let px = block_from(|i, c| {
                let edge = if (i % 4) + (i / 4) > 3 { 4.0 } else { 1.0 };
                base[c] * edge * (1.0 + (rng(&mut seed.wrapping_add(i as u64)) % 100) as f32 / 400.0)
            });
            lo += mse(&px, 1);
            hi += mse(&px, 9);
        }
        assert!(hi <= lo, "effort 9 {hi} vs effort 1 {lo}");
    }

    /// Eight different blocks: smooth, flat, edged and noisy, dark to bright.
    fn mixed_blocks(seed: u64) -> [HdrBlock; crate::simd::LANES] {
        let mut seed = seed;
        std::array::from_fn(|l| {
            let base = [0, 1, 2].map(|_| (rng(&mut seed) % 4000) as f32 / 100.0 + 0.01);
            let kind = l % 4;
            let noise: [[f32; 3]; 16] =
                std::array::from_fn(|_| [0, 1, 2].map(|_| (rng(&mut seed) % 1000) as f32 / 300.0));
            block_from(|i, c| {
                let (x, y) = ((i % 4) as f32, (i / 4) as f32);
                base[c]
                    * match kind {
                        0 => 1.0 + 0.1 * x + 0.05 * y,
                        1 => 1.0,
                        2 => {
                            if x + y > 3.0 {
                                6.0
                            } else {
                                0.5
                            }
                        }
                        _ => 1.0 + noise[i][c],
                    }
            })
        })
    }

    #[test]
    fn batch_lanes_are_independent() {
        for seed in [3u64, 99, 12345] {
            let px = mixed_blocks(seed);
            for effort in [0, 2, 5] {
                for signed in [false, true] {
                    let px = if signed { signed_blocks(&px) } else { px };
                    let all = batch::encode_batch(&px, effort, signed);
                    for (l, b) in px.iter().enumerate() {
                        assert_eq!(
                            all[l],
                            encode_block(b, effort, signed),
                            "seed {seed} effort {effort} lane {l} signed {signed}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn batch_backends_agree() {
        for seed in [5u64, 77, 4242] {
            let px = mixed_blocks(seed);
            for effort in [0, 3, 9] {
                for signed in [false, true] {
                    let px = if signed { signed_blocks(&px) } else { px };
                    let generic = batch::encode8_generic(&px, effort, signed);
                    assert_eq!(
                        batch::encode_batch(&px, effort, signed),
                        generic.to_vec(),
                        "seed {seed} effort {effort} signed {signed}"
                    );
                }
            }
        }
    }

    #[test]
    fn surface_conversion_matches_read_block() {
        // NaN, negatives, -0, past HALF_MAX, rounding ties and denormals, plus random bits, in a
        // 37x11 image (interior and edge blocks).
        let special = [
            f32::NAN,
            -1.0,
            -0.0,
            0.0,
            65504.0,
            65519.0,
            65520.0,
            1e9,
            f32::INFINITY,
            1.0 + 1.0 / 2048.0,
            3e-6,
            6e-8,
        ];
        let mut r = 0x9e37_79b9u32;
        let (w, h) = (37u32, 11u32);
        let pixels = (0..w * h)
            .map(|i| {
                std::array::from_fn(|c| {
                    r ^= r << 13;
                    r ^= r >> 17;
                    r ^= r << 5;
                    match r % 4 {
                        0 => special[(i as usize * 3 + c) % special.len()],
                        1 => f32::from_bits(r),
                        _ => (r >> 8) as f32 / 1024.0,
                    }
                })
            })
            .collect();
        let s = crate::hdr::HdrSurface {
            width: w,
            height: h,
            pixels,
        };
        for signed in [false, true] {
            let got = encode_surface(&s, 1, signed);
            for i in 0..s.block_count() as u32 {
                let b = s.read_block_as(i % s.blocks_x(), i / s.blocks_x(), signed);
                assert_eq!(
                    got[i as usize * 16..][..16],
                    encode_block(&b, 1, signed),
                    "block {i} signed {signed}"
                );
            }
        }
    }

    /// `px` with every other texel's channels negated, and lanes 1 and 5 wholly negative, so
    /// signed blocks cross zero, sit below it and straddle it in one channel only.
    fn signed_blocks(px: &[HdrBlock; crate::simd::LANES]) -> [HdrBlock; crate::simd::LANES] {
        std::array::from_fn(|l| {
            std::array::from_fn(|i| {
                std::array::from_fn(|c| {
                    let neg = match l {
                        1 | 5 => true,
                        2 | 6 => c == 1 && i % 3 == 0,
                        _ => i % 2 == 1,
                    };
                    let h = px[l][i][c];
                    if neg && h != 0 { h | 0x8000 } else { h }
                })
            })
        })
    }

    #[test]
    fn signed_blocks_encode_well() {
        // Signs agree with unsigned where the data is non-negative: a flat block and a ramp.
        let sblock = |f: &dyn Fn(usize, usize) -> f32| -> HdrBlock {
            std::array::from_fn(|i| [0, 1, 2].map(|c| f32_to_sf16(f(i, c))))
        };
        let flat = sblock(&|_, c| [0.25, -3.0, 1200.0][c]);
        assert!(mse_as(&flat, 5, true) < 1.0, "{}", mse_as(&flat, 5, true));
        let ramp = sblock(&|i, c| -(1.0 + (i % 4) as f32 * 0.25 + (i / 4) as f32 * 0.0625) * [0.5, 2.0, 8.0][c]);
        for e in [0, 5] {
            assert!(mse_as(&ramp, e, true) < 30.0, "{}", mse_as(&ramp, e, true));
        }
        // A gradient through zero: -1 to 1 across the block, linear on the signed scale only near
        // zero, so well under one 8-bit step (about 11 half steps near 0.5) per channel is the bar.
        let cross = sblock(&|i, c| ((i % 4) as f32 - 1.5) / 1.5 * [1.0, 0.5, 0.25][c]);
        let e = mse_as(&cross, 5, true);
        assert!(e.sqrt() < 2000.0, "{e}");
        // Effort helps on random signed blocks, and every block decodes with the signs it was given.
        let mut seed = 99u64;
        let (mut lo, mut hi) = (0.0, 0.0);
        for k in 0..100 {
            let px = signed_blocks(&mixed_blocks(seed + k))[(k % 8) as usize];
            lo += mse_as(&px, 1, true);
            hi += mse_as(&px, 9, true);
            let d = decode(&encode_block(&px, 9, true), true);
            for i in 0..16 {
                for c in 0..3 {
                    let (a, b) = (half_key(px[i][c], true), half_key(d[i][c], true));
                    assert!(
                        a.signum() * b.signum() >= 0 || (a - b).abs() < 2048,
                        "{k} {i} {c}: {a} {b}"
                    );
                }
            }
            seed = rng(&mut seed);
        }
        assert!(hi <= lo, "effort 9 {hi} vs effort 1 {lo}");
    }

    #[test]
    fn signed_ldr_round_trips() {
        // Every 8-bit value through (2u - 255) / 255 and back on a flat block.
        for u in 0..=255u8 {
            let px = [[u, 255 - u, u / 2, 255]; 16];
            let mut out = [[0u8; 4]; 16];
            decode_ldr(&encode_ldr(&px, 5, true), &mut out, true);
            for c in 0..3 {
                assert!(
                    (i32::from(out[0][c]) - i32::from(px[0][c])).abs() <= 1,
                    "{u} {c}: {:?}",
                    out[0]
                );
            }
        }
    }

    #[test]
    fn signed_decode_handles_negative_endpoints() {
        // Mode 11 (index 10, 10-bit raw endpoints): w = -1 (all ones in 10 bits), x = +1.
        let b = Bc6hBlock {
            mode: 10,
            fields: [[0x3ff; 3], [1; 3], [0; 3], [0; 3]],
            idx: [0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15],
            ..Default::default()
        };
        let d = decode(&pack(&b), true);
        assert_eq!(super::half_key(d[0][0], true), -93);
        // -1 at 10 bits unquantizes to -((1 << 15) + 0x4000) >> 9 = -96, finished to -(96*31>>5) = -93.
        assert_eq!(d[0], [0x8000 | 93; 3]);
        assert_eq!(d[1], [93; 3]);
    }
}
