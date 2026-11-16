//! Rate-distortion optimization for BC6H: the LZ-aware pass of [`crate::rdo`] for HDR surfaces.
//!
//! Each block, in raster order, picks among its base encoding and candidates built from earlier
//! blocks of its chunk, minimizing `J = SSE / (16 · HALF_PER_LDR) + λ · bits` with the bits from
//! [`LzModel`]. The error is on half-float bit patterns, which grow like the logarithm of the value
//! (1024 steps per octave), so it is a relative error: λ means the same at every exposure.
//! [`HALF_PER_LDR`] puts it on about the scale of the 8-bit formats' error, so λ does too.
//!
//! A BC6H block keeps its mode, endpoints and partition in its low bits and its indices in its top
//! 63 (one region) or 46 (two regions) bits, so the candidates are:
//!
//! - a whole earlier block, copied;
//! - an earlier block's mode, endpoints and partition, with indices chosen for this block's texels
//!   (its low bytes match);
//! - an earlier block's mode, partition and indices, with least-squares endpoints for this block's
//!   texels (its high bytes match).
//!
//! Like the other formats, a chunk that real Zstd does not shrink goes back to its base encoding.
//!
//! Signed blocks (BC6H_SF16) run the same pass with errors on the signed scale of
//! [`super::half_key`].

use super::{
    Bc6hBlock, HdrBlock, MODES, W3, W4, decode, decode_block, endpoints, finish, half_key, interp, pack,
    region_anchors, region_of, sse, unpack, unquantize,
};
use crate::format::Format;
use crate::hdr::HdrSurface;
use crate::rdo::{LzModel, LzState, RdoParams, RdoStats, VERIFY_QUICK, bit_budget};
use rayon::prelude::*;
use std::collections::HashSet;

/// Squared half-float steps per squared 8-bit step: near 0.5 one 8-bit step (1/255) is about 11
/// half-float steps, near 0.1 about 57, so 64 sits between and λ moves sizes about as much as it
/// does for BC7.
pub const HALF_PER_LDR: f64 = 64.0;
/// How far back the re-index candidates look (whole copies look `max_back` far). On the 20 Poly
/// Haven HDRIs, 64 is +2.4% BD-rate against 256.
const REINDEX_BACK: usize = 256;
/// How far back the refit candidates look: 32 is +0.3% BD-rate against 256 for half the RDO time.
const REFIT_BACK: usize = 32;

/// A block already chosen: its bytes, decode and fields.
struct Prev {
    bytes: [u8; 16],
    dec: HdrBlock,
    fields: Option<Bc6hBlock>,
}

/// Squared distance between two texels: their bits unsigned (`k` the halves, as unsigned palettes
/// and targets hold their keys as their bits), [`half_key`] signed.
fn sq(a: [i32; 3], b: [i32; 3]) -> u64 {
    (0..3)
        .map(|c| {
            let d = i64::from(a[c]) - i64::from(b[c]);
            (d * d) as u64
        })
        .sum()
}

/// Each channel of `h` as [`sq`] measures it.
fn keys(h: [u16; 3], signed: bool) -> [i32; 3] {
    h.map(|v| if signed { half_key(v, true) } else { i32::from(v) })
}

/// The first bit of `b`'s indices: everything below it (mode, endpoints, partition) is the head.
fn index_start(b: &Bc6hBlock) -> u32 {
    if MODES[usize::from(b.mode)].two_regions { 82 } else { 65 }
}

/// `b` with each texel's index chosen for `src` (anchors restricted to the lower half, as the format
/// requires), and its error, or `None` once the error passes `cap`.
fn reindex(b: &Bc6hBlock, src: &HdrBlock, cap: u64, signed: bool) -> Option<(Bc6hBlock, u64)> {
    let m = MODES[usize::from(b.mode)];
    let e = endpoints(b, signed);
    let wt: &[i32] = if m.two_regions { &W3 } else { &W4 };
    let nw = wt.len();
    let mut pal = [[[0i32; 3]; 16]; 2];
    for (r, pr) in pal.iter_mut().enumerate().take(m.regions()) {
        for (k, p) in pr.iter_mut().enumerate().take(nw) {
            for c in 0..3 {
                let h = finish(interp(e[r][0][c], e[r][1][c], wt[k]), signed);
                p[c] = if signed { half_key(h, true) } else { i32::from(h) };
            }
        }
    }
    let anchors = region_anchors(m, b.partition);
    let mut out = *b;
    let mut err = 0u64;
    for i in 0..16 {
        let r = region_of(m, b.partition, i);
        let lim = if anchors.contains(&i) { nw / 2 } else { nw };
        let x = keys(src[i], signed);
        let mut best = (u64::MAX, 0usize);
        for (k, &p) in pal[r][..lim].iter().enumerate() {
            let d = sq(p, x);
            if d < best.0 {
                best = (d, k);
            }
        }
        out.idx[i] = best.1 as u8;
        err += best.0;
        if err > cap {
            return None;
        }
    }
    Some((out, err))
}

