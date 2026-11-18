# Roadmap

Each item should land with a benchmark delta (see `bench-results/`). Major and minor in `Cargo.toml` name the
milestone in progress; every green push to main is released as the next patch (`vX.Y.Z`, with binaries;
`.github/workflows/release.yml`). Ideas tried and dropped are catalogued in [DEADENDS.md](DEADENDS.md).

## Next up

1. ~~ASTC LDR (4x4, 6x6, 8x8) output, with the same RDO pass.~~ Done (v1.0 below); next for it: a GPU kernel,
   ~~more footprints~~ (all 14 2D footprints done), ~~a faster RDO pass~~ (the light pass's quick form,
   1e03746), effort 0 faster than astcenc fast at 5x4-6x5.
2. BC6H: ~~RDO with a log-space distortion~~ (done: 42% smaller Zstd files at λ 2), ~~signed BC6H~~ (done),
   a GPU kernel (tried: not merged, it slowed the CPU path; DEADENDS, branch `wt/bc6h-gpu`).
3. ~~ETC2 / EAC for mobile fallbacks.~~ Done (v1.0 below), with ~~punch-through alpha (ETC2 RGB A1)~~ and
   ~~signed EAC~~ and ~~a faster CPU encoder and RDO candidate error~~ (1.3-2x, byte-identical); next for it: a
   SIMD batch encoder and a GPU kernel.

## v0.1 (pre-release): encoder and container

- [x] BC1, BC4, BC5 and BC7 encoders and decoders, mip generation, KTX2 writer/reader with Zstd
  supercompression, DDS I/O.
- [x] A byte-level LZ rate model shared by every format and an RDO pass over it.

## v0.2 (pre-release): close the gap to the best open BCn RDO

- [x] **Head-to-head harness** (`mipple score` + DDS I/O; now `scripts/compete.py`): mipple vs bc7enc_rdo on Kodak, BD-rate. Result: mipple −13.4% (Zstd) vs bc7enc_rdo's bc7enc.cpp base. See `docs/BENCHMARKS.md`.
- [x] **bc7f partition pre-selection** at effort 1 (principal-axis split + popcount): 2.1× faster base encode, −0.18 dB, and a better RDO curve. (Superseded: effort 1 now ranks all 64 partitions with vectorized line fits.)
- [x] **Re-run head-to-head with bc7e** (bc7enc_rdo built with ISPC) and **bc7f**: done in `scripts/compete.py`.
- [x] **Fast regression check** (`mipple check`): synthetic 256² × 4 formats × 5 λ in ~30 s against a committed baseline; BD-rate, λ=0 PSNR/size and per-image gates; byte-identical count; CI step.
- [x] **BC7 modes 4 and 5** in the scalar encoder (alpha from effort 1, all opaque rotations from effort 2).

## v0.9: competitive parity and Oodle Texture (current)

