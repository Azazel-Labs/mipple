//! End-to-end encode/decode: image -> mips -> base blocks -> RDO -> [`EncodedTexture`].

use crate::alpha::{self, AlphaMode};
use crate::format::{BlockOpts, EntropyMode, Format};
use crate::metrics::normal_from_rg;
use crate::mips::{self, MipAlpha, MipFilter};
use crate::rdo::{self, RdoParams, RdoStats};
use crate::surface::{Block, Surface};
use rayon::prelude::*;
use std::borrow::Cow;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::Instant;
// `std::time::Instant::now` panics on wasm32-unknown-unknown (the browser demo in web/).
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use web_time::Instant;

/// Encoder settings.
#[derive(Clone, Debug)]
pub struct EncodeOptions {
    pub format: Format,
    /// 0..=[`crate::format::MAX_EFFORT`], higher is slower and better.
    pub effort: u8,
    /// Data is sRGB-encoded color (affects mip filtering, the KTX2 vkFormat/DFD and, for ASTC,
    /// which decode mode the blocks are encoded for).
    pub srgb: bool,
    /// Input is a tangent-space normal map (renormalized mips).
    pub normal_map: bool,
    /// Entropy-reduced base encoding (BC7); see [`BlockOpts::reduce_entropy`].
    pub entropy: EntropyMode,
    /// Generate a full mip chain.
    pub mips: bool,
    /// What alpha means: with transparency, the colour under transparent texels is bled from the
    /// visible colour around it and freed for the encoder, and mips weight RGB by alpha
    /// ([`crate::alpha`]). [`AlphaMode::Auto`] (default) picks it for BC3, BC7, ETC2 RGBA8 and ASTC
    /// colour textures.
    pub alpha: AlphaMode,
    /// An alpha-test cutoff (0-1) whose coverage every mip keeps (cutouts drawn with alpha test or
    /// alpha to coverage); `None` averages alpha plainly.
    pub alpha_cutoff: Option<f32>,
    /// RDO settings (`lambda == 0` disables RDO). [`EncodeOptions::new`] leaves RDO off, except
    /// for ASTC, which gets a light pass ([`RdoParams::default_for`]). BC6H RDO runs in
    /// [`encode_hdr`].
    pub rdo: RdoParams,
    /// Where base encoding runs: [`GpuMode::Auto`] (default) uses the GPU where its encoder is the
    /// faster one ([`crate::gpu::Gpu::preferred`]). Formats without a GPU encoder, builds without
    /// the `gpu` feature and machines without a usable GPU use the CPU in every mode.
    pub gpu: GpuMode,
}

/// What a texture holds ([`EncodeOptions::with_role`]): sets how its values are read and how
/// errors are weighed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum Role {
    /// Colour: sRGB for formats that store colour ([`Format::default_srgb`]), alpha as
    /// transparency where the format keeps it.
    #[default]
    Albedo,
    /// A tangent-space normal map: linear, renormalized mips, Z reconstructed from X and Y for
    /// BC5 and EAC RG11, whose RDO weighs each block's error by how far it moves the unit normal
    /// ([`RdoParams::normal_map`]).
    Normal,
    /// Linear data channels (roughness, AO, height, packed masks): no sRGB, alpha as data.
    Mask,
}

/// Where base encoding runs ([`EncodeOptions::gpu`]). Every GPU encoder writes the same blocks as
/// the CPU encoder, so this only changes speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GpuMode {
    /// The GPU for formats whose GPU encoder is faster than the CPU's, else the CPU.
    #[default]
    Auto,
    /// The GPU for every format it has an encoder for.
    On,
    /// The CPU only.
    Off,
}

