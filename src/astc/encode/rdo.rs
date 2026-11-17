//! Rate-distortion optimization for ASTC: the LZ-aware pass of [`crate::rdo`], for blocks whose
//! footprints are not the 4x4 source blocks that pass scores against.
//!
//! Each block, in raster order, picks among its base encoding and candidates built from earlier
//! blocks of its chunk, minimizing `J = smooth · SSE / 16 + λ · bits` (the same scale as the other
//! formats, whatever the footprint), with the bits from [`LzModel`]. An ASTC block keeps its mode,
//! partitioning and endpoints in its low bytes and its weights in its high bytes, so the candidates
//! are:
//!
//! - a whole earlier block, copied;
//! - an earlier block's mode, partitioning and endpoints, with weights chosen for this block's
//!   texels (its low bytes match);
//! - an earlier block's mode, partitioning and weights, with endpoints refitted to this block's
//!   texels (its high bytes match).
//!
//! The re-chosen half is polished with the other half held. Like the other formats, a chunk that
//! real Zstd does not shrink goes back to its base encoding.

use super::{
    Footprint, Layout, MAX_TEXELS, NO_CCS, Texels, polish_parts, quantize, read_block, refit, round_pos, sym_endpoints,
    tables,
};
use crate::astc::{COLOR_UNQUANT, PartitionFn, Symbolic, color_range, decode_block, decode_symbolic_tw};
use crate::format::Format;
use crate::rdo::{LzModel, LzState, RdoParams, RdoStats, bit_budget};
use crate::surface::{Rgba, Surface};

/// How far back the reweight and refit candidates look (whole copies look `max_back` far). On Kodak,
/// 256 is 4.8-6.6% smaller at equal PSNR than 32 for 2.4x the pass's time, and 512 another 0.3-0.7%
/// for 1.8x more (docs/DEADENDS.md).
const REFIT_BACK: usize = 256;
/// Polish passes over the re-chosen half of a candidate.
const PASSES: usize = 1;

/// A block already chosen: its bytes and, when the candidates can build on it, its fields, labels
/// and grid.
struct Prev {
    bytes: [u8; 16],
    sym: Option<Box<PrevSym>>,
}

/// An earlier block's fields, labels and grid, and what refitting endpoints to its weights needs
/// from it alone (`None` if its mode leaves no range for the refitted endpoints).
struct PrevSym {
    sym: Symbolic,
    lay: Layout,
    gi: usize,
    /// Each partition's decoded endpoints.
    eps: [([i32; 4], [i32; 4]); 3],
    refit: Option<RefitBase>,
}

/// The parts of a refit onto an earlier block's weights that do not depend on the texels: the
/// weights infilled to the texels, the endpoint mode and range written, and each partition's and
/// plane's texel count, weight sum and squared weight sum.
struct RefitBase {
    w: [[u8; MAX_TEXELS]; 2],
    cem: u8,
    color_quant: u8,
    sums: [[[i32; 3]; 2]; 3],
}

/// The current block's per-channel texel sums and squared sums, over all its texels.
struct BlockSums {
    sx: [i32; 4],
    sxx: [i32; 4],
}

impl BlockSums {
    fn of(t: &Texels) -> Self {
        let mut b = BlockSums {
            sx: [0; 4],
            sxx: [0; 4],
        };
        for c in 0..4 {
            for &x in &t.xl[c][..t.n] {
                b.sx[c] += x as i32;
                b.sxx[c] += x as i32 * x as i32;
            }
        }
        b
    }
}

/// One block's texels for the error kernels, all four channels counted (a copied block may carry
/// alpha the source lacks, or lack alpha the source has).
fn texels(px: &[Rgba]) -> Texels<'_> {
    let mut t = Texels {
        px,
        f: [[0.0; 4]; MAX_TEXELS],
        xl: [[0; MAX_TEXELS]; 4],
        n: px.len(),
        ch: 4,
    };
    for (i, p) in px.iter().enumerate() {
        t.f[i] = p.map(f32::from);
        for c in 0..4 {
            t.xl[c][i] = p[c] as i16;
        }
    }
    t
}

