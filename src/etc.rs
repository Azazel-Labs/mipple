//! ETC2 RGB block codec (8 bytes per 4x4 block): the ETC1 individual and differential modes
//! plus ETC2's T, H and planar modes, selected by overflow of the differential color sums.
//!
//! Texels inside a block are addressed column-major (`p = x * 4 + y`), as the format stores its
//! selectors; [`Block`] is raster order, so reads and writes transpose.
//!
//! The encoder tries every mode and keeps the one with the lowest squared RGB error:
//! - ETC1 (both flips, individual and differential): the error splits into a chroma term that
//!   depends only on how far the base color is from the subblock mean, plus a luma term that
//!   depends only on the base's channel sum and the table. Candidate bases on the 444/555
//!   lattice are ranked with that split (one luma table per distinct sum), and the best few
//!   are scored exactly, with clamping. Differential pairs come from those lists, plus
//!   projections into the ±4 delta window.
//! - T and H: two clusters from a principal-axis split, colors from the cluster means, every
//!   distance, then alternating reassignment and refit, and (higher efforts) a ±1 descent.
//! - Planar: a least-squares plane per channel, then a search of its quantized neighborhood.
//!   Channels are independent, so each is searched on its own.
//!
//! ETC2 RGB A1 ([`decode_a1`], [`encode_block_a1`]) reuses the layout with bit 33 as an opaque
//! flag instead of the individual/differential bit (so there is no individual mode). Opaque
//! blocks decode as ETC2; in a punch-through block, selector 2 is transparent black (in
//! differential mode selector 0 also loses its modifier, leaving the base color and ±b), and
//! planar blocks are always opaque. Blocks with any texel below alpha 128 are punch-through:
//! differential with the transparent texels on selector 2 and the rest scored on the other
//! three, then T and H with the same mask.

use crate::surface::{Block, Surface};

/// ETC1 intensity modifier tables: selector 0 → +a, 1 → +b, 2 → −a, 3 → −b.
pub const MODIFIERS: [[i32; 2]; 8] = [
    [2, 8],
    [5, 17],
    [9, 29],
    [13, 42],
    [18, 60],
    [24, 80],
    [33, 106],
    [47, 183],
];

/// T and H mode distances.
pub const DISTANCES: [i32; 8] = [3, 6, 11, 16, 23, 32, 41, 64];

#[inline(always)]
fn modifier(table: usize, sel: usize) -> i32 {
    let m = MODIFIERS[table][sel & 1];
    if sel & 2 != 0 { -m } else { m }
}

#[inline(always)]
fn ext4(x: i32) -> i32 {
    x * 17
}

#[inline(always)]
fn ext5(x: i32) -> i32 {
    (x << 3) | (x >> 2)
}

#[inline(always)]
fn ext6(x: i32) -> i32 {
    (x << 2) | (x >> 4)
}

#[inline(always)]
fn ext7(x: i32) -> i32 {
    (x << 1) | (x >> 6)
}

#[inline(always)]
fn sext3(x: u32) -> i32 {
    ((x as i32) << 29) >> 29
}

/// Raster index of column-major texel `p`.
#[inline(always)]
fn raster(p: usize) -> usize {
    (p & 3) * 4 + (p >> 2)
}

/// Whether texel `p` is in the second subblock.
#[inline(always)]
fn second(p: usize, flip: bool) -> bool {
    if flip { p & 3 >= 2 } else { p >= 8 }
}

/// Which ETC2 mode a block uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Individual,
    Differential,
    T,
    H,
    Planar,
}

/// The mode an encoded block decodes in.
pub fn mode(raw: &[u8]) -> Mode {
    let hi = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    if hi & 2 == 0 {
        return Mode::Individual;
    }
    let r = (hi >> 27 & 31) as i32 + sext3(hi >> 24 & 7);
    let g = (hi >> 19 & 31) as i32 + sext3(hi >> 16 & 7);
    let b = (hi >> 11 & 31) as i32 + sext3(hi >> 8 & 7);
    if !(0..32).contains(&r) {
        Mode::T
    } else if !(0..32).contains(&g) {
        Mode::H
    } else if !(0..32).contains(&b) {
        Mode::Planar
    } else {
        Mode::Differential
    }
}

/// Decodes one 8-byte ETC2 RGB block (alpha 255).
pub fn decode(raw: &[u8], out: &mut Block) {
    let hi = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let lo = u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]);
    let put = |out: &mut Block, p: usize, c: [i32; 3]| {
        out[raster(p)] = [
            c[0].clamp(0, 255) as u8,
            c[1].clamp(0, 255) as u8,
            c[2].clamp(0, 255) as u8,
            255,
        ];
    };
    let field = |shift: u32, bits: u32| (hi >> shift & ((1 << bits) - 1)) as i32;
    if let Some((pal, flip)) = paints(raw) {
        for p in 0..16 {
            put(out, p, pal[usize::from(second(p, flip))][selector(lo, p)]);
        }
        return;
    }
    let o = [
        ext6(field(25, 6)),
        ext7(field(24, 1) << 6 | field(17, 6)),
        ext6(field(16, 1) << 5 | field(11, 2) << 3 | field(7, 3)),
    ];
    let h = [
        ext6(field(2, 5) << 1 | field(0, 1)),
        ext7((lo >> 25 & 127) as i32),
        ext6((lo >> 19 & 63) as i32),
    ];
    let v = [
        ext6((lo >> 13 & 63) as i32),
        ext7((lo >> 6 & 127) as i32),
        ext6((lo & 63) as i32),
    ];
    for p in 0..16 {
        let (x, y) = ((p >> 2) as i32, (p & 3) as i32);
        let c: [i32; 3] = std::array::from_fn(|k| (x * (h[k] - o[k]) + y * (v[k] - o[k]) + 4 * o[k] + 2) >> 2);
        put(out, p, c);
    }
}

/// Selector of column-major texel `p` in the low word.
#[inline(always)]
fn selector(lo: u32, p: usize) -> usize {
    ((lo >> (16 + p) & 1) << 1 | (lo >> p & 1)) as usize
}

/// Unclamped paint colors by subblock and selector.
type Paints = [[[i32; 3]; 4]; 2];

/// The four colors (unclamped) a texel can take in each subblock, by selector, and the flip bit, for
/// individual, differential, T and H blocks (T and H give both subblocks the same paint colors).
/// `None` for planar blocks, which have no selectors.
fn paints(raw: &[u8]) -> Option<(Paints, bool)> {
    let hi = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let field = |shift: u32, bits: u32| (hi >> shift & ((1 << bits) - 1)) as i32;
    let etc1 = |c1: [i32; 3], c2: [i32; 3]| {
        let tables = [(hi >> 5 & 7) as usize, (hi >> 2 & 7) as usize];
        let pal =
            |c: [i32; 3], t: usize| -> [[i32; 3]; 4] { std::array::from_fn(|sel| c.map(|v| v + modifier(t, sel))) };
        Some(([pal(c1, tables[0]), pal(c2, tables[1])], hi & 1 != 0))
    };
    match mode(raw) {
        Mode::Individual => etc1(
            [ext4(field(28, 4)), ext4(field(20, 4)), ext4(field(12, 4))],
            [ext4(field(24, 4)), ext4(field(16, 4)), ext4(field(8, 4))],
        ),
        Mode::Differential => {
            let base = [field(27, 5), field(19, 5), field(11, 5)];
            let delta = [sext3(hi >> 24 & 7), sext3(hi >> 16 & 7), sext3(hi >> 8 & 7)];
            etc1(
                base.map(ext5),
                [
                    ext5(base[0] + delta[0]),
                    ext5(base[1] + delta[1]),
                    ext5(base[2] + delta[2]),
                ],
            )
        }
        Mode::T => {
            let c1 = [field(27, 2) << 2 | field(24, 2), field(20, 4), field(16, 4)].map(ext4);
            let c2 = [field(12, 4), field(8, 4), field(4, 4)].map(ext4);
            let d = DISTANCES[(field(2, 2) << 1 | field(0, 1)) as usize];
            let paint = [c1, c2.map(|v| v + d), c2, c2.map(|v| v - d)];
            Some(([paint; 2], false))
        }
        Mode::H => {
            let q1 = [
                field(27, 4),
                field(24, 3) << 1 | field(20, 1),
                field(19, 1) << 3 | field(15, 3),
            ];
            let q2 = [field(11, 4), field(7, 4), field(3, 4)];
            let order = i32::from(pack444(q1) >= pack444(q2));
            let d = DISTANCES[(field(2, 1) << 2 | field(0, 1) << 1 | order) as usize];
            let (c1, c2) = (q1.map(ext4), q2.map(ext4));
            let paint = [
                c1.map(|v| v + d),
                c1.map(|v| v - d),
                c2.map(|v| v + d),
                c2.map(|v| v - d),
            ];
            Some(([paint; 2], false))
        }
        Mode::Planar => None,
    }
}

/// `raw` with its colors, mode, tables and flip kept and every selector re-chosen for `px` by
/// weighted squared RGB error (for RDO: a copy of another block's first four bytes). `None` for
/// planar blocks, whose last four bytes are colors.
pub fn reselect(raw: &[u8], px: &Block, w: &[u32; 4]) -> Option<[u8; 8]> {
    reselect_err(raw, px, w).map(|(b, _)| b)
}

/// [`reselect`], with the result's weighted R, G, B error ([`sse`] without the alpha term).
pub fn reselect_err(raw: &[u8], px: &Block, w: &[u32; 4]) -> Option<([u8; 8], u64)> {
    let (pal, flip) = paints(raw)?;
    let pal = pal.map(|s| s.map(|c| c.map(|v| v.clamp(0, 255))));
    let mut lo = 0u32;
    let mut err = 0u64;
    for p in 0..16 {
        let t = px[raster(p)];
        let mut best = (u32::MAX, 0u8);
        for (k, c) in pal[usize::from(second(p, flip))].iter().enumerate() {
            let d = |ch: usize| (i32::from(t[ch]) - c[ch]).unsigned_abs().pow(2) * w[ch];
            let e = d(0) + d(1) + d(2);
            if e < best.0 {
                best = (e, k as u8);
            }
        }
        lo |= set_selector(p, best.1);
        err += u64::from(best.0);
    }
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&raw[..4]);
    out[4..].copy_from_slice(&lo.to_be_bytes());
    Some((out, err))
}

/// An individual or differential block with `raw`'s flip and the selectors in `sels` (the last four
/// bytes of another block), its base colors and tables fit to `px` by least squares per table (for
/// RDO: a copy of another block's selectors). `None` when `raw` is a T, H or planar block.
pub fn refit_selectors(raw: &[u8], sels: &[u8], px: &Block) -> Option<[u8; 8]> {
    refit_selectors_err(raw, sels, px).map(|(b, _)| b)
}

