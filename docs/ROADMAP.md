# Roadmap

Each item should land with a benchmark delta (see `bench-results/`). Major and minor in `Cargo.toml` name the
milestone in progress; every green push to main is released as the next patch (`vX.Y.Z`, with binaries;
`.github/workflows/release.yml`). How things work: [ARCHITECTURE.md](ARCHITECTURE.md). Numbers:
[BENCHMARKS.md](BENCHMARKS.md). Ideas tried and dropped: [DEADENDS.md](DEADENDS.md).

## Next up

1. **BC7 RDO clearly faster than Oodle Texture** at equal or better quality (below).
2. **Close the BC7 SSIM gap to XUBC7** and widen the thinnest cells of the lead over Oodle (below).
3. **GPU kernels and SIMD batch encoders** for ASTC, ETC2 / EAC, BC3 and BC6H.

## Shipped

### v0.1-v0.2 (pre-release)

- [x] BC1, BC4, BC5 and BC7 encoders and decoders, mip generation, KTX2 with Zstd supercompression, DDS I/O.
- [x] A byte-level LZ rate model shared by every format, and an RDO pass over it.
- [x] Head-to-head harness (`mipple score`, `scripts/compete.py`) and the fast regression check
  (`mipple check`, a CI step) with robust (Pareto, piecewise-linear) BD-rate.

### v0.9: competitive parity and Oodle Texture (current)

Goal: meet or beat the quality and speed of the best open encoders per format, then Oodle Texture.

- [x] **Formats:** BC1, BC3, BC4, BC5, BC6H (unsigned and signed, `.hdr` / `.pfm` input), BC7, ETC2 RGB /
  RGB A1 / RGBA8, EAC R11 / RG11 (unsigned and signed), and ASTC LDR at all 14 2D footprints, to KTX2
  (and DDS or `.astc` where the format allows), through the CLI, library, `mipple check` and the web demo.
- [x] **Base encoders:** a SIMD layer (`src/simd.rs`, portable and AVX2, runtime dispatch) and batch
  encoders for BC1, BC4/BC5, BC6H and BC7. Per format, some mipple effort beats each competitor on quality
  and speed at once: BC1 effort 0 against Intel ISPC, BC4/BC5 against ISPC, stb_dxt and rgbcx, BC7 against
  bc7e u0 / u2 / u4 and every ISPC and bc7f profile, BC6H against every ISPC profile, ETC2 against Compressonator,
  and ASTC against astcenc 5.7.1: effort 0 beats fast on PSNR, SSIM, Zstd-19 and Deflate-9 size and time at
  every footprint, and some setting beats thorough on all five at every footprint (43-68% of its time at
  8x8 and up, c01269f). With RDO off, ASTC effort 0 is faster than ISPC fast at 4x4-6x6 with higher PSNR
  and SSIM and smaller files.
- [x] **RDO for every format:** near, localized (learned rankers, CC0 Poly Haven only) and far searches,
  field-swap refinement, deferred decisions, smooth-block protection, a luma term for BC7, a light pass on
  by default for ASTC, chunks in one longest-first pool with output independent of the thread count, and
  a real-Zstd safety net. Against bc7enc_rdo, BD-rate BC7 −33.7%, BC1 −26.8%, BC4 −20.0%, BC5 −26.1%,
  BC3 −15.7%; against Basis, BC7 −54.3% vs UASTC RDO and −8.3% vs XUBC7 at equal PSNR.
- [x] **vs Oodle Texture 2.9.16 (Unreal Engine 5.8), true source:** `scripts/ue_oodle/` gets Oodle's real
  blocks out of a stock UE cook (bit-exact, no engine source) and scores them next to a mipple sweep
  (`bench-results/oodle-ue/`). On Kodak-24, mipple is smaller at equal RMSE and at equal SSIM at every λ
  in BC1, BC4, BC5 and BC7.
- [x] **Normal maps** (`--role normal`): BC5 / EAC RG11 / ASTC RDO weighs X/Y error by how far it moves
  the unit normal; ASTC stores X/Y as luminance+alpha (`rrrg`); `bench` reports lit PSNR.
- [x] **Alpha as transparency** (BC7 / BC3 colour textures by default): alpha-weighted mips, push-pull bleed,
  hidden texels freed for the encoder, coverage-preserving mips (`--alpha-cutoff`).
- [x] **Lossless field split** (`--prep on|auto`): −4% to −8% Zstd-19 without RDO.
- [x] **GPU base encoders** (wgpu, native and WebGPU): BC1 efforts 0-1, BC4/BC5, BC7 efforts 0-5, each
  writing exactly the CPU encoder's blocks; `GpuMode::Auto` picks the GPU only where it beats the CPU.
- [x] **Tooling:** `cargo xtask fmt|clippy|ci`, CI on Linux / Windows / macOS, a release per green push, and
  cloud workflows on fresh GitHub-hosted VMs: `bench.yml` (competitor tables), `ab.yml` (paired timing),
  `profile.yml` (perf on one pinned core) and `oodle.yml` (the Oodle table).

## Open (v0.9.x)

- [ ] **BC7 RDO clearly faster than Oodle** (reopened 2026-11-13). Quality is ahead in every cell; CPU is
  about level. On one pinned P-core, Oodle's share of a UE cook is about 20-22 CPU-seconds for Kodak-24 at
  λ 30, and mipple's λ 1 is about 20 by the cloud A/B estimate (after refinement 64 deep, then the luma
  term at 1.082×); not yet re-timed side by side. Target: under Oodle's CPU at λ 1 with every table cell
  still ahead, then about half.
  - Where λ 1 time goes (a busy-time profile, kodim05): main RDO loop 76%, of which the far search 18% (half
    of it mode-6 re-indexing), the ranker prefilter scan 15%, near candidates 11% and refinement about 10%;
    the two extra bases (`pipeline::rdo_bases`) 20%; Zstd check and KTX2 write about 7%. Switching stages off
    instead (8 Kodak images): refinement 29% of RDO time (files 0.85% larger without it), the deferred
    decision 22% (+0.87%), the far search 19% (+3.0%); without the ranker's picks RDO is 51% slower, since
    they tighten every later bound.
  - Next: structural cuts to the far re-indexing and the second base, from the cross-domain research
    sweep (hash-based block matching from screen-content video coding, lower-bound cascades).