/// Makes `lay` the labels of `sym`'s partitioning (the only parts of a layout polish and refit
/// read), over `fp`'s texels: those past them are left as [`Layout::blank`] leaves them.
fn set_labels(fp: &Footprint, sym: &Symbolic, lay: &mut Layout) {
    let n = fp.texels();
    lay.count = sym.partitions;
    lay.seed = sym.seed;
    lay.ccs = if sym.mode.dual { sym.ccs as usize } else { NO_CCS };
    if sym.partitions == 1 {
        lay.labels[..n].fill(0);
        lay.lab16[..n].fill(0);
        return;
    }
    let pf = PartitionFn::new(sym.seed as u32, sym.partitions as u32, n as u32);
    for i in 0..n {
        let l = pf.at(i as u32 % fp.bw, i as u32 / fp.bw);
        lay.labels[i] = l;
        lay.lab16[i] = l as i16;
    }
}

/// `sym`'s fields, labels and grid, if the candidates can build on them: up to three partitions,
/// each luminance (0), direct (8, 12), base+offset (9, 13) or base+scale (6, 10), as the encoder
/// writes. `tw` holds its weights infilled to the texels, as its decode left them. Filled in one of
/// `spare`'s boxes if it has one (all from `fp`, so their labels past its texels are blank).
#[allow(clippy::vec_box, reason = "spare recycles the boxes it hands out")]
fn prev_sym(
    fp: &Footprint,
    sym: Symbolic,
    tw: &[[u8; MAX_TEXELS]; 2],
    spare: &mut Vec<Box<PrevSym>>,
) -> Option<Box<PrevSym>> {
    let p = sym.partitions as usize;
    if p > 3
        || sym.cems[..p]
            .iter()
            .any(|&c| !matches!(c, 0 | 6 | 8 | 9 | 10 | 12 | 13))
    {
        return None;
    }
    let gi = fp
        .grids
        .iter()
        .position(|g| g.gx == sym.mode.gx && g.gy == sym.mode.gy)?;
    let mut b = spare.pop().unwrap_or_else(|| {
        Box::new(PrevSym {
            sym: sym.clone(),
            lay: Layout::blank(),
            gi,
            eps: [([0; 4], [0; 4]); 3],
            refit: None,
        })
    });
    set_labels(fp, &sym, &mut b.lay);
    b.refit = refit_base(fp, &b.lay, gi, &sym, tw);
    for (q, e) in b.eps.iter_mut().enumerate() {
        *e = if q < p {
            sym_endpoints(&sym, q)
        } else {
            ([0; 4], [0; 4])
        };
    }
    b.sym = sym;
    b.gi = gi;
    Some(b)
}

/// The texel-independent parts of refitting endpoints to `sym`'s weights (see [`RefitBase`]), given
/// its weights infilled to the texels, `tw`.
fn refit_base(
    fp: &Footprint,
    lay: &Layout,
    gi: usize,
    sym: &Symbolic,
    tw: &[[u8; MAX_TEXELS]; 2],
) -> Option<RefitBase> {
    debug_assert_eq!(fp.grids[gi].rev.len(), sym.mode.gx as usize * sym.mode.gy as usize);
    let planes = 1 + sym.mode.dual as usize;
    let mut w = [[0u8; MAX_TEXELS]; 2];
    w[..planes].copy_from_slice(&tw[..planes]);
    // Refitted endpoints are written direct: RGB (CEM 8) for RGB heads, RGBA (CEM 12) for the rest.
    let p = sym.partitions as usize;
    let cem = if sym.cems[..p].iter().all(|c| matches!(c, 0 | 6 | 8 | 9)) {
        8
    } else {
        12
    };
    let color_quant = if sym.cems[..p].iter().any(|&c| c != cem) {
        color_range(sym.mode, sym.partitions, &[cem; 4])?
    } else {
        sym.color_quant
    };
    let mut sums = [[[0i32; 3]; 2]; 3];
    for i in 0..fp.texels() {
        let s = &mut sums[lay.labels[i] as usize];
        for (pl, wp) in w.iter().enumerate().take(planes) {
            let wi = wp[i] as i32;
            s[pl][0] += 1;
            s[pl][1] += wi;
            s[pl][2] += wi * wi;
        }
    }
    Some(RefitBase {
        w,
        cem,
        color_quant,
        sums,
    })
}