/// [`refit_selectors`], with the result's squared R, G, B error (unweighted).
pub fn refit_selectors_err(raw: &[u8], sels: &[u8], px: &Block) -> Option<([u8; 8], u32)> {
    let flip = match mode(raw) {
        Mode::Individual | Mode::Differential => raw[3] & 1 != 0,
        _ => return None,
    };
    let lo = u32::from_be_bytes([sels[0], sels[1], sels[2], sels[3]]);
    // Per subblock and selector: texel count, and per channel the sum and sum of squares; a
    // painted value v then costs Σ x² − 2vΣ x + n v² over those texels.
    let mut n = [[0i32; 4]; 2];
    let mut s1 = [[[0i32; 3]; 4]; 2];
    let mut s2 = [[[0i32; 3]; 4]; 2];
    for p in 0..16 {
        let k = usize::from(second(p, flip));
        let q = selector(lo, p);
        let t = px[raster(p)];
        n[k][q] += 1;
        for c in 0..3 {
            let v = i32::from(t[c]);
            s1[k][q][c] += v;
            s2[k][q][c] += v * v;
        }
    }
    let best = |k: usize, bits: u32| -> Pick {
        // The eight tables side by side, so the loops run across tables.
        let max = (1 << bits) - 1;
        let tot: [i32; 3] = std::array::from_fn(|c| s1[k][0][c] + s1[k][1][c] + s1[k][2][c] + s1[k][3][c]);
        let m: [[i32; 8]; 4] = std::array::from_fn(|q| std::array::from_fn(|t| modifier(t, q)));
        let msum: [i32; 8] =
            std::array::from_fn(|t| n[k][0] * m[0][t] + n[k][1] * m[1][t] + n[k][2] * m[2][t] + n[k][3] * m[3][t]);
        let mut e = [0i32; 8];
        let mut q = [[0i32; 8]; 3];
        for c in 0..3 {
            for t in 0..8 {
                q[c][t] = (((tot[c] - msum[t]) * max + 4 * 255) / (8 * 255)).clamp(0, max);
                let col = if bits == 5 { ext5(q[c][t]) } else { ext4(q[c][t]) };
                for s in 0..4 {
                    let v = (col + m[s][t]).clamp(0, 255);
                    e[t] += s2[k][s][c] - 2 * v * s1[k][s][c] + n[k][s] * v * v;
                }
            }
        }
        let mut b = Pick {
            q: [0; 3],
            err: u32::MAX,
            table: 0,
        };
        for t in 0..8 {
            if (e[t] as u32) < b.err {
                b = Pick {
                    q: [q[0][t], q[1][t], q[2][t]],
                    err: e[t] as u32,
                    table: t as u8,
                };
            }
        }
        b
    };
    let ind = [best(0, 4), best(1, 4)];
    let dif = [best(0, 5), best(1, 5)];
    let fits = (0..3).all(|c| (-4..4).contains(&(dif[1].q[c] - dif[0].q[c])));
    let diff = fits && dif[0].err + dif[1].err <= ind[0].err + ind[1].err;
    let pick = if diff { dif } else { ind };
    let hi = etc1_header(flip, diff, pick);
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&hi.to_be_bytes());
    out[4..].copy_from_slice(&lo.to_be_bytes());
    Some((out, pick[0].err + pick[1].err))
}

#[inline(always)]
fn pack444(q: [i32; 3]) -> i32 {
    q[0] << 8 | q[1] << 4 | q[2]
}

/// Search settings for one effort level.
#[derive(Clone, Copy, Debug)]
struct Cfg {
    /// Chroma radius of the ETC1 candidate box (lattice steps).
    chroma: i32,
    /// Extra luma reach of the ETC1 candidate box.
    luma: i32,
    /// ETC1 candidates per subblock scored exactly.
    short: usize,
    /// ETC1 least-squares refits of the chosen base colors.
    refits: u32,
    /// Try T and H at all.
    th: bool,
    /// Principal-axis splits tried for T and H.
    splits: usize,
    /// Reassign/refit rounds for T and H.
    rounds: u32,
    /// ±1 descent passes for T and H.
    descent: u32,
    /// Planar neighborhood radius per parameter.
    planar: i32,
}

fn cfg(effort: u8) -> Cfg {
    let c = |chroma, luma, short, refits, th, splits, rounds, descent, planar| Cfg {
        chroma,
        luma,
        short,
        refits,
        th,
        splits,
        rounds,
        descent,
        planar,
    };
    match effort {
        0 => c(0, 1, 2, 0, false, 1, 0, 0, 0),
        1 => c(1, 1, 2, 1, true, 1, 1, 0, 1),
        2 => c(1, 1, 3, 1, true, 1, 1, 0, 1),
        3 => c(1, 2, 4, 1, true, 1, 2, 0, 1),
        4 => c(1, 2, 4, 2, true, 2, 2, 1, 1),
        5 => c(1, 2, 6, 2, true, 2, 2, 2, 1),
        6 => c(1, 3, 8, 2, true, 3, 3, 4, 2),
        7 => c(2, 3, 10, 3, true, 3, 3, 8, 2),
        8 => c(2, 3, 14, 3, true, 4, 4, 16, 2),
        _ => c(2, 4, 20, 4, true, 6, 4, 64, 3),
    }
}

/// Block texels, channel-major, column-major texel order.
type Px = [[i32; 16]; 3];
/// One subblock: 8 texels per channel.
type Sub = [[i32; 8]; 3];

fn texels(px: &Block) -> Px {
    std::array::from_fn(|c| std::array::from_fn(|p| i32::from(px[raster(p)][c])))
}

#[inline(always)]
fn subblock(px: &Px, flip: bool, s: bool) -> Sub {
    let mut out = [[0; 8]; 3];
    let mut n = 0;
    for p in 0..16 {
        if second(p, flip) == s {
            for c in 0..3 {
                out[c][n] = px[c][p];
            }
            n += 1;
        }
    }
    out
}

/// Exact ETC1 subblock error of base color `c` (8-bit) for every table: (best error, its table).
/// A table whose modifiers never clamp (all of `c` within its largest modifier of 0 and 255) gives
/// texel offset e = s − c the error |e|² + min over m of (3m² − 2mΣe), and the four modifiers
/// ±a, ±b reduce to a and b against |Σe|.
#[inline(always)]
fn sub_error(s: &Sub, c: [i32; 3]) -> (u32, u8) {
    let lo = c[0].min(c[1]).min(c[2]);
    let hi = c[0].max(c[1]).max(c[2]);
    let mut e2 = [0i32; 8];
    let mut ad = [0i32; 8];
    let mut sd = [0i32; 8];
    for i in 0..8 {
        let (dr, dg, db) = (s[0][i] - c[0], s[1][i] - c[1], s[2][i] - c[2]);
        e2[i] = dr * dr + dg * dg + db * db;
        sd[i] = dr + dg + db;
        ad[i] = sd[i].abs();
    }
    let base: i32 = e2.iter().sum();
    let mut best = (u32::MAX, 0u8);
    for t in 0..8 {
        let [a, b] = MODIFIERS[t];
        let total = if lo >= b && hi + b <= 255 {
            let (ka, kb) = (3 * a * a, 3 * b * b);
            let mut f = [0i32; 8];
            for i in 0..8 {
                f[i] = (ka - 2 * a * ad[i]).min(kb - 2 * b * ad[i]);
            }
            base + f.iter().sum::<i32>()
        } else {
            let mut e = [i32::MAX; 8];
            for sel in 0..4 {
                let m = modifier(t, sel);
                if lo + m >= 0 && hi + m <= 255 {
                    // This modifier clamps no channel: |e − m|² = |e|² − 2mΣe + 3m².
                    let k = 3 * m * m;
                    for i in 0..8 {
                        e[i] = e[i].min(e2[i] - 2 * m * sd[i] + k);
                    }
                    continue;
                }
                let (r, g, b) = (
                    (c[0] + m).clamp(0, 255),
                    (c[1] + m).clamp(0, 255),
                    (c[2] + m).clamp(0, 255),
                );
                for i in 0..8 {
                    let (dr, dg, db) = (s[0][i] - r, s[1][i] - g, s[2][i] - b);
                    e[i] = e[i].min(dr * dr + dg * dg + db * db);
                }
            }
            e.iter().sum::<i32>()
        } as u32;
        if total < best.0 {
            best = (total, t as u8);
        }
    }
    best
}

/// Best selector per subblock texel for base `c` and `table`.
#[inline(always)]
fn sub_selectors(s: &Sub, c: [i32; 3], table: usize) -> [u8; 8] {
    std::array::from_fn(|i| {
        let mut best = (i32::MAX, 0u8);
        for sel in 0..4 {
            let m = modifier(table, sel);
            let e: i32 = (0..3).map(|k| (s[k][i] - (c[k] + m).clamp(0, 255)).pow(2)).sum();
            if e < best.0 {
                best = (e, sel as u8);
            }
        }
        best.1
    })
}

/// Ranks base colors for one subblock with the chroma/luma split of the unclamped error.
struct Ranker {
    sum: [i32; 3],
    luma: [i32; 8],
    /// Luma term per channel sum, `lo..lo + memo.len()`, `-1` until computed.
    lo: i32,
    memo: [i32; 512],
}

impl Ranker {
    #[inline(always)]
    fn new(s: &Sub) -> Self {
        Self {
            sum: std::array::from_fn(|c| s[c].iter().sum()),
            luma: std::array::from_fn(|i| s[0][i] + s[1][i] + s[2][i]),
            lo: 0,
            memo: [-1; 512],
        }
    }

    /// Resets the memo for channel sums in `lo..=hi`.
    fn window(&mut self, lo: i32, hi: i32) {
        self.lo = lo;
        let n = ((hi - lo + 1) as usize).min(self.memo.len());
        self.memo[..n].fill(-1);
    }

    /// Smallest Σ (s_i − 3m_i)² over tables for base channel sum `l`.
    #[inline(always)]
    fn luma_term(&mut self, l: i32) -> i32 {
        let k = (l - self.lo) as usize;
        if k < self.memo.len() && self.memo[k] >= 0 {
            return self.memo[k];
        }
        let mut best = i32::MAX;
        // The modifiers come in ± pairs: the nearest is a or b against |s_i − l|.
        let ad: [i32; 8] = std::array::from_fn(|i| (self.luma[i] - l).abs());
        for [a, b] in MODIFIERS {
            let (a3, b3) = (3 * a, 3 * b);
            let mut e = [0i32; 8];
            for i in 0..8 {
                let (x, y) = (ad[i] - a3, ad[i] - b3);
                e[i] = (x * x).min(y * y);
            }
            best = best.min(e.iter().sum());
        }
        if k < self.memo.len() {
            self.memo[k] = best;
        }
        best
    }

    /// 24× the unclamped error of base `c`, up to a constant.
    #[inline(always)]
    fn score(&mut self, c: [i32; 3]) -> i64 {
        let d: [i64; 3] = std::array::from_fn(|k| i64::from(self.sum[k] - 8 * c[k]));
        let dot = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let s = d[0] + d[1] + d[2];
        3 * dot - s * s + 8 * i64::from(self.luma_term(c[0] + c[1] + c[2]))
    }
}

/// A short list of lattice colors with their scores, best first.
#[derive(Clone, Copy)]
struct Short<const N: usize> {
    q: [[i32; 3]; N],
    score: [i64; N],
    len: usize,
}

impl<const N: usize> Short<N> {
    fn new() -> Self {
        Self {
            q: [[0; 3]; N],
            score: [0; N],
            len: 0,
        }
    }

    #[inline(always)]
    fn push(&mut self, cap: usize, q: [i32; 3], score: i64) {
        let cap = cap.min(N);
        if self.len == cap && score >= self.score[cap - 1] {
            return;
        }
        let mut i = self.len.min(cap - 1);
        if self.len < cap {
            self.len += 1;
        }
        while i > 0 && self.score[i - 1] > score {
            self.q[i] = self.q[i - 1];
            self.score[i] = self.score[i - 1];
            i -= 1;
        }
        self.q[i] = q;
        self.score[i] = score;
    }
}

const SHORT: usize = 24;

/// Ranks the lattice (`bits` per channel) around subblock mean and keeps the best `cap`.
#[inline(always)]
fn shortlist(rank: &mut Ranker, bits: u32, cfg: &Cfg) -> Short<SHORT> {
    let max = (1 << bits) - 1;
    let ext = if bits == 4 { ext4 } else { ext5 };
    let q0: [i32; 3] = std::array::from_fn(|c| ((rank.sum[c] * max + 4 * 255) / (8 * 255)).clamp(0, max));
    let reach = cfg.chroma + cfg.luma;
    let lo: [i32; 3] = q0.map(|q| (q - reach).max(0));
    let hi: [i32; 3] = q0.map(|q| (q + reach).min(max));
    rank.window(
        ext(lo[0]) + ext(lo[1]) + ext(lo[2]),
        ext(hi[0]) + ext(hi[1]) + ext(hi[2]),
    );
    let mut out = Short::new();
    for r in lo[0]..=hi[0] {
        for g in lo[1]..=hi[1] {
            for b in lo[2]..=hi[2] {
                let d = [r - q0[0], g - q0[1], b - q0[2]];
                let spread = d.iter().max().unwrap() - d.iter().min().unwrap();
                if spread > 2 * cfg.chroma {
                    continue;
                }
                let q = [r, g, b];
                out.push(cfg.short, q, rank.score(q.map(ext)));
            }
        }
    }
    out
}