- [ ] **Widen the BC7 lead over Oodle:** the thinnest cells are equal RMSE at λ 20 (−0.7% after the luma
  term) and equal SSIM at λ 40, and per image, smooth-sky shots (kodim07, kodim20) were up to 5.5% behind at
  λ 30-40 on SSIM. Oodle's output has about 3× more index-half repeats. 128 KiB chunks are −0.9% / −1.0% BD-rate (RMSE / SSIM) at the same CPU time, but one image alone
  takes 1.6× the wall time, so they stay off (DEADENDS).
- [ ] **Close the SSIM gap to XUBC7:** at equal SSIM, XUBC7 (bc7e level 2) is still 12.9% smaller than
  mipple's BC7 RDO on Kodak-24 and wins 23 of 24 images, after the luma term took it from +16.2%. SSIM's
  loss sits in near-flat windows (luma variance below 8: a fifth of the windows, 69% of the loss), which an
  error weight alone does not reach without costing RMSE (DEADENDS lists the levers tried).
- [ ] **Far index patterns for BC1:** Oodle reuses far index patterns (about half mipple's distinct BC1
  patterns on kodim18); this needs a cheap way to find patterns that fit (refit candidates and a pattern
  lower bound did not pay; DEADENDS).
- [ ] **BC7 effort 5:** a cheaper p-bit search would gain about +0.05 dB, but the WGSL kernel must follow.
- [ ] **ASTC:** a GPU kernel, a SIMD batch encoder, HDR; for normal maps, closing effort 0's gap to astcenc
  `-normal -fast` at 6x6 and 8x8.
- [ ] **ETC2 / EAC:** a SIMD batch encoder (eight blocks per call, as BC1 / BC6H; what it would widen is the
  scalar glue between kernels, which must keep its order to stay byte-identical) and a GPU kernel.
- [ ] **BC6H and BC3 GPU kernels.** A BC6H kernel was tried and not merged: its exact-integer CPU path was
  1.15-1.32× slower (DEADENDS, branch `wt/bc6h-gpu`).
- [ ] **Competitors:** add bc7g to `scripts/compete.py` when it ships (unreleased, checked 2026-11-16).
  NVTT 3 sits behind an NVIDIA developer login, so it stays a manual download.

## v1.0+: perception and speed

- [ ] **Perceptual distortion:** per-block masking (activity-weighted SSE) and λ normalized across texture
  types, as Oodle does; an NVIDIA FLIP metric in `bench`.
- [ ] **Mesh-aware texel density:** read the model a texture is mapped to (glTF first) and, from each
  triangle's UV-to-surface scale, get every texel's world-space density (texels per metre). With a usual
  viewing distance, field of view and screen resolution, that gives on-screen texels per pixel. Uses:
  - A density report and heatmap: over- and under-dense UV islands, and texels no triangle maps to.
  - Error weights for RDO and the base encoders. Unmapped texels (gutters, unused space) weigh zero
    beyond the bleed the mips need. Magnified regions get more bits and minified regions fewer.
  - A suggested top resolution: drop the mips that are never sampled at the usual distance.
- [ ] **Analytical mode prediction** (bc7f): closed-form SSE estimates (line-fit residual + endpoint/weight
  quantization noise) to choose mode pairs (1 vs 3, 0 vs 2, 4 vs 5) without trial encodes. See
  `docs/research/bc7f-bc7g.md`.
- [ ] **AVX-512 and NEON backends** for `src/simd.rs` (needs hardware to tune).
- [ ] **More GPU base encoders:** BC7 modes 3/0/2/5 up to effort 8 (where the GPU should win; a workgroup
  per block, a thread per candidate) and the RDO alternates; BC1 efforts 2+ (needs per-lane cluster-fit
  pruning and exact split scoring).

## v1.2: "output B": latent-space supercompression + transcoder

- [ ] Intermediate format: transform-coded BC7/ASTC block parameters (weight-grid DCT, endpoint DPCM,
  context-modeled entropy coder), following Basis' XUASTC/XUBC7 ideas.
- [ ] Runtime transcoder (CPU + compute shader) to BC7, BC1, ASTC 4x4, ETC2; measure transcode Mpix/s.
- [ ] KTX2 vendor supercompression scheme + DFD for the intermediate.

## Pipeline quality (unversioned)

- [ ] RDO rate model for the split layout (`--prep`), so RDO and the transform stop fighting (design notes and
  other next steps: [research/field-split.md](research/field-split.md)); a KTX2 key/value flag for the layout
  and a GPU (compute shader) inverse, so loaders can undo it after hardware decompression.
- [ ] Alpha-weighted RGB error for partly transparent texels (per-texel weights in the BC7 encoders and RDO), and
  RDO matching of hidden texels to anything (now they are pinned to the visible mean).
- [ ] Better mip filters (Kaiser/Lanczos).
- [ ] Per-role presets: `--role` sets the colour space and normal weighting today; still open: a role choosing
  format, λ and smooth protection, per-channel weights for `mask`, and a `ui` role.
- [ ] Rate model for GDeflate (GPU decompression); `--backend` switch for verification.
- [ ] `ktx validate` (KTX-Software) in CI.