Goal: meet or beat the quality and speed of the best open encoders per format, then Oodle Texture. Status and
numbers: [BENCHMARKS.md](BENCHMARKS.md#competitive-benchmark).

Shipped:

- [x] **Entropy-reduced base encoding** (bc7enc `-e`): mode-6 preference and 64-level mode-6 endpoints, kept through RDO refits. `--entropy auto` (on for λ ≥ 1) is the default: −3.2% BD-rate on Kodak-4 and synthetic, λ < 1 unchanged. Plain `on` gives −5.1% over a full λ sweep.
- [x] **Multi-format head-to-head** (`scripts/compete.py`, `tools/competitors/`): bc7enc_rdo (bc7e, rgbcx, RDO),
  bc7f, Intel ISPC Texture Compressor, icbc, stb_dxt, Compressonator, texconv; core pinning for single-thread
  timings on hybrid CPUs.
- [x] **Robust BD-rate** (Pareto front, piecewise-linear) in `mipple check` and `compete.py`.
- [x] **SIMD layer** (`src/simd.rs`): `std::simd`-named types, portable and AVX2 backends, runtime dispatch.
- [x] **BC1:** 3-color mode with black, SIMD cluster fit with a learned split order. Top-end quality matches
  rgbcx L18 / icbc 9. Batch encoder for every effort; effort 0 is faster than Intel ISPC and +0.34 dB.
- [x] **BC4/BC5 fast tier:** effort 0 beats Intel ISPC and effort 1 beats stb_dxt and rgbcx on PSNR and speed.
- [x] **BC7 fast encoder for every effort** (`bc7/fast.rs`): modes 6, 1, 3, 0, 2 and 5, ±1 endpoint/p-bit
  refinement, partitions ranked with an index quantization term. Beats bc7e u0/u2/u4, every Intel ISPC profile and
  every bc7f profile on both PSNR and speed; effort 8 is 46.10 dB at 1.2× bc7e u6's speed (46.08 dB).
- [x] **RDO candidate loop:** field dedup and per-field memoization, fused bounded error kernels, an exact LZ
  rate lower bound and table-driven rate costs make the 256-back-reference search the default and faster than
  bc7enc_rdo's RDO in every format (Kodak 24, 20 threads).
- [x] **BC4/BC5 RDO:** aligned byte-span candidates (ERT-style), wider error-cap floor (`--min-rms` 3), frozen
  literal costs, a 192-deep BC5 search, and a localized BC5 search ranked by a small MLP trained only on CC0 Poly
  Haven textures.
- [x] **BC1 RDO:** field-swap refinement (`--refine-back`) and far endpoints (`--far-back`).
- [x] **BC7 RDO:** 8 KiB window and 64 KiB chunks, a far search (whole-block copies and re-indexed endpoints from
  1024 blocks back, behind exact mean and segment bounds), the plain search at effort 8+ as the base (a second
  base with entropy reduction), and a cross-mode candidate. vs bc7enc_rdo: BD-rate −33.7% (bc7e) / −32.3% (`-e`).
- [x] **Smooth-block weight** (5× ramping to 1 at channel std 14): Kodak-24 BD-rate −2.3% to −3.6% in every
  format. Since 0.9.18 4× falling as k²/(k²+σ²) with k = 12, which put BC7 ahead of Oodle Texture at equal RMSE
  (was 2-3% behind at λ 10-40) while staying ahead at equal SSIM. vs bc7enc_rdo: BC1 −26.8%, BC4 −20.0%, BC5 −26.1%.
- [x] **Lossless field split** (`src/prep.rs`, BC7Prep-style, `mipple encode --prep on|auto`): −4% to −8%
  Zstd-19 without RDO. With strong RDO it can lose (RDO optimizes whole-block matches in the interleaved
  layout); `auto` keeps the smaller layout per level.
- [x] **vs Oodle Texture 2.9.16 (Unreal Engine 5.8), true source:** `scripts/ue_oodle/` gets Oodle's real blocks
  out of a stock UE cook (bit-exact, no engine source) and scores them on Zstd-19 next to a mipple sweep
  (`bench-results/oodle-ue/`). Kodak-24 at Oodle λ 10-40, mipple's size at equal RMSE / equal SSIM: BC1 −11.5% to
  −11.0% / −7.5% to −6.7%, BC4 −15.1% to −6.6% / −5.1% to −2.1%, BC7 −2.5% to −1.2% / −3.7% to −1.4% (BC7 rerun with the ranker's 512-block reach), BC5
  −21.2% to −10.7% / −6.7% to −4.2% (rerun at 4e8547d); ahead at every λ in every format, λ 0-1 included.
- [x] **Learned BC7 ranker** (`src/ranker/bc7.rs`, trained only on CC0 Poly Haven): a 32-deep exhaustive search,
  the 1024-block far search, then 16 blocks from anywhere in the 64 KiB chunk, picked by a two-key prefilter
  (decoded value SAD and least-squares refit residual) and a small MLP. Kodak-24 BD-rate −0.22% at 0.86× the
  time; Poly Haven held-out −0.73%.
- [x] **Learned BC4 ranker** (`src/ranker/bc4.rs`, CC0 Poly Haven only): 128 deep plus 32 blocks from the 16 KiB
  chunk (64 by shape SAD, 64 by value SAD, then a 13-16-1 MLP). Kodak-24 BD-rate −0.77% at 0.95× the time; Poly
  Haven held-out −0.71%.
- [x] **Mode-6 base for BC7 RDO:** a mode-6-only encode as a third starting point per block. Kodak-24 BD-rate
  −0.22% at 1.06× the time; vs Oodle 2.9.16 at equal RMSE +3.4-5.8% → +2.3-4.0%, at equal SSIM −0.4-2.8% →
  −1.3-4.8%.
- [x] **Learned BC1 ranker** (tried, not merged): the whole chunk holds little for BC1 (an exhaustive 2048-block
  search gains −0.09% BD-rate at 4.65× the time).
- [x] **Tooling:** `cargo xtask fmt|clippy|ci`, rustfmt at 120 columns, clippy lints in `Cargo.toml`, CI on
  Linux/Windows/macOS, a release for every green push.

Open for v0.9.x:

- [x] **BC7 vs Oodle 2.9.16 at equal RMSE:** Oodle kept 1.8-3.1% fewer bits at equal RMSE on Kodak-24 until the
  smooth weight's new shape (0.9.18); mipple is now 0.6-1.5% smaller at equal RMSE and 0.3-3.1% at equal SSIM.
- [ ] **Widen the BC7 lead:** the margin at λ 30-40 on SSIM is thin (−0.7%, −0.3%), and per image smooth-sky
  shots (kodim07, kodim20) are still up to 5.5% behind there. Oodle's RDO output has about 3× more index-half
  repeats. 128 KiB chunks are another −0.9% / −1.0% BD-rate (RMSE / SSIM) at the same CPU time (0.3-0.6 points
  at equal RMSE λ 10-40 against Oodle, 0.6-0.8 at equal SSIM). With the chunk pool they cost no wall time when
  many images are encoded in one process, but one image alone is still 1.6× (its few long chunks are the
  critical path), so they stay off by default.
- [ ] **BC7 RDO clearly faster than Oodle** (reopened 2026-11-13): quality is ahead in every cell, but CPU is
  behind. On one pinned P-core, Oodle's share of a UE cook is about 20-22 CPU-seconds for Kodak-24 at λ 30 and
  mipple's λ 1 is 31.2-31.6 after the near-flat mode-6 gates on both bases (was 34.2-34.6; about 1.5×;
  all-thread runs put Oodle at 47 and mipple at 55-98; details in [BENCHMARKS.md](BENCHMARKS.md)). Target: under Oodle's CPU at λ 1 with every table cell still ahead, then
  about half.
  Where λ 1 CPU goes (busy time, kodim05): main RDO loop 76%, of which far search 18% (half of it mode-6
  re-indexing), ranker prefilter scan 15%, near candidates 11%, refinement about 10%; the two extra bases
  (`pipeline::rdo_bases`) 20%; Zstd check and KTX2 write about 7%. Switching each stage off instead (8 Kodak
  images, λ 1, one pinned core, best of three, 9.31 s of RDO) gives a different picture: refinement 29% (its
  files 0.85% larger without it), the deferred decision 22% (+0.87%), the far search 19% (+3.0%), near depth
  32 rather than 8 8% (+0.1%); without the ranker's picks RDO is 51% slower, since they tighten every later
  bound. Refinement's time is spread thin: per block about 400 tries, 216 scored (mode-6 halves by table
  lookups; the 4% of blocks that aren't mode 6 decode), 14 rate estimates and 0.06 wins.
  - Kept: in refinement, a mode-6 block skips other modes' first halves (about 3% less at λ 1, byte-identical on
    Kodak-24 and the Poly Haven test set).
  - Kept: cheaper defaults (d242683): near search 8 deep instead of 16, refinement 128 instead of 256, deferral
    margin 0.15 instead of 0.3. Each alone (cloud A/B, Kodak-24) saves about 5% / 11-13% / 5% for 0.1 / 0.1-0.2 /
    0.1-0.2 points of the lead; together 1.22× / 1.24× / 1.21× less time at λ 1 / 10 / 30, with every cell still
    ahead (equal RMSE −1.4% to −2.8%, equal SSIM −1.8% to −3.8%). Refinement 64 deep on top is 1.31× / 1.38× for
    −1.2% / −1.6% minimum leads, and deferral 0.1 1.26× / 1.29× for −1.3% / −1.7%.
  - Kept: masked 128-bit block compares instead of runtime-length slice compares (`bcmp`), byte-identical,
    1.4-2.2% less time (cloud A/B).
  - Kept: the ranker prefilter keeps each top-8 list sorted by insertion, with an exact cut-off, instead of a
    32-entry buffer trimmed by selection, so more SIMD lanes skip the test: byte-identical, 2.3-2.5% less time
    (cloud A/B).
  - Kept: mode 6's per-texel error table (`Mode6Lo::new`) built by channel under AVX2, and the 16-texel nearest
    search stopping at the caller's error bound after the first eight texels: byte-identical, 1.005× / 1.017×
    less time at λ 1 / 10 (cloud A/B, EPYC 7763, 24/24 identical).
  - Tooling: `.github/workflows/ab.yml` (paired timing, `head_args` to time one build against itself with other
    settings), `profile.yml` (perf on one pinned core) and `oodle.yml` (the Oodle table re-run with extra
    arguments) run on fresh GitHub-hosted VMs, so speed and quality settings can be checked off the dev machine.
  - Rejected (quality, or slower with no gain): see [DEADENDS.md](DEADENDS.md#bc7-rdo).
  - Measured, not pursued: whole blocks and index halves are only 0-10% repeated within a 2048-block ranker
    reach at λ 1 on most images (30-60% at λ 4+), so a deduplicated prefilter scan pays mostly at higher λ.
  - Kept: the ranker prefilter scans 512 blocks back instead of 2048 (`--local-reach`): 1.17× / 1.21× less
    time at λ 1 / 10 (cloud A/B), every cell still ahead (equal RMSE −2.5% to −1.2%, equal SSIM −3.7% to
    −1.4%, from −2.8% to −1.4% and −3.8% to −1.8%). The scan was 18-21% of a cloud profile at λ 1; an earlier
    timing on the busy dev machine had shown no gain. 1024 was 1.07× faster for 0.1-0.2 points, 256 no faster than
    512 for more loss, and keeping 12 or 16 blocks per key instead of 8 (`--local-m`) cost time for no gain.
  - Measured, not pursued: a shallower rate-model hash chain saves no time (depth 4 is slower).
  - Kept: the fast encoder queues the blocks its mode-6 gate (`busy_err`) sends on to the multi-subset search
    and searches them eight at a time. At λ 1, 7-22% of Kodak blocks are busy but 25-59% of 8-block batches
    held one. Cloud A/B against bc98005: 1.05× / 1.06× less time at λ 1 / 10, the Kodak-24 table vs Oodle
    unchanged to 0.1%, and plain effort 2 1.25× faster with identical output.
  - Kept, byte-identical (cloud A/B, λ 1 / 10, 24/24 identical): the endpoint refit's per-subset sums in
    integers (96b5d77, 1.004× / 1.005×), an O(1) per-channel bound in refinement that skips the 16-texel decode
    of a borrowed mode-6 head (280a075, 1.025× / 1.024×), and those refit sums taken with AVX2 (8c520ae,
    1.018× / 1.015×). Together about 1.05× since 5b90602.
  - Measured: a cloud A/B of 7c3c514 against 941af16 (31.2-31.6 CPU-seconds on the pinned P-core) is 1.557× /
    1.662× less time at λ 1 / 10, which puts λ 1 at about 20 CPU-seconds, level with Oodle's 20-22. Not yet
    re-timed side by side on the dev machine, whose timings swing ±20% under its current load.
  - Kept: refinement 64 deep instead of 128 for BC7 (cloud A/B, Kodak-24): 1.080× / 1.098× less time at λ 1 /
    10, which puts λ 1 at about 18.5 CPU-seconds by the same estimate, under Oodle's 20-22. Every cell stays
    ahead: equal RMSE λ 10-40 −1.6% / −1.0% / −1.6% / −2.3% (was −1.6% / −1.2% / −1.8% / −2.5%), equal SSIM
    −3.6% / −2.2% / −1.7% / −1.2% (was −3.7% / −2.3% / −1.9% / −1.4%); λ 0 and 1 unchanged. 96 deep was 1.034×
    / 1.045× for 0.1 point in the same cells, the same rate. A shallower depth for the endpoint half alone
    gained 1.02-1.07× (noisy local timing; see DEADENDS).
  - The luma term (default since 96cae40) costs 1.082× at λ 1 in the same cloud A/B, which puts λ 1
    back at about 20 CPU-seconds by this estimate, level with Oodle's 20-22 (at higher quality per λ).
  - Next: structural cuts to the far re-indexing and the second base, using the cross-domain research
    sweep's ideas (hash-based block matching from screen-content video coding, lower-bound cascades).
- [x] BC7 mode 4 in the fast encoder, p-bit search in quantization, and a faster uber step. Kodak-24, λ 0, one
  thread: effort 8 adds mode 4, 46.018 → 46.033 dB in the same time (17 images up, none down; cloud A/B on an EPYC
  7763, 14.32 → 14.40 s). It
  also feeds RDO below λ 1, whose base is effort 8: `mipple check` λ 0.5 normal_rocks +0.84 dB and 1.7% smaller
  Zstd. Effort 9 adds the p-bit search by exact error (multi-subset fits, refits and polish, and mode 6's
  fits). It polishes the top two candidates like effort 8, not every one: 46.084 dB (24/24 images up) for
  about 2× effort 8's time, where it was 46.018 dB at 1.1× (cloud A/B: 16.04 → 28.33 s, 1.77× the old effort 9's). The uber
  trials skip stretch ranges that repeat an earlier index map, with the same output. Efforts 0-5 are
  unchanged, since the GPU kernel must match them. Efforts 6-7 are unchanged too: both additions fall below
  the effort curve there (see DEADENDS). RDO's second base (λ ≥ 1) stays as it was. Open: a cheaper p-bit
  search for effort 5, which would gain +0.05 dB but needs the WGSL kernel to follow.
