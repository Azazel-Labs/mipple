# Dead ends

Ideas that were tried, measured and dropped, with enough detail to tell whether a new idea is
the same one. Each entry gives what was tried, what it measured, and why it doesn't pay. Small
tweaks that changed nothing don't belong here. Neither does work that is only paused: that goes
in [ROADMAP.md](ROADMAP.md).

Measurements are on Kodak-24 unless noted. BD-rate is at equal quality, so negative is better.
CPU times come from paired runs on the same core.

## RDO (all formats)

- **Whole-chunk or very large searches for BC1.** Searching the whole chunk (2048 blocks) for
  BC1 gains only −0.09% BD-rate at 4.65× the time, so a learned BC1 ranker has nothing to find.
  BC1 blocks carry too little structure for distant matches to beat near ones.
- **Seeding a chunk with the base encoder's preceding blocks.** This came out at +2.9% BD-rate
  (16 KiB chunks). RDO copies those blocks, but RDO itself then changes them, so the matches it
  counted on aren't in the final output.
- **Larger chunks as a speed or size lever.** One chunk per image is only −0.86% (BC5) and
  −0.54% (BC1) against 16 KiB chunks. On 20 threads, 32 KiB chunks were slower (13.3 s against
  10-12.5 s for λ 0.5 over the corpus). 128 KiB chunks for BC7 are −0.86% / −1.03% (RMSE / SSIM)
  at the same CPU time. With every level's chunks in one pool, all 24 images in one process take
  the same wall time as 64 KiB (7.9 s), but one image at a time is still 1.6× (23.6 s against
  14.5 s for the 24), since an image has only three top-level chunks and one long level-1 chunk.
  The chunk size can't follow the batch size without changing the output, so 64 KiB stays.
- **Splitting one block's search across threads (by back distance).** This was no faster on a
  155H: the join for each block waits for the slowest core.
- **Frozen (non-adaptive) LZ costs in the rate model.** They score even in `mipple check`
  (λ ≤ 4) but worse at λ 2-8 (−19.8% → −19.2% against the competitor).
- **Far index-pattern reuse via refit candidates.** Refitting endpoints to index patterns from
  2048 blocks gains −0.17%, at several times the time. It needs a cheap way to find patterns that
  fit before this is worth retrying.
- **Far endpoints for BC4 without a tighter bound.** −0.38% at 1.4× the time.
- **A 12x12 (3x3-block) window for the neighbourhood smoothness weight.** Against the four
  overlapping 8x8 windows at the same k it loses 0.5-0.9 points of BC7 size at Oodle Texture's
  RMSE (λ 10-20 fall behind Oodle) for no SSIM gain at λ ≥ 20. Large windows mask texels next to
  edges that the error metric still sees.
- **A smaller near-tie threshold for the deferred decision, to save time.** `--defer` 0.1 against 0.3
  (λ 1, one thread on one P-core) keeps most of the time cost: 1.49× / 1.13× / 1.27× the RDO time of no
  deferral for BC5 / BC7 / BC3, against 1.51× / 1.21× / 1.31×, for about two thirds of the BD-rate gain
  (BC5 −0.16% vs −0.27%). The cost is in deferring at all, so 0.3 stays for every format. BC7 later moved to 0.15
  (d242683) as one of three cheaper defaults: against Oodle it costs only 0.1-0.2 points of the λ 20-40
  equal-RMSE lead for about 5% less time (cloud A/B, Kodak-24).