/// An ETC1 subblock choice.
#[derive(Clone, Copy, Debug)]
struct Pick {
    q: [i32; 3],
    err: u32,
    table: u8,
}

/// A lattice color `q` (`bits` per channel) with its exact subblock error and table.
#[inline(always)]
fn exact_pick(s: &Sub, q: [i32; 3], bits: u32) -> Pick {
    let c = if bits == 4 { q.map(ext4) } else { q.map(ext5) };
    let (err, table) = sub_error(s, c);
    Pick { q, err, table }
}

/// The ETC1 half of the search for one flip: best individual and differential encodings.
#[inline(always)]
fn etc1_flip(px: &Px, flip: bool, cfg: &Cfg, best: &mut Best, individual: bool) {
    let subs = [subblock(px, flip, false), subblock(px, flip, true)];
    let mut ranks = [Ranker::new(&subs[0]), Ranker::new(&subs[1])];

    // Individual: each subblock on its own.
    let mut ind = [Pick {
        q: [0; 3],
        err: u32::MAX,
        table: 0,
    }; 2];
    for k in (0..2).filter(|_| individual) {
        let list = shortlist(&mut ranks[k], 4, cfg);
        for i in 0..list.len {
            let p = exact_pick(&subs[k], list.q[i], 4);
            if p.err < ind[k].err {
                ind[k] = p;
            }
        }
        for _ in 0..cfg.refits {
            let c = ind[k].q.map(ext4);
            let q = refit(&subs[k], c, ind[k].table as usize, 4);
            if q == ind[k].q {
                break;
            }
            let p = exact_pick(&subs[k], q, 4);
            if p.err >= ind[k].err {
                break;
            }
            ind[k] = p;
        }
    }
    let err = u64::from(ind[0].err) + u64::from(ind[1].err);
    if individual && err < best.err {
        best.set(err, pack_etc1(&subs, flip, false, ind));
    }

    // Differential: pairs within the delta window.
    let lists = [shortlist(&mut ranks[0], 5, cfg), shortlist(&mut ranks[1], 5, cfg)];
    let mut picks: [[Pick; SHORT]; 2] = [[Pick {
        q: [0; 3],
        err: u32::MAX,
        table: 0,
    }; SHORT]; 2];
    for k in 0..2 {
        for i in 0..lists[k].len {
            picks[k][i] = exact_pick(&subs[k], lists[k].q[i], 5);
        }
    }
    let ok = |a: [i32; 3], b: [i32; 3]| (0..3).all(|c| (-4..=3).contains(&(b[c] - a[c])));
    let mut pair: Option<[Pick; 2]> = None;
    let mut pair_err = u64::MAX;
    let mut consider = |a: Pick, b: Pick, pair: &mut Option<[Pick; 2]>| {
        let e = u64::from(a.err) + u64::from(b.err);
        if e < pair_err && ok(a.q, b.q) {
            pair_err = e;
            *pair = Some([a, b]);
        }
    };
    for a in &picks[0][..lists[0].len] {
        for b in &picks[1][..lists[1].len] {
            consider(*a, *b, &mut pair);
        }
    }
    // Projections of each side's best few into the other side's window.
    let proj = lists[0].len.min(lists[1].len).min(3);
    for i in 0..proj {
        let a = picks[0][i];
        for j in 0..proj {
            let qb: [i32; 3] = std::array::from_fn(|c| picks[1][j].q[c].clamp(a.q[c] - 4, a.q[c] + 3));
            if qb != picks[1][j].q {
                consider(a, exact_pick(&subs[1], qb, 5), &mut pair);
            }
        }
        let b = picks[1][i];
        for j in 0..proj {
            let qa: [i32; 3] = std::array::from_fn(|c| picks[0][j].q[c].clamp(b.q[c] - 3, b.q[c] + 4));
            if qa != picks[0][j].q {
                consider(exact_pick(&subs[0], qa, 5), b, &mut pair);
            }
        }
    }
    let Some(mut pair) = pair else { return };
    for _ in 0..cfg.refits {
        let mut improved = false;
        for k in 0..2 {
            let q = refit(&subs[k], pair[k].q.map(ext5), pair[k].table as usize, 5);
            let other = pair[1 - k].q;
            // Keep the pair inside the delta window.
            let q: [i32; 3] = if k == 0 {
                std::array::from_fn(|c| q[c].clamp(other[c] - 3, other[c] + 4))
            } else {
                std::array::from_fn(|c| q[c].clamp(other[c] - 4, other[c] + 3))
            };
            if q != pair[k].q {
                let p = exact_pick(&subs[k], q, 5);
                if p.err < pair[k].err {
                    pair[k] = p;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
    let err = u64::from(pair[0].err) + u64::from(pair[1].err);
    if err < best.err {
        best.set(err, pack_etc1(&subs, flip, true, pair));
    }
}

/// Least-squares base for a subblock given its selectors, quantized to `bits`.
#[inline(always)]
fn refit(s: &Sub, c: [i32; 3], table: usize, bits: u32) -> [i32; 3] {
    let sels = sub_selectors(s, c, table);
    let msum: i32 = sels.iter().map(|&k| modifier(table, k as usize)).sum();
    let max = (1 << bits) - 1;
    std::array::from_fn(|k| {
        let t = s[k].iter().sum::<i32>() - msum;
        ((t * max + 4 * 255) / (8 * 255)).clamp(0, max)
    })
}

fn etc1_header(flip: bool, diff: bool, picks: [Pick; 2]) -> u32 {
    let mut hi: u32 = u32::from(picks[0].table) << 5 | u32::from(picks[1].table) << 2 | u32::from(flip);
    if diff {
        let (a, b) = (picks[0].q, picks[1].q);
        hi |= 2;
        for (c, shift) in [27, 19, 11].into_iter().enumerate() {
            hi |= (a[c] as u32) << shift | ((b[c] - a[c]) as u32 & 7) << (shift - 3);
        }
    } else {
        for (c, shift) in [28, 20, 12].into_iter().enumerate() {
            hi |= (picks[0].q[c] as u32) << shift | (picks[1].q[c] as u32) << (shift - 4);
        }
    }
    hi
}

#[inline(always)]
fn pack_etc1(subs: &[Sub; 2], flip: bool, diff: bool, picks: [Pick; 2]) -> u64 {
    let hi = etc1_header(flip, diff, picks);
    let ext = if diff { ext5 } else { ext4 };
    let mut lo = 0u32;
    for k in 0..2 {
        let sels = sub_selectors(&subs[k], picks[k].q.map(ext), picks[k].table as usize);
        let mut n = 0;
        for p in 0..16 {
            if second(p, flip) == (k == 1) {
                lo |= set_selector(p, sels[n]);
                n += 1;
            }
        }
    }
    u64::from(hi) << 32 | u64::from(lo)
}

#[inline(always)]
fn set_selector(p: usize, s: u8) -> u32 {
    u32::from(s >> 1 & 1) << (16 + p) | u32::from(s & 1) << p
}

/// Best encoding so far.
struct Best {
    err: u64,
    bits: u64,
}

impl Best {
    fn set(&mut self, err: u64, bits: u64) {
        self.err = err;
        self.bits = bits;
    }
}

/// Error of the 16 texels against the nearest of four paint colors, and the nearest of each texel
/// (the first on ties).
#[inline(always)]
fn paint_error_sels(px: &Px, paint: &[[i32; 3]; 4]) -> (u32, [u8; 16]) {
    let mut e = [i32::MAX; 16];
    let mut sel = [0i32; 16];
    for (k, c) in paint.iter().enumerate() {
        let c = c.map(|v| v.clamp(0, 255));
        for p in 0..16 {
            let (dr, dg, db) = (px[0][p] - c[0], px[1][p] - c[1], px[2][p] - c[2]);
            let d = dr * dr + dg * dg + db * db;
            if d < e[p] {
                e[p] = d;
                sel[p] = k as i32;
            }
        }
    }
    (e.iter().sum::<i32>() as u32, sel.map(|k| k as u8))
}

/// [`paint_error_sels`] for an A1 punch-through block: paint color 2 is transparent, so opaque
/// texels take the nearest of the other three and transparent texels (selector 2) cost nothing.
#[inline(always)]
fn punch_error_sels(px: &Px, paint: &[[i32; 3]; 4], opaque: u16) -> (u32, [u8; 16]) {
    let mut e = [i32::MAX; 16];
    let mut sel = [0i32; 16];
    for k in [0, 1, 3] {
        let c = paint[k].map(|v| v.clamp(0, 255));
        for p in 0..16 {
            let (dr, dg, db) = (px[0][p] - c[0], px[1][p] - c[1], px[2][p] - c[2]);
            let d = dr * dr + dg * dg + db * db;
            if d < e[p] {
                e[p] = d;
                sel[p] = k as i32;
            }
        }
    }
    let mut total = 0;
    for p in 0..16 {
        if opaque >> p & 1 == 0 {
            sel[p] = 2;
        } else {
            total += e[p];
        }
    }
    (total as u32, sel.map(|k| k as u8))
}

/// T or H mode colors (444) and distance index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Th {
    h: bool,
    c1: [i32; 3],
    c2: [i32; 3],
    d: usize,
    /// ETC2 A1 punch-through: the opaque texels (bit p for texel p); paint color 2 is transparent.
    punch: Option<u16>,
}

impl Th {
    #[inline(always)]
    fn paint(&self) -> [[i32; 3]; 4] {
        let (c1, c2, d) = (self.c1.map(ext4), self.c2.map(ext4), DISTANCES[self.d]);
        if self.h {
            [
                c1.map(|v| v + d),
                c1.map(|v| v - d),
                c2.map(|v| v + d),
                c2.map(|v| v - d),
            ]
        } else {
            [c1, c2.map(|v| v + d), c2, c2.map(|v| v - d)]
        }
    }

    /// H mode stores the distance's low bit in the color order, so equal colors need an odd one.
    /// Punch-through H blocks cannot swap the colors to match (which one loses its +d color to
    /// transparency depends on the order), so the order must already match.
    #[inline(always)]
    fn valid(&self) -> bool {
        if self.h && self.punch.is_some() {
            return usize::from(pack444(self.c1) >= pack444(self.c2)) == self.d & 1;
        }
        !(self.h && self.c1 == self.c2 && self.d & 1 == 0)
    }

    /// Error of the block (`u32::MAX` if it cannot be packed) and the selectors of its texels.
    #[inline(always)]
    fn error_sels(&self, px: &Px) -> (u32, [u8; 16]) {
        let (e, sels) = match self.punch {
            None => paint_error_sels(px, &self.paint()),
            Some(opaque) => punch_error_sels(px, &self.paint(), opaque),
        };
        (if self.valid() { e } else { u32::MAX }, sels)
    }

    /// Refits both colors to the texels each paint color won (`sels`, this block's selectors).
    #[inline(always)]
    fn refit_sels(&self, px: &Px, sels: &[u8; 16]) -> Th {
        let d = DISTANCES[self.d];
        // Offset of each paint color from its group's base color, and the group.
        let (off, group): ([i32; 4], [usize; 4]) = if self.h {
            ([d, -d, d, -d], [0, 0, 1, 1])
        } else {
            ([0, d, 0, -d], [0, 1, 1, 1])
        };
        let mut sum = [[0i32; 3]; 2];
        let mut n = [0i32; 2];
        for p in 0..16 {
            if self.punch.is_some_and(|o| o >> p & 1 == 0) {
                continue;
            }
            let s = sels[p] as usize;
            let g = group[s];
            n[g] += 1;
            for c in 0..3 {
                sum[g][c] += px[c][p] - off[s];
            }
        }
        let q = |g: usize, old: [i32; 3]| {
            if n[g] == 0 {
                old
            } else {
                std::array::from_fn(|c| ((sum[g][c] * 2 + n[g] * 17) / (n[g] * 34)).clamp(0, 15))
            }
        };
        Th {
            c1: q(0, self.c1),
            c2: q(1, self.c2),
            ..*self
        }
    }

    fn pack(&self, px: &Px) -> u64 {
        let mut s = *self;
        let mut flip_sel = 0u8;
        // Bit 33: differential for ETC2, opaque for A1.
        let mut hi: u32 = if s.punch.is_some() { 0 } else { 2 };
        if s.h {
            let order = usize::from(pack444(s.c1) >= pack444(s.c2));
            if order != s.d & 1 {
                std::mem::swap(&mut s.c1, &mut s.c2);
                flip_sel = 2;
            }
            let (c1, c2) = (s.c1.map(|v| v as u32), s.c2.map(|v| v as u32));
            hi |= c1[0] << 27 | (c1[1] >> 1) << 24 | (c1[1] & 1) << 20 | (c1[2] >> 3) << 19 | (c1[2] & 7) << 15;
            hi |= c2[0] << 11 | c2[1] << 7 | c2[2] << 3;
            hi |= ((s.d as u32) >> 2) << 2 | ((s.d as u32) >> 1 & 1);
            // Red must not overflow; green must.
            if c1[0] as i32 + sext3(c1[1] >> 1) < 0 {
                hi |= 1 << 31;
            }
            let (t, u) = ((c1[1] & 1) << 1 | c1[2] >> 3, (c1[2] >> 1) & 3);
            hi |= if t + u >= 4 { 7 << 21 } else { 1 << 18 };
        } else {
            let (c1, c2) = (s.c1.map(|v| v as u32), s.c2.map(|v| v as u32));
            hi |= (c1[0] >> 2) << 27 | (c1[0] & 3) << 24 | c1[1] << 20 | c1[2] << 16;
            hi |= c2[0] << 12 | c2[1] << 8 | c2[2] << 4;
            hi |= ((s.d as u32) >> 1) << 2 | (s.d as u32 & 1);
            // Red must overflow.
            let (t, u) = (c1[0] >> 2, c1[0] & 3);
            hi |= if t + u >= 4 { 7 << 29 } else { 1 << 26 };
        }
        let sels = self.error_sels(px).1;
        let mut lo = 0;
        for (p, &k) in sels.iter().enumerate() {
            lo |= set_selector(p, k ^ flip_sel);
        }
        u64::from(hi) << 32 | u64::from(lo)
    }
}

/// Splits the texels in two along the principal axis; returns the best `n` splits as group
/// masks (bit p set: texel p in the second group).
fn splits(px: &Px, n: usize) -> ([u16; 8], usize) {
    let mean: [f32; 3] = std::array::from_fn(|c| px[c].iter().sum::<i32>() as f32 / 16.0);
    let mut cov = [[0f32; 3]; 3];
    for p in 0..16 {
        let d: [f32; 3] = std::array::from_fn(|c| px[c][p] as f32 - mean[c]);
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j];
            }
        }
    }
    let mut v = [1f32, 1.0, 1.0];
    for _ in 0..4 {
        let w: [f32; 3] = std::array::from_fn(|i| cov[i][0] * v[0] + cov[i][1] * v[1] + cov[i][2] * v[2]);
        let m = w.iter().fold(0f32, |a, &x| a.max(x.abs()));
        if m < 1e-6 {
            break;
        }
        v = w.map(|x| x / m);
    }
    let mut order: [(f32, u8); 16] = std::array::from_fn(|p| {
        let proj = (0..3).map(|c| px[c][p] as f32 * v[c]).sum::<f32>();
        (proj, p as u8)
    });
    order.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    // Within-group squared error of each split point, from prefix sums.
    let mut total = [0i64; 3];
    let mut total2 = 0i64;
    for p in 0..16 {
        for c in 0..3 {
            total[c] += i64::from(px[c][p]);
            total2 += i64::from(px[c][p]).pow(2);
        }
    }
    let mut pre = [0i64; 3];
    let mut cand: [(i64, u16); 15] = [(0, 0); 15];
    let mut mask = 0u16;
    for k in 1..16 {
        let p = order[k - 1].1 as usize;
        mask |= 1 << p;
        for c in 0..3 {
            pre[c] += i64::from(px[c][p]);
        }
        let (na, nb) = (k as i64, 16 - k as i64);
        // Σ|p|² − |S_a|²/n_a − |S_b|²/n_b, scaled by n_a·n_b.
        let sa: i64 = pre.iter().map(|x| x * x).sum();
        let sb: i64 = (0..3).map(|c| (total[c] - pre[c]).pow(2)).sum();
        cand[k - 1] = (total2 * na * nb - sa * nb - sb * na, !mask);
    }
    cand.sort_unstable_by_key(|c| c.0);
    let mut out = [0u16; 8];
    let n = n.min(8);
    for i in 0..n {
        out[i] = cand[i].1;
    }
    (out, n)
}