- [x] **Smarter entropy reduction** (tried, not merged): RDO already chooses each block's start by J among
  three bases, so a rate-aware weight, a fourth base or a bias on the start moved Kodak-24 BD-rate by under
  0.1% (DEADENDS.md).
- [x] **Far endpoints for BC4/BC5:** each pair's exact error in a few operations (a per-block prefix table summed
  over the palette's midpoints), only the 8 lowest LZ-costed, gated at 0.7 of the starting J: Kodak-24 BD-rate
  BC4 −0.20%, BC5 −0.56% for 1.115× / 1.101× the time at λ 2 (cloud A/B) (the interval bound: BC4 −0.38% for 1.4×).
- [ ] **Far index patterns for BC1:** Oodle's far index-pattern reuse (about half mipple's distinct BC1 index
  patterns on kodim18) still needs a cheap way to find patterns that fit. Refit candidates over 2048 blocks:
  −0.17%, several times slower; with a lower bound on each pattern's error (least-squares fit from popcounts
  and subset sums): −0.08% for 1.18× the RDO time (DEADENDS).
- [x] Rebalance RDO chunks for many threads without making output depend on the thread count (BC5 on Kodak:
  24 chunks on 20 threads run as two uneven rounds). Every level's chunks (and every input's, when `encode`
  gets several) now go to one longest-first pool, and all CPU bases run before it in parallel; chunk
  boundaries are unchanged, so the output is byte-identical at any thread count. Kodak-24 with mips on a 155H
  (22 threads, a busy machine; best per image of 3): BC7 λ 1 14.1-14.6 s against 25.9-26.4 s (1.8×), BC5 λ 1
  22.2-24.1 s against 35.7-37.4 s (1.6×), CPU time within 2%. One process for all 24 images: BC7 9.5-10.7 s,
  BC5 22.2 s. Tried before: splitting each block's search over back distances across threads; no faster on a
  155H, since per-block joins wait for the slowest core.