/// The `eb`-bit endpoint value whose unquantized value is nearest `x` (on the 16-bit scale;
/// signed: -32767..=32767, the magnitude quantized with `eb - 1` bits and the sign kept).
fn quantize(x: f32, eb: u8, signed: bool) -> i32 {
    if signed {
        let a = x.abs();
        let q = if eb >= 16 {
            (a.round() as i32).min(0x7fff)
        } else {
            let max = (1 << (eb - 1)) - 1;
            let q0 = ((a * (1 << (eb - 1)) as f32 / 32768.0) as i32).clamp(0, max);
            let mut best = (f32::MAX, q0);
            for q in (q0 - 1).max(0)..=(q0 + 1).min(max) {
                let d = (unquantize(q, eb, true) as f32 - a).abs();
                if d < best.0 {
                    best = (d, q);
                }
            }
            best.1
        };
        return if x < 0.0 { -q } else { q };
    }
    if eb >= 15 {
        return (x.round() as i32).clamp(0, 0xffff);
    }
    let max = (1 << eb) - 1;
    let q0 = ((x * (1 << eb) as f32 / 65536.0) as i32).clamp(0, max);
    let mut best = (f32::MAX, q0);
    for q in (q0 - 1).max(0)..=(q0 + 1).min(max) {
        let d = (unquantize(q, eb, false) as f32 - x).abs();
        if d < best.0 {
            best = (d, q);
        }
    }
    best.1
}

/// `b`'s mode, partition and indices with least-squares endpoints for `src` (deltas clamped to their
/// bits), and its error, or `None` when it passes `cap`.
fn refit(b: &Bc6hBlock, src: &HdrBlock, cap: u64, signed: bool) -> Option<(Bc6hBlock, u64)> {
    let m = MODES[usize::from(b.mode)];
    let wt: &[i32] = if m.two_regions { &W3 } else { &W4 };
    // Per region: sums of s², st, t², s·x, t·x and x, with t the weight and s = 1 - t.
    let mut acc = [[0f64; 3 + 9]; 2];
    let mut n = [0f64; 2];
    for i in 0..16 {
        let r = region_of(m, b.partition, i);
        let t = f64::from(wt[usize::from(b.idx[i])]) / 64.0;
        let s = 1.0 - t;
        let a = &mut acc[r];
        a[0] += s * s;
        a[1] += s * t;
        a[2] += t * t;
        for c in 0..3 {
            // The 16-bit value that `finish` maps to the middle of this half-float step (signed:
            // of the magnitude's step, with the sign).
            let x = if signed {
                let k = f64::from(half_key(src[i][c], true));
                (k.abs() + 0.5).copysign(k) * 32.0 / 31.0
            } else {
                (f64::from(src[i][c]) + 0.5) * 64.0 / 31.0
            };
            a[3 + c] += s * x;
            a[6 + c] += t * x;
            a[9 + c] += x;
        }
        n[r] += 1.0;
    }
    let mut q = [[0i32; 3]; 4];
    for r in 0..m.regions() {
        let a = &acc[r];
        let det = a[0] * a[2] - a[1] * a[1];
        for c in 0..3 {
            let (e0, e1) = if det.abs() > 1e-9 {
                (
                    (a[3 + c] * a[2] - a[6 + c] * a[1]) / det,
                    (a[6 + c] * a[0] - a[3 + c] * a[1]) / det,
                )
            } else {
                let mean = a[9 + c] / n[r].max(1.0);
                (mean, mean)
            };
            let (lo, hi) = if signed { (-32767.0, 32767.0) } else { (0.0, 65535.0) };
            q[2 * r][c] = quantize(e0.clamp(lo, hi) as f32, m.eb, signed);
            q[2 * r + 1][c] = quantize(e1.clamp(lo, hi) as f32, m.eb, signed);
        }
    }
    let mut out = *b;
    for c in 0..3 {
        out.fields[0][c] = q[0][c] as u16;
        for k in 1..2 * m.regions() {
            out.fields[k][c] = if m.transformed {
                let half = 1 << (m.db[c] - 1);
                let d = (q[k][c] - q[0][c]).clamp(-half, half - 1);
                (d & ((1 << m.db[c]) - 1)) as u16
            } else {
                q[k][c] as u16
            };
        }
    }
    let err = sse(src, &decode_block(&out, signed), signed);
    (err <= cap).then_some((out, err))
}