/// Mean (444) of the texels in `include` on one side of `mask`.
#[inline(always)]
fn group_mean444(px: &Px, mask: u16, second: bool, include: u16) -> [i32; 3] {
    let mut sum = [0i32; 3];
    let mut n = 0;
    for p in 0..16 {
        if (mask >> p & 1 != 0) == second && include >> p & 1 != 0 {
            n += 1;
            for c in 0..3 {
                sum[c] += px[c][p];
            }
        }
    }
    if n == 0 {
        return [0; 3];
    }
    sum.map(|s| ((s * 2 + n * 17) / (n * 34)).clamp(0, 15))
}

/// T and H search; `punch` (the opaque texels) for an A1 punch-through block.
#[inline(always)]
fn th_search(px: &Px, cfg: &Cfg, best: &mut Best, punch: Option<u16>) {
    let include = punch.unwrap_or(u16::MAX);
    // Transparent texels take the opaque mean, so they do not steer the split.
    let filled;
    let split_px = match punch {
        None => px,
        Some(opaque) => {
            filled = fill_transparent(px, opaque);
            &filled
        }
    };
    let (masks, n) = splits(split_px, cfg.splits);
    for h in [false, true] {
        let mut top = (
            u32::MAX,
            Th {
                h,
                c1: [0; 3],
                c2: [0; 3],
                d: 0,
                punch,
            },
        );
        for &mask in &masks[..n] {
            let (ga, gb) = (
                group_mean444(px, mask, false, include),
                group_mean444(px, mask, true, include),
            );
            // H's color order is free (packing swaps), except in punch-through blocks.
            let both = [(ga, gb), (gb, ga)];
            let orders = if h && punch.is_none() { &both[..1] } else { &both[..] };
            for &(c1, c2) in orders {
                for d in 0..8 {
                    let mut th = Th { h, c1, c2, d, punch };
                    let (mut e, mut sels) = th.error_sels(px);
                    for _ in 0..cfg.rounds {
                        let next = th.refit_sels(px, &sels);
                        if next == th {
                            break;
                        }
                        let (ne, ns) = next.error_sels(px);
                        if ne >= e {
                            break;
                        }
                        (th, e, sels) = (next, ne, ns);
                    }
                    if e < top.0 {
                        top = (e, th);
                    }
                }
            }
        }
        // ±1 descent over the six color components and the distance.
        for _ in 0..cfg.descent {
            let cur = top;
            descent_pass(px, cur.1, &mut top);
            if top.1 == cur.1 {
                break;
            }
        }
        if u64::from(top.0) < best.err {
            best.set(u64::from(top.0), top.1.pack(px));
        }
    }
}

/// Per texel, the smallest squared distance to the paint colors one T/H color makes ([`Th::paint`]):
/// group 0 those of c1, group 1 those of c2. Punch-through blocks drop paint color 2 (transparent).
#[inline(always)]
fn group_min(px: &Px, c: [i32; 3], d: i32, h: bool, g: usize, punch: bool) -> [i32; 16] {
    // Multiples of the distance added to the color.
    let offs: &[i32] = match (h, g, punch) {
        (false, 0, _) => &[0],
        (false, _, false) => &[1, 0, -1],
        (false, _, true) => &[1, -1],
        (true, 1, true) => &[-1],
        (true, _, _) => &[1, -1],
    };
    let c = c.map(ext4);
    let mut e = [i32::MAX; 16];
    for &o in offs {
        let col = c.map(|v| (v + o * d).clamp(0, 255));
        for p in 0..16 {
            let (dr, dg, db) = (px[0][p] - col[0], px[1][p] - col[1], px[2][p] - col[2]);
            e[p] = e[p].min(dr * dr + dg * dg + db * db);
        }
    }
    e
}

/// The error of a T/H block ([`Th::error_sels`]) from its two groups' [`group_min`]s.
#[inline(always)]
fn group_error(a: &[i32; 16], b: &[i32; 16], opaque: u16) -> u32 {
    let mut e = 0;
    for p in 0..16 {
        let m = a[p].min(b[p]);
        e += if opaque >> p & 1 != 0 { m } else { 0 };
    }
    e as u32
}

/// One ±1 descent pass around `cur` (as in [`th_search`]), each group's per-texel minimum
/// computed once per distance: a move changes one color, so the other group is reused.
#[inline(always)]
fn descent_pass(px: &Px, cur: Th, top: &mut (u32, Th)) {
    let (h, punch) = (cur.h, cur.punch.is_some());
    let opaque = cur.punch.unwrap_or(u16::MAX);
    let d0 = cur.d.saturating_sub(1);
    let d1 = (cur.d + 2).min(8);
    let mut ga = [[0i32; 16]; 3];
    let mut gb = [[0i32; 16]; 3];
    for d in d0..d1 {
        ga[d - d0] = group_min(px, cur.c1, DISTANCES[d], h, 0, punch);
        gb[d - d0] = group_min(px, cur.c2, DISTANCES[d], h, 1, punch);
    }
    for k in 0..6 {
        for step in [-1, 1] {
            let mut th = cur;
            let v = if k < 3 { &mut th.c1[k] } else { &mut th.c2[k - 3] };
            *v += step;
            if !(0..16).contains(v) {
                continue;
            }
            for d in d0..d1 {
                let th = Th { d, ..th };
                let e = if !th.valid() {
                    u32::MAX
                } else if k < 3 {
                    group_error(&group_min(px, th.c1, DISTANCES[d], h, 0, punch), &gb[d - d0], opaque)
                } else {
                    group_error(&ga[d - d0], &group_min(px, th.c2, DISTANCES[d], h, 1, punch), opaque)
                };
                if e < top.0 {
                    *top = (e, th);
                }
            }
        }
    }
}

/// `px` with each transparent texel (not in `opaque`) set to the mean of the opaque ones.
fn fill_transparent(px: &Px, opaque: u16) -> Px {
    let n = opaque.count_ones().max(1) as i32;
    let mut out = *px;
    for c in 0..3 {
        let mean = (0..16).filter(|&p| opaque >> p & 1 != 0).map(|p| px[c][p]).sum::<i32>() / n;
        for p in (0..16).filter(|&p| opaque >> p & 1 == 0) {
            out[c][p] = mean;
        }
    }
    out
}