- [x] **Basis UASTC + RDO + Zstd** and **XUBC7** in the head-to-head (`compete.py --basisu`: basisu 073eb6a,
  `-export_dds BC7`, then score; sizes are what ships). On Kodak-24, mipple's BC7 RDO is −54.3% BD-rate (Zstd-19,
  equal PSNR) against UASTC RDO and −8.3% against XUBC7 (bc7e level 2), but **+12.9% at equal SSIM** against
  XUBC7 (+16.2% before the luma term below; BENCHMARKS.md, "vs Basis UASTC and XUBC7"). Still open: **bc7g**
  is unreleased (checked 2026-11-16), add it to `scripts/compete.py` when it ships; NVTT 3 sits behind an
  NVIDIA developer login (proprietary license), so it stays a manual download.
- [ ] Close the SSIM gap to XUBC7: at equal PSNR its BC7 has much higher SSIM than mipple's RDO (before the
  luma term, at about 39.7 dB, XUBC7 bc7f rdo 100 vs mipple λ 2: 0.9891 vs 0.9761), and it is smaller at
  equal SSIM on 23 of 24 Kodak images. **Partly closed:** BC7 RDO now adds γ·3 × the smooth-weighted Rec. 709 luma error to each
  candidate's distortion (`--luma-weight`, default 0.25 for BC7), which SSIM gains from first-order and RGB
  RMSE loses only second-order. Kodak-24, Zstd-19, against XUBC7 bc7e level 2: equal SSIM +16.2% → +12.9%,
  equal PSNR −8.5% → −8.3% (bc7f: +15.7% → +12.6%, −11.6% → −11.3%); every image gains (kodim15 +6.2% →
  +3.1%, kodim20 −3.7% → −9.1%), but XUBC7 still wins 23 of 24. Every Oodle cell stays ahead: equal RMSE λ
  20 −1.0% → −0.7% is the thinnest, equal SSIM widens 0.4-0.9 points. At equal quality it costs
  1.04-1.05× the BC7 RDO time (λ 1 / 4; local). At the same λ, where it also lands at higher
  quality, a cloud A/B (Kodak-24, fd244b0 against dbc6856) takes 1.082× / 1.064× the time at λ 1 / 10. More weight trades further (0.5: +10.4%
  with λ 20 RMSE at −0.1%). A contrast-loss term, a stronger or sharper smoothness weight and per-block
  error caps all bought a third as much SSIM or less per point of the Oodle lead (DEADENDS.md). Still
  open: the remaining 13%. SSIM's loss sits in near-flat windows (luma variance below 8: a fifth of the
  windows, 69% of the SSIM loss), which an error weight alone does not reach without costing RMSE.