- **Averaging the per-channel smoothness terms** (k²/(k²+σ²) per channel instead of with the
  largest channel's variance). BC7 moves strongly toward RMSE (−1.9% to −2.8% size at Oodle
  Texture's RMSE) but falls behind Oodle on SSIM at λ 30-40 (+0.6% / +1.1%).

## BC7 RDO

- **Cutting refinement or the ranker's picks to save CPU.** `--refine-back 0` saves 16% CPU,
  `64` saves 12% and `--local 0` saves 26%, but each puts λ 40 SSIM behind Oodle (+0.2% to
  +1.0%). Refinement and the ranker carry real quality. (Measured when the λ 40 SSIM lead was −0.3%; with today's wider lead
  BC7 refines 64 deep by default, see the ROADMAP's BC7 speed item.)
- **A shorter far search or fewer ranker picks (cloud A/B and Oodle table, Kodak-24, at 5ab6183's
  defaults).** `--far-back 512` saved 6% at λ 1 but only 1% at λ 10, for 0.2-0.4 points of every
  λ 10-40 lead. `--local 6` saved 2% for 0.1-0.2 points. A shorter ranker reach buys far more
  time for the same loss (see the ROADMAP's BC7 speed item).
- **Far re-indexing of multi-subset heads (modes 0-3, 7).** These almost never won, and removing
  them moved every Kodak-24 delta against Oodle by 0.02% or less. Only mode-6 heads are worth
  re-indexing.
- **Hybrids in refinement: a mode-6 block trying other modes' first halves over its indices.**
  These never won on Kodak-24 or the Poly Haven test set.
- **Cheaper second base:**
  - Effort 6 or 7, or no uber step, for the second base loses 0.3-0.6%.
  - Fewer polished candidates, or a single uber step, loses 0.1-0.25%.
  - Building the second base only where the entropy-reduced base errs by 64λ or more costs 1.9×
    the CPU, because that base also bounds the search.
- **Ranker prefilter with 4 blocks per key** (instead of 8) lost about 0.07%.
- **A ranker reach of 256 blocks, or more blocks per prefilter key.** On the cloud, a 256-block reach
  saved no more time than 512 (1.18× / 1.21× vs 2048) and lost 0.1-0.3 points more of each lead over
  Oodle. At 512, keeping 12 or 16 blocks per key (`--local-m`) instead of 8 gave back most of the time
  for no gain. (The 512 reach itself is kept. An earlier timing on the busy dev machine had shown no gain
  from 1024 or 512.)
- **Per-field refinement depth.** Refining only 64 blocks back for the endpoint half saves about 2% of
  λ 1 time, and for the index half about 4%, but each shrinks the lead over Oodle by 0.1-0.3 points
  (λ 40 SSIM −1.5% to −1.3% / −1.2%). Re-checked at 2bb3f8f (128 deep): 16 or 32 deep for the endpoint
  half, 128 for the index half, gained only 1.02-1.07× (noisy local timing, 8 Kodak images). Most of
  refinement's wins borrow the index half, from every distance back, and endpoint-half wins cluster within
  16 blocks but still come from further back.
- **Starting from the plain base instead of the entropy-reduced one (`--entropy off`).** Slower (36.9 s
  vs 32.9 s for Kodak-24 at λ 1 on one pinned core) and 2% larger at λ 1, and the SSIM lead over Oodle at
  λ 30 / 40 falls from −1.8% / −1.5% to −1.0% / −0.6%.
- **A different hash-chain depth for the rate model (16 now).** Depth 4 is slower (44 s vs 37 s for
  Kodak-24 at λ 1): the weaker model's estimates leave looser bounds, so the search does more work. Depth 8
  saves nothing, and depths 32 / 64 add 6% / 22% to RDO time. Quality moves by 0.1 point or less.
- **Byte-identical micro-variants that didn't help:**
  - u16 hash-chain deltas;
  - skipping `texels_far` when cached;
  - an equality-mask intra scan;
  - skipping runs of eight dropped blocks in the far loop (±0);
  - the candidate dedup table with each tag beside its key (one cache line per probe instead of two;
    ±0 over five paired runs);
  - building the mode-6 alternate base inside the plain effort-8 pass, which fits the same line (390/390
    check encodes byte-identical, but that pass is only about 2% of base time: 2.51 vs 2.58 s best of five,
    medians reversed);
  - skipping the Zstd safety check for every BC7 chunk (8 Kodak images at λ 1: 8.71 vs 8.63 s, same bytes;
    the rate-model skip already covers the chunks where it would cost);
  - deduplicating refinement candidates after their error test (+4-5%, since refinement
    candidates often repeat earlier ones).
- **Exact (greedy) pruning with the deferred decision kept.** Deferring needs the search's pruning relaxed by
  the same margin, so the runner-up is found. With exact pruning, the runner-up is whatever survives by
  chance: on 8 Kodak images at λ 1 that keeps 31% of deferral's size gain (1,599,944 bytes against 1,590,486
  with deferral and 1,604,292 without) for 41% of its time saved (RDO 7.79 s against 8.42 / 6.90 s).
- **Gating refinement per block.** Refinement's wins are spread thin: no single feature predicts them (the
  best candidate's bits, best J over the starting J, runner-up over best, J itself). The most selective cut,
  runner-up over best above 1.3, skips only 17% of blocks while dropping 3% of refinement's gain.
- **Larger near-flat mode-6 gates on the BC7 RDO bases.** The plain effort-8 base keeps mode 6 when a
  block's mode-6 error is at most 384 and the entropy-reduced base when its score is at most 512 (both
  gains, see `bc7::rdo_alt_cfg`). Higher gates trade RMSE for SSIM vs Oodle on Kodak-24 rather than gain:
  the plain gate at 512 (with the reduced one at 512) moves λ 20 / 30 at RMSE from −1.7% / −2.3% to −1.6% /
  −2.2% while SSIM gains 0.1-0.2, at 768 to −1.3% / −1.5%, and at 4096 alone λ 30 at RMSE goes to +0.2%. The
  reduced gate at 1024 or 2048 moves λ 40 at RMSE −3.2% to −3.1% for about 2% less time.
- **The batch (effort-5 search) encoder as BC7 RDO's plain second base.** It is the encoder that builds the
  entropy-reduced base, so it costs a fraction of the effort-8 alt, but RDO keeps much of the alt's extra
  quality: Kodak-24 vs Oodle at RMSE λ 20 / 30 / 40 went from −1.7% / −2.3% / −3.2% to −1.1% / −0.9% / −2.1%
  (SSIM 0.2-0.3 behind too), for no measurable time saved on 8 Kodak images (the gated effort-8 alt is already
  only about 10% of λ 1).
- **A deduplicated prefilter scan at low λ.** Whole blocks and index halves are only 0-10%
  repeated within a 2048-block ranker reach at λ 1 (30-60% at λ 4+), so deduplication pays
  only at higher λ.
- **Video-codec style fast-decision gates (skip the far search, build the plain alt lazily) as a
  25% speedup.** The ceilings are too low (Kodak 1-6, λ 1-8). Once the far back-reference search runs
  after the ranker's picks (same quality, −0.02% BD-rate), it improves only about 11% of blocks for
  1.2-1.5% of the summed J, and skipping it outright saves only about 20% of RDO time at +1.6% / +2.2%
  BD-rate (PSNR / SSIM). Single-feature gates (slack/λ, bits, ranker score) lose 22-35% of the far
  search's gain for 30% of its time; a linear gate over eight features, fitted on Poly Haven logs, was
  no better end to end than skipping whenever J is already below 0.8× its starting value (about −13% RDO
  time for +0.24% / +0.11%). Dropping the plain effort-8 alternate base saves its 13% of encode time
  but makes RDO about 10% slower, since its start bounds the search, at +1.3% BD-rate; its wins can't
  be predicted from the entropy base's error (computing it for 39% of blocks keeps half its wins at λ 1).
- **Rate-constrained elimination (tighter error bounds for candidates that can't continue a match).**
  A candidate that can't take the 1-bit match continuation costs at least λ·min_bits, so its error
  bound can drop to (best − λ·min_bits)·16/smooth in the far prefilter, the re-index and the near
  search. Exact (byte-identical on 576 bench rows and every check encode), but paired pinned timing
  on Kodak showed no gain (0.97-1.08× across λ 1-16, the sign flipping between runs). A continuation
  is possible in 15-95% of blocks, and there the candidate generator's memos need the loose bound
  (they assume the bound only shrinks); elsewhere λ·min_bits is a small part of best. The matching
  whole-block exit (best ≤ λ·min_bits) can never fire: without a continuation every candidate,
  the base included, costs at least the minimum. The floor is in the encoder anyway (9a28d13, shared
  with ASTC, where the cloud A/B measured 1.3-4.3% less time); for BC7 the cloud A/B on Kodak-24 at
  λ 1 and 10 again showed no change.
- **The rate model's hash chains as one 16-entry ring per bucket** (one cache line, no links to chase;
  510ab06). Byte-identical, and the match search was the largest self-time function in a local λ 1
  profile, but the cloud A/B on Kodak-24 measured 0.999× for BC7 λ 1 and λ 10, 0.995× for ASTC 6x6 and
  0.996× for BC1. Kept, since it is no slower.
- **A pattern-establishment prepass (crunch / GST style).** Each chunk encoded twice; in pass 2
  block i also tries the endpoint and index fields pass 1 chose for the next 256 blocks, with a credit
  of C bits for fields pass 1 introduced. Best BC1 result (6 Kodak images): C = 4, −0.66% BD-rate for
  2.4× the RDO time; C ≥ 16 loses (+1.6% to +12%), the credited fields costing more distortion than
  their reuse saves. Offering the base encoding's future fields instead of pass 1's did better for less
  (−0.7% to −1.0% at 1.8×), but the control with the previous 256 blocks' base fields gave −0.22%, so
  the real establishment effect is only about 0.45 points. BC7 (mode-6 endpoint fields of future base
  blocks): −0.35% to −0.50% BD-rate PSNR, −0.14% to −0.19% SSIM for 1.6-2× the RDO time.
- **Carrying re-index lower bounds from the neighbouring block (Elkan / LAESA).** The carried bound
  Σ max(0, √e_t − δ_t)² from the left or upper block's exact per-texel palette errors is valid, but
  neighbouring blocks are usually more than 2√T apart in weighted distance, so the triangle
  inequality rarely bites. Even as a free, ideal cache it would remove only 0.2-1.5% of BC7 RDO
  cycles at λ ≥ 4 and 0.4-6% at λ 1, before paying for the bound and the caches.
- **Steered re-indexing of mode-6 index bytes toward window runs ("lossy LZ" indices).** For the
  chosen block's endpoints and the 4 lowest-error re-indexed endpoint pairs of the search, index
  bytes were forced to equal the window's bytes at a repeat offset or 16·d back (d ≤ 16-64), the
  other indices left nearest; a per-texel, per-index error table priced each span in O(1) and
  survivors were costed exactly. Kodak-24: −0.11% / −0.23% BD-rate at equal PSNR (−0.88% / −1.11%
  at equal SSIM) for 1.9× / 3.2× the RDO time; repeat offsets alone gave +0.15% PSNR, −0.54% SSIM
  for 1.46×. Steered candidates win in 15-28% of blocks, but each saves few real Zstd bits: the
  rate model over-values 3-4-byte matches (a minimum span of 4-5 beats 3), and an estimated-J
  shortlist ranked too poorly to cut the pricing. Most of the gain is the endpoints plus a partial
  index run from the same source, so a cheap retry should start there, and only if SSIM is the
  target.
- **Prefetching the rate model's hash buckets.** `cost_until` stalls most on loading each
  position's 64-byte hash bucket (11.5% of its samples on that load). Prefetching every
  position's bucket before the parse (71363be) was byte-identical but 1.002× / 0.999× at λ 1 / 10
  in a cloud A/B: the out-of-order core already overlaps those loads, and calls that stop early
  pay for prefetches they never use.
- **Tighter exact rate-model budgets at the alternative bases and the fast pool.** Costing each
  alternative base with `cost_until` under the deferral threshold (skipping it when its
  distortion alone is above), and sorting the fast pool by error so the scan stops at the first
  entry whose distortion reaches the threshold, are both exact. They removed only 3-4% of the
  rate-model work at their sites: the early exit on the remaining bits already stops most losers
  within a few positions.
- **The far mode-6 re-index compiled for AVX2.** Its segment and per-texel lower bounds and the
  palette search ran in an AVX2 copy (no FMA, so byte-identical) instead of the SSE2 build, so the
  bounds' 16-texel `f32` loops could run eight lanes wide (a570dcf). Cloud A/B: 0.999× at λ 1 and
  10, 24/24 identical. The bounds are 6% of a profile, but their time is in the scalar `f64`
  segment tests and the call overhead, not the texel loop.
- **Rate-aware entropy reduction (a start chosen with rate in mind, not the fixed mode-6 weight).** RDO
  already picks each block's start by J among three bases: the entropy-reduced one (mode-6 error × 0.4), the
  plain effort-8 alt and the mode-6-only alt. On kodim05 at λ 1 they start 39% / 57% / 4% of blocks, and only
  32% of final blocks equal their start. Kodak-24, λ 1-8, pooled BD-rate against e2d7a80 (PSNR / SSIM):
  mode-6 weight 1 +0.01% / +0.05%, weight 0.25 +0.01% / −0.03%, a λ-scaled multi-subset penalty in the reduced
  encoder −0.02% / +0.02%, the 64-level mode-6 encoding as a fourth base +0.01% / +0.03%, a λ·B-bit bias
  against non-mode-6 starts (B 2 / 8 / 32, with or without keeping the true best) −0.02% to +0.00% / −0.07% to
  −0.01%. Every Oodle-table cell moved 0.04 points or less. All within noise; each extra base costs time.
- **SSIM levers other than the luma error term (closing the gap to XUBC7).** All on Kodak-24, the
  12-λ grid (0-16), Zstd-19, against 3ab7142. "XUBC7" is the BD-rate against Basis XUBC7 (bc7e level 2,
  q100) at equal PSNR / equal luma SSIM, which was −8.50% / +16.18%. "Oodle" is the size against Oodle
  2.9.16 at λ 10 / 20 / 30 / 40, which was −1.56 / −1.04 / −1.61 / −2.29% at equal RMSE and −3.59 / −2.23
  / −1.71 / −1.18% at equal SSIM. `--luma-weight 0.25` (now the default) took XUBC7 to −8.32% / +12.95%
  for a λ 20 RMSE lead of −0.74%: about 11 points of the SSIM gap per point of that lead. Other levers buy a
  third of that or less, and variants of the luma term no more than the plain one.
  - *A contrast-loss penalty.* When a candidate decodes to less variance than the source (σy² < σx², mean
    over channels), its distortion is multiplied by ((2σx² + C2) / (σx² + σy² + C2))^α, SSIM's contrast
    term, with C2 = 58.5. It is added only when the candidate could become best or runner-up, so the bounds
    stay valid. α 1: XUBC7 −8.39% / +15.91%, Oodle RMSE λ 20 −0.93%. α 2: −8.24% / +15.72%, −0.77%.
    SSIM's loss sits in low-variance windows: on four images, windows with luma variance below 8 hold 20% of
    the windows but 69% of the SSIM loss from λ 0 to 0.5, and only 43% of the MSE added. Blocks there lose
    little contrast in absolute terms.
  - *A stronger or narrower smoothness weight.* `--smooth-scale` 5 / 6 (default 4): XUBC7 −8.21% / +14.83%
    and −7.85% / +13.79%, Oodle RMSE λ 20 −0.64% / −0.11%. `--smooth-window-std 8` (default 16): +14.40%,
    with λ 10 RMSE at −0.48%. k 20 with scale 6: +14.65%, λ 20 at −0.43%. k 24 with scale 5: no SSIM gain
    (+16.31%), and λ 40 SSIM falls to −0.31%. Scale 16 (Kodak-8): 7 points of gap, but every equal-RMSE
    cell falls behind Oodle (+1.3% to +4.0%). Adding a contrast penalty (α 1) on top of scale 5 gives
    +14.59% at −0.44%.
  - *A second, sharper flatness weight.* β·k²/(k²+σ²) with a small k, added to the window term to target
    near-flat windows. k 3 / β 8: +9.28% at equal SSIM, but −6.02% at equal PSNR and every equal-RMSE cell
    behind Oodle (+0.33% to +2.68%). k 4 / β 4: +11.13%, with λ 20 / 30 behind (+1.04% / +0.22%). With
    the luma term on top (k 3 / β 8, γ 0.25): +6.27%, with every RMSE cell behind (+1.1% to +3.4%).
  - *The smoothness window measured on luma variance* instead of the largest channel's: +16.38%. RMSE
    leads widen 0.06-0.11 points, but SSIM leads shrink 0.2-0.3 (λ 40 −1.18% → −0.87%).
  - *Variants of the luma term.* For reference, plain γ 0.3 / 0.35 / 0.5 give +12.43% / +11.94% / +10.42%
    at a λ 20 RMSE lead of −0.64% / −0.46% / −0.11%. Only the mean-removed (AC) luma error, γ 0.35 / 0.5:
    +12.39% / +11.11% at −0.40% / −0.02%. The luma term weighted by flatness ((smooth − 1) / (scale − 1))
    instead of the smooth weight, γ 0.5 / 1: +14.32% / +12.75% at −0.88% / −0.55%, at best level with the
    plain term. γ 0.5 with `--smooth-scale 3`: +12.44% at −0.85%, but the equal-SSIM leads shrink below
    today's (λ 40 −1.18% → −0.91%); γ 0.75 with scale 3: +10.44% at −0.19%; γ 0.5 with k 24: +11.73% at
    −0.36%, λ 40 SSIM −0.52%.
  - *Per-block error caps (Kodak-8, 8 λ, base +19.66% at equal SSIM).* `--max-rms-ratio 2 --min-rms 1`:
    +19.01%, but −5.17% → −2.89% at equal PSNR and λ 20-40 behind Oodle at RMSE (up to +3.4%). Ratio 3-5
    with min-rms 1, ratio 2 alone, or `--min-rms 1.5` alone: within 0.1 point. The SSIM loss is not
    in a few blocks with large errors.
  - *Adding the luma term before the bit cost (speed).* For candidates already decoded by the block-error
    pass, the term was added to the distortion before the `dist ≥ t` cut and `cost_until`'s bit budget, so
    both are tighter. On kodim01-08, the output was byte-identical at λ 1 and 4. It changed on kodim20 at λ 1
    and kodim23 at λ 4, both within 0.01 dB. It was no faster: 1.15× / 1.01× the time of the committed form
    at λ 1 / 4 (one pinned P-core, 8 images, best of 5). The kernels' error bound, which comes from the best
    score, stays as loose as before.

## BC7 base encoder (`bc7::fast`)

Kodak-24 at λ 0, mean PSNR. CPU times are the best of 1-3 runs on an idle E-core of a loaded machine (±15%),
each config paired with the unmodified build on the same core. Effort 8 is 46.018 dB, and the line from
effort 6 to 8 is +0.38 dB for 3.3-4.2× the CPU time.

- **Mode 4 at efforts 6-7.** +0.021 dB at effort 6 (45.640 → 45.661) for 1.20-1.24× the time, +0.019 dB at
  effort 7. At effort 8 it costs 0.97-1.04× for +0.015 dB (17 images up, none down), so it went there.
- **p-bit search at efforts 6-7.** In the multi-subset fits and refits: +0.051 dB at effort 6 for 1.58× the
  CPU time, and Zstd files 0.37% larger. With mode 6's fits too: +0.067 dB at 1.60×, and effort 7 +0.069 dB
  at 2.3-2.6×. All well below the effort 6-8 line. At effort 8 the search costs 2.06× (+0.054 dB), so it went
  to effort 9. Efforts 0-5 were not tried for time: the GPU kernel must match them (effort 5 would gain
  +0.052 dB).
- **p-bit search in mode 6's uber trials.** −0.002 dB at effort 8 on its own, and −0.003 dB beside the other
  three sites (46.068 vs 46.072), at 2.4× effort 8's time instead of 2.1×.
- **Polishing every candidate (the old effort 9).** +0.0004 dB over effort 8 for 1.10× its time. With mode 4
  and the p-bit search on, it gives +0.003 dB for 1.10× again (9 images up, 3 down at two decimals), and Zstd
  files 0.03% larger. Effort 9 now polishes the top two candidates, like effort 8.
- **Cheaper uber or polish at effort 8.** Uber 3 instead of 4 (one fewer stretch step): 0.82× the CPU time
  for −0.023 dB. Polishing only each mode's best (`polish_top` 1): 0.78× for −0.028 dB. Both drop effort 8
  below bc7e u6's 46.01 dB, which it has to beat (see BENCHMARKS). Untimed and also worse: mode-6 uber 2
  (−0.012 dB) and `POLISH_REACH` 1.25 instead of 1.5 (−0.012 dB). The exact cut is kept instead: with 2-bit
  indices (modes 2 and 3), 12 of uber 4's 24 stretch ranges map every index the same way as an earlier one
  (or not at all), so they are skipped.

## Field split (`--prep`)

- **The transform alone at higher λ.** The field split loses to RDO at higher λ, because RDO
  optimizes whole-block matches in the interleaved stream. Details are in
  [research/field-split.md](research/field-split.md). It needs an RDO rate model for the split
  layout before it can help there.

## BC6H

Measured on 4 Poly Haven HDRIs, single thread, mPSNR. ISPC fast scores 45.10 dB in 0.186 s;
mipple effort 0 scores 45.18 dB in 0.385 s. (The 20-image results are in BENCHMARKS.md.)

- **Picking the most precise mode whose deltas fit.** This loses quality. Precise modes with
  clamped deltas often win, so rank modes by estimated endpoint error after quantizing and
  clamping instead.
- **Predicting one region against two from line-fit estimates, then encoding only one.** This
  saves 20-35% of the time but loses 0.5-0.9 dB at effort 0. The estimates ignore endpoint
  quantization, which is where two-region modes lose.
- **An error pre-skip before ranking partitions.** At effort 0, 78% of blocks reach the
  two-region search and two regions win 76% of those, so there is little to skip.
- **Dropping the least-squares refit at effort 0.** It loses 0.17 dB for 22% less time, which is
  below the ISPC quality-time curve.
- **Scalar micro-optimization to reach ISPC fast's speed.** At effort 0 the encoder's time is
  spread over a dozen small functions (indexing, partition ranking, mode estimates, PCA, refit,
  packing), and no single one is worth 2×. ISPC runs 8 blocks per instruction, so matching it
  needs a batch encoder across blocks, as BC7 has.
- **Insetting the line-fit endpoints at the one-region effort 0.** Pulling both ends in by a
  fraction of the fitted extent (all 20 HDRIs, mPSNR): with no refit, 1/64 gains 0.007 dB and
  1/32 / 1/16 lose 0.20 / 1.12 dB; with the refit, 1/32 loses 0.08 dB. The refit already moves the
  ends where they should be.
- **Skipping the effort 0 refit when the first trial's error is small.** Lanes are eight blocks,
  and time falls only when all eight skip: a threshold that saves 21% loses 0.10 dB, the same as
  no refit at all. It would need lanes compacted across groups to pay. Keeping the indices
  through the refit (re-quantizing the refitted ends only) costs 0.02 dB and skips the whole second
  indexing instead.
- **Indexing by projection only (no check of the neighbouring index).** At effort 0 it loses
  0.016 dB on the first indexing, 0.03 dB on the refit and 0.06 dB on both.
- **EXR input through the `image` crate's `exr` feature.** It raised iteration builds from 2-4
  minutes to about 22. Radiance `.hdr` covers the test corpus. EXR could return through a
  separate helper crate or binary.

## ASTC LDR

Measured on Kodak images 1-3, single thread, effort 0, unless noted. ISPC fast takes 0.235 /
0.151 / 0.160 s at 4x4 / 6x6 / 8x8.

- **Compiling for AVX2 (`target-cpu=x86-64-v3`) to vectorize the per-block code.** About 1%
  faster at first, 6% after decimation went dense (0.424 / 0.379 / 0.324 s -> 0.401 / 0.357 /
  0.302 s). The loops run over 16-64 texels with gathers through grid taps and per-partition
  filters, which LLVM doesn't vectorize; rewriting polish's loops as structure-of-arrays for the
  autovectorizer did nothing either. Explicit i16 lanes (`src/astc/lanes.rs`) are what worked.
- **Stopping the candidate loop once an estimate exceeds the best error by a margin.** At 1.5×
  and 2× it saves under 3% at efforts 0 and 2. The shortlist is short and its estimates are close
  to each other, so few candidates get cut.
- **A smaller shortlist, or half of polish, at effort 0.** Measuring two decimations per
  candidate instead of four, or polishing only weights or only endpoints, each loses 0.1-0.3 dB
  at 6x6 and 8x8.
- **Reworking the estimate shortlist's data structure.** Its insert line shows as about 12% of
  samples at 6x6, but the cost is branch misses across the whole estimate loop (about 150
  estimates and 70 inserts per block, close to a coin flip each). Collecting every estimate in a
  buffer and trimming it once at the end was slower. A branchless insert of packed (score bits,
  index) keys by min/max came out within 3% (kept, since it is no slower). Visiting only the grids
  whose first estimate's bound (computed for every grid at once, four lanes at a time) is under
  the shortlist's last score cut the grid visits from 55 to about 20 per block at 6x6, output
  byte-identical, and was no faster either. The lever is fewer estimates, not a faster insert.
- **Summing the measured decimation error in lanes.** Taking `decimate_err`'s error sum eight
  texels at a time, with the grid padded once per block instead of per candidate, timed identical
  to the scalar loop in interleaved runs on one P-core. Decimation's cost is the per-grid filter
  taps, not the sum.
- **Scalar micro-optimization to reach ISPC fast's speed.** After the cheap early-outs, the time
  is spread over polish (about 21%), layouts, candidate encoding, decimation and spectra, and no
  single stage is worth 2×. What finally matched ISPC fast was less work per block, not faster
  code: one candidate encoded instead of two, which a better candidate ranking (decimation error
  weighted 1.5×) made affordable, and the dual-plane layouts gated by how far the texels sit from
  one line. Going well past ISPC's speed likely needs work across blocks in SIMD, as for BC6H.
- **Other ways to spend effort 0's time at 6x6.** Kodak 24, one P-core, against ISPC fast in the
  same run (1.13-1.18 s, 37.563 dB). The old effort 0 (one candidate, three polish passes,
  shortlist 4) gave 37.577 dB at 1.21 s. With two candidates: one polish pass and the default
  shortlist of 8 is 37.639 dB at 1.23 s; two passes 37.692 dB at 1.36 s; three passes 37.702 dB
  at 1.39 s; a refine round on top 37.766 dB at 1.62 s. Three candidates with one pass is
  37.686 dB at 1.47 s. Each shortlist entry costs about 7% time, and a shortlist of 2 loses
  0.16 dB. Dropping the dual-plane layouts loses 0.09-0.15 dB for little time. Raising the
  dual-plane and 2-partition line thresholds, which gate those layouts by how far texels sit from
  one line, barely saves any time. Gating the second candidate by its estimate against the
  first's measured error saves little time and loses quality, like the estimate-margin stop above. Kept: two candidates,
  two passes and a shortlist of 5 (37.649 dB at ISPC's time). A second candidate makes Zstd
  output about 0.9% larger than the old effort 0, though it is still 1.7% smaller than ISPC's.
- **A first-try cascade over a wider candidate pool.** Trying 32 candidates once each (no
  refinement) and refining only the best 16 by that error, at 8x8 effort 6 on the Kodak tile:
  33.671 dB against 33.658, with SSIM lower and files 0.08% larger; encoding 32 candidates outright
  gives 33.667 dB, and a pool of 48 33.673 dB for 1.4× the time. A looser early-abandon bound for
  the pool's errors changed nothing. The estimate's ranking is not the weak link after refinement:
  polish is, so polishing several finalists (`Effort::top`, +0.077 dB at 8 kept) won instead.
- **Fewer polish passes for the runner-up finalists.** Polishing every kept encoding with one or two
  passes and only the best of those with the rest loses 0.002-0.013 dB at 8x8 effort 6 and saves no
  time: the full passes stop early once nothing improves.
- **A second polish pass on ASTC RDO candidates.** Polishing a re-weighted or refitted candidate's
  re-chosen half twice instead of once is even on Kodak 24 (BD-rate within 0.25% at every footprint)
  for up to 7% more RDO time; not polishing at all costs 0.5-1.4%. One pass stays.
- **Looking further back than 256 blocks for ASTC re-weights and refits.** 512 instead of 256 is
  another 0.3% / 0.5% / 0.7% smaller at equal PSNR at 4x4 / 6x6 / 8x8 for 1.8× the RDO time (one thread,
  one P-core, with the error lower bounds on). 256 against 32 was worth 4.8-6.6% for 2.4×, so the curve
  has flattened; 256 also matches the whole-copy window.
- **A rate penalty on rare block modes in the plain ASTC encode.** astcenc's files are 4-8% smaller
  under Zstd, and the block-mode bits are the largest single part of that: its fast preset writes
  65% of 8x8 blocks in one mode, while mipple spreads them over about 300. Adding a per-mode cost
  (from a mode histogram) to the candidate score at the final choice alone saves 0.5-0.7% of Zstd
  size per 0.01-0.04 dB at effort 2. Adding it to the estimate stage as well saves 1.2-1.7% at 8x8 for
  0.015-0.03 dB. That is too little, and the PSNR it costs is more than the 0.002 dB margin by which
  effort 0 beats astcenc fast at 6x6. Light RDO (λ 0.05-0.2) wins on both size and PSNR instead.
- **Restricting weight grids to one orientation to concentrate block modes.** Allowing only grids
  no wider than tall (or only no taller than wide) roughly halves the modes the encoder can pick.
  On Kodak 24 at effort 0 that is 1.2% smaller under Zstd for 0.56-0.57 dB at 6x6 and 8x8 (wider-only:
  2.0% for 0.78-0.84 dB), and at effort 2 0.7-0.8% for 0.41-0.48 dB. Window-1 light RDO at λ 1 saves
  5.9% at 8x8 for 0.008 dB.
- **Skipping polish tries that would fail again.** A weight or endpoint-code step depends only on the
  block's current state, so a step that failed can be skipped until a weight or endpoint it reads
  changes. Tracking that (a bit per weight and per code, cleared on each accepted step) keeps the
  output byte-identical but makes the effort-0 encode about 3% slower on Kodak 6x6. The idle-count
  exit already ends the second pass soon after the last accepted step, so little is skipped.
- **Carrying the chosen candidate's symbols instead of reading its bytes back.** The RDO pass reads
  each finished block's 16 bytes back into symbols for the next block. Keeping the winning
  candidate's symbols instead is byte-identical but no faster (Kodak 6x6 λ 0.3 w1 3.05 s against
  3.03 s, within noise).
- **A lower Zstd level for the full keep-or-revert check.** Measured when the check still ran on
  every chunk Zstd level 1 did not shrink by about the rate model's estimate: 1,461 of 14,480 chunks
  of 24 Kodak, 16 Poly Haven and 6 synthetic images (4x4, 6x6 and 8x8; λ 0.3, 1 and 2 at window 1, λ
  0.5 and 2 at the full window). Level 12 disagrees with level 15 on 106 of those keep-or-revert
  decisions and level 9 on 235. Against a perfect level-19 check that costs 2.8 KB and 15.5 KB at
  level 19, where level 15 costs 0.8 KB. Levels 11 and up take 460 to 600 µs per chunk, levels 9 and
  10 only 60 to 70 µs. Level 9 on Kodak 6x6 λ 0.3 w1: 2.95 s against 3.01 s, but worse on both
  quality and size (37.663 dB and 3.200 bpt against 37.664 dB and 3.198 bpt). Checking only the
  chunks level 1 says RDO grew does better: 186 checks, and 330 bytes over a perfect check.
- **Dropping refit_to at window 1.** Only reweighting the previous block's head, without refitting
  endpoints to its weights: Kodak 6x6 λ 0.3 w1 3.02 s against 3.03 s, 3.201 bpt against 3.198.
- **Counting partition texels in registers and caching the starting error in polish_parts.**
  Byte-identical, 3.00 s against 3.01 s on Kodak 6x6 λ 0.3 w1, within noise.
- **Prefetching the hash buckets for the base block's full LZ parse.** The parse's chain loop is
  29% of `cost_until`, so `LzModel::cost` prefetched all 14 positions' buckets first. Byte-identical,
  2.96 s against 2.98 s on Kodak 6x6 λ 0.3 w1, within noise. The buckets (512 KiB) already sit in L2.
- **Building the ASTC path for AVX2 (`-C target-cpu=x86-64-v3`).** The ASTC kernels are SSE2-only, so a
  whole-crate x86-64-v3 build bounds what runtime AVX2 dispatch could gain. It writes the same bytes and
  is 2-3% faster on one pinned P-core (Kodak 6x6 effort 0 λ 0.3 w1 3.28 s against 3.36 s, effort 2
  9.12 s against 9.38 s, best of 3). Too little for multiversioning the whole encoder.
- **Reweighting without polishing the weights.** Taking the previous head's projected, quantized
  weights as they are (no coordinate-descent pass) is 6-8% faster on Kodak 6x6 effort 0 λ 0.3 w1 (one
  pinned core, interleaved, 2.98-3.03 s against 3.17-3.28 s), but worse on rate-distortion: 37.663 dB /
  3.209 bpt, and λ 0.35 37.657 dB / 3.193 bpt, against 37.664 dB / 3.198 bpt polished at λ 0.3.
- **A shallower match-finder chain for ASTC window 1.** `--chain-depth` 8 / 4 / 2 against 16 on Kodak 6x6
  effort 0 λ 0.3 w1: the same PSNR and bpt within 0.001, and no time difference beyond noise.
- **Less smooth-block protection for ASTC PSNR.** `--smooth-scale` 1 / 2 / 3 at lower λ lands on the same
  PSNR-size curve as the default 4 on Kodak 6x6 effort 0 w1 (1 at λ 0.08: 37.666 dB / 3.203 bpt; 2 at
  λ 0.15: 37.666 / 3.203; 3 at λ 0.25: 37.662 / 3.189; default at λ 0.3: 37.664 / 3.198), with lower SSIM.
  `--max-rms-ratio 3` and `--min-rms 1.5` change nothing at λ 0.3.
- **Skipping polish tries whose inputs haven't changed.** A weight's try reads only its texels' infill
  sums and their partitions' endpoints, so a dirty flag per weight and endpoint code (set when a step
  touches those) skips tries that would fail again. Byte-identical (390/390), but polish's samples fell
  only 6.5% at 6x6 effort 0 λ 0.3 w1 and not measurably at 4x4 effort 5 (16.3% of the time either way):
  steps keep landing until the last pass, so few tries repeat a failure.