/// Least-squares plane weights: [O, H, V] = W · texels, for the basis (1 − x/4 − y/4, x/4, y/4).
fn plane_weights() -> &'static [[f32; 16]; 3] {
    static W: std::sync::LazyLock<[[f32; 16]; 3]> = std::sync::LazyLock::new(|| {
        let basis = |p: usize| {
            let (x, y) = ((p >> 2) as f64 / 4.0, (p & 3) as f64 / 4.0);
            [1.0 - x - y, x, y]
        };
        let mut g = [[0f64; 3]; 3];
        for p in 0..16 {
            let b = basis(p);
            for i in 0..3 {
                for j in 0..3 {
                    g[i][j] += b[i] * b[j];
                }
            }
        }
        let inv = invert3(g);
        std::array::from_fn(|i| {
            std::array::from_fn(|p| {
                let b = basis(p);
                (0..3).map(|j| inv[i][j] * b[j]).sum::<f64>() as f32
            })
        })
    });
    &W
}

fn invert3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let c = |i: usize, j: usize| {
        let (i1, i2, j1, j2) = ((i + 1) % 3, (i + 2) % 3, (j + 1) % 3, (j + 2) % 3);
        m[j1][i1] * m[j2][i2] - m[j1][i2] * m[j2][i1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(1, 0) + m[0][2] * c(2, 0);
    std::array::from_fn(|i| std::array::from_fn(|j| c(i, j) / det))
}

/// Squared error of one planar channel with expanded origin, horizontal and vertical values.
#[inline(always)]
fn plane_error(v: &[i32; 16], o: i32, h: i32, w: i32) -> i32 {
    let mut e = 0;
    for p in 0..16 {
        let (x, y) = ((p >> 2) as i32, (p & 3) as i32);
        let d = ((x * (h - o) + y * (w - o) + 4 * o + 2) >> 2).clamp(0, 255) - v[p];
        e += d * d;
    }
    e
}

#[inline(always)]
fn planar_search(px: &Px, cfg: &Cfg, best: &mut Best) {
    let w = plane_weights();
    let mut q = [[0i32; 3]; 3]; // [channel][O, H, V]
    let mut err = 0u64;
    for c in 0..3 {
        let bits = if c == 1 { 7 } else { 6 };
        let max = (1 << bits) - 1;
        let ext = if c == 1 { ext7 } else { ext6 };
        let fit: [i32; 3] = std::array::from_fn(|k| {
            let v: f32 = (0..16).map(|p| w[k][p] * px[c][p] as f32).sum();
            ((v * max as f32 / 255.0).round() as i32).clamp(0, max)
        });
        let r = cfg.planar;
        let mut top = (i32::MAX, fit);
        for o in (fit[0] - r).max(0)..=(fit[0] + r).min(max) {
            for h in (fit[1] - r).max(0)..=(fit[1] + r).min(max) {
                for v in (fit[2] - r).max(0)..=(fit[2] + r).min(max) {
                    let e = plane_error(&px[c], ext(o), ext(h), ext(v));
                    if e < top.0 {
                        top = (e, [o, h, v]);
                    }
                }
            }
        }
        q[c] = top.1;
        err += top.0 as u64;
    }
    if err < best.err {
        best.set(err, pack_planar(q));
    }
}

fn pack_planar(q: [[i32; 3]; 3]) -> u64 {
    let u = q.map(|c| c.map(|v| v as u32));
    let ([r0, rh, rv], [g0, gh, gv], [b0, bh, bv]) = (u[0], u[1], u[2]);
    let mut hi: u32 = 2;
    hi |= r0 << 25 | (g0 >> 6) << 24 | (g0 & 63) << 17 | (b0 >> 5) << 16 | (b0 >> 3 & 3) << 11 | (b0 & 7) << 7;
    hi |= (rh >> 1) << 2 | (rh & 1);
    // Red and green must not overflow; blue must.
    if (r0 >> 2) as i32 + sext3((r0 & 3) << 1 | g0 >> 6) < 0 {
        hi |= 1 << 31;
    }
    if (g0 >> 2 & 15) as i32 + sext3((g0 & 3) << 1 | b0 >> 5) < 0 {
        hi |= 1 << 23;
    }
    let (t, s) = (b0 >> 3 & 3, b0 >> 1 & 3);
    hi |= if t + s >= 4 { 7 << 13 } else { 1 << 10 };
    let lo = gh << 25 | bh << 19 | rv << 13 | gv << 6 | bv;
    u64::from(hi) << 32 | u64::from(lo)
}

#[inline(always)]
fn encode_px(px: &Px, cfg: &Cfg) -> [u8; 8] {
    let mut best = Best { err: u64::MAX, bits: 0 };
    planar_search(px, cfg, &mut best);
    if best.err > 0 {
        etc1_flip(px, false, cfg, &mut best, true);
    }
    if best.err > 0 {
        etc1_flip(px, true, cfg, &mut best, true);
    }
    if cfg.th && best.err > 0 {
        th_search(px, cfg, &mut best, None);
    }
    best.bits.to_be_bytes()
}

/// Encodes one block.
pub fn encode_block(px: &Block, effort: u8) -> [u8; 8] {
    encode_px(&texels(px), &cfg(effort))
}

/// Squared RGB error of an encoded block against its source.
pub fn block_error(raw: &[u8], px: &Block) -> u32 {
    let mut out = [[0u8; 4]; 16];
    decode(raw, &mut out);
    px.iter()
        .zip(&out)
        .map(|(a, b)| (0..3).map(|c| u32::from(a[c].abs_diff(b[c])).pow(2)).sum::<u32>())
        .sum()
}

/// Weighted squared error (R, G, B, A by `w`) of an ETC2 RGB block, or with `a1` an ETC2 RGB A1
/// block, against `px`: the value of decoding it and taking [`crate::surface::block_sse`], from
/// the paint colors without writing the decoded block (for RDO).
pub fn sse(raw: &[u8], px: &Block, w: &[u32; 4], a1: bool) -> u64 {
    let pp = if a1 {
        a1_paints(raw)
    } else {
        paints(raw).map(|(p, f)| (p, f, false))
    };
    let Some((pal, flip, punch)) = pp else {
        // Planar (A1 planar blocks are opaque ETC2 ones).
        let mut out = [[0u8; 4]; 16];
        decode(&as_differential(raw), &mut out);
        return crate::surface::block_sse(px, &out, w);
    };
    let lo = u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]);
    let pal = pal.map(|s| s.map(|c| c.map(|v| v.clamp(0, 255))));
    let w = w.map(u64::from);
    let mut e = 0u64;
    for p in 0..16 {
        let t = px[raster(p)];
        let sel = selector(lo, p);
        let (c, a) = if punch && sel == 2 {
            ([0; 3], 0)
        } else {
            (pal[usize::from(second(p, flip))][sel], 255)
        };
        let d = |x: u8, y: i32| u64::from((i32::from(x) - y).unsigned_abs().pow(2));
        e += w[0] * d(t[0], c[0]) + w[1] * d(t[1], c[1]) + w[2] * d(t[2], c[2]) + w[3] * d(t[3], a);
    }
    e
}

/// Encodes one ETC2 RGBA8 block: an 8-bit EAC alpha block (bytes 0..8), then ETC2 RGB.
pub fn encode_block_rgba(px: &Block, effort: u8) -> [u8; 16] {
    let mut out = [0u8; 16];
    let a: [u8; 16] = std::array::from_fn(|i| px[i][3]);
    out[..8].copy_from_slice(&crate::eac::encode_a8(&a, effort));
    out[8..].copy_from_slice(&encode_block(px, effort));
    out
}

/// Decodes one 16-byte ETC2 RGBA8 block.
pub fn decode_rgba(raw: &[u8], out: &mut Block) {
    decode(&raw[8..16], out);
    for (t, a) in out.iter_mut().zip(crate::eac::decode_a8(&raw[..8])) {
        t[3] = a;
    }
}

/// Encodes a whole surface as ETC2 RGBA8; returns raw 16-byte blocks in raster order.
pub fn encode_surface_rgba(surface: &Surface, effort: u8) -> Vec<u8> {
    let (alpha, color) = rayon::join(
        || crate::eac::encode_surface(surface, (3, 1), crate::eac::Kind::A8, effort),
        || encode_surface(surface, effort),
    );
    let mut out = Vec::with_capacity(alpha.len() * 2);
    for (a, c) in alpha.as_chunks::<8>().0.iter().zip(color.as_chunks::<8>().0) {
        out.extend_from_slice(a);
        out.extend_from_slice(c);
    }
    out
}

fn encode_span(surface: &Surface, first: usize, out: &mut [u8], effort: u8, a1: bool) {
    let cfg = cfg(effort);
    let bw = surface.blocks_x() as usize;
    for (k, o) in out.as_chunks_mut::<8>().0.iter_mut().enumerate() {
        let i = first + k;
        let px = surface.read_block((i % bw) as u32, (i / bw) as u32);
        *o = if a1 {
            encode_a1_px(&texels(&px), opaque_mask(&px), &cfg)
        } else {
            encode_px(&texels(&px), &cfg)
        };
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
fn encode_span_avx2(surface: &Surface, first: usize, out: &mut [u8], effort: u8, a1: bool) {
    encode_span(surface, first, out, effort, a1);
}

fn encode_surface_with(surface: &Surface, effort: u8, a1: bool) -> Vec<u8> {
    use rayon::prelude::*;
    const SPAN: usize = 256;
    let mut out = vec![0u8; surface.block_count() * 8];
    let lvl = crate::simd::level();
    out.par_chunks_mut(SPAN * 8).enumerate().for_each(|(ci, o)| match lvl {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: `level()` returns Avx2 only when the CPU reports AVX2 and FMA.
        crate::simd::Level::Avx2 => unsafe { encode_span_avx2(surface, ci * SPAN, o, effort, a1) },
        _ => encode_span(surface, ci * SPAN, o, effort, a1),
    });
    out
}

/// Encodes a whole surface; returns raw 8-byte blocks in raster order.
pub fn encode_surface(surface: &Surface, effort: u8) -> Vec<u8> {
    encode_surface_with(surface, effort, false)
}

/// Encodes a whole surface as ETC2 RGB A1; returns raw 8-byte blocks in raster order.
pub fn encode_surface_a1(surface: &Surface, effort: u8) -> Vec<u8> {
    encode_surface_with(surface, effort, true)
}

/// The opaque texels of a block for ETC2 RGB A1 (alpha 128 and up): bit p for column-major
/// texel p.
fn opaque_mask(px: &Block) -> u16 {
    (0..16).filter(|&p| px[raster(p)][3] >= 128).fold(0, |m, p| m | 1 << p)
}

/// `raw` with bit 33 set: the ETC2 block whose mode an A1 block decodes in.
fn as_differential(raw: &[u8]) -> [u8; 8] {
    let mut r = [0u8; 8];
    r.copy_from_slice(&raw[..8]);
    r[3] |= 2;
    r
}

/// The paint colors of an A1 block by subblock and selector, its flip, and whether it is
/// punch-through (selector 2 then transparent). `None` for planar blocks.
fn a1_paints(raw: &[u8]) -> Option<(Paints, bool, bool)> {
    let r = as_differential(raw);
    let (mut pal, flip) = paints(&r)?;
    let punch = raw[3] & 2 == 0;
    if punch && mode(&r) == Mode::Differential {
        // Selector 0 loses its modifier: the base color, midway between +a and -a.
        for s in &mut pal {
            s[0] = std::array::from_fn(|c| (s[0][c] + s[2][c]) / 2);
        }
    }
    Some((pal, flip, punch))
}

/// Decodes one 8-byte ETC2 RGB A1 block: transparent texels are (0, 0, 0, 0), the rest opaque.
pub fn decode_a1(raw: &[u8], out: &mut Block) {
    let Some((pal, flip, punch)) = a1_paints(raw) else {
        return decode(&as_differential(raw), out);
    };
    let lo = u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]);
    for p in 0..16 {
        let sel = selector(lo, p);
        out[raster(p)] = if punch && sel == 2 {
            [0; 4]
        } else {
            let c = pal[usize::from(second(p, flip))][sel];
            [
                c[0].clamp(0, 255) as u8,
                c[1].clamp(0, 255) as u8,
                c[2].clamp(0, 255) as u8,
                255,
            ]
        };
    }
}