- [x] BC3: a BC4 alpha half and a 4-color BC1 color half, both halves through the same RDO pass (each
  half's copy, re-index and refit, like BC5), alpha as transparency like BC7; −15.7% BD-rate against
  bc7enc_rdo at a fifth of its time (BENCHMARKS.md). No GPU kernel yet.
- [x] BC6H, unsigned (all 14 modes, Radiance `.hdr` input, scored with mPSNR and log2 RMSE): on 20 Poly
  Haven HDRIs every ISPC profile has a mipple effort that is better and faster; effort 2 beats ISPC veryslow
  in 4% of its time (BENCHMARKS.md). EXR input was dropped
  for build time (DEADENDS.md).
  - [x] Eight-block batch encoder (one block per lane, per-lane mode and partition): 2.0× faster at
    effort 0 and 2.6-2.8× at efforts 5 and 9, with the same quality.
  - [x] Partition ranking across lanes (byte-identical output): effort 0 (now 1) is 0.13 dB ahead of ISPC fast
    and 13% faster. The mode estimates still run one lane at a time.
  - [x] A one-region effort 0 (the refit keeps the indices), half-float conversion with F16C straight from
    the f32 rows, one FMA per palette entry, and a per-mode packer. Pinned to one P-core, effort 0 is
    0.09 dB ahead of ISPC veryfast and 6% faster, effort 1 0.13 dB ahead of ISPC fast in 53% of its time,
    effort 2 ahead of ISPC basic in 35% of its time; efforts 1-9 encode the same blocks as before.
  - [x] RDO for BC6H: half-float bit-pattern distortion (log-like, so λ means the same across exposures);
    whole copies, re-indexed borrowed endpoints and endpoints refit to borrowed indices. On the 20 HDRIs,
    λ 2 makes the Zstd files 42% smaller at the quality of effort 0 (BENCHMARKS.md).
  - [x] Signed BC6H (`bc6h-sf16`; KTX2 vkFormat 144, DDS `DXGI_FORMAT_BC6H_SF16`), from `.pfm` (or 8/16-bit
    images read as `2v - 1`), with RDO: errors on each half's magnitude with its sign, so a sign error costs
    the distance through zero. texconv's signed encoder clamps negative values to zero and ISPC has none, so
    on 20 signed HDR detail layers every mipple effort is far ahead of the only competitor (BENCHMARKS.md).
  - [ ] GPU kernel.
- [x] ASTC LDR 4x4, 6x6 and 8x8 (1-3 partitions, dual plane, every weight grid of the footprint), to KTX2
  (sRGB or UNORM) or `.astc`, through the CLI, library, `mipple check` and the web demo, with
  base+offset and base+scale endpoints and per-partition CEMs. On Kodak, one thread on a cloud EPYC
  (BENCHMARKS.md, 6727c53), effort 0 is 0.77 / 0.10 / 0.26 dB ahead of ISPC fast at 4x4 / 6x6 / 8x8 with
  higher SSIM in about 90% of its time, and every astcenc 5.7.1 preset is beaten in PSNR and SSIM in less
  time: fast by effort 1 at 4x4 and effort 0 above, medium by effort 2 (55-83% of its time), thorough by
  effort 3 / 4 / 5 at 4x4 / 6x6 / 8x8 (41-65% of its time).
  - [x] RDO for ASTC: its own pass over the surface (blocks are not 4x4) with whole-block copies, earlier
    heads re-weighted and earlier weights refitted. On Kodak at λ 1, Zstd size falls 40% / 19% / 12% at
    4x4 / 6x6 / 8x8, costing 3.0 and 0.18 dB at 4x4 and 6x6 and nothing at 8x8 (PSNR 0.05 dB higher). At
    ISPC fast's PSNR the files are 17.7% / 16.6% / 29.8% smaller after Zstd. Cached refit sums and exact
    bounds on re-weights and whole copies made the pass 1.2-1.9× faster, output unchanged (BENCHMARKS.md).
  - [x] Effort 0 faster than ISPC fast at 6x6 and 8x8; close the PSNR gap to astcenc medium / thorough.
  - [x] Smaller files too: with a light RDO pass one block back (`-l <λ> --max-back 1 --refine-back 1`),
    some setting beats each astcenc preset (fast / medium / thorough) at once on PSNR, SSIM, Zstd and
    Deflate size and time at 4x4, 6x6 and 8x8 (BENCHMARKS.md, 8598d56; each pair on one cloud runner).
  - [x] That light pass is the ASTC default (λ 0.03 / 0.3 / 0.5; `-l 0` turns it off): at effort 5, Zstd
    files 0.7-2.8% smaller for 0.005-0.021 dB.
  - [x] Every other 2D LDR footprint (5x4, 5x5, 6x5, 8x5, 8x6, 10x5, 10x6, 10x8, 10x10, 12x10, 12x12): CLI,
    library, KTX2 (vkFormat 159-184), `.astc`, `mipple check`; the default λ interpolates by texel count,
    rising past 8x8 to 2 / 3.5 / 4.5 / 6 at 10x8 / 10x10 / 12x10 / 12x12, where effort 0 beats astcenc fast
    on PSNR, SSIM, Zstd and Deflate size and time at once (BENCHMARKS.md, e570c02). Effort 0 has its
    own floor on λ from 8x5 to 10x8 (0.85-3), so it beats fast on all five there too, level on time at 8x5
    (BENCHMARKS.md, b7b5f20).
  - [x] Effort 0's default light pass against fast's time at 4x4-6x6. At 1e03746 (cloud bench 44094984522,
    EPYC 9V45, Kodak-24, one pinned thread) it wins PSNR and SSIM at every footprint and both sizes except
    Zstd at 6x5 (3.826 against 3.825 bpt, at +0.14 dB), and it is faster than fast at 4x4 (1.65 against
    1.88 s) and 6x6 (1.29 / 1.45), but still slower at 5x4 (1.47 / 1.41), 5x5 (1.48 / 1.29) and 6x5
    (1.47 / 1.34). The base encode is about 70% of that time and polish about 30% of the base encode.
    Steps since 4e03e6c (where cloud bench 44061144254, EPYC 7763, had it slower than fast at every
    footprint but 6x6):
    - The Zstd check skips chunks level 1 says grew by 4 bytes or fewer (3691688): 1.011× less time at 4x4,
      level at 5x5 and 6x5 (cloud A/B 44043405979).
    - Layouts, QuadSums and the light pass's previous-block fields filled in place (f129d7b): 1.055× at 4x4
      effort 0, 1.040× at 6x5, 1.033× at 4x4 effort 2 (cloud A/B 44058354730, Xeon 8573C).
    - The dominant-axis iteration as lanes (4e03e6c): 1.011-1.013× (cloud A/B 44061093046, EPYC 7763).
    - Block texels in kept memory and the RDO pass's SSE as lanes (bc905be): 1.038× at 4x4 effort 0, 1.027×
      at 5x5, 1.028× at 6x5, 1.016× at 4x4 effort 2 (cloud A/B 44073666086, EPYC 9V74).
    - All of those output identical. Then the light pass's quick form at 4x4-8x5 (1e03746): each block tries
      the previous block whole, reweighted and refitted, priced by literal costs and prefix / suffix matches
      instead of the LZ model's parse. Files 0.08-0.29% smaller for 0.004-0.006 dB, 0.2-0.4 s less on
      Kodak-24 (local, one thread).
    - Tried and dropped: a lighter effort-0 search at 5x4-6x5 paid for with a lower λ. The search cuts cost
      0.016-0.153 dB, and a lower λ buys back only 0.012-0.021 dB for files 0.7-1.6% larger (DEADENDS).
    - ISE packing in 64-bit words (4c8b1a4); light-pass candidates that cannot win skipped before they are
      written (440c8a9: 1.018-1.028× at 5x4-6x5, cloud A/B 44191868149, 24/24 identical) or polished
      (ccd79ee: 1.025-1.027× at 5x4-8x8, cloud A/B 44205118060, EPYC 9V74, files +0.002-0.004% at 5x4-6x5);
      and no refitted candidate at effort 0 below 36 texels (07ed5d3: 1.051-1.056× at 5x4-6x5, cloud A/B
      44237101083, EPYC 7763, files 0.01-0.05% larger).
    - At afdc3b8 (cloud bench 44237114084, EPYC 9V74, Kodak-24, one pinned thread) effort 0 is faster than
      fast at 5x4 (2.21 / 2.38 s), 5x5 (2.19 / 2.22) and 6x5 (2.16 / 2.28), with higher PSNR (+0.05 / +0.09
      / +0.14 dB) and SSIM and smaller files after Zstd and Deflate, except Zstd at 6x5 (3.828 against
      3.825 bpt).
    - Effort 0's λ at 6x5 raised from 0.219 to 0.26 (a10e278). Done: on one EPYC 7763 runner (Kodak-24,
      one pinned thread) effort 0 beats astcenc fast on PSNR, SSIM, Zstd-19 and Deflate-9 size and time at
      once at every footprint from 4x4 to 6x6: 4x4 2.35 / 3.06 s and 6x6 1.93 / 2.48 at de44939 (cloud
      bench 44239094693); 5x4 2.12 / 2.32, 5x5 2.11 / 2.15 and 6x5 2.08 / 2.21 at a10e278 (cloud bench
      44241505134), with 6x5 now 3.812 against 3.825 bpt after Zstd. Intel ISPC fast is faster at 4x4,
      5x4, 5x5 and 6x5 (2.21, 2.11, 2.02, 1.82 s), but 0.20-0.90 dB behind, with effort 0's files 3.4-4.2%
      smaller after Zstd.
    - [x] Effort 0 with RDO off (`-l 0`) faster than ISPC fast at 4x4-6x6 with higher PSNR and SSIM and
      smaller files: 6x5 takes effort 0's settings above 30 texels (e697b25; 16% less time for 0.117 dB).
      Cloud bench 44248429239 (EPYC 9V74, Kodak-24): `-l 0` in 77-85% of ISPC fast's time, +0.10 to
      +0.90 dB, files 1.4-2.8% smaller after Zstd; the default effort 0 still beats astcenc fast on all
      five columns (6x5 39.097 / 39.080 dB, 1.46 / 1.80 s). On a Zen 5 EPYC 9V45 (44243279811, before
      e697b25) 5x5 effort 0 was level with astcenc fast on time (1.27 / 1.26 s): margin left to widen.
    - [ ] Some setting beats astcenc thorough on all five columns at every footprint (BENCHMARKS.md, ee1dd5a):
      done at 4x4-8x8 (earlier runs), 5x4 and 5x5 (effort 4, 50-56% of its time), 10x8 (effort 6, light
      pass λ 0.5, 70%) and 10x10 and 12x10 (effort 7, light λ 0.5, 81-89%). Open at 12x12: effort 7 at λ 1
      wins on quality and size but took 110% of thorough's time on a Xeon 6975P. The gap at 10x8 and up is in
      the base encode (even `-l 0` at effort 5 is 0.015-0.055 dB short).
  - [ ] A GPU kernel, a SIMD batch encoder, HDR.