- **Removing the memmoves and the per-block QuadSums memset from the 4x4 rerank.** The keep-best insert
  and the shortlist insert by hand instead of `copy_within`, and the re-rank table kept in scratch and
  filled only for the texel groups used. Byte-identical; 19.92 s against 19.35 s CPU on Kodak 1-8 4x4
  effort 5 (6 interleaved rounds, one pinned core), within noise.
- **A cheaper light RDO at effort 0 with `--refine-back 0`.** The goal was to keep the default light pass's
  smaller files at effort 0 but skip its reweight / refit candidates, so it would take less time than astcenc
  fast. The cloud run at 75123bb (EPYC 7763, Kodak 24, one pinned thread) gave Zstd bpt, with the default
  light pass, `-l 0`, and refine-back 0 in that order: 7.044 / 7.094 / 7.092 at 4x4, 4.562 / 4.645 / 4.638
  at 5x5, and 3.837 / 3.911 / 3.903 at 6x5. Refine-back 0 keeps only 0.1-0.3% of the 1-2% the light pass
  saves, in 1.26-1.34x the time of `-l 0`. With `-l 0.15` added it is still behind at 5x5 (4.628) and
  6x5 (3.899), and takes 3.2-4.5 s per run. Whole-block copies alone barely help: the reweight and refit
  candidates carry the size win.