/// `sym` with weights chosen for `t`: each texel projected onto its partition's endpoint segment,
/// decimated to the grid, quantized and polished with the endpoints held. Returns it and its error,
/// or `None` if no weights for these endpoints can bring the error down to `max_err`.
#[expect(
    clippy::too_many_arguments,
    reason = "the earlier block's parts and the texels' sums"
)]
fn reweight(
    fp: &Footprint,
    t: &Texels,
    lay: &Layout,
    gi: usize,
    sym: &Symbolic,
    eps: &[([i32; 4], [i32; 4]); 3],
    bs: &BlockSums,
    max_err: f32,
) -> Option<(Symbolic, u32)> {
    let grid = &fp.grids[gi];
    let n = t.n;
    let planes = 1 + sym.mode.dual as usize;
    let ccs = lay.ccs;
    let count = lay.count as usize;
    // A texel decodes to its weight's point on the segment rounded per channel, within 0.5 of it
    // in each channel: so within `slack` of the segment over the first plane's channels, and its
    // error is at least its distance from the segment less that.
    let slack = if ccs < NO_CCS { 0.75f32.sqrt() } else { 1.0 };
    // That bound, (distance - slack)^2 where positive, is convex in the texel, so over a partition
    // it is at least its texel count times its value at their mean: checked first, from sums.
    let mut sx = [[0i32; 4]; 3];
    let mut m = [0i32; 3];
    if count == 1 {
        sx[0] = bs.sx;
        m[0] = n as i32;
    } else {
        for i in 0..n {
            let l = lay.labels[i] as usize;
            m[l] += 1;
            for c in 0..4 {
                sx[l][c] += t.xl[c][i] as i32;
            }
        }
    }
    let mut mean_lb = 0f64;
    for p in 0..count {
        if m[p] == 0 {
            continue;
        }
        let (e0, e1) = eps[p];
        let mu: [f64; 4] = std::array::from_fn(|c| sx[p][c] as f64 / m[p] as f64);
        let (mut dot, mut len2) = (0f64, 0f64);
        for c in (0..4).filter(|&c| c != ccs) {
            let d = (e1[c] - e0[c]) as f64;
            dot += (mu[c] - e0[c] as f64) * d;
            len2 += d * d;
        }
        let f = if len2 > 0.0 { (dot / len2).clamp(0.0, 1.0) } else { 0.0 };
        let d2: f64 = (0..4)
            .filter(|&c| c != ccs)
            .map(|c| (mu[c] - e0[c] as f64 - f * (e1[c] - e0[c]) as f64).powi(2))
            .sum();
        mean_lb += m[p] as f64 * (d2.sqrt() - slack as f64).max(0.0).powi(2);
    }
    // A little under, for the per-texel bound's float rounding: this only skips what it rejects.
    if mean_lb * 0.999 - 0.01 > max_err as f64 {
        return None;
    }
    let mut inv = [0f32; 3];
    for p in 0..count {
        let (e0, e1) = eps[p];
        let len2: i32 = (0..4).filter(|&c| c != ccs).map(|c| (e1[c] - e0[c]).pow(2)).sum();
        inv[p] = if len2 > 0 { 64.0 / len2 as f32 } else { 0.0 };
    }
    // Each partition's first endpoint and span, the span zero in the second plane's channel (so
    // it adds nothing to the dot product) and a mask zeroing that channel's distance term.
    let mut e0f = [[0f32; 4]; 3];
    let mut df = [[0f32; 4]; 3];
    let mut e0i = [[0i32; 4]; 3];
    let mut di = [[0i32; 4]; 3];
    for p in 0..count {
        let (e0, e1) = eps[p];
        for c in 0..4 {
            e0f[p][c] = e0[c] as f32;
            df[p][c] = (e1[c] - e0[c]) as f32;
            e0i[p][c] = e0[c];
            di[p][c] = if c == ccs { 0 } else { e1[c] - e0[c] };
        }
    }
    let mask: [f32; 4] = std::array::from_fn(|c| if c == ccs { 0.0 } else { 1.0 });
    let mut lb = 0f32;
    let mut u = [0f32; MAX_TEXELS];
    let mut u2 = [0f32; MAX_TEXELS];
    for i in 0..n {
        let p = lay.labels[i] as usize;
        let (e0, e1) = eps[p];
        let x = t.px[i].map(i32::from);
        let mut dot = 0i32;
        for c in 0..4 {
            dot += (x[c] - e0i[p][c]) * di[p][c];
        }
        u[i] = (dot as f32 * inv[p]).clamp(0.0, 64.0);
        let mut d2 = 0f32;
        for c in 0..4 {
            let r = t.f[i][c] - e0f[p][c] - u[i] * (1.0 / 64.0) * df[p][c];
            d2 += r * r * mask[c];
        }
        lb += (d2.sqrt() - slack).max(0.0).powi(2);
        if ccs < NO_CCS {
            let d = e1[ccs] - e0[ccs];
            u2[i] = if d != 0 {
                (64.0 * (x[ccs] - e0[ccs]) as f32 / d as f32).clamp(0.0, 64.0)
            } else {
                0.0
            };
            let r = x[ccs] as f32 - e0[ccs] as f32 - u2[i] * (1.0 / 64.0) * d as f32;
            lb += (r.abs() - 0.5).max(0.0).powi(2);
        }
        if lb > max_err {
            return None;
        }
    }
    let mut out = sym.clone();
    quantize(
        grid,
        sym.mode.quant as usize,
        planes,
        &u[..n],
        &u2[..n],
        &mut out.weights,
    );
    let err = polish_parts::<true, false>(t, lay, grid, &mut out, PASSES);
    Some((out, err))
}