- [x] ETC2 / EAC for mobile fallbacks: ETC2 RGB (all five modes), ETC2 RGBA8 and EAC R11 / RG11 (unsigned),
  KTX2 only, with the RDO pass. On Kodak, one thread, effort 0 is 0.37 dB ahead of Compressonator in an
  eighth of its time and effort 1 0.94 dB ahead in half; with RDO, files are about 25% smaller at
  Compressonator's PSNR (BENCHMARKS.md). etcpak and etc2comp were not available to grade.
  - [x] Signed EAC R11 / RG11 (`eac-r11-snorm`, `eac-rg11-snorm`; vkFormat 154/156), checked against a
    transcription of the Khronos specification.
  - [x] Punch-through alpha (`etc2-rgb-a1`, vkFormat 149/150): texels below alpha 128 decode transparent
    black, checked against `texture2ddecoder`.
  - [x] A faster CPU encoder and a cheaper RDO candidate error than a full decode, byte-identical (552 Kodak
    encodes across efforts 0-9, λ 0 / 1 / 4 and all ETC2 / EAC formats; `mipple check` 210/210). The subblock
    error, luma term and paint errors take exact shortcuts (no clamping, so |e − m|² = |e|² − 2mΣe + 3m²); T and
    H error and selectors come from one pass, and the descent reuses the unchanged group's errors. Refitting
    another block's selectors uses per-selector moments (count, Σx, Σx²) and returns its error, as does
    re-choosing the selectors, so those RDO candidates skip the decode; EAC refits score an unclamped palette in
    closed form. Cloud A/B (ec28caa against 9868e5d, Xeon 6973P-C, Kodak 1-12, one pinned thread, 12/12
    identical each): ETC2 RGB 1.327× less time at the default effort, 1.740× at λ 1; RGBA8 1.697× at λ 1; EAC
    R11 1.293× at λ 1. Local estimates on a busy machine for the rest: 1.6-2.0× at efforts 1-9, RGB A1 1.61× at
    λ 1.
  - [ ] A SIMD batch encoder (eight blocks per call, as BC1/BC6H) and a GPU kernel. The per-block kernels
    already run 8 or 16 texels wide; what a batch would widen is the scalar glue between them (shortlists, the
    T/H descent), which must keep its order to stay byte-identical.