impl EncodeOptions {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            effort: crate::format::DEFAULT_EFFORT,
            srgb: format.default_srgb(),
            normal_map: false,
            entropy: EntropyMode::default(),
            mips: true,
            alpha: AlphaMode::Auto,
            alpha_cutoff: None,
            rdo: RdoParams::default_for(format),
            gpu: GpuMode::Auto,
        }
    }

    /// These options for a texture holding `role`: sets [`Self::srgb`] and [`Self::normal_map`].
    pub fn with_role(mut self, role: Role) -> Self {
        self.normal_map = role == Role::Normal;
        self.srgb = role == Role::Albedo && self.format.default_srgb();
        self
    }

    /// The per-block settings these options imply.
    pub fn block_opts(&self) -> BlockOpts {
        BlockOpts {
            effort: self.effort,
            reduce_entropy: self.entropy.resolve(self.rdo.lambda),
            srgb: self.srgb && self.format.supports_srgb(),
            luma_alpha: self.normal_map && self.format.is_astc(),
        }
    }

    /// The per-block settings of the base encoding, on the CPU or the GPU: [`Self::block_opts`]
    /// with BC4/BC5's effort floor under RDO ([`base_block_opts`]).
    pub fn base_opts(&self) -> BlockOpts {
        base_block_opts(self.format, self.block_opts(), rdo_params(self).enabled())
    }

    /// Whether alpha is treated as transparency ([`Self::alpha`]): only for formats that keep
    /// alpha (BC3, BC7, ETC2 RGBA8, ASTC) and never for normal maps.
    pub fn alpha_transparency(&self) -> bool {
        (matches!(
            self.format,
            Format::Bc3 | Format::Bc7 | Format::Etc2Rgba | Format::Etc2RgbA1
        ) || self.format.is_astc())
            && !self.normal_map
            && match self.alpha {
                AlphaMode::Auto => self.srgb,
                AlphaMode::Transparency => true,
                AlphaMode::Data => false,
            }
    }

    /// How mips treat alpha.
    pub fn mip_alpha(&self) -> MipAlpha {
        MipAlpha {
            transparency: self.alpha_transparency(),
            coverage_cutoff: self.alpha_cutoff,
        }
    }

    pub fn mip_filter(&self) -> MipFilter {
        if self.normal_map {
            MipFilter::Normal
        } else if self.srgb {
            MipFilter::Srgb
        } else {
            MipFilter::Linear
        }
    }
}

/// One encoded mip level: raw GPU blocks in raster order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// A fully encoded texture, ready to be written to KTX2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedTexture {
    pub format: Format,
    pub srgb: bool,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Level>,
    /// How a sampler should map the stored channels to RGBA, as KTX2's `KTXswizzle` spells it
    /// (`None`: as stored). An ASTC normal map's is [`ASTC_NORMAL_SWIZZLE`].
    pub swizzle: Option<String>,
}

/// The `KTXswizzle` of an ASTC normal map, stored as `rrrg` (X in red, green and blue, Y in alpha):
/// X and Y to red and green, blue 0 and alpha 1. A shader reading the stored channels takes X from
/// green and Y from alpha (`.ga`), as for astcenc's `-normal`.
pub const ASTC_NORMAL_SWIZZLE: &str = "ga01";

/// An ASTC normal map's stored layout, `rrrg`: X (red) in red, green and blue, Y (green) in alpha.
/// The ASTC encoder writes these gray blocks as luminance+alpha (CEM 4 and 5), whose endpoints
/// spend nothing on a blue channel, and a dual-plane block gives X and Y separate weights, as BC5
/// does. [`decode_blocks`] undoes it.
pub fn astc_normal_swizzle(s: &Surface) -> Surface {
    Surface {
        pixels: s.pixels.iter().map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        width: s.width,
        height: s.height,
    }
}

/// Timing and RDO statistics for one encode.
#[derive(Clone, Debug, Default)]
pub struct EncodeReport {
    /// The GPU adapter that base-encoded the levels, or `None` for the CPU.
    pub gpu: Option<String>,
    pub base_secs: f64,
    pub rdo_secs: f64,
    pub rdo: RdoStats,
}

/// Base-encodes every block of a surface in parallel. Returns (source blocks, encoded blocks).
///
/// # Panics
///
/// For ASTC, whose blocks do not pair with 4x4 source blocks: use [`encode`] (or
/// [`crate::astc::encode::encode_surface`]) instead.
pub fn encode_base(surface: &Surface, format: Format, opts: impl Into<BlockOpts>) -> (Vec<Block>, Vec<[u8; 16]>) {
    assert!(
        !format.is_astc(),
        "encode_base pairs blocks with 4x4 source blocks; {format} blocks are {:?} texels",
        format.block_dims()
    );
    let opts = opts.into();
    let src = surface.blocks();
    let enc = if direct(format, opts) {
        encode_surface_blocks(surface, format, opts)
    } else {
        encode_src_blocks(&src, format, opts)
    };
    (src, enc)
}

/// Effort floor for RDO's base encodings: BC7's plain search ([`rdo_bases`]) and BC4/BC5
/// ([`base_block_opts`]).
pub const RDO_BASE_EFFORT: u8 = 8;