/// [`reselect`] for ETC2 RGB A1: transparent texels of `px` take selector 2 in a punch-through
/// block. `None` for planar blocks, and for opaque ones when `px` has transparent texels.
pub fn reselect_a1(raw: &[u8], px: &Block, w: &[u32; 4]) -> Option<[u8; 8]> {
    let (pal, flip, punch) = a1_paints(raw)?;
    let opaque = opaque_mask(px);
    if !punch && opaque != u16::MAX {
        return None;
    }
    let pal = pal.map(|s| s.map(|c| c.map(|v| v.clamp(0, 255))));
    let mut lo = 0u32;
    for p in 0..16 {
        if opaque >> p & 1 == 0 {
            lo |= set_selector(p, 2);
            continue;
        }
        let t = px[raster(p)];
        let mut best = (u32::MAX, 0u8);
        for (k, c) in pal[usize::from(second(p, flip))].iter().enumerate() {
            if punch && k == 2 {
                continue;
            }
            let e: u32 = (0..3)
                .map(|ch| (i32::from(t[ch]) - c[ch]).unsigned_abs().pow(2) * w[ch])
                .sum();
            if e < best.0 {
                best = (e, k as u8);
            }
        }
        lo |= set_selector(p, best.1);
    }
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&raw[..4]);
    out[4..].copy_from_slice(&lo.to_be_bytes());
    Some(out)
}

/// Encodes one ETC2 RGB A1 block: texels below alpha 128 become transparent.
pub fn encode_block_a1(px: &Block, effort: u8) -> [u8; 8] {
    encode_a1_px(&texels(px), opaque_mask(px), &cfg(effort))
}

fn encode_a1_px(px: &Px, opaque: u16, cfg: &Cfg) -> [u8; 8] {
    let mut best = Best { err: u64::MAX, bits: 0 };
    if opaque == u16::MAX {
        // ETC2 without the individual mode.
        planar_search(px, cfg, &mut best);
        for flip in [false, true] {
            if best.err > 0 {
                etc1_flip(px, flip, cfg, &mut best, false);
            }
        }
        if cfg.th && best.err > 0 {
            th_search(px, cfg, &mut best, None);
        }
    } else if opaque == 0 {
        // Punch-through differential, every selector 2.
        best.bits = 0xffff_0000;
    } else {
        for flip in [false, true] {
            if best.err > 0 {
                punch_flip(px, opaque, flip, cfg, &mut best);
            }
        }
        if cfg.th && best.err > 0 {
            th_search(px, cfg, &mut best, Some(opaque));
        }
    }
    best.bits.to_be_bytes()
}

/// The opaque texels of one subblock, compacted, and their count.
fn opaque_sub(px: &Px, opaque: u16, flip: bool, s: bool) -> (Sub, usize) {
    let mut out = [[0; 8]; 3];
    let mut n = 0;
    for p in 0..16 {
        if second(p, flip) == s && opaque >> p & 1 != 0 {
            for c in 0..3 {
                out[c][n] = px[c][p];
            }
            n += 1;
        }
    }
    (out, n)
}

/// Punch-through differential palette offset by selector (2, transparent, has none).
#[inline(always)]
fn punch_modifier(table: usize, sel: usize) -> i32 {
    if sel & 1 == 0 { 0 } else { modifier(table, sel) }
}

/// Exact error of the first `n` texels of a punch-through differential subblock with base `c`
/// (8-bit) for every table: (best error, its table).
fn punch_sub_error(s: &Sub, n: usize, c: [i32; 3]) -> (u32, u8) {
    let mut best = (u32::MAX, 0u8);
    for t in 0..8 {
        let mut total = 0u32;
        for i in 0..n {
            let mut e = i32::MAX;
            for sel in [0, 1, 3] {
                let m = punch_modifier(t, sel);
                let d: i32 = (0..3).map(|k| (s[k][i] - (c[k] + m).clamp(0, 255)).pow(2)).sum();
                e = e.min(d);
            }
            total += e as u32;
        }
        if total < best.0 {
            best = (total, t as u8);
        }
    }
    best
}