/// `sym` with endpoints refitted to `t` for its weights, quantized and polished with the weights
/// held. Returns it and its error, or `None` if no endpoints for these weights can bring the error
/// down to `max_err`.
#[expect(
    clippy::too_many_arguments,
    reason = "the earlier block's parts and the texels' sums"
)]
fn refit_to(
    fp: &Footprint,
    t: &Texels,
    lay: &Layout,
    gi: usize,
    sym: &Symbolic,
    rb: &RefitBase,
    bs: &BlockSums,
    max_err: f32,
) -> Option<(Symbolic, u32)> {
    let grid = &fp.grids[gi];
    let tb = tables();
    let w = &rb.w;
    let cem = rb.cem;
    let p = sym.partitions as usize;
    let mut out = sym.clone();
    if sym.cems[..p].iter().any(|&c| c != cem) {
        out.cems = [cem; 4];
        out.color_quant = rb.color_quant;
    }
    let cq = out.color_quant as usize;
    let (cqt, cu) = (&tb.cq[cq], &COLOR_UNQUANT[cq]);
    let nv = if cem == 12 { 4 } else { 3 };
    // With the weights held, no endpoints fit better than each channel's least-squares line (not
    // clamped to the color range), and decoding moves each texel at most 0.5 per channel from its
    // line point, so at most sqrt(n) over the block. The weights' sums are the earlier block's;
    // only the texels' products with them (and, with partitions, the texels' sums) are per pair.
    let count = lay.count as usize;
    let mut sx = [[0i32; 4]; 3];
    let mut sxx = [[0i32; 4]; 3];
    let mut sxw = [[0i32; 4]; 3];
    for c in 0..4 {
        let wp = &w[(c == lay.ccs) as usize][..t.n];
        let xs = &t.xl[c][..t.n];
        if count == 1 {
            sx[0][c] = bs.sx[c];
            sxx[0][c] = bs.sxx[c];
            sxw[0][c] = xs.iter().zip(wp).map(|(&x, &wi)| x as i32 * wi as i32).sum();
        } else {
            for (i, (&x, &wi)) in xs.iter().zip(wp).enumerate() {
                let (x, l) = (x as i32, lay.labels[i] as usize);
                sx[l][c] += x;
                sxx[l][c] += x * x;
                sxw[l][c] += x * wi as i32;
            }
        }
    }
    let mut res = 0f64;
    for pi in 0..count {
        for c in 0..4 {
            let ws = &rb.sums[pi][(c == lay.ccs) as usize];
            let (n, sw, sww) = (ws[0] as f64, ws[1] as f64, ws[2] as f64);
            let (sx, sxw, sxx) = (sx[pi][c] as f64, sxw[pi][c] as f64, sxx[pi][c] as f64);
            res += if cem == 8 && c == 3 {
                // RGB endpoints decode alpha to 255.
                sxx - 510.0 * sx + 65025.0 * n
            } else {
                let det = n * sww - sw * sw;
                if det > 1e-6 {
                    sxx - (sww * sx * sx - 2.0 * sw * sx * sxw + n * sxw * sxw) / det
                } else if n > 0.0 {
                    sxx - sx * sx / n
                } else {
                    0.0
                }
            }
            .max(0.0);
        }
    }
    if (res.sqrt() - (t.n as f64).sqrt()).max(0.0).powi(2) > max_err as f64 {
        return None;
    }
    let fits = refit(t, &w[0], &w[1], lay.ccs, &lay.lab16, lay.count);
    // Each partition's endpoints direct, or blue-contracted: stored as (2r - b, 2g - b, b) and
    // swapped, which the decoder halves back. Whichever the decoder reads as meant and fits closer;
    // if neither, the partition's mean color.
    let at = |x: f32| cqt[round_pos(x.clamp(0.0, 255.0))];
    let inv = |f: &[f32; 4]| [2.0 * f[0] - f[2], 2.0 * f[1] - f[2], f[2], f[3]];
    let sum = |c: &[u8; 8], odd: usize| (0..3).map(|ch| cu[c[2 * ch + odd] as usize] as i32).sum::<i32>();
    for (p, (f0, f1)) in fits.iter().enumerate().take(lay.count as usize) {
        let (i0, i1) = (inv(f0), inv(f1));
        let direct: [u8; 8] = std::array::from_fn(|i| at(if i % 2 == 0 { f0 } else { f1 }[i / 2 % 4]));
        let contracted: [u8; 8] = std::array::from_fn(|i| at(if i % 2 == 0 { i1 } else { i0 }[i / 2 % 4]));
        let mut pick: Option<(u32, [u8; 8])> = None;
        for (o, ok) in [
            (direct, sum(&direct, 1) >= sum(&direct, 0)),
            (contracted, sum(&contracted, 1) < sum(&contracted, 0)),
        ] {
            if !ok {
                continue;
            }
            let v: [i32; 8] = std::array::from_fn(|i| cu[o[i] as usize] as i32);
            let e = crate::astc::decode::endpoints(cem, &v[..2 * nv]).expect("LDR mode");
            let err = t.part_error(&e, &lay.lab16, p, &w[0], &w[1], lay.ccs);
            if pick.is_none_or(|(b, _)| err < b) {
                pick = Some((err, o));
            }
        }
        let o = pick.map_or_else(
            || std::array::from_fn(|i| at((f0[i / 2 % 4] + f1[i / 2 % 4]) * 0.5)),
            |(_, o)| o,
        );
        out.colors[2 * nv * p..2 * nv * (p + 1)].copy_from_slice(&o[..2 * nv]);
    }
    let err = polish_parts::<false, true>(t, lay, grid, &mut out, PASSES);
    Some((out, err))
}