/// `bo` for a BC4/BC5 base that RDO starts from: effort [`RDO_BASE_EFFORT`] or more (endpoint
/// window radius 4 instead of 1 at the default effort). RDO keeps most of the accuracy it adds,
/// and the base costs little next to RDO.
pub fn base_block_opts(format: Format, bo: BlockOpts, rdo: bool) -> BlockOpts {
    match format {
        Format::Bc4 | Format::Bc5 if rdo => BlockOpts {
            effort: bo.effort.max(RDO_BASE_EFFORT),
            ..bo
        },
        _ => bo,
    }
}

/// One surface's starting point for RDO, made as [`encode`] makes it: the base encoding and the
/// alternate bases each block may start from (BC7 only). `src` is `surface.blocks()`, or empty when
/// the format reads the surface directly and `rdo` is off.
///
/// # Panics
///
/// When `rdo` is set for ASTC, whose RDO pass starts from its plain encode.
pub fn rdo_bases(
    surface: &Surface,
    src: &[Block],
    format: Format,
    bo: BlockOpts,
    rdo: bool,
) -> (Vec<[u8; 16]>, Vec<Vec<[u8; 16]>>) {
    assert!(
        !(rdo && format.is_astc()),
        "{format}'s RDO starts from its plain encode"
    );
    let bo = base_block_opts(format, bo, rdo);
    // BC7 RDO: the plain search at RDO_BASE_EFFORT or more, as the base or (entropy-reduced
    // base) the second base. Its cost is small next to RDO's, and RDO keeps much of the
    // quality it adds (Kodak subset vs Oodle 2.9.16 on Zstd-19, λ 10-40 at equal RMSE:
    // -e 5 +8.3% -> +4.0%).
    let bc7_rdo = format == Format::Bc7 && rdo;
    let plain = BlockOpts {
        reduce_entropy: false,
        effort: if bc7_rdo {
            bo.effort.max(RDO_BASE_EFFORT)
        } else {
            bo.effort
        },
        ..bo
    };
    let base = if format == Format::Bc7 && !bo.reduce_entropy {
        plain
    } else {
        bo
    };
    let encode = |o: BlockOpts| {
        if direct(format, o) {
            encode_surface_blocks(surface, format, o)
        } else {
            encode_src_blocks(src, format, o)
        }
    };
    let blocks = encode(base);
    // BC7: with an entropy-reduced base, the plain (error-only) encoding; always, a mode-6-only
    // encoding, whose blocks share one layout and so give later blocks more to copy (Oodle's RDO
    // output is mostly mode 6).
    let mut alts = Vec::new();
    if bc7_rdo {
        if bo.reduce_entropy {
            alts.push(match crate::bc7::rdo_alt_cfg(plain) {
                Some(cfg) => unflatten(format, &crate::bc7::fast::encode_surface(surface, cfg, plain)),
                None => encode(plain),
            });
        }
        alts.push(unflatten(
            format,
            &crate::bc7::fast::encode_surface(surface, crate::bc7::mode6_cfg(), plain),
        ));
    }
    (blocks, alts)
}

/// Formats whose encoder reads the surface directly instead of materialized blocks. ASTC must:
/// its blocks are not 4x4.
fn direct(format: Format, opts: BlockOpts) -> bool {
    match format {
        Format::Bc7 => crate::bc7::fast_cfg(opts).is_some(),
        _ => true,
    }
}

/// Raw blocks (`format.block_bytes()` each, raster order over [`Format::block_dims`] blocks) of a
/// direct-path format.
fn encode_surface_raw(surface: &Surface, format: Format, opts: BlockOpts) -> Vec<u8> {
    match format {
        Format::Bc1 => crate::bc1::encode_surface(surface, opts.effort),
        Format::Bc3 => crate::bc3::encode_surface(surface, opts.effort),
        Format::Bc6h | Format::Bc6hSf16 => crate::bc6h::encode_surface_ldr(surface, opts.effort, format.is_signed()),
        Format::Bc4 => crate::bc4::encode_surface(surface, 1, opts.effort),
        Format::Bc5 => crate::bc4::encode_surface(surface, 2, opts.effort),
        Format::Etc2Rgb => crate::etc::encode_surface(surface, opts.effort),
        Format::Etc2Rgba => crate::etc::encode_surface_rgba(surface, opts.effort),
        Format::Etc2RgbA1 => crate::etc::encode_surface_a1(surface, opts.effort),
        Format::EacR11 => crate::eac::encode_surface(surface, (0, 1), crate::eac::Kind::R11, opts.effort),
        Format::EacRg11 => crate::eac::encode_surface(surface, (0, 2), crate::eac::Kind::R11, opts.effort),
        Format::EacR11Snorm => crate::eac::encode_surface(surface, (0, 1), crate::eac::Kind::R11Snorm, opts.effort),
        Format::EacRg11Snorm => crate::eac::encode_surface(surface, (0, 2), crate::eac::Kind::R11Snorm, opts.effort),
        crate::format::astc!() => {
            let (bw, bh) = format.block_dims();
            crate::astc::encode::encode_surface(surface, bw, bh, opts.srgb, opts.effort, opts.luma_alpha)
        }
        Format::Bc7 => crate::bc7::fast::encode_surface(
            surface,
            crate::bc7::fast_cfg(opts).expect("BC7 direct path without a fast configuration"),
            opts,
        ),
    }
}