## v1.0+: perception and speed

- [ ] **Perceptual distortion:** per-block masking (activity-weighted SSE) and λ normalized across texture
  types, as Oodle does; an NVIDIA FLIP metric in `bench`.
- [x] **Normal maps:** `--role albedo|normal|mask`; BC5 / EAC RG11 RDO weighs each block by how far its X/Y
  error moves the rebuilt unit normal (−1.2% BD-rate on lit PSNR, better RG PSNR and angle too), and `bench`
  reports lit PSNR. ASTC stores X/Y as luminance+alpha (`rrrg`, `KTXswizzle` `ga01`) with the same RDO
  weight (−0.35 to −0.83% BD-rate on lit PSNR); effort 5 matches astcenc `-normal -medium` at 1.5-2x its
  time. Still open: per-channel weights for `mask`, and closing effort 0's 0.06-0.13 dB gap to `-fast`
  at 6x6 and 8x8.
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
- [ ] **Cheaper chunk boundaries:** worth less than thought: a single chunk per Kodak image is only −0.86% BD-rate
  for BC5 and −0.54% for BC1 vs 16 KiB chunks. Seeding each chunk with the base encoder's preceding blocks is
  worse (+2.9%: they change under it).
- [ ] GPU compute base encoders (wgpu, `gpu` cargo feature, on by default; `src/gpu/`). Each kernel
  writes exactly the CPU encoder's blocks (tests compare every effort, block for block); `GpuMode::Auto`
  uses one only where it beats the CPU, `--gpu` forces it. Done: BC4/BC5 (`bc4.wgsl`; the CPU stays
  the default, since 20 AVX2 threads take 1-3 ms for a 1.6 MP BC5 vs 14 ms on an Intel Arc iGPU);
  BC1 efforts 0-1 (`bc1.wgsl`, after making the CPU line fit and refits exact integer arithmetic;
  CPU default again: 9 vs 71 ms for 16 MP at effort 0); BC7 effort 0 (`bc7.wgsl`, mode 6, after
  making the BC7 fast path's line fit, endpoints and index projection exact; 14 vs 59 ms for 16 MP).
  BC7 efforts 1-2 on the same kernel (mode-6 least-squares refits, then mode 1 on bc7f's partition
  estimate), after making the two-subset fits (principal axis, endpoints, LS refits, quantizer
  rounding and index projection) exact on the CPU at unchanged speed and quality. BC7 effort 3
  (mode 1 on the best line-fit-ranked partition), after making the partition ranking exact
  (scatter matrices cut to 10 bits, a 5-bit power step, floored estimates; -0.004 dB on Kodak,
  ~15% slower ranking on the CPU). BC7 efforts 4-5: subset refits on the ranked candidates and ±1
  endpoint/p-bit refinement of the mode-1 or mode-6 winner. WebGPU in the web demo: the same
  kernels behind an async API (`Gpu::open_async`, `Gpu::encode_surfaces_async`), every mip level
  and a 256-block spot check against the CPU in one submission with one readback (one wait on the
  device per encode: 6-12 ms base for a 512² BC1/BC4/BC5/BC7 e0-2 mip chain in Chrome, 42 vs 902 ms
  for BC7 e5 on one wasm thread).
  Next: BC7 modes 3/0/2/5 up to effort 8 (where the GPU should win; a workgroup per block, a
  thread per candidate) and the RDO alternates, BC1 efforts 2+ (needs per-lane cluster-fit pruning
  and exact split scoring).
  Tried: BC6H (exact-integer CPU path 1.15-1.32x slower; the Arc iGPU only wins at effort 2; see
  DEADENDS).

## v1.2: "output B": latent-space supercompression + transcoder

- [ ] Intermediate format: transform-coded BC7/ASTC block parameters (weight-grid DCT, endpoint DPCM, context-modeled entropy coder), following Basis' XUASTC/XUBC7 ideas.
- [ ] Runtime transcoder (CPU + compute shader) to BC7, BC1, ASTC 4x4, ETC2; measure transcode Mpix/s.
- [ ] KTX2 vendor supercompression scheme + DFD for the intermediate.

## Pipeline quality (unversioned)

- [ ] RDO rate model for the split layout (`--prep`), so RDO and the transform stop fighting (design notes and
  other next steps: [research/field-split.md](research/field-split.md)); a KTX2
  key/value flag for the layout and a GPU (compute shader) inverse, so loaders can undo it after hardware
  decompression.
- [x] Alpha as transparency (BC7 colour textures by default): alpha-weighted mips, push-pull bleed of hidden colour,
  hidden texels beyond a 2-texel ring freed for the encoder; alpha-coverage-preserving mips (`--alpha-cutoff`).
- [ ] Alpha-weighted RGB error for partly transparent texels (per-texel weights in the BC7 encoders and RDO), and
  RDO matching of hidden texels to anything (now they are pinned to the visible mean).
- [ ] Better mip filters (Kaiser/Lanczos).
- [ ] Per-role presets: `--role albedo|normal|mask` sets the colour space and normal weighting (done);
  still open: a role choosing format, λ and smooth protection, and a `ui` role.
- [ ] Rate model for GDeflate (GPU decompression); `--backend` switch for verification.
- [ ] `ktx validate` (KTX-Software) in CI.