/// Squared error over all four channels.
fn sse(a: &[Rgba], b: &[Rgba]) -> u64 {
    let mut s = 0u32;
    for (x, y) in a.iter().zip(b) {
        for c in 0..4 {
            s += (x[c] as i32 - y[c] as i32).pow(2) as u32;
        }
    }
    s as u64
}

/// [`crate::rdo`]'s distortion weight for smooth blocks, over this footprint's texels.
fn smooth_factor(px: &[Rgba], p: &RdoParams) -> f64 {
    let n = px.len() as f32;
    // The four channels side by side, each summed in texel order as one at a time would.
    let mut sum = [0f32; 4];
    for t in px {
        for c in 0..4 {
            sum[c] += t[c] as f32;
        }
    }
    let mean = sum.map(|s| s / n);
    let mut var = [0f32; 4];
    for t in px {
        for c in 0..4 {
            var[c] += (t[c] as f32 - mean[c]).powi(2);
        }
    }
    let max_std = var.iter().fold(0f32, |m, &v| m.max((v / n).sqrt()));
    let k2 = p.smooth_std * p.smooth_std;
    (1.0 + (p.smooth_scale - 1.0) * k2 / (k2 + max_std * max_std)) as f64
}

#[expect(clippy::too_many_arguments, reason = "one chunk's surface, position and settings")]
fn optimize_chunk(
    format: Format,
    fp: &Footprint,
    s: &Surface,
    nx: usize,
    first: usize,
    data: &mut [u8],
    srgb: bool,
    p: &RdoParams,
) -> RdoStats {
    let count = data.len() / 16;
    let n = fp.texels();
    let (bw, bh) = (fp.bw, fp.bh);
    let window = p.window_for(format);
    let max_back = p.max_back_for(format).min(window / 16);
    // `--refine-back` sets the reweight and refit reach for ASTC.
    let refit_back = p.refine_back.unwrap_or(REFIT_BACK);
    let mut model = LzModel::take_cached(window, p.chain_depth);
    model.set_literals(data);
    let lambda = p.lambda as f64;
    let ratio2 = (p.max_rms_ratio as f64).powi(2);
    let floor = (n * 4) as f64 * (p.min_rms as f64).powi(2);
    let original = p.verify_zstd_level.map(|_| data.to_vec());
    let mut stats = RdoStats {
        blocks: count,
        ..Default::default()
    };
    let mut prevs: Vec<Prev> = Vec::with_capacity(count);
    // Boxes of blocks past `refit_back`, for the next blocks' fields.
    let mut spare: Vec<Box<PrevSym>> = Vec::new();
    // The chosen blocks' decodes, for whole copies up to `max_back` back: a ring of that many.
    let slots = max_back.clamp(1, count);
    let mut decs: Vec<Rgba> = vec![[0; 4]; slots * n];
    // Each chosen block's decoded channel sums.
    let mut dsums: Vec<[i32; 4]> = Vec::with_capacity(count);
    let mut px = vec![[0u8; 4]; n];
    let mut dec = [[0u8; 4]; MAX_TEXELS];
    // The base's and the chosen block's weights infilled to the texels, from their decodes.
    let mut dec_tw = [[0u8; MAX_TEXELS]; 2];
    let mut best_tw = [[0u8; MAX_TEXELS]; 2];
    let mut seen = crate::rdo::BlockSet::default();
    let mut heads = crate::rdo::BlockSet::default();
    let mut tails = crate::rdo::BlockSet::default();
    for i in 0..count {
        let bi = first + i;
        read_block(s, bw, bh, (bi % nx) as u32, (bi / nx) as u32, &mut px);
        let t = texels(&px);
        let bs = BlockSums::of(&t);
        let smooth = smooth_factor(&px, p);
        let score = |err: u64, bits: f32| smooth * err as f64 / 16.0 + lambda * bits as f64;
        let base: [u8; 16] = data[i * 16..i * 16 + 16].try_into().unwrap();
        // Read once: the decode and (if the block stays) the fields later blocks build on.
        let base_sym = Symbolic::read(u128::from_le_bytes(base), bw, bh);
        match &base_sym {
            Some(sym) => decode_symbolic_tw(sym, bw, bh, srgb, &mut dec[..n], &mut dec_tw),
            None => decode_block(&base, bw, bh, srgb, &mut dec[..n]),
        }
        let base_err = sse(&px, &dec[..n]);
        let (base_bits, base_rep) = model.cost(&base);
        let min_bits = lambda * model.min_cost(16) as f64;
        // Error budget: a multiple of the base error, plus a floor so near-lossless blocks still
        // have room to move.
        let limit = (base_err as f64 * ratio2 + floor) as u64;
        let mut best: (f64, [u8; 16], f32, LzState) = (score(base_err, base_bits), base, base_bits, base_rep);
        let consider = |c: [u8; 16], err: u64, best: &mut (f64, [u8; 16], f32, LzState)| {
            if err > limit {
                return;
            }
            let dist = smooth * err as f64 / 16.0;
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
                // The error is at least each channel's summed difference squared over the texels
                // (Cauchy-Schwarz): too much, and `consider` would reject it, so skip the decode.
                let lb =
                    (0..4).map(|c| ((bs.sx[c] - dsums[j][c]) as f64).powi(2)).sum::<f64>() / n as f64 * (1.0 - 1e-9);
                let floor = if model.may_continue(&prevs[j].bytes) {
                    0.0
                } else {
                    min_bits
                };
                if lb > limit as f64 || smooth * lb / 16.0 + floor >= best.0 {
                    continue;
                }
                let k = j % slots * n;
                consider(prevs[j].bytes, sse(&px, &decs[k..k + n]), &mut best);
            }
        }
        heads.clear();
        tails.clear();
        for d in 1..=refit_back.min(i) {
            let j = i - d;
            let Some(ps) = &prevs[j].sym else { continue };
            let PrevSym {
                sym,
                lay,
                gi,
                eps,
                refit,
            } = &**ps;
            let v = u128::from_le_bytes(prevs[j].bytes);
            let below = 128 - sym.mode.weight_bits();
            let config = if sym.partitions == 1 { 17 } else { 29 };
            // Candidates from the same fields come out the same: each head and each set of weights
            // (with its mode, partitioning and second-plane channel) once.
            // A candidate must beat the best on distortion alone (with a little room for rounding
            // in the bounds), and on distortion plus the fewest bits a block can cost unless it
            // continues the previous match. Both kinds keep this block's first byte (its mode's low
            // bits), so whether one can is known before building it.
            let floor = if model.may_continue(&prevs[j].bytes) {
                0.0
            } else {
                min_bits
            };
            let cap = (limit as f64).min((best.0 - floor).max(0.0) * 16.0 / smooth) as f32 * 1.001 + 1.0;
            let mut cands = [None, None];
            if heads.insert(v & ((1u128 << below) - 1)) {
                cands[0] = reweight(fp, &t, lay, *gi, sym, eps, &bs, cap);
            }
            let tail = (v & ((1u128 << config) - 1)) | (sym.ccs as u128) << config | (v >> below) << below;
            if tails.insert(tail)
                && let Some(rb) = refit
            {
                cands[1] = refit_to(fp, &t, lay, *gi, sym, rb, &bs, cap);
            }
            for (c, err) in cands.into_iter().flatten() {
                let Some(b) = c.write() else { continue };
                if seen.insert(b) {
                    let b = b.to_le_bytes();
                    debug_assert_eq!(b[0], prevs[j].bytes[0]);
                    debug_assert_eq!(err as u64, {
                        let mut d = [[0u8; 4]; MAX_TEXELS];
                        decode_block(&b, bw, bh, srgb, &mut d[..n]);
                        sse(&px, &d[..n])
                    });
                    consider(b, err as u64, &mut best);
                }
            }
        }
        stats.est_bits_before += base_bits as f64;
        stats.est_bits_after += best.2 as f64;
        if best.1 != base {
            stats.changed += 1;
        }
        data[i * 16..i * 16 + 16].copy_from_slice(&best.1);
        // Block `i - max_back`'s slot: no later block copies it whole.
        let di = &mut decs[i % slots * n..][..n];
        let (sym, tw) = if best.1 == base {
            di.copy_from_slice(&dec[..n]);
            (base_sym, &dec_tw)
        } else {
            let sym = Symbolic::read(u128::from_le_bytes(best.1), bw, bh);
            match &sym {
                Some(sym) => decode_symbolic_tw(sym, bw, bh, srgb, di, &mut best_tw),
                None => decode_block(&best.1, bw, bh, srgb, di),
            }
            (sym, &best_tw)
        };
        dsums.push(std::array::from_fn(|c| di.iter().map(|d| d[c] as i32).sum()));
        prevs.push(Prev {
            bytes: best.1,
            sym: if refit_back > 0 {
                sym.and_then(|s| prev_sym(fp, s, tw, &mut spare))
            } else {
                None
            },
        });
        // No later block reweights or refits block `i - refit_back`: its box goes to the next block
        // (a chunk's worth is megabytes of fresh pages, and a box a heap call).
        if let Some(old) = i.checked_sub(refit_back)
            && let Some(b) = prevs[old].sym.take()
        {
            spare.push(b);
        }
        model.push(&best.1, best.3);
    }
    model.give_back();
    // Safety net: the rate model is an estimate. If real Zstd says RDO did not help this chunk,
    // keep the base encoding.
    if let (Some(level), Some(original), true) = (p.verify_zstd_level, original, stats.changed > 0) {
        let level = level.min(VERIFY_MAX_LEVEL);
        let size = crate::rdo::zstd_size;
        // Only a chunk Zstd level 1 says RDO grew by more than a few bytes gets the check at `level`
        // (see [`VERIFY_MAX_LEVEL`] and [`VERIFY_SLACK`]).
        let (now, was) = (size(data, 1), size(&original, 1));
        if now > was && (level == 1 || now > was + VERIFY_SLACK && size(data, level) >= size(&original, level)) {
            data.copy_from_slice(&original);
            stats.changed = 0;
            stats.est_bits_after = stats.est_bits_before;
            stats.reverted_chunks = 1;
        }
    }
    stats
}