/// Expands raw blocks to the 16-byte slots RDO works on.
fn unflatten(format: Format, raw: &[u8]) -> Vec<[u8; 16]> {
    let n = format.block_bytes();
    raw.chunks_exact(n)
        .map(|c| {
            let mut b = [0u8; 16];
            b[..n].copy_from_slice(c);
            b
        })
        .collect()
}

fn encode_surface_blocks(surface: &Surface, format: Format, opts: BlockOpts) -> Vec<[u8; 16]> {
    unflatten(format, &encode_surface_raw(surface, format, opts))
}

fn encode_src_blocks(src: &[Block], format: Format, opts: BlockOpts) -> Vec<[u8; 16]> {
    let w = format.error_weights();
    if format == Format::Bc7 && opts.searched().effort <= 1 {
        // BC7 search levels 0-1 encode 8 blocks per SIMD batch.
        let opts = opts.searched();
        src.par_chunks(crate::simd::LANES)
            .flat_map_iter(|c| crate::bc7::encode_batch(c, &w, opts))
            .collect()
    } else {
        src.par_iter().map(|b| format.encode_block(b, &w, opts)).collect()
    }
}

/// Concatenates the meaningful bytes of each block.
pub fn flatten(format: Format, blocks: &[[u8; 16]]) -> Vec<u8> {
    let n = format.block_bytes();
    let mut out = Vec::with_capacity(blocks.len() * n);
    for b in blocks {
        out.extend_from_slice(&b[..n]);
    }
    out
}

/// The surfaces [`encode`] base-encodes, top level first: the mip chain (or the surface alone).
/// With alpha as transparency, the top level's hidden colour is bled (the mips bleed their own)
/// and each level's colour beyond the visible texels' ring is freed. [`encode_prepared`] encodes
/// them.
pub fn prepare<'a>(surface: &'a Surface, opts: &EncodeOptions) -> Vec<Cow<'a, Surface>> {
    let transparency = opts.alpha_transparency() && surface.has_alpha();
    let top = if transparency {
        let mut s = surface.clone();
        alpha::bleed(&mut s);
        Cow::Owned(s)
    } else {
        Cow::Borrowed(surface)
    };
    let levels = if opts.mips {
        mips::chain_with(&top, opts.mip_filter(), opts.mip_alpha())
            .into_iter()
            .map(Cow::Owned)
            .collect()
    } else {
        vec![top]
    };
    if opts.block_opts().luma_alpha {
        return levels.iter().map(|l| Cow::Owned(astc_normal_swizzle(l))).collect();
    }
    if !transparency {
        return levels;
    }
    levels
        .into_iter()
        .map(|l| alpha::hide(&l).map_or(l, Cow::Owned))
        .collect()
}

/// RDO settings for `opts`: entropy reduction is on when the base encoding asks for it.
fn rdo_params(opts: &EncodeOptions) -> RdoParams {
    RdoParams {
        reduce_entropy: opts.rdo.reduce_entropy || opts.block_opts().reduce_entropy,
        // The 8-bit RDO does not apply to BC6H, whose own pass runs in `encode_hdr`.
        lambda: if opts.format.is_bc6h() { 0.0 } else { opts.rdo.lambda },
        normal_map: opts.normal_map,
        ..opts.rdo
    }
}

/// Whether a GPU kernel can make the base blocks for `opts`: the format has one at this effort,
/// and the encode is not BC7 RDO, which starts from a higher-effort base and alternate bases that
/// only the CPU makes. Other formats' RDO starts from the GPU's base.
pub fn gpu_eligible(opts: &EncodeOptions) -> bool {
    let bc7_rdo = opts.format == Format::Bc7 && rdo_params(opts).enabled();
    !bc7_rdo && crate::gpu::Gpu::has_kernel(opts.format, opts.base_opts())
}