## ETC2 / EAC

Measured on the first 8 Kodak images (ETC2 RGBA: the twelve RGBA images of BENCHMARKS.md), effort 5,
λ 0.5, 1 and 2.

- **Field-swap refinement for ETC2 RGB.** `--refine-back 256` against 0 is even: sizes within 0.2% and
  PSNR within 0.02 dB at each λ, so it is off by default. It stays on for ETC2 RGBA, where the alpha half
  gains (1.2-1.7% smaller for 0.02-0.05 dB), and for EAC (RG11 2-4% smaller for 0.04-0.16 dB).
- **Scoring 11-bit EAC with an 8-bit rounding term.** Signed EAC trails unsigned in 8-bit PSNR on smooth
  mid-range blocks (blocks spanning 4-5 levels: about twice the error) because its palette steps sit half
  an 8-bit step off those values. Adding `W × (8-bit error)²` to each texel's 11-bit squared error (W in
  11-bit × 4 units², random blocks, effort 5) only trades one error for the other: at a 4-level span, W 1024
  cuts the 8-bit error 88% but raises the 11-bit error 64%. W 64 gives −11% for +1%. Even an exhaustive
  search scored in 8 bits leaves signed blocks some error there, so this is the format, not the search. The
  11-bit error is what a shader samples, so the encoder keeps it.

## Engineering

- **Closures in `#[target_feature]` kernels.** A closure, including one inside `array::from_fn`,
  doesn't inherit the caller's target features, so AVX2 intrinsics inside it become real
  function calls. This made the first batch encoder 3× slower than scalar. Batch kernels use
  plain loops.