/// The check runs at most at this level, a fraction of level 19's time. It keeps or reverts the same
/// chunks as level 19 for all 24 Kodak images (6x6, λ 0.25, window 1) and for 146 of 150 encodes of
/// 16 Poly Haven, 6 synthetic and 8 Kodak images (4x4 to 8x8, λ 0.5 to 4); the other four end up
/// within 41 bytes of level 19's Zstd-19 size (three larger, one smaller), at the same PSNR.
///
/// Only chunks Zstd level 1 says RDO grew get this check. Over 14,480 chunks of 24 Kodak, 16 Poly
/// Haven and 6 synthetic images (footprints 4x4, 6x6 and 8x8; λ 0.3, 1 and 2 at window 1, λ 0.5 and
/// 2 at the full window) that is 186 chunks, and the result comes to 330 bytes over a perfect level-19
/// check across all of them. Checking every chunk level 1 did not shrink by about the rate model's
/// estimate (1,461 chunks) came to 766, and no check at all to 491, with one chunk 8.2% larger.
const VERIFY_MAX_LEVEL: i32 = 15;

/// Bytes a chunk may grow at Zstd level 1 and still be kept without the check at the output's level.
/// At the small default λ most chunks the check reads grew this little, and level 19 keeps nearly all
/// of them. Over 62 images (24 Kodak, 32 Poly Haven, 6 synthetic) at the defaults (4x4, 5x5, 6x5, 8x8)
/// it skips 314 of 463 level-15 checks and lands 56 bytes over a perfect level-19 check, against 78
/// for checking all of them. At λ 0.5 (4x4) and 2 (6x6) no chunk grows at level 1.
const VERIFY_SLACK: usize = 4;