/// Encodes a surface (and optionally its mip chain).
pub fn encode(surface: &Surface, opts: &EncodeOptions) -> (EncodedTexture, EncodeReport) {
    encode_batch(&[(surface, opts)]).pop().unwrap()
}

/// Encodes several surfaces, each as [`encode`] would (the same blocks), but with every level's RDO
/// chunks, across all of them, in one pool: [`encode`] of one at a time leaves threads idle once a
/// texture has fewer chunks left than there are threads. The reports share one RDO pass, so each
/// gives that pass's wall time.
pub fn encode_batch(items: &[(&Surface, &EncodeOptions)]) -> Vec<(EncodedTexture, EncodeReport)> {
    let prepared: Vec<Vec<Cow<Surface>>> = items.iter().map(|&(s, opts)| prepare(s, opts)).collect();
    let textures: Vec<_> = items
        .iter()
        .zip(&prepared)
        .map(|(&(_, opts), levels)| {
            let (name, bases) = gpu_bases(levels, opts);
            let mut bases = bases.into_iter();
            (&levels[..], opts, name, move |_: usize, _: &Surface| bases.next())
        })
        .collect();
    encode_with(textures)
}

/// GPU base blocks for every level of one texture when the GPU makes them (one submission, so the
/// GPU is waited on once per texture): the adapter's name and the levels' raw blocks.
fn gpu_bases(levels: &[Cow<Surface>], opts: &EncodeOptions) -> (Option<String>, Vec<Vec<u8>>) {
    let use_gpu = gpu_eligible(opts)
        && match opts.gpu {
            GpuMode::Auto => crate::gpu::Gpu::preferred(opts.format, opts.base_opts()),
            GpuMode::On => true,
            GpuMode::Off => false,
        };
    let gpu = use_gpu
        .then(crate::gpu::Gpu::shared)
        .flatten()
        .filter(|g| g.encodes(opts.format));
    let bases = gpu.and_then(|g| {
        let refs: Vec<&Surface> = levels.iter().map(|l| &**l).collect();
        g.encode_surfaces(&refs, opts.format, opts.base_opts())
    });
    let name = bases.as_ref().and(gpu).map(|g| g.info().name.clone());
    (name, bases.unwrap_or_default())
}

/// [`encode`] of [`prepare`]'s levels with base blocks the caller already made, one entry per level
/// (raw blocks as [`crate::gpu::Gpu::encode_surfaces_async`] returns them; `None` encodes that level
/// on the CPU). This is the browser's way in: it cannot wait for the GPU inside [`encode`]. `gpu`
/// names the adapter for the report. The bases are ignored unless [`gpu_eligible`] holds, and the
/// result is the same as [`encode`]'s, since every GPU kernel writes the CPU's blocks.
pub fn encode_prepared(
    levels: &[Cow<Surface>],
    opts: &EncodeOptions,
    gpu: Option<&str>,
    mut bases: Vec<Option<Vec<u8>>>,
) -> (EncodedTexture, EncodeReport) {
    let eligible = gpu_eligible(opts);
    let base = move |i: usize, _: &Surface| bases.get_mut(i).and_then(Option::take).filter(|_| eligible);
    encode_with(vec![(levels, opts, gpu.map(str::to_owned), base)])
        .pop()
        .unwrap()
}

/// One level between its base encoding and RDO.
enum Work {
    /// Final raw blocks: no RDO.
    Raw(Vec<u8>),
    /// ASTC blocks for ASTC's RDO pass, which reads the surface itself (its footprints are not 4x4
    /// source blocks).
    Astc(Vec<u8>),
    /// Source blocks, base blocks and alternate bases (BC7) for the 4x4 RDO pass.
    Blocks {
        src: Vec<Block>,
        blocks: Vec<[u8; 16]>,
        alts: Vec<Vec<[u8; 16]>>,
    },
}