fn optimize_chunk(s: &HdrSurface, nx: usize, first: usize, data: &mut [u8], p: &RdoParams, signed: bool) -> RdoStats {
    let format = if signed { Format::Bc6hSf16 } else { Format::Bc6h };
    let count = data.len() / 16;
    let window = p.window_for(format);
    let max_back = p.max_back_for(format).min(window / 16);
    let mut model = LzModel::new(window, p.chain_depth);
    model.set_literals(data);
    let lambda = p.lambda as f64;
    let ratio2 = (p.max_rms_ratio as f64).powi(2);
    // In half-float SSE: 48 channels at `min_rms` 8-bit steps.
    let floor = 48.0 * (p.min_rms as f64).powi(2) * HALF_PER_LDR;
    let scale = 1.0 / (16.0 * HALF_PER_LDR);
    let original = p.verify_zstd_level.map(|_| data.to_vec());
    let mut stats = RdoStats {
        blocks: count,
        ..Default::default()
    };
    let mut prevs: Vec<Prev> = Vec::with_capacity(count);
    let mut seen: HashSet<u128> = HashSet::new();
    let mut heads: HashSet<u128> = HashSet::new();
    let mut tails: HashSet<(u128, u8, u8)> = HashSet::new();
    for i in 0..count {
        let bi = first + i;
        let src = s.read_block_as((bi % nx) as u32, (bi / nx) as u32, signed);
        let score = |err: u64, bits: f32| err as f64 * scale + lambda * bits as f64;
        let base: [u8; 16] = data[i * 16..i * 16 + 16].try_into().unwrap();
        let base_err = sse(&src, &decode(&base, signed), signed);
        let (base_bits, base_rep) = model.cost(&base);
        let min_bits = lambda * model.min_cost(16) as f64;
        let limit = (base_err as f64 * ratio2 + floor) as u64;
        let mut best: (f64, [u8; 16], f32, LzState) = (score(base_err, base_bits), base, base_bits, base_rep);
        let consider = |c: [u8; 16], err: u64, best: &mut (f64, [u8; 16], f32, LzState)| {
            if err > limit {
                return;
            }
            let dist = err as f64 * scale;
            if dist >= best.0 || (dist + min_bits >= best.0 && !model.may_continue(&c)) {
                return;
            }
            let Some((bits, rep)) = model.cost_until(&c, bit_budget(best.0, dist, lambda)) else {
                return;
            };
            let j = dist + lambda * bits as f64;
            if j < best.0 {
                *best = (j, c, bits, rep);
            }
        };
        seen.clear();
        seen.insert(u128::from_le_bytes(base));
        for d in 1..=max_back.min(i) {
            let j = i - d;
            if seen.insert(u128::from_le_bytes(prevs[j].bytes)) {
                consider(prevs[j].bytes, sse(&src, &prevs[j].dec, signed), &mut best);
            }
        }
        heads.clear();
        for d in 1..=REINDEX_BACK.min(i) {
            let j = i - d;
            let Some(f) = &prevs[j].fields else { continue };
            let v = u128::from_le_bytes(prevs[j].bytes);
            // Each head once: the same head re-indexes the same way.
            if !heads.insert(v & ((1u128 << index_start(f)) - 1)) {
                continue;
            }
            // A candidate must beat the best on distortion alone.
            let cap = (limit as f64).min(best.0 / scale) as u64;
            if let Some((b, err)) = reindex(f, &src, cap, signed) {
                let c = pack(&b);
                if seen.insert(u128::from_le_bytes(c)) {
                    debug_assert_eq!(err, sse(&src, &decode(&c, signed), signed));
                    consider(c, err, &mut best);
                }
            }
        }
        tails.clear();
        for d in 1..=REFIT_BACK.min(i) {
            let j = i - d;
            let Some(f) = &prevs[j].fields else { continue };
            let v = u128::from_le_bytes(prevs[j].bytes);
            // Each mode, partition and index set once.
            if !tails.insert((v >> index_start(f), f.mode, f.partition)) {
                continue;
            }
            let cap = (limit as f64).min(best.0 / scale) as u64;
            if let Some((b, err)) = refit(f, &src, cap, signed) {
                let c = pack(&b);
                if seen.insert(u128::from_le_bytes(c)) {
                    consider(c, err, &mut best);
                }
            }
        }
        stats.est_bits_before += base_bits as f64;
        stats.est_bits_after += best.2 as f64;
        if best.1 != base {
            stats.changed += 1;
        }
        data[i * 16..i * 16 + 16].copy_from_slice(&best.1);
        prevs.push(Prev {
            bytes: best.1,
            dec: decode(&best.1, signed),
            fields: unpack(&best.1),
        });
        model.push(&best.1, best.3);
    }
    // Safety net: the rate model is an estimate. If real Zstd says RDO did not help this chunk,
    // keep the base encoding.
    if let (Some(level), Some(original)) = (p.verify_zstd_level, original) {
        let size = crate::rdo::zstd_size;
        let quick = level > 1 && size(data, 1) as f64 <= VERIFY_QUICK * size(&original, 1) as f64;
        if !quick && size(data, level) >= size(&original, level) {
            data.copy_from_slice(&original);
            stats.changed = 0;
            stats.est_bits_after = stats.est_bits_before;
            stats.reverted_chunks = 1;
        }
    }
    stats
}