/// Runs RDO in place over one level's ASTC blocks (`data`, raster order, as
/// [`super::encode_surface`] writes them) of surface `s`.
pub fn optimize(format: Format, s: &Surface, srgb: bool, data: &mut [u8], p: &RdoParams) -> RdoStats {
    assert!(format.is_astc(), "ASTC RDO for {format}");
    let blocks = data.len() / 16;
    if !p.enabled() || blocks == 0 {
        return RdoStats {
            blocks,
            ..Default::default()
        };
    }
    crate::sched::run(vec![chunk_tasks(format, s, srgb, data, *p)])[0]
}

/// [`optimize`]'s chunks as independent tasks, for [`crate::sched::run`] to run beside other levels'
/// chunks. RDO must be enabled.
pub(crate) fn chunk_tasks<'a>(
    format: Format,
    s: &'a Surface,
    srgb: bool,
    data: &'a mut [u8],
    p: RdoParams,
) -> Vec<crate::sched::Task<'a>> {
    assert!(format.is_astc(), "ASTC RDO for {format}");
    let (bw, bh) = format.block_dims();
    let fp = Footprint::get(bw, bh);
    let nx = s.width.div_ceil(bw) as usize;
    let chunk = (p.chunk_bytes_for(format) / 16).max(1);
    data.chunks_mut(chunk * 16)
        .enumerate()
        .map(|(k, d)| {
            crate::sched::Task::new(d.len() / 16, move || {
                optimize_chunk(format, fp, s, nx, k * chunk, d, srgb, &p)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every candidate's error matches its real decode (the debug assertion in [`optimize_chunk`]),
    /// and RDO moves blocks and lowers the estimated rate, at every footprint, with and without alpha.
    #[test]
    fn candidates_score_their_decode() {
        for s in [crate::synth::bricks(64), crate::synth::cutout(64)] {
            for format in Format::ALL.into_iter().filter(|f| f.is_astc()) {
                let (bw, bh) = format.block_dims();
                let mut data = super::super::encode_surface(&s, bw, bh, true, 0);
                let p = RdoParams {
                    lambda: 4.0,
                    verify_zstd_level: None,
                    ..Default::default()
                };
                let st = optimize(format, &s, true, &mut data, &p);
                assert!(st.changed > 0, "{format}");
                assert!(st.est_bits_after < st.est_bits_before, "{format}");
            }
        }
    }
}