/// Base-encodes one level, taking its base blocks from `on_gpu` when given (a GPU encode) and from
/// the CPU otherwise.
fn base_work(level: &Surface, opts: &EncodeOptions, rdo_on: bool, on_gpu: Option<Vec<u8>>) -> Work {
    if !rdo_on && (on_gpu.is_some() || direct(opts.format, opts.block_opts())) {
        // Straight to raw bytes: no source blocks, no 16-byte slots.
        return Work::Raw(on_gpu.unwrap_or_else(|| encode_surface_raw(level, opts.format, opts.block_opts())));
    }
    if opts.format.is_astc() {
        let data = encode_surface_raw(level, opts.format, opts.block_opts());
        return if rdo_on { Work::Astc(data) } else { Work::Raw(data) };
    }
    // Source blocks are only materialized when an encoder or RDO needs them.
    let src = if rdo_on || !direct(opts.format, opts.block_opts()) {
        level.blocks()
    } else {
        Vec::new()
    };
    // A GPU base is RDO's only starting point (the CPU adds alternate bases for BC7).
    let (blocks, alts) = match on_gpu {
        Some(raw) => (unflatten(opts.format, &raw), Vec::new()),
        None => rdo_bases(level, &src, opts.format, opts.block_opts(), rdo_on),
    };
    if rdo_on {
        Work::Blocks { src, blocks, alts }
    } else {
        Work::Raw(flatten(opts.format, &blocks))
    }
}

/// One texture for [`encode_with`]: its levels, options, GPU adapter name and base-block source.
type Texture<'a, B> = (&'a [Cow<'a, Surface>], &'a EncodeOptions, Option<String>, B);