/// The punch-through differential half of the A1 search for one flip: candidate bases on the 555
/// lattice around each subblock's opaque mean, scored exactly, paired within the delta window
/// and refit. A subblock with no opaque texel takes the other's base.
fn punch_flip(px: &Px, opaque: u16, flip: bool, cfg: &Cfg, best: &mut Best) {
    let subs = [opaque_sub(px, opaque, flip, false), opaque_sub(px, opaque, flip, true)];
    let exact = |k: usize, q: [i32; 3]| {
        let (err, table) = punch_sub_error(&subs[k].0, subs[k].1, q.map(ext5));
        Pick { q, err, table }
    };
    let r = cfg.luma.min(3);
    let lists: [Vec<Pick>; 2] = std::array::from_fn(|k| {
        let (s, n) = (&subs[k].0, subs[k].1 as i32);
        if n == 0 {
            return Vec::new();
        }
        let q0: [i32; 3] = std::array::from_fn(|c| {
            let sum: i32 = s[c][..n as usize].iter().sum();
            ((sum * 62 + n * 255) / (n * 510)).clamp(0, 31)
        });
        let mut list = Vec::new();
        for dr in -r..=r {
            for dg in -r..=r {
                for db in -r..=r {
                    let q = [q0[0] + dr, q0[1] + dg, q0[2] + db];
                    if q.iter().all(|v| (0..32).contains(v)) {
                        list.push(exact(k, q));
                    }
                }
            }
        }
        list.sort_by_key(|p| p.err);
        list.truncate(cfg.short.max(1));
        list
    });
    let ok = |a: [i32; 3], b: [i32; 3]| (0..3).all(|c| (-4..=3).contains(&(b[c] - a[c])));
    let free = |q: [i32; 3]| Pick { q, err: 0, table: 0 };
    let mut pair = match (lists[0].first(), lists[1].first()) {
        (None, None) => return,
        (Some(&a), None) => [a, free(a.q)],
        (None, Some(&b)) => [free(b.q), b],
        (Some(_), Some(_)) => {
            let mut pair: Option<[Pick; 2]> = None;
            let mut pair_err = u64::MAX;
            let mut consider = |a: Pick, b: Pick| {
                let e = u64::from(a.err) + u64::from(b.err);
                if e < pair_err && ok(a.q, b.q) {
                    pair_err = e;
                    pair = Some([a, b]);
                }
            };
            for &a in &lists[0] {
                for &b in &lists[1] {
                    consider(a, b);
                }
            }
            // Projections of each side's best few into the other side's window.
            let proj = lists[0].len().min(lists[1].len()).min(3);
            for i in 0..proj {
                let a = lists[0][i];
                for b in &lists[1][..proj] {
                    let qb: [i32; 3] = std::array::from_fn(|c| b.q[c].clamp(a.q[c] - 4, a.q[c] + 3));
                    consider(a, exact(1, qb));
                }
                let b = lists[1][i];
                for a in &lists[0][..proj] {
                    let qa: [i32; 3] = std::array::from_fn(|c| a.q[c].clamp(b.q[c] - 3, b.q[c] + 4));
                    consider(exact(0, qa), b);
                }
            }
            pair.expect("projections give a pair in the window")
        }
    };
    // Least-squares refits of each subblock with opaque texels, kept inside the delta window.
    for _ in 0..cfg.refits {
        let mut improved = false;
        for k in 0..2 {
            let (s, n) = (&subs[k].0, subs[k].1);
            if n == 0 {
                continue;
            }
            let (c, t) = (pair[k].q.map(ext5), pair[k].table as usize);
            let mut sum = [0i32; 3];
            for i in 0..n {
                let mut sel = (i32::MAX, 0);
                for q in [0, 1, 3] {
                    let m = punch_modifier(t, q);
                    let d: i32 = (0..3).map(|ch| (s[ch][i] - (c[ch] + m).clamp(0, 255)).pow(2)).sum();
                    if d < sel.0 {
                        sel = (d, m);
                    }
                }
                for ch in 0..3 {
                    sum[ch] += s[ch][i] - sel.1;
                }
            }
            let n = n as i32;
            let other = pair[1 - k].q;
            let q: [i32; 3] = std::array::from_fn(|ch| {
                let q = ((sum[ch] * 62 + n * 255) / (n * 510)).clamp(0, 31);
                if k == 0 {
                    q.clamp(other[ch] - 3, other[ch] + 4)
                } else {
                    q.clamp(other[ch] - 4, other[ch] + 3)
                }
            });
            if q != pair[k].q {
                let p = exact(k, q);
                if p.err < pair[k].err {
                    pair[k] = p;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
    let err = u64::from(pair[0].err) + u64::from(pair[1].err);
    if err >= best.err {
        return;
    }
    let hi = etc1_header(flip, true, pair) & !2;
    let mut lo = 0u32;
    for k in 0..2 {
        let (c, t) = (pair[k].q.map(ext5), pair[k].table as usize);
        let paint: [[i32; 3]; 4] = std::array::from_fn(|sel| c.map(|v| v + punch_modifier(t, sel)));
        let sels = punch_error_sels(px, &paint, opaque).1;
        for p in (0..16).filter(|&p| second(p, flip) == (k == 1)) {
            lo |= set_selector(p, sels[p]);
        }
    }
    best.set(err, u64::from(hi) << 32 | u64::from(lo));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(bits: u64) -> Block {
        let mut out = [[0u8; 4]; 16];
        decode(&bits.to_be_bytes(), &mut out);
        out
    }

    fn rgb(b: &Block, x: usize, y: usize) -> [u8; 3] {
        let t = b[y * 4 + x];
        [t[0], t[1], t[2]]
    }

    /// Selectors as (msb, lsb) planes from a column-major list.
    fn sels(s: [u8; 16]) -> u32 {
        (0..16).map(|p| set_selector(p, s[p])).fold(0, |a, b| a | b)
    }

    #[test]
    fn individual_mode() {
        // R1=0xA R2=0x3, G1=0x5 G2=0xC, B1=0xF B2=0x0, tables 2 and 7, no flip.
        let hi: u32 = 0xA << 28 | 0x3 << 24 | 0x5 << 20 | 0xC << 16 | 0xF << 12 | 2 << 5 | 7 << 2;
        let s = [0, 1, 2, 3, 3, 2, 1, 0, 0, 1, 2, 3, 1, 1, 1, 1];
        let b = decoded(u64::from(hi) << 32 | u64::from(sels(s)));
        // Texel p=1 is (x0, y1): subblock 0, selector 1 → +29 on (170, 85, 255).
        assert_eq!(rgb(&b, 0, 1), [199, 114, 255]);
        // p=2 (x0, y2): selector 2 → −9.
        assert_eq!(rgb(&b, 0, 2), [161, 76, 246]);
        // p=8 is (x2, y0): subblock 1 (51, 204, 0), table 7 selector 0 → +47.
        assert_eq!(rgb(&b, 2, 0), [98, 251, 47]);
        // p=11 (x2, y3): selector 3 → −183, clamped.
        assert_eq!(rgb(&b, 2, 3), [0, 21, 0]);
        assert_eq!(mode(&(u64::from(hi) << 32).to_be_bytes()), Mode::Individual);
    }

    #[test]
    fn differential_mode_with_flip() {
        // Base (20, 3, 31), delta (−4, +3, −1); tables 0 and 1; flip.
        let hi: u32 = 20 << 27
            | (-4i32 as u32 & 7) << 24
            | 3 << 19
            | 3 << 16
            | 31 << 11
            | (-1i32 as u32 & 7) << 8
            | 1 << 2
            | 2
            | 1;
        let b = decoded(u64::from(hi) << 32);
        // Selector 0 everywhere. Top half (y < 2): base (165, 24, 255) + 2.
        assert_eq!(rgb(&b, 3, 1), [167, 26, 255]);
        // Bottom half: (16, 6, 30) → (132, 49, 247) + 5.
        assert_eq!(rgb(&b, 0, 2), [137, 54, 252]);
        assert_eq!(mode(&(u64::from(hi) << 32).to_be_bytes()), Mode::Differential);
    }

    #[test]
    fn t_mode() {
        let th = Th {
            h: false,
            c1: [13, 2, 9],
            c2: [4, 8, 15],
            d: 5,
            punch: None,
        };
        let px: Px = [[0; 16]; 3];
        let mut bits = th.pack(&px);
        assert_eq!(mode(&bits.to_be_bytes()), Mode::T);
        // Replace selectors with 0, 1, 2, 3 in columns.
        bits = bits & !0xffff_ffff | u64::from(sels([0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3]));
        let b = decoded(bits);
        assert_eq!(rgb(&b, 0, 0), [221, 34, 153]);
        assert_eq!(rgb(&b, 1, 2), [68 + 32, 136 + 32, 255]);
        assert_eq!(rgb(&b, 2, 3), [68, 136, 255]);
        assert_eq!(rgb(&b, 3, 1), [36, 104, 223]);
    }

    #[test]
    fn h_mode_both_orders() {
        for (c1, c2, d) in [
            ([2, 9, 13], [7, 1, 0], 6),
            ([2, 9, 13], [7, 1, 0], 3),
            ([7, 1, 0], [2, 9, 13], 6),
        ] {
            let th = Th {
                h: true,
                c1,
                c2,
                d,
                punch: None,
            };
            let px: Px = [[0; 16]; 3];
            let bits = th.pack(&px) & !0xffff_ffff | u64::from(sels([0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3]));
            assert_eq!(mode(&bits.to_be_bytes()), Mode::H);
            let b = decoded(bits);
            let dd = DISTANCES[d];
            let got: Vec<[u8; 3]> = (0..4).map(|x| rgb(&b, x, 0)).collect();
            let want = th.paint().map(|c| c.map(|v| v.clamp(0, 255) as u8));
            // The packed block may swap the colors; the four paint colors are the same set.
            let mut g = got.clone();
            g.sort_unstable();
            let mut w = want.to_vec();
            w.sort_unstable();
            assert_eq!(g, w, "d={dd}");
        }
    }

    #[test]
    fn planar_mode() {
        let q = [[10, 50, 63], [127, 0, 64], [0, 33, 21]];
        let b = decoded(pack_planar(q));
        assert_eq!(mode(&pack_planar(q).to_be_bytes()), Mode::Planar);
        let (o, h, v) = (ext6(10), ext6(50), ext6(63));
        assert_eq!(b[0][0], o as u8);
        assert_eq!(i32::from(rgb(&b, 1, 0)[0]), (h - o + 4 * o + 2) >> 2);
        assert_eq!(i32::from(rgb(&b, 0, 1)[0]), (v - o + 4 * o + 2) >> 2);
        assert_eq!(
            i32::from(rgb(&b, 3, 3)[0]),
            ((3 * (h - o) + 3 * (v - o) + 4 * o + 2) >> 2).clamp(0, 255)
        );
        // Green falls below zero at the far corner: (254, 0, 129).
        let (o, h, v) = (ext7(127), ext7(0), ext7(64));
        assert_eq!(
            i32::from(rgb(&b, 3, 0)[1]),
            ((3 * (h - o) + 4 * o + 2) >> 2).clamp(0, 255)
        );
        assert_eq!(
            i32::from(rgb(&b, 3, 3)[1]),
            ((3 * (h - o) + 3 * (v - o) + 4 * o + 2) >> 2).clamp(0, 255)
        );
    }

    #[test]
    fn planar_free_bits_cover_all_origins() {
        for r0 in 0..64 {
            for g0 in 0..128 {
                for b0 in 0..64 {
                    let bits = pack_planar([[r0, 0, 0], [g0, 0, 0], [b0, 0, 0]]);
                    assert_eq!(mode(&bits.to_be_bytes()), Mode::Planar);
                }
            }
        }
    }

    #[test]
    fn th_free_bits_cover_all_colors() {
        let px: Px = [[0; 16]; 3];
        for v in 0..4096 {
            let c = [v >> 8, v >> 4 & 15, v & 15];
            let t = Th {
                h: false,
                c1: c,
                c2: [1, 2, 3],
                d: 0,
                punch: None,
            };
            assert_eq!(mode(&t.pack(&px).to_be_bytes()), Mode::T);
            for d in [0, 1] {
                let h = Th {
                    h: true,
                    c1: c,
                    c2: [5, 9, 1],
                    d,
                    punch: None,
                };
                if h.valid() {
                    assert_eq!(mode(&h.pack(&px).to_be_bytes()), Mode::H);
                }
            }
        }
    }

    fn rng(seed: &mut u32) -> u32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        *seed
    }

    fn sample_block(seed: &mut u32, kind: u32) -> Block {
        let mut b = [[0u8; 4]; 16];
        let base = [rng(seed) as u8, rng(seed) as u8, rng(seed) as u8];
        let other = [rng(seed) as u8, rng(seed) as u8, rng(seed) as u8];
        for (i, t) in b.iter_mut().enumerate() {
            let (x, y) = ((i & 3) as i32, (i >> 2) as i32);
            *t = match kind {
                0 => [base[0], base[1], base[2], 255],
                1 => std::array::from_fn(|c| {
                    if c == 3 {
                        255
                    } else {
                        (i32::from(base[c]) + x * 9 - y * 7).clamp(0, 255) as u8
                    }
                }),
                2 => {
                    let src = if rng(seed) & 1 == 0 { base } else { other };
                    [src[0], src[1], src[2], 255]
                }
                _ => [rng(seed) as u8, rng(seed) as u8, rng(seed) as u8, 255],
            };
        }
        b
    }

    #[test]
    fn effort_improves_and_modes_appear() {
        let mut seed = 0x1234_5678;
        let mut totals = [0u64; 10];
        let mut modes = std::collections::HashSet::new();
        for k in 0..400 {
            let b = sample_block(&mut seed, k % 4);
            for e in 0..10u8 {
                let raw = encode_block(&b, e);
                totals[e as usize] += u64::from(block_error(&raw, &b));
                modes.insert(mode(&raw));
            }
        }
        assert!(totals[9] <= totals[5] && totals[5] <= totals[0], "{totals:?}");
        assert_eq!(modes.len(), 5, "{modes:?}");
    }

    #[test]
    fn flat_and_gradient_blocks_are_close() {
        let mut seed = 0xdead_beef;
        for k in 0..200 {
            let mut b = sample_block(&mut seed, k % 2);
            if k % 2 == 1 {
                // Keep the gradient unclamped, so it is a plane.
                let base = [b[0][0] / 2 + 32, b[0][1] / 2 + 32, b[0][2] / 2 + 32];
                for (i, t) in b.iter_mut().enumerate() {
                    let (x, y) = ((i & 3) as i32, (i >> 2) as i32);
                    for c in 0..3 {
                        t[c] = (i32::from(base[c]) + x * 9 - y * 7) as u8;
                    }
                }
            }
            let raw = encode_block(&b, 5);
            let e = block_error(&raw, &b);
            assert!(e <= 16 * 3 * 9, "kind {} error {e} mode {:?}", k % 2, mode(&raw));
        }
    }

    /// RDO helpers: re-chosen selectors never lose against the block's own, and a block's own
    /// selectors refit stay close to it.
    #[test]
    fn reselect_and_refit_selectors() {
        let mut seed = 0x0bad_cafe;
        let w = [1, 1, 1, 0];
        for k in 0..400 {
            let b = sample_block(&mut seed, k % 4);
            let raw = encode_block(&b, 3);
            let e = block_error(&raw, &b);
            let Some(r) = reselect(&raw, &b, &w) else {
                assert_eq!(mode(&raw), Mode::Planar);
                continue;
            };
            assert_eq!(r[..4], raw[..4]);
            assert!(block_error(&r, &b) <= e, "{k}");
            if let Some(f) = refit_selectors(&raw, &raw[4..], &b) {
                assert_eq!(f[4..], raw[4..]);
                assert!(matches!(mode(&f), Mode::Individual | Mode::Differential));
                assert!(
                    block_error(&f, &b) <= e * 2 + 16 * 3 * 4,
                    "{k}: {} vs {e}",
                    block_error(&f, &b)
                );
            }
        }
    }

    #[test]
    fn surface_matches_blocks() {
        let mut s = Surface::new(20, 12);
        let mut seed = 7u32;
        for (i, p) in s.pixels.iter_mut().enumerate() {
            let (x, y) = ((i % 20) as u32, (i / 20) as u32);
            *p = [(x * 13) as u8, (y * 17) as u8, (rng(&mut seed) % 32) as u8, 255];
        }
        for effort in [0, 5] {
            let raw = encode_surface(&s, effort);
            let bw = s.blocks_x();
            for (k, b) in raw.as_chunks::<8>().0.iter().enumerate() {
                let px = s.read_block(k as u32 % bw, k as u32 / bw);
                assert_eq!(*b, encode_block(&px, effort));
            }
        }
    }

    fn decoded_a1(raw: &[u8]) -> Block {
        let mut out = [[0u8; 4]; 16];
        decode_a1(raw, &mut out);
        out
    }

    /// `sample_block` with texels below alpha 128 at random (none for `holes` 0, all for 16).
    fn holed_block(seed: &mut u32, kind: u32, holes: u32) -> Block {
        let mut b = sample_block(seed, kind);
        for t in &mut b {
            t[3] = if rng(seed) % 16 < holes {
                (rng(seed) % 128) as u8
            } else {
                128 + (rng(seed) % 128) as u8
            };
        }
        b
    }

    /// An A1 block's alpha mismatches against its source's 1-bit alpha, and its RGB error over
    /// the source's opaque texels.
    fn a1_error(raw: &[u8], px: &Block) -> (usize, u32) {
        let out = decoded_a1(raw);
        let mut miss = 0;
        let mut err = 0;
        for (a, b) in px.iter().zip(&out) {
            let opaque = a[3] >= 128;
            miss += usize::from(b[3] != if opaque { 255 } else { 0 });
            if opaque {
                err += (0..3).map(|c| u32::from(a[c].abs_diff(b[c])).pow(2)).sum::<u32>();
            } else {
                assert_eq!(*b, [0; 4]);
            }
        }
        (miss, err)
    }

    #[test]
    fn a1_punch_through_decode() {
        // The differential block of `differential_mode_with_flip` (tables 0 and 1, flip), with
        // selector y on texel (x, y) so the top half takes 0 and 1, the bottom 2 and 3.
        let hi: u32 = 20 << 27
            | (-4i32 as u32 & 7) << 24
            | 3 << 19
            | 3 << 16
            | 31 << 11
            | (-1i32 as u32 & 7) << 8
            | 1 << 2
            | 2
            | 1;
        let lo = sels(std::array::from_fn(|p| (p % 4) as u8));
        let opaque = (u64::from(hi) << 32 | u64::from(lo)).to_be_bytes();
        // Bit 33 set: opaque, decoded as ETC2.
        assert_eq!(decoded_a1(&opaque), decoded(u64::from_be_bytes(opaque)));
        let punch = (u64::from(hi & !2) << 32 | u64::from(lo)).to_be_bytes();
        let b = decoded_a1(&punch);
        // Top half: base (165, 24, 255) with no modifier, then +8.
        assert_eq!(b[1], [165, 24, 255, 255]);
        assert_eq!(b[4 + 1], [173, 32, 255, 255]);
        // Bottom half: transparent, then (132, 49, 247) − 17.
        assert_eq!(b[8 + 1], [0; 4]);
        assert_eq!(b[12 + 1], [115, 32, 230, 255]);
        // Planar blocks ignore the opaque flag.
        let planar = [0x00, 0x00, 0x04, 0x00, 0x12, 0x34, 0x56, 0x78];
        assert_eq!(mode(&as_differential(&planar)), Mode::Planar);
        assert!(decoded_a1(&planar).iter().all(|t| t[3] == 255));
    }

    /// Encoded A1 blocks keep the source's 1-bit alpha exactly, reach every punch-through mode,
    /// and stay near ETC2 on opaque blocks (A1 has no individual mode).
    #[test]
    fn a1_encode_alpha_and_modes() {
        let mut seed = 0x5eed_a1a1;
        let mut punch_modes = std::collections::HashSet::new();
        let (mut a1, mut etc2) = (0u64, 0u64);
        for k in 0..600 {
            let holes = [0, 1, 4, 8, 15, 16][k % 6];
            let b = holed_block(&mut seed, (k / 6 % 4) as u32, holes);
            for effort in [0, 5, 9] {
                let raw = encode_block_a1(&b, effort);
                let (miss, err) = a1_error(&raw, &b);
                assert_eq!(miss, 0, "{k} effort {effort} raw {raw:02x?}");
                if opaque_mask(&b) == u16::MAX {
                    if effort == 5 {
                        a1 += u64::from(err);
                        etc2 += u64::from(block_error(&encode_block(&b, 5), &b));
                    }
                } else if raw[3] & 2 == 0 {
                    punch_modes.insert(mode(&as_differential(&raw)));
                }
            }
        }
        assert!(a1 * 100 <= etc2 * 115, "{a1} vs {etc2}");
        assert_eq!(punch_modes.len(), 3, "{punch_modes:?}");
    }

    /// Re-chosen A1 selectors keep the alpha and never lose against the block's own.
    #[test]
    fn a1_reselect() {
        let mut seed = 0x0a1_5e1;
        let w = [1, 1, 1, 0];
        for k in 0..400 {
            let b = holed_block(&mut seed, (k % 4) as u32, [0, 2, 8][k % 3]);
            let raw = encode_block_a1(&b, 3);
            let (_, e) = a1_error(&raw, &b);
            match reselect_a1(&raw, &b, &w) {
                None => assert!(
                    mode(&as_differential(&raw)) == Mode::Planar || (raw[3] & 2 != 0 && opaque_mask(&b) != u16::MAX)
                ),
                Some(r) => {
                    assert_eq!(r[..4], raw[..4]);
                    let (miss, er) = a1_error(&r, &b);
                    assert_eq!(miss, 0, "{k}");
                    assert!(er <= e, "{k}: {er} vs {e}");
                }
            }
            // Another block's head: alpha still holds when it is accepted.
            let other = holed_block(&mut seed, (k % 4) as u32, [0, 2, 8][k % 3]);
            if let Some(r) = reselect_a1(&raw, &other, &w) {
                assert_eq!(a1_error(&r, &other).0, 0, "{k}");
            }
        }
    }

    #[test]
    fn a1_surface_matches_blocks() {
        let mut s = Surface::new(20, 12);
        let mut seed = 11u32;
        for (i, p) in s.pixels.iter_mut().enumerate() {
            let (x, y) = ((i % 20) as u32, (i / 20) as u32);
            let a = if (x + 2 * y) % 7 < 3 { 0 } else { 255 };
            *p = [(x * 13) as u8, (y * 17) as u8, (rng(&mut seed) % 32) as u8, a];
        }
        for effort in [0, 5] {
            let raw = encode_surface_a1(&s, effort);
            let bw = s.blocks_x();
            for (k, b) in raw.as_chunks::<8>().0.iter().enumerate() {
                let px = s.read_block(k as u32 % bw, k as u32 / bw);
                assert_eq!(*b, encode_block_a1(&px, effort), "block {k} effort {effort}");
            }
        }
    }

    /// [`refit_selectors`] as it was: each table's error summed over the subblock's texels.
    fn refit_selectors_ref(raw: &[u8], sels: &[u8], px: &Block) -> Option<[u8; 8]> {
        let flip = match mode(raw) {
            Mode::Individual | Mode::Differential => raw[3] & 1 != 0,
            _ => return None,
        };
        let lo = u32::from_be_bytes([sels[0], sels[1], sels[2], sels[3]]);
        let px = texels(px);
        // Per subblock: its texels and their selectors.
        let mut subs = [[[0i32; 8]; 3]; 2];
        let mut ss = [[0usize; 8]; 2];
        let mut n = [0usize; 2];
        for p in 0..16 {
            let k = usize::from(second(p, flip));
            for c in 0..3 {
                subs[k][c][n[k]] = px[c][p];
            }
            ss[k][n[k]] = selector(lo, p);
            n[k] += 1;
        }
        // Error and quantized base color of subblock k at `bits` per channel with table t.
        let fit = |k: usize, bits: u32, t: usize| -> (u32, [i32; 3]) {
            let msum: i32 = ss[k].iter().map(|&q| modifier(t, q)).sum();
            let max = (1 << bits) - 1;
            let q: [i32; 3] = std::array::from_fn(|c| {
                let sum = subs[k][c].iter().sum::<i32>() - msum;
                ((sum * max + 4 * 255) / (8 * 255)).clamp(0, max)
            });
            let col = q.map(if bits == 5 { ext5 } else { ext4 });
            let mut e = 0;
            for i in 0..8 {
                let m = modifier(t, ss[k][i]);
                for c in 0..3 {
                    e += (subs[k][c][i] - (col[c] + m).clamp(0, 255)).pow(2) as u32;
                }
            }
            (e, q)
        };
        let best = |k: usize, bits: u32| -> Pick {
            let mut b = Pick {
                q: [0; 3],
                err: u32::MAX,
                table: 0,
            };
            for t in 0..8 {
                let (err, q) = fit(k, bits, t);
                if err < b.err {
                    b = Pick { q, err, table: t as u8 };
                }
            }
            b
        };
        let ind = [best(0, 4), best(1, 4)];
        let dif = [best(0, 5), best(1, 5)];
        let fits = (0..3).all(|c| (-4..4).contains(&(dif[1].q[c] - dif[0].q[c])));
        let diff = fits && dif[0].err + dif[1].err <= ind[0].err + ind[1].err;
        let hi = etc1_header(flip, diff, if diff { dif } else { ind });
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(&hi.to_be_bytes());
        out[4..].copy_from_slice(&lo.to_be_bytes());
        Some(out)
    }

    #[test]
    fn refit_and_reselect_errors() {
        let mut seed = 0x0dd_ba11;
        for k in 0..2000 {
            let b = sample_block(&mut seed, k % 4);
            let raw = encode_block(&b, (k % 4) as u8);
            let other = encode_block(&sample_block(&mut seed, (k / 4) % 4), 1);
            for sels in [&raw[4..], &other[4..]] {
                let got = refit_selectors_err(&raw, sels, &b);
                assert_eq!(got.map(|g| g.0), refit_selectors_ref(&raw, sels, &b), "{k}");
                if let Some((v, e)) = got {
                    assert_eq!(e, block_error(&v, &b), "{k}");
                }
            }
            for w in [[1, 1, 1, 0], [3, 5, 2, 0]] {
                if let Some((v, e)) = reselect_err(&other, &b, &w) {
                    assert_eq!(e, sse(&v, &b, &w, false), "{k}");
                }
            }
        }
    }

    fn value(seed: &mut u32, lo: i32, hi: i32) -> i32 {
        let r = rng(seed);
        let x = (r >> 8) as i32;
        match r % 4 {
            0 => lo + x % 24,
            1 => hi - x % 24,
            _ => lo + x % (hi - lo + 1),
        }
    }

    /// The ETC1 subblock error, luma term, paint errors and T/H descent against direct evaluation
    /// over every modifier, paint color and move, with values near 0 and 255 so clamping matters.
    #[test]
    fn fast_kernels_match_direct() {
        let mut seed = 0x5eed_1234;
        for k in 0..4000 {
            let mut s: Sub = [[0; 8]; 3];
            for v in s.iter_mut().flatten() {
                *v = value(&mut seed, 0, 255);
            }
            let c = [0; 3].map(|_| value(&mut seed, 0, 255));
            let mut want = (u32::MAX, 0u8);
            for t in 0..8 {
                let mut total = 0i32;
                for i in 0..8 {
                    let e = |sel| -> i32 {
                        (0..3)
                            .map(|ch| (s[ch][i] - (c[ch] + modifier(t, sel)).clamp(0, 255)).pow(2))
                            .sum()
                    };
                    total += (0..4).map(e).min().unwrap();
                }
                if (total as u32) < want.0 {
                    want = (total as u32, t as u8);
                }
            }
            assert_eq!(sub_error(&s, c), want, "sub_error {k}");

            let mut rank = Ranker::new(&s);
            let l = value(&mut seed, 0, 765);
            let near = |t: usize, i: usize| -> i32 {
                (0..4)
                    .map(|sel| (rank.luma[i] - l - 3 * modifier(t, sel)).pow(2))
                    .min()
                    .unwrap()
            };
            let want = (0..8).map(|t| (0..8).map(|i| near(t, i)).sum::<i32>()).min().unwrap();
            rank.window(l, l);
            assert_eq!(rank.luma_term(l), want, "luma_term {k}");

            let mut px: Px = [[0; 16]; 3];
            for v in px.iter_mut().flatten() {
                *v = value(&mut seed, 0, 255);
            }
            let mut paint = [[0; 3]; 4];
            for v in paint.iter_mut().flatten() {
                *v = value(&mut seed, -60, 315);
            }
            let opaque = rng(&mut seed) as u16;
            let nearest = |p: usize, ks: &[usize]| {
                let mut best = (i32::MAX, 0u8);
                for &k in ks {
                    let e: i32 = (0..3).map(|ch| (px[ch][p] - paint[k][ch].clamp(0, 255)).pow(2)).sum();
                    if e < best.0 {
                        best = (e, k as u8);
                    }
                }
                best
            };
            let all: [(i32, u8); 16] = std::array::from_fn(|p| nearest(p, &[0, 1, 2, 3]));
            let want = (all.iter().map(|x| x.0).sum::<i32>() as u32, all.map(|x| x.1));
            assert_eq!(paint_error_sels(&px, &paint), want, "paint {k}");
            let some: [(i32, u8); 16] = std::array::from_fn(|p| {
                if opaque >> p & 1 == 0 {
                    (0, 2)
                } else {
                    nearest(p, &[0, 1, 3])
                }
            });
            let want = (some.iter().map(|x| x.0).sum::<i32>() as u32, some.map(|x| x.1));
            assert_eq!(punch_error_sels(&px, &paint, opaque), want, "punch {k}");

            for punch in [None, Some(opaque)] {
                let th = Th {
                    h: k % 2 == 1,
                    c1: [0; 3].map(|_| (rng(&mut seed) % 16) as i32),
                    c2: [0; 3].map(|_| (rng(&mut seed) % 16) as i32),
                    d: (rng(&mut seed) % 8) as usize,
                    punch,
                };
                let mut top = (th.error_sels(&px).0, th);
                let mut want = top;
                descent_pass(&px, th, &mut top);
                for m in 0..6 {
                    for step in [-1, 1] {
                        let mut moved = th;
                        let v = if m < 3 { &mut moved.c1[m] } else { &mut moved.c2[m - 3] };
                        *v += step;
                        if !(0..16).contains(v) {
                            continue;
                        }
                        for d in th.d.saturating_sub(1)..(th.d + 2).min(8) {
                            let next = Th { d, ..moved };
                            let e = next.error_sels(&px).0;
                            if e < want.0 {
                                want = (e, next);
                            }
                        }
                    }
                }
                assert_eq!(top, want, "descent {k}");
            }
        }
    }
}