/// Runs RDO in place over one level's BC6H blocks (`data`, raster order, as
/// [`super::encode_surface`] writes them, signed when `signed`) of surface `s`.
pub fn optimize(s: &HdrSurface, data: &mut [u8], p: &RdoParams, signed: bool) -> RdoStats {
    let blocks = data.len() / 16;
    if !p.enabled() || blocks == 0 {
        return RdoStats {
            blocks,
            ..Default::default()
        };
    }
    let nx = s.blocks_x() as usize;
    let chunk = (p.chunk_bytes_for(Format::Bc6h) / 16).max(1);
    data.par_chunks_mut(chunk * 16)
        .enumerate()
        .map(|(k, d)| optimize_chunk(s, nx, k * chunk, d, p, signed))
        .reduce(RdoStats::default, RdoStats::merge)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every re-indexed candidate's error matches its decode (the debug assertion in
    /// [`optimize_chunk`]), and RDO moves blocks and lowers the estimated rate.
    #[test]
    fn candidates_score_their_decode() {
        let mut s = HdrSurface::from_ldr(&crate::synth::bricks(64), true);
        for signed in [false, true] {
            if signed {
                // Half the texels' channels negative, so signed candidates cross zero.
                for (i, p) in s.pixels.iter_mut().enumerate() {
                    for (c, v) in p.iter_mut().enumerate() {
                        if (i + c) % 3 == 0 {
                            *v = -*v;
                        }
                    }
                }
            }
            let mut data = super::super::encode_surface(&s, 2, signed);
            let p = RdoParams {
                lambda: 4.0,
                verify_zstd_level: None,
                ..Default::default()
            };
            let st = optimize(&s, &mut data, &p, signed);
            assert!(st.changed > 0, "signed {signed}");
            assert!(st.est_bits_after < st.est_bits_before, "signed {signed}");
        }
    }

    #[test]
    fn signed_refit_and_reindex_match_decode() {
        let mut seed = 0x1234_5678u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..200 {
            let src: HdrBlock = std::array::from_fn(|_| {
                std::array::from_fn(|_| super::super::f32_to_sf16((next() % 2001) as f32 / 500.0 - 2.0))
            });
            let base = super::super::encode_block(&src, 3, true);
            let f = unpack(&base).unwrap();
            let (b, err) = reindex(&f, &src, u64::MAX, true).unwrap();
            assert_eq!(err, sse(&src, &decode(&pack(&b), true), true));
            // Re-indexing the encoder's own endpoints never does worse than its indices.
            assert!(err <= sse(&src, &decode(&base, true), true));
            let (b, err) = refit(&f, &src, u64::MAX, true).unwrap();
            assert_eq!(err, sse(&src, &decode(&pack(&b), true), true));
        }
    }
}