/// Encodes each texture's levels, taking each level's base blocks from `base(index, level)` when it
/// returns them (a GPU encode) and from the CPU otherwise: first every level's base encoding (the CPU
/// ones in parallel), then RDO over every level's chunks in one pool ([`crate::sched::run`]).
fn encode_with<B: FnMut(usize, &Surface) -> Option<Vec<u8>>>(
    mut textures: Vec<Texture<'_, B>>,
) -> Vec<(EncodedTexture, EncodeReport)> {
    let mut reports: Vec<EncodeReport> = textures.iter().map(|_| EncodeReport::default()).collect();
    let t0 = Instant::now();
    // GPU bases first (in order), then every level's CPU base at once: a small level's encode
    // does not fill the threads by itself.
    let mut items = Vec::new();
    for (ti, (levels, _, gpu, base)) in textures.iter_mut().enumerate() {
        let levels: &[Cow<'_, Surface>] = levels;
        for (i, level) in levels.iter().enumerate() {
            let on_gpu = base(i, level);
            if on_gpu.is_some() {
                reports[ti].gpu.clone_from(gpu);
            }
            items.push((ti, level, on_gpu));
        }
    }
    let all_opts: Vec<&EncodeOptions> = textures.iter().map(|t| t.1).collect();
    let base_one = |(ti, level, on_gpu): (usize, &Cow<'_, Surface>, Option<Vec<u8>>)| {
        let opts = all_opts[ti];
        (ti, base_work(level, opts, rdo_params(opts).enabled(), on_gpu))
    };
    let flat: Vec<(usize, Work)> = items.into_par_iter().map(base_one).collect();
    let base_secs = t0.elapsed().as_secs_f64();
    let mut works: Vec<Vec<Work>> = textures.iter().map(|_| Vec::new()).collect();
    for (ti, w) in flat {
        works[ti].push(w);
    }
    for r in &mut reports {
        r.base_secs = base_secs;
    }
    let t1 = Instant::now();
    // One job per level with RDO to do, in texture then level order.
    let mut jobs = Vec::new();
    let mut job_of = Vec::new();
    for (ti, ((levels, opts, _, _), w)) in textures.iter().zip(works.iter_mut()).enumerate() {
        let p = rdo_params(opts);
        let ew = opts.format.error_weights();
        for (level, work) in levels.iter().zip(w.iter_mut()) {
            let tasks = match work {
                Work::Raw(_) => continue,
                Work::Astc(data) => {
                    crate::astc::encode::rdo::chunk_tasks(opts.format, level, opts.block_opts().srgb, data, p)
                }
                Work::Blocks { src, blocks, alts } => {
                    let row = (level.width as usize).div_ceil(4);
                    rdo::chunk_tasks(opts.format, src, blocks, alts, ew, RdoParams { row, ..p })
                }
            };
            jobs.push(tasks);
            job_of.push(ti);
        }
    }
    let stats = crate::sched::run(jobs);
    let rdo_secs = t1.elapsed().as_secs_f64();
    for (ti, s) in job_of.into_iter().zip(stats) {
        reports[ti].rdo = RdoStats::merge(reports[ti].rdo, s);
    }
    textures
        .iter()
        .zip(works)
        .zip(reports)
        .map(|(((levels_in, opts, _, _), w), mut report)| {
            if rdo_params(opts).enabled() {
                report.rdo_secs = rdo_secs;
            }
            let levels: Vec<Level> = levels_in
                .iter()
                .zip(w)
                .map(|(level, work)| {
                    let data = match work {
                        Work::Raw(data) => {
                            let blocks = data.len() / opts.format.block_bytes();
                            report.rdo = RdoStats::merge(
                                report.rdo,
                                RdoStats {
                                    blocks,
                                    ..Default::default()
                                },
                            );
                            data
                        }
                        Work::Astc(data) => data,
                        Work::Blocks { blocks, .. } => flatten(opts.format, &blocks),
                    };
                    Level {
                        width: level.width,
                        height: level.height,
                        data,
                    }
                })
                .collect();
            let (width, height) = levels_in.first().map_or((0, 0), |l| (l.width, l.height));
            let tex = EncodedTexture {
                format: opts.format,
                srgb: opts.srgb && opts.format.supports_srgb(),
                width,
                height,
                levels,
                swizzle: opts.block_opts().luma_alpha.then(|| ASTC_NORMAL_SWIZZLE.to_owned()),
            };
            (tex, report)
        })
        .collect()
}

/// Encodes an HDR image to BC6H (half floats: unsigned for [`Format::Bc6h`], signed for
/// [`Format::Bc6hSf16`]), with a box-filtered mip chain when `opts.mips`, then RDO when `opts.rdo`
/// enables it ([`crate::bc6h::rdo`]). Uses `opts.format`, `opts.effort`, `opts.mips` and
/// `opts.rdo`; `opts.format` must be BC6H.
pub fn encode_hdr(surface: &crate::hdr::HdrSurface, opts: &EncodeOptions) -> (EncodedTexture, EncodeReport) {
    assert!(opts.format.is_bc6h(), "encode_hdr encodes BC6H");
    let signed = opts.format.is_signed();
    let chain = if opts.mips {
        surface.chain()
    } else {
        vec![surface.clone()]
    };
    let t0 = Instant::now();
    let levels: Vec<Level> = chain
        .iter()
        .map(|l| Level {
            width: l.width,
            height: l.height,
            data: crate::bc6h::encode_surface(l, opts.effort, signed),
        })
        .collect();
    let base_secs = t0.elapsed().as_secs_f64();
    let t1 = Instant::now();
    let mut levels = levels;
    let blocks = levels.iter().map(|l| l.data.len() / 16).sum();
    let rdo = if opts.rdo.enabled() {
        // Every level's chunks in one pool.
        let jobs = chain
            .iter()
            .zip(levels.iter_mut())
            .map(|(l, lv)| crate::bc6h::rdo::chunk_tasks(l, &mut lv.data, opts.rdo, signed))
            .collect();
        crate::sched::run(jobs)
            .into_iter()
            .fold(RdoStats::default(), RdoStats::merge)
    } else {
        RdoStats {
            blocks,
            ..Default::default()
        }
    };
    let report = EncodeReport {
        base_secs,
        rdo_secs: if opts.rdo.enabled() {
            t1.elapsed().as_secs_f64()
        } else {
            0.0
        },
        rdo,
        ..Default::default()
    };
    let tex = EncodedTexture {
        format: opts.format,
        srgb: false,
        width: surface.width,
        height: surface.height,
        levels,
        swizzle: None,
    };
    (tex, report)
}

/// Decodes one mip level of a BC6H texture to linear float RGB.
pub fn decode_level_hdr(tex: &EncodedTexture, level: usize) -> crate::hdr::HdrSurface {
    assert!(tex.format.is_bc6h(), "decode_level_hdr decodes BC6H");
    let l = &tex.levels[level];
    crate::bc6h::decode_surface(&l.data, l.width, l.height, tex.format.is_signed())
}

/// Decodes raw blocks back to an image. For BC5 with `normal_map`, Z is reconstructed into blue;
/// for ASTC with `normal_map`, the stored `rrrg` layout ([`astc_normal_swizzle`]) comes back as X in
/// red (read from green, as a shader would), Y in green (from alpha), Z reconstructed in blue and
/// alpha 255. `srgb` is the decode mode, which only ASTC's decode depends on (other formats ignore
/// it).
pub fn decode_blocks(format: Format, data: &[u8], width: u32, height: u32, srgb: bool, normal_map: bool) -> Surface {
    if format.is_astc() {
        let (bw, bh) = format.block_dims();
        let mut s = crate::astc::decode_rgba(data, width, height, bw, bh, srgb);
        if normal_map {
            for p in &mut s.pixels {
                let (x, y) = (p[1], p[3]);
                let v = normal_from_rg(x, y);
                *p = [x, y, ((v[2] + 1.0) * 127.5 + 0.5) as u8, 255];
            }
        }
        return s;
    }
    let mut s = Surface::new(width, height);
    let n = format.block_bytes();
    let bw = width.div_ceil(4);
    let mut out = [[0u8; 4]; 16];
    for (i, raw) in data.chunks_exact(n).enumerate() {
        format.decode_block(raw, &mut out);
        if format == Format::Bc5 && normal_map {
            for t in &mut out {
                let v = normal_from_rg(t[0], t[1]);
                t[2] = ((v[2] + 1.0) * 127.5 + 0.5) as u8;
            }
        }
        let i = i as u32;
        s.write_block(i % bw, i / bw, &out);
    }
    s
}

/// Decodes one mip level of an encoded texture.
pub fn decode_level(tex: &EncodedTexture, level: usize, normal_map: bool) -> Surface {
    let l = &tex.levels[level];
    decode_blocks(tex.format, &l.data, l.width, l.height, tex.srgb, normal_map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_set_colour_space_and_normal_weighting() {
        let albedo = EncodeOptions::new(Format::Bc7).with_role(Role::Albedo);
        assert!(albedo.srgb && !albedo.normal_map);
        let normal = EncodeOptions::new(Format::Bc7).with_role(Role::Normal);
        assert!(!normal.srgb && normal.normal_map && rdo_params(&normal).normal_map);
        let mask = EncodeOptions::new(Format::Bc7).with_role(Role::Mask);
        assert!(!mask.srgb && !mask.normal_map);
        // Formats without an sRGB variant stay linear as albedo.
        assert!(!EncodeOptions::new(Format::Bc5).with_role(Role::Albedo).srgb);
    }

    /// An ASTC normal map is stored `rrrg` as luminance+alpha blocks, says so in KTX2's
    /// `KTXswizzle`, and decodes with `normal_map` back to X and Y in red and green.
    #[test]
    fn astc_normal_maps_store_x_and_y_as_luminance_alpha() {
        let src = crate::synth::normal_rocks(64);
        let opts = EncodeOptions::new(Format::Astc6x6).with_role(Role::Normal);
        let (tex, _) = encode(&src, &opts);
        assert_eq!(tex.swizzle.as_deref(), Some(ASTC_NORMAL_SWIZZLE));
        let raw = crate::astc::decode_rgba(&tex.levels[0].data, 64, 64, 6, 6, false);
        assert!(raw.pixels.iter().all(|p| p[0] == p[1] && p[1] == p[2]));
        let ktx = crate::ktx2::write(&tex, Some(3), None, crate::ktx2::PrepMode::Off).unwrap();
        let back = crate::ktx2::read(&ktx).unwrap();
        assert_eq!(back.swizzle.as_deref(), Some(ASTC_NORMAL_SWIZZLE));
        let dec = decode_level(&back, 0, true);
        let lit = crate::metrics::normal_lit_psnr(&src, &dec);
        assert!(lit > 30.0, "lit PSNR {lit}");
        // Colour textures keep their layout and no swizzle.
        assert_eq!(encode(&src, &EncodeOptions::new(Format::Astc6x6)).0.swizzle, None);
    }

    /// Textures encoded together, and on any number of threads, come out as each encoded alone: the
    /// chunk schedule does not reach the output.
    #[test]
    fn batch_and_thread_count_keep_the_output() {
        let surfaces = [crate::synth::bricks(96), crate::synth::foliage(80)];
        for format in [Format::Bc7, Format::Bc5, Format::Bc1, Format::Astc6x6] {
            let mut opts = EncodeOptions::new(format);
            opts.effort = 2;
            opts.rdo.lambda = 2.0;
            // Small chunks, so each level has several.
            opts.rdo.chunk_bytes = Some(2048);
            let alone: Vec<EncodedTexture> = surfaces.iter().map(|s| encode(s, &opts).0).collect();
            for threads in [1, 3] {
                let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
                let items: Vec<(&Surface, &EncodeOptions)> = surfaces.iter().map(|s| (s, &opts)).collect();
                let batch = pool.install(|| encode_batch(&items));
                for (a, (b, _)) in alone.iter().zip(&batch) {
                    assert!(a == b, "{format} on {threads} thread(s)");
                }
            }
        }
    }
}
