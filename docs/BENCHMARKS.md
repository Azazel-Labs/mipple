# Benchmarks (v0.9, 2026-11-07)

Sizes are measured **after** a real compressor (Zstd level 19, Deflate level 9) on the raw blocks of the top
mip level. Raw BC1/BC4 = 4 bits/texel (bpt), BC5/BC7 = 8 bpt. PSNR is over the channels the format stores, as
a per-image mean. mipple RDO defaults: window 2048 bytes and 16 KiB chunks (BC7: 16 KiB and 64 KiB; BC1: 8 KiB window; BC5: 3072
bytes), 256 back-references (capped by the window: 192 for BC5), partial-span copies from the nearest 8, smooth scale 5 (ramping to 1 at channel std 14; since 0.9.18 4×, halved at std 12, which only the Oodle section below is measured with), max RMS ratio 10, min RMS 3. Machine: 2-core cloud VM; timings are only comparable within a table. Raw rows are in `bench-results/`.

## Base encoders vs competitors, one P-core (Kodak first 6)

`scripts/compete.py --threads 1 --base-only --affinity 0x1000 --repeat 3` on an Intel Core Ultra 7 155H:
every encoder pinned to one performance core (logical CPU 12) and timed with its own timer, keeping the
fastest of three runs per image. Note: on this CPU logical CPUs 2-9 and 20-21 are efficiency cores. A
competitor config counts as beaten when some mipple effort is at least as good (PSNR) and at least as fast. The
tables below are from development runs on the first 6 images; the committed rows are the all-24 run
(`bench-results/compete-kodak24-1t-p12/`, next section).

Timing notes: at these speeds an image takes about a millisecond, so mipple prints times with microsecond
precision (it printed 0.1 ms before, which quantized fast configs by ~15%). Every encoder starts from the
RGBA image inside its timed region: the harness used to build Intel ISPC's R8/RG8 input plane before
starting the clock, which the other encoders (stb_dxt, rgbcx, mipple) do not get; it is now filled inside
the timed loop with a plain strided copy. The harness still allocates and zeroes the output before timing,
while mipple's time includes allocating its output.

### BC7 top efforts vs bc7e u6

Interleaved, fastest of 2. Effort 8 adds bc7e-style "uber" refits
(least-squares fits from index sets with the extremes nudged inward and stretched, for every multi-subset
mode and mode 6) and refines every mode's best encoding and its two best partition candidates before the modes
compete; effort 9 refines every candidate. (Since then effort 8 also tries mode 4, and effort 9 searches
p-bits by exact error instead of refining every candidate; see the ROADMAP's BC7 mode 4 item.)

| Config | PSNR (dB) | Time, 6 images (s) |
| --- | --- | --- |
| mipple e7 | 45.73 | 1.44 |
| **mipple e8** | **46.10** | **9.36** |
| bc7e u6 (`-u6`, bc7e.ispc) | 46.08 | 11.30 |

With that, every base-encoder configuration of every competitor in these runs is beaten on both PSNR and
speed by some mipple effort, for all four formats.

### RDO vs bc7enc_rdo

`bench-results/compete-kodak24-rdo/`: all 24 Kodak images, 20 threads per encoder. mipple effort 5 λ 0.25-8
against bc7enc_rdo's RDO at its top base level, λ 0.1-4 (BC7: bc7e u6 λ 0.05-1 and bc7enc.cpp `-e`). BD-rate of
mipple's curve (negative = fewer bits at equal PSNR; it does not depend on the thread count) and the range of
total encode times over each RDO sweep (base encode plus RDO, 24 images):

| Format | vs | Zstd-19 | Deflate-9 | mipple time (s) | competitor time (s) |
| --- | --- | --- | --- | --- | --- |
| BC1 | bc7enc_rdo rgbcx L18 | −26.8% | −23.9% | 7.8-11.7 | 11.9-13.3 |
| BC4 | bc7enc_rdo rgbcx hr5 | −20.0% | −18.4% | 4.1-5.9 | 8.3-9.4 |
| BC5 | bc7enc_rdo rgbcx hr5 | −26.1% | −23.8% | 6.6-9.8 | 10.8-12.3 |
| BC7 | bc7enc_rdo bc7e u6 | −31.5% | −29.2% | 17.1-28.5 | 51.8-57.8 |
| BC7 | bc7enc_rdo bc7enc.cpp -e | −30.5% | −28.6% | 17.1-28.5 | 38.1-44.4 |

Each format's RDO rows for both encoders were run together, interleaved per image, so each table row compares
like with like; the BC7 rows come from a later run with the far search (below), the BC1 rows from a later one with
far endpoints (below), the BC4 rows from a later one with frozen literal costs (below), the BC5 rows from a
later one with the localized search (below); mipple's rows were then rerun alone with the smooth weighting
below (bc7enc_rdo's rows are unchanged). BC5 is the closest: mipple's slowest setting (λ 0.25, 9.8 s) is 9% faster
than bc7enc_rdo's fastest (λ 2, 10.8 s). With the error floor at RMS 1.5 per channel instead of
3 (before `--min-rms`), BC4 was −11.0% and BC5 −16.1%; BC1 and BC7 are unchanged.

Every mipple RDO setting is faster than every bc7enc_rdo RDO setting of the same format, at better rate-distortion.
The wide search (256 back-references) became affordable through: field-level dedup (a previous block whose
endpoints or indices an earlier one already had adds no new re-index/refit candidate, and BC4/BC5 half results
are memoized by field value), fused bounded error kernels for BC1/BC4/BC5 (stop once the error cannot win; BC5
reuses the base block's half errors), partial spans only from the nearest 8 blocks, an exact lower bound on the
LZ cost to skip candidates that cannot win, and table-driven literal/length/offset costs. All of these except the
search width and the span distance leave the output byte-identical. BC7 spends the time it has left over on
context: an 8 KiB window and 64 KiB chunks (6 per Kodak image) instead of 2 KiB and 16 KiB took BC7 from
−23.7% / −20.8% to −28.4% / −26.7% at about 1.5× the time (12.2-18.9 s before).

Field-swap refinement (`--refine-back`, default 256 for BC1/BC4, 64 for BC7): after the search picks a block, try replacing
one of its fields (endpoints or indices) with that field of each of the previous 256 blocks, so a block can
take its endpoints from one earlier block and its indices from another. Kodak-24 BD-rate (`mipple check`): BC1
−0.98%, BC4 −0.39%; vs bc7enc_rdo BC1 −19.2% → −19.8%, BC4 −16.8% → −17.0%. It is off for BC5, where it gains
−0.17% at depth 32 and −0.31% at 256 for 4% and 20% more RDO time, which BC5 cannot spare against bc7enc_rdo.
BC7 splits each block into its two 8-byte halves (in mode 6, the most common RDO mode: mode, endpoints and P0;
then P1 and the indices) and reaches past the exhaustive copy depth to the whole window. Mode-6 halves are scored
exactly without a decode (a per-texel error table for each index when the low half is kept, per-texel weights
when the high half is). Kodak-24 BD-rate −0.12% at 1.07× the time, and the equal-RMSE gap to Oodle at λ 10-40
narrows from +2.3-4.0% to +1.8-3.1% (depth 1024 is no better and 1.2× the time).

Literal costs: the rate model used to learn its order-0 literal costs from the bytes emitted so far in the
chunk, so the first blocks of every chunk saw near-uniform 8-bit literals, and RDO's own copies skewed the
counts. BC4/BC5/BC7 now fix them per chunk from the chunk's base encoding: Kodak-24 BD-rate (`mipple check`)
BC4 −1.24%, BC5 −0.44%, BC7 −0.06% (Kodak-8); vs bc7enc_rdo BC4 −17.0% → −17.9%, BC5 −20.7% → −21.1%, at the
same or lower time. Variants that did no better: counting only bytes the parse codes as literals (BC4 −0.45%),
statistics over a sliding 2-8 KiB of base blocks or the whole image, and scaling the costs by 0.85 or 1.15. BC1
keeps the learned costs: frozen ones are even in `mipple check` (λ ≤ 4) but worse at λ 2-8, −19.8% → −19.2%
vs bc7enc_rdo.

Far endpoints (`--far-back`, default 1024 for BC1): beyond the 256-block search, try the endpoints of every
earlier block in the window (BC1's window is now 8 KiB, 1024 blocks), re-indexed for the current block. Oodle
Texture reuses endpoints from hundreds to thousands of blocks back, which a 256-block search cannot see. It is
made cheap by trying each distinct pair once, at its nearest position (a per-position "latest occurrence"
flag, no hashing in the scan), and only when a lower bound on its error leaves it a chance: the mean of any
indices' palette colors lies on the endpoints' segment, so the block error is at least 16 times the squared
distance from the block's mean color to that segment (less a slack for the truncated interpolation). About 1 in
5 distinct pairs passes; 3-color pairs are skipped. Kodak-24 BD-rate (`mipple check`) −0.55% for 1.09× the
time (2048 blocks with a 16 KiB window: −0.59%, 1.16×; trying every far block's endpoints and indices without
the bound: −0.69%, 4×); vs bc7enc_rdo −19.8% → −20.4%. Far index patterns (refit endpoints) add only
−0.17% at several times the cost; a cheap lower bound on each pattern's error brings that to −0.08% for
1.18× the RDO time (see DEADENDS).

BC4/BC5 far endpoints (`--far-back` 1024, also on by default; past the 2-3 KiB windows, since a re-indexed
block rarely matches bytes anyway). For BC4 the bound above (each texel's distance from the endpoint interval)
passed 77% of the distinct pairs, for −0.38% at 1.4× the time. Instead, each pair's exact error with its best
indices takes a few operations: per source block and channel, a 256-entry table gives the count of the texels at
or below each value and the sum of their distances below it, and summing by parts over the 7 midpoints of the
8-entry palette (stored per far block) gives the error exactly. Only the 8 lowest-error candidates are built and
LZ-costed (4 / 16: −0.18% / −0.23% against −0.22%; most far wins are a better endpoint pair, not cheaper
bytes), and the scan is skipped when the near search has already cut J to 0.7 of its start (as the BC7 far
search is gated at 0.8). Kodak-24 BD-rate (λ 0.5-8): BC4 −0.20% Zstd / −0.20% Deflate, BC5 −0.56% / −0.54%,
for 1.115× / 1.101× the time at λ 2 (cloud A/B, ec28caa against 9868e5d, Xeon 6973P-C, Kodak 1-12, one pinned
thread; ungated, locally, 1.19× / 1.16× for −0.22% / −0.56%).
The gain is at λ ≤ 2; at λ 8 the size is up to 0.14% larger at 0.03-0.04 dB more PSNR. The compete times
above predate it; BC5's slowest setting (9.8 s) was only 9% under bc7enc_rdo's fastest, and scaled by the
cloud A/B's 1.10× it comes to about 10.8 s, level with it (an estimate; not re-timed side by side).

Deeper BC5 search: BC5's window is now 3072 bytes, so its search reaches 192 blocks instead of 128 (Kodak-24
BD-rate −2.8%; 256 blocks: −3.6%). It became affordable by making each back-reference cheaper, all
byte-identical: BC4/BC5 candidates carry their exact error from generating them (whole-half copies, re-indexed
endpoints and refit indices are memoized per field value together with their error, and combined candidates
sum half errors), so losers are dropped before any decode; a half an earlier back-reference already had is
skipped outright; a re-index whose endpoint interval alone already loses is deferred until a combined
candidate needs it; and the LZ cost stops as soon as its bits exceed what could still win. Kodak-24 BC5 RDO
time at the old depth fell to 0.69× (BC4 0.9×), which pays for the 192-deep search; vs bc7enc_rdo BC5 went
from −21.1% to −23.5% at 7.4-9.2 s against its 9.7-11.2 s. Restricting the far half of the search to some
candidate types does not make it cheaper. A tighter LZ-cost lower bound (exact literal costs, matches only
where the 3-byte prefix occurs earlier) skipped almost nothing: RDO's candidates are copies, so they nearly
always contain a match.

Localized BC5 search (`--local`, default 16 for BC5): instead of the nearest 192 blocks, BC5 searches the nearest
128 plus 16 blocks picked from anywhere earlier in the chunk, plus the blocks around the one directly above
(±2). The picks are made in two steps: a shape distance (sum of absolute differences between the two blocks'
mean-removed 4×4 texels, both channels) keeps the 128 most similar, then a small neural network (23 features of
the pair: shape and level distances, base-block cost, distance, λ; one hidden layer of 16, ReLU) ranks those and
keeps 16. Every pick then goes through the normal exact candidate search, so the network only decides where to
look, never what is written. Inference is plain Rust (SSE2 with the scalar operation order, no FMA), so output
is identical on every machine; a unit test pins it to the scalar sum. The network was fitted
(`scripts/train_ranker.py`) only on CC0 textures from Poly Haven (`scripts/fetch_polyhaven.py`: every 27th
texture, 1k diffuse and normal maps; 323 encodes), never on Kodak or RAD's images. Ranked by the network, the
best back-reference is in the top 16 for 63% of blocks where one helps, against 50% for shape distance alone.
BD-rate: Kodak-8 −0.90%, Kodak-24 −0.76%, a disjoint Poly Haven held-out set −0.44%, at about the same time
(Kodak-24 1.02×); vs bc7enc_rdo BC5 went from −23.5% to −24.1% (Zstd-19), still faster at every setting.
`--local 0` restores the 192-deep search byte for byte.

Localized BC4 search (`--local`, default 32 for BC4, with `--max-back 128`): the same idea with a single-channel
ranker (`src/ranker/bc4.rs`). Beyond the nearest 128 blocks, 64 earlier blocks of the chunk by shape distance and
64 by value distance (exact integer SAD) go to a 13-16-1 network, which keeps 32; those and the blocks around the
one above get the exact search. Trained (`scripts/train_ranker_bc4.py`) only on single-channel images derived
from CC0 Poly Haven textures (324 encodes, 9.35M candidate rows). On a validation split the best block is in its
top 32 for 42% of groups against 41% for the best hand-made heuristic (min of refit residual and copy SSE) and 34%
for shape distance; end to end that heuristic gives −0.47% against the network's −0.67%. BD-rate against the old
256-deep search: held-out Poly Haven (48 images never used for tuning) −0.71% Zstd-19 / −0.85% Deflate, Kodak-24
−0.77% / −1.00%, at 0.95× the time (20 threads). `--local 0` restores the old search byte for byte.

BC7 far search (`--far-back`, now default 1024 for BC7, with a 16 KiB window): beyond the 256-block search, try
each earlier block in the window as a whole-block copy, and each distinct endpoint field re-indexed. Two exact
bounds keep it cheap: a copy is skipped when the difference of the two blocks' mean colors already costs more
than the best candidate (Cauchy-Schwarz), and a re-index when, in some subset, the mean texel is too far from
the endpoints' segment (as for BC1, now computed from the dequantized endpoints before any palette is built,
with the per-partition subset means cached per block). Re-indexing also stopped unpacking the index bits it
replaces. At the old settings BC7 RDO is byte-identical and 0.94× the time (Kodak-8). Kodak-8 BD-rate
(`mipple check`): −0.85% at 512 blocks for 1.07×, −1.46% at 1024 (16 KiB window) for 1.25×, −1.67% at 2048
(32 KiB); a wider window alone is neutral. Copies alone give −0.52% at 1024 almost for free. Kodak-24 vs
bc7enc_rdo: bc7e u6 −28.4% → −29.4%, `-e` −26.7% → −27.9%, at 22.8-34.0 s against 38.1-57.8 s.

Chunk boundaries cost little: on Kodak-24 BC5 (before frozen literals), 64 KiB chunks are −0.78% BD-rate
against the default 16 KiB and a single chunk per image −0.86% (BC1: −0.54%), but with 24 chunks per image on
20 threads larger chunks are slower (32 KiB: 13.3 s vs 10-12.5 s for λ 0.5 over the corpus; measured before
the chunk pool, which runs every level's chunks, and every input's when `encode` gets several, in one
longest-first queue). An earlier note here put 64 KiB at −2.0%; that does not reproduce at the current
defaults. Seeding each chunk's rate model with the preceding blocks as the base encoder left them made it much
worse (+2.9% at 16 KiB): RDO copies those blocks, and the thread optimizing them changes many.


### BC4 and BC5

Fastest of 10 runs per image.

BC4:

| Encoder config | PSNR (dB) | Zstd-19 bpt | Deflate-9 bpt | Time (s) | Mpix/s |
| --- | --- | --- | --- | --- | --- |
| mipple e0 | 42.91 | 3.834 | 3.840 | 0.01 | 453.71 |
| mipple e1 | 43.52 | 3.835 | 3.842 | 0.01 | 198.26 |
| mipple e2 | 44.28 | 3.830 | 3.836 | 0.03 | 82.78 |
| mipple e5 | 44.64 | 3.831 | 3.837 | 0.06 | 36.58 |
| mipple e6 | 44.81 | 3.831 | 3.838 | 0.11 | 22.24 |
| mipple e8 | 44.98 | 3.831 | 3.838 | 0.22 | 10.71 |
| rgbcx hl | 43.08 | 3.833 | 3.840 | 0.01 | 196.61 |
| rgbcx hr5 | 44.79 | 3.817 | 3.827 | 5.65 | 0.42 |
| stb_bc4 | 43.08 | 3.831 | 3.837 | 0.02 | 100.82 |
| ispc_bc4 | 42.79 | 3.835 | 3.841 | 0.01 | 337.04 |

| Competitor | PSNR | Mpix/s | Beaten by | Gap |
| --- | --- | --- | --- | --- |
| rgbcx hl | 43.08 | 196.61 | mipple e1 (43.52 dB, 198.26 Mpix/s) | ✓ |
| rgbcx hr5 | 44.79 | 0.42 | mipple e6 (44.81 dB, 22.24 Mpix/s) | ✓ |
| stb_bc4 | 43.08 | 100.82 | mipple e1 (43.52 dB, 198.26 Mpix/s) | ✓ |
| ispc_bc4 | 42.79 | 337.04 | mipple e0 (42.91 dB, 453.71 Mpix/s) | ✓ |

BC5:

| Encoder config | PSNR (dB) | Zstd-19 bpt | Deflate-9 bpt | Time (s) | Mpix/s |
| --- | --- | --- | --- | --- | --- |
| mipple e0 | 42.99 | 7.332 | 7.358 | 0.01 | 280.87 |
| mipple e1 | 43.59 | 7.337 | 7.364 | 0.02 | 118.56 |
| mipple e2 | 44.35 | 7.341 | 7.366 | 0.05 | 44.10 |
| mipple e5 | 44.70 | 7.344 | 7.368 | 0.11 | 21.33 |
| mipple e6 | 44.87 | 7.346 | 7.370 | 0.17 | 13.66 |
| mipple e8 | 45.03 | 7.348 | 7.373 | 0.48 | 4.88 |
| rgbcx hl | 43.17 | 7.337 | 7.364 | 0.03 | 81.36 |
| rgbcx hr5 | 44.87 | 7.327 | 7.358 | 11.01 | 0.21 |
| stb_bc5 | 43.17 | 7.335 | 7.361 | 0.05 | 50.74 |
| ispc_bc5 | 42.86 | 7.331 | 7.359 | 0.01 | 205.16 |

| Competitor | PSNR | Mpix/s | Beaten by | Gap |
| --- | --- | --- | --- | --- |
| rgbcx hl | 43.17 | 81.36 | mipple e1 (43.59 dB, 118.56 Mpix/s) | ✓ |
| rgbcx hr5 | 44.87 | 0.21 | mipple e6 (44.87 dB, 13.66 Mpix/s) | ✓ |
| stb_bc5 | 43.17 | 50.74 | mipple e1 (43.59 dB, 118.56 Mpix/s) | ✓ |
| ispc_bc5 | 42.86 | 205.16 | mipple e0 (42.99 dB, 280.87 Mpix/s) | ✓ |

### BC7

`--repeat 2 --interleave` (per image, every config runs in rotation and keeps its fastest time, so load that
drifts over minutes hits all configs alike). Efforts 0-7 run
`src/bc7/fast.rs`, read straight from the surface: mode 6 (exact two-candidate indices, least-squares refits),
then mode 1 on 1-4 partitions per lane ranked by bc7f's popcount estimate or by line-fit error; from effort 4
±1 refinement of each lane's winner; at efforts 6-7 also modes 3, 0, 2 (3-subset partitions ranked the same
way) and dual-plane mode 5 (each color channel rotated into the scalar plane). Like the scalar encoder and
bc7e, opaque blocks pick p-bits by endpoint error, so alpha can decode as 254 (opaque images are scored on
RGB). Efforts 8-9 are the full encoders. Compressonator and texconv are not in this run (both were far behind
in the earlier runs).

| Encoder config | PSNR (dB) | Zstd-19 bpt | Deflate-9 bpt | Time (s) | Mpix/s |
| --- | --- | --- | --- | --- | --- |
| mipple e0 | 41.62 | 6.685 | 6.864 | 0.02 | 155.22 |
| mipple e1 | 43.34 | 6.777 | 6.941 | 0.03 | 72.37 |
| mipple e2 | 44.05 | 6.956 | 7.095 | 0.08 | 28.12 |
| mipple e3 | 44.86 | 7.109 | 7.238 | 0.17 | 13.52 |
| mipple e4 | 45.39 | 7.193 | 7.312 | 0.50 | 4.68 |
| mipple e5 | 45.46 | 7.212 | 7.331 | 0.59 | 4.02 |
| mipple e6 | 45.71 | 7.342 | 7.439 | 1.74 | 1.36 |
| mipple e7 | 45.73 | 7.345 | 7.442 | 2.38 | 0.99 |
| mipple e8 | 45.89 | 7.466 | 7.558 | 40.91 | 0.06 |
| mipple e9 | 45.91 | 7.486 | 7.574 | 134.40 | 0.02 |
| bc7e u0 | 41.79 | 6.732 | 6.900 | 0.06 | 38.05 |
| bc7e u2 | 45.40 | 7.384 | 7.495 | 1.00 | 2.37 |
| bc7e u4 | 45.70 | 7.364 | 7.469 | 1.93 | 1.22 |
| bc7e u6 | 46.08 | 7.368 | 7.477 | 16.96 | 0.14 |
| bc7f:fastest | 41.35 | 6.574 | 6.766 | 0.05 | 43.53 |
| bc7f:faster | 41.35 | 6.574 | 6.766 | 0.06 | 41.10 |
| bc7f:fast | 42.99 | 6.729 | 6.892 | 0.08 | 30.60 |
| bc7f:default | 43.88 | 6.947 | 7.088 | 0.13 | 18.07 |
| bc7f:partial | 44.22 | 7.014 | 7.147 | 0.22 | 10.88 |
| ispc_bc7:ultrafast | 41.78 | 6.730 | 6.899 | 0.06 | 42.13 |
| ispc_bc7:veryfast | 45.22 | 7.312 | 7.434 | 0.81 | 2.93 |
| ispc_bc7:fast | 45.38 | 7.379 | 7.496 | 1.37 | 1.72 |
| ispc_bc7:basic | 45.58 | 7.410 | 7.515 | 3.36 | 0.70 |
| ispc_bc7:slow | 45.69 | 7.462 | 7.560 | 12.49 | 0.19 |

| Competitor | PSNR | Mpix/s | Beaten by | Gap |
| --- | --- | --- | --- | --- |
| bc7e u0 | 41.79 | 38.05 | mipple e1 (43.34 dB, 72.37 Mpix/s) | ✓ |
| bc7e u2 | 45.40 | 2.37 | mipple e5 (45.46 dB, 4.02 Mpix/s) | ✓ |
| bc7e u4 | 45.70 | 1.22 | mipple e6 (45.71 dB, 1.36 Mpix/s) | ✓ |
| bc7e u6 | 46.08 | 0.14 | ✗ | best is mipple e9, 0.17 dB short (at this run; effort 8 now beats it, see "BC7 top efforts vs bc7e u6" above and the Kodak-24 run below) |
| bc7f:fastest | 41.35 | 43.53 | mipple e0 (41.62 dB, 155.22 Mpix/s) | ✓ |
| bc7f:faster | 41.35 | 41.10 | mipple e0 (41.62 dB, 155.22 Mpix/s) | ✓ |
| bc7f:fast | 42.99 | 30.60 | mipple e1 (43.34 dB, 72.37 Mpix/s) | ✓ |
| bc7f:default | 43.88 | 18.07 | mipple e2 (44.05 dB, 28.12 Mpix/s) | ✓ |
| bc7f:partial | 44.22 | 10.88 | mipple e3 (44.86 dB, 13.52 Mpix/s) | ✓ |
| ispc_bc7:ultrafast | 41.78 | 42.13 | mipple e1 (43.34 dB, 72.37 Mpix/s) | ✓ |
| ispc_bc7:veryfast | 45.22 | 2.93 | mipple e4 (45.39 dB, 4.68 Mpix/s) | ✓ |
| ispc_bc7:fast | 45.38 | 1.72 | mipple e4 (45.39 dB, 4.68 Mpix/s) | ✓ |
| ispc_bc7:basic | 45.58 | 0.70 | mipple e6 (45.71 dB, 1.36 Mpix/s) | ✓ |
| ispc_bc7:slow | 45.69 | 0.19 | mipple e6 (45.71 dB, 1.36 Mpix/s) | ✓ |

What the pieces are worth (Kodak 1-6, mean PSNR, one thread, in-process):

| Search | PSNR (dB) | Time (ms) |
| --- | --- | --- |
| mode 6, exact indices (effort 0) | 41.62 | 12 |
| + mode 1 on the popcount partition, blocks with error > 512 (effort 1) | 43.34 | 28 |
| + mode 1 on the best line-fit-ranked partition (effort 3) | 44.86 | 160 |
| mode 1 on 4 ranked partitions, refit, refinement (effort 5) | 45.46 | 563 |
| same without refinement | 45.30 | 331 |
| + mode 3 | 45.52 | 842 |
| + modes 0 and 2 on 1 ranked 3-subset partition each | 45.66 | 1087 |
| + mode 5 (best of three rotations), 2 partitions per 3-subset mode, 2 refinement passes (effort 6) | 45.71 | 1380 |
| line-fit ranking with the batch encoder's `fits8` instead of the one-step λ estimate | same | 2× ranking time |
| 2-subset search only for blocks whose mode-6 error > 64 | −0.08 to −0.14 | |

### BC1

| Encoder config | PSNR (dB) | Zstd-19 bpt | Deflate-9 bpt | Time (s) | Mpix/s |
| --- | --- | --- | --- | --- | --- |
| mipple e0 | 35.80 | 2.978 | 3.102 | 0.02 | 128.22 |
| mipple e1 | 35.88 | 2.974 | 3.099 | 0.04 | 66.84 |
| mipple e2 | 36.44 | 3.005 | 3.121 | 0.24 | 9.98 |
| mipple e3 | 36.54 | 2.996 | 3.114 | 0.47 | 5.07 |
| mipple e4 | 36.60 | 3.004 | 3.117 | 0.63 | 3.76 |
| mipple e5 | 36.62 | 3.001 | 3.113 | 1.00 | 2.35 |
| mipple e6 | 36.67 | 2.991 | 3.105 | 2.69 | 0.88 |
| mipple e7 | 36.67 | 2.987 | 3.101 | 3.07 | 0.77 |
| mipple e8 | 36.67 | 2.986 | 3.100 | 3.48 | 0.68 |
| rgbcx L0 | 35.73 | 2.980 | 3.105 | 0.08 | 28.09 |
| rgbcx L6 | 36.36 | 3.027 | 3.133 | 0.46 | 5.12 |
| rgbcx L12 | 36.60 | 2.996 | 3.108 | 3.91 | 0.60 |
| rgbcx L18 | 36.67 | 2.983 | 3.096 | 12.09 | 0.20 |
| icbc:1 | 35.78 | 2.986 | 3.112 | 0.20 | 12.04 |
| icbc:5 | 36.50 | 2.921 | 3.014 | 0.62 | 3.82 |
| icbc:8 | 36.60 | 2.987 | 3.103 | 1.53 | 1.54 |
| icbc:9 | 36.64 | 2.997 | 3.111 | 3.17 | 0.74 |
| stb_bc1 | 35.55 | 2.999 | 3.124 | 0.17 | 13.60 |
| stb_bc1hq | 35.76 | 2.985 | 3.112 | 0.21 | 11.06 |
| ispc_bc1 | 35.46 | 2.989 | 3.113 | 0.02 | 125.49 |

| Competitor | PSNR | Mpix/s | Beaten by | Gap |
| --- | --- | --- | --- | --- |
| rgbcx L0 | 35.73 | 28.09 | mipple e0 (35.80 dB, 128.22 Mpix/s) | ✓ |
| rgbcx L6 | 36.36 | 5.12 | mipple e2 (36.44 dB, 9.98 Mpix/s) | ✓ |
| rgbcx L12 | 36.60 | 0.60 | mipple e4 (36.60 dB, 3.76 Mpix/s) | ✓ |
| rgbcx L18 | 36.67 | 0.20 | mipple e6 (36.67 dB, 0.88 Mpix/s) | ✓ |
| icbc:1 | 35.78 | 12.04 | mipple e0 (35.80 dB, 128.22 Mpix/s) | ✓ |
| icbc:5 | 36.50 | 3.82 | mipple e3 (36.54 dB, 5.07 Mpix/s) | ✓ |
| icbc:8 | 36.60 | 1.54 | mipple e5 (36.62 dB, 2.35 Mpix/s) | ✓ |
| icbc:9 | 36.64 | 0.74 | mipple e6 (36.67 dB, 0.88 Mpix/s) | ✓ |
| stb_bc1 | 35.55 | 13.60 | mipple e0 (35.80 dB, 128.22 Mpix/s) | ✓ |
| stb_bc1hq | 35.76 | 11.06 | mipple e0 (35.80 dB, 128.22 Mpix/s) | ✓ |
| ispc_bc1 | 35.46 | 125.49 | mipple e0 (35.80 dB, 128.22 Mpix/s) | ✓ |

Every effort now runs in the SIMD batch encoder (`src/bc1/batch.rs`). What each step is worth on
kodim01-03 (pooled PSNR):

| Search | PSNR (dB) |
| --- | --- |
| line fit + 2 refits (effort 1) | 36.12 |
| 4-color cluster fit, every split, snapped (old effort 4) | 36.54 |
| + 3-color fit with dark texels on black | 36.86 |
| same, splits scored by closed-form error only | 36.74 |
| line fit + 3-color fit only | 36.51 |

The 3-color fit is worth +0.3 dB on photos. Scoring splits with the closed-form least-squares error is 5×
cheaper but loses 0.12 dB, so the shipped scorer uses it as a bound and snaps only splits that can still win
(`prune` adds a share of the expected snapping penalty to the bound: 0.8 costs 0.02 dB and halves the time).
On kodim01 mipple's top effort beats rgbcx L18 on high-variance blocks and loses slightly on smooth ones.

## Base encoders vs competitors, all 24 Kodak images, one P-core

`bench-results/compete-kodak24-1t-p12/`, with `--threads 1 --base-only --affinity 0x1000 --repeat 3 --interleave`.
Every one of the 33 competitor configs is beaten on both PSNR and speed by some mipple effort. The last one to fall
was ISPC BC1 (`ispc_bc1`): mipple effort 0 is 0.34 dB better (36.05 vs 35.71) and now slightly faster (467 vs
450 Mpix/s; it was 3% slower before the change below). BC7: effort 8 beats bc7e u6 (46.02 dB in 33.4 s vs
46.01 dB in 38.7 s; mode 4, added since, gives +0.015 dB in the same time: cloud A/B 0.994×), and effort 6 beats ISPC
`slow` (45.64 vs 45.63 dB, 6.6× faster).

The BC1 effort-0 speedup came from dependency chains, not arithmetic: the line fit summed 16 texels per
accumulator in one serial chain (mean, covariance, projection min/max, refit sums), about 64 cycles of latency
each, which out-of-order execution could not hide across ~1000-instruction batches. Four interleaved chains
per sum (`reduce16!` in `src/bc1/batch.rs`) give +11% in-process (444 → 490 Mpix/s), and a dedicated effort-0 instantiation with the configuration as a constant another ~4%. The covariance and
refit sums are now taken over raw integer products, which are exact in f32 in any order, so the output is
unchanged apart from rounding (BC1 BD-rate ±0.00% in `mipple check`). The helpers are macros, not closures:
a closure does not inherit the caller's AVX2 target feature, and one that was not inlined made effort 1 15×
slower.

Two fixes went into this run:

- `--interleave` keyed the fastest time per config by label alone, so configs with the same label in different
  formats (e.g. `mipple e0` in BC1 and BC7) shared one minimum. The key is now (format, label).
- BC1 built its single-color match tables on first use, which cost about 2 ms per process inside the timed
  encode (~10% of an image at effort 0). They are now `const` tables built at compile time.

## vs Basis UASTC and XUBC7 (basisu)

`bench-results/compete-kodak24-basis/`: all 24 Kodak images, one thread per encoder, mipple effort 5 at λ 0-16
against Binomial's `basisu` (basis_universal master 073eb6a of 2026-10-30, newer than release 2.50; Apache-2.0,
built from source with CMake). Neither Basis format is a BC7 encoder. Both are intermediate formats that ship
in a KTX2 and are transcoded to BC7 at load time. `compete.py --basisu` writes each encode as a KTX2,
transcodes it with `basisu -export_dds BC7`, and grades that BC7 with `mipple score` like every other row.
The size columns hold what ships:

- **UASTC** (`-uastc -uastc_level 2`, RDO `-uastc_rdo_l` 0.5-8 with the default 4096-byte dictionary; λ 0 is
  no RDO) ships as UASTC blocks under Zstd supercompression. The KTX2 is written without basisu's Zstd, and
  its raw blocks get the same Zstd-19 and Deflate-9 as mipple's. Check on kodim01 at λ 1: basisu's own KTX2 at
  Zstd level 19 is 352,474 bytes, against 352,290 bytes of Zstd-19 blocks here (the difference is the KTX2
  header and metadata). The extra "UASTC ... as BC7" rows give the transcoded BC7 blocks after Zstd and
  Deflate, which is what a game would ship if it transcoded at build time.
- **XUBC7** (`-xubc7`) is Basis' newer BC7 intermediate, with a weight-grid DCT below quality 100 and its
  own entropy coder. Its size is the KTX2's level-0 bytes plus the supercompression global data, in both
  columns. It is already entropy coded, so no Zstd or Deflate is applied on top. There are three
  curves, each over `-xubc7_rdo_level`, at `-effort 3` (the default) with
  `-xubc7_num_stripes 1`:
  - the bc7f base (`-xubc7_bc7f`) at `-quality 100`, which is no weight-grid DCT;
  - the bc7e_scalar level-2 base (`-xubc7_bc7e_scalar_level 2`) at `-quality 100`;
  - the bc7f base at `-quality 75`, with the DCT.

Both formats use `-linear`, so no sRGB perceptual weighting, matching what PSNR measures. On 4 images, UASTC
gave identical results with `-srgb`.

BD-rate of mipple's RDO curve against each family, for corpus totals. Negative means mipple needs fewer bits.
The "equal SSIM" column interpolates on 10·log10(1/(1−SSIM)), with SSIM on luma.

| vs | Zstd-19, equal PSNR | Deflate-9, equal PSNR | Zstd-19, equal SSIM |
| --- | --- | --- | --- |
| Basis UASTC RDO (shipped UASTC) | **−54.3%** | **−51.9%** | **−45.0%** |
| Basis UASTC RDO, transcoded BC7 | **−50.2%** | **−48.3%** | **−40.3%** |
| Basis XUBC7, bc7f, q100 | **−11.3%** | **−7.3%** | +12.6% |
| Basis XUBC7, bc7e level 2, q100 | **−8.3%** | **−4.6%** | +12.9% |
| Basis XUBC7, bc7f, q75 (DCT) | **−34.3%** | **−29.3%** | **−29.8%** |

mipple's rows are encoded with the BC7 luma error term (default `--luma-weight 0.25`, below). Before it the
four XUBC7 / UASTC columns read −55.9 / −53.5 / −43.8% (UASTC), −51.9 / −49.9 / −39.0% (as BC7), −11.6 / −7.7
/ +15.7% (bc7f q100), −8.5 / −4.9 / +16.2% (bc7e level 2) and −34.8 / −29.9 / −28.6% (q75). mipple's λ 16 now
stops at 32.97 dB, above UASTC λ 8 (32.18 dB), so the UASTC BD-rates cover one point less of its curve.

Matched points (24-image means; Zstd-19 bits per texel):

| Config | PSNR (dB) | SSIM | bpt |
| --- | --- | --- | --- |
| mipple λ 0.25 | 45.38 | 0.99702 | 6.177 |
| mipple λ 0 | 45.37 | 0.99749 | 7.175 |
| XUBC7 bc7e2 rdo 0 | 45.32 | 0.99746 | 5.597 |
| mipple λ 0.5 | 44.28 | 0.99485 | 5.299 |
| XUBC7 bc7f rdo 25 | 43.48 | 0.99632 | 5.070 |
| UASTC λ 0 | 43.22 | 0.99622 | 7.418 |
| mipple λ 1 | 42.06 | 0.98870 | 4.086 |
| XUBC7 bc7f rdo 50 | 41.92 | 0.99464 | 4.571 |
| UASTC λ 1 | 41.01 | 0.98591 | 6.355 |
| mipple λ 2 | 40.18 | 0.97984 | 3.308 |
| XUBC7 bc7f rdo 100 | 39.67 | 0.98909 | 3.789 |
| mipple λ 4 | 38.03 | 0.96685 | 2.671 |
| XUBC7 bc7f q75 rdo 0 | 36.37 | 0.95718 | 3.901 |

### UASTC

UASTC is not close. Its λ 0 encode is 2.2 dB below mipple's base encode, and it is still 3% larger after
Zstd (7.42 against 7.18 bpt). Its RDO then spends PSNR much faster than it saves bits: λ 1 costs 2.2 dB
for 14% fewer bits.

### XUBC7

**Lossless top.** At rdo 0, XUBC7 entropy-codes a full-quality BC7 encode losslessly. With the bc7e base,
that is 45.32 dB at 5.60 bpt, 22% below Zstd-19 on mipple's λ 0 blocks (45.37 dB, 7.18 bpt) and 17% below
mipple's curve at the same PSNR. That is the gain from a format-aware entropy coder over Zstd on raw blocks,
the v1.2 "output B" item in ROADMAP.md.

**PSNR below the top.** mipple's RDO is ahead in PSNR. At 39.67 dB, mipple's curve (between λ 2 and λ 4) is
about 3.14 bpt against 3.79 for XUBC7 rdo 100, 17% smaller, and the BD-rate over the whole range is −8.3% to
−11.3%. XUBC7's DCT
curve (q75) loses PSNR quickly: rdo 0 is 36.37 dB at 3.90 bpt, where mipple needs about 2.25 bpt.

**SSIM.** The ranking flips: XUBC7 has much higher SSIM at the same PSNR. At about 39.7 dB, XUBC7 rdo 100
scores 0.98909 and mipple about 0.9773 (0.97606 at λ 2 before the luma term). At equal SSIM, mipple needs
about 13% more bits than either q100 curve (16% before), and XUBC7 wins on 23 of 24 images (all but
kodim20). Its error is not simply moved from luma to chroma. Before the luma term, on kodim01 (bc7f rdo 100 against mipple λ 2, both near 39 dB), mipple has the higher luma PSNR
(41.27 against 41.11 dB) and still the lower SSIM (0.9888 against 0.9923). A lead from the same pair: of
the 4x4 blocks with source luma std ≥ 2, 3.9% of mipple's keep less than 70% of their luma std, against
1.3% of XUBC7's. Over-flattened texture is what SSIM's contrast term punishes. This is now a ROADMAP item.
Oodle, for comparison, stays behind mipple at equal SSIM (previous section).

**Luma error term.** BC7 RDO now adds γ·3 × the smooth-weighted Rec. 709 luma error to each candidate's
distortion (`--luma-weight γ`, default 0.25 for BC7 and 0 for other formats, normal maps and `--role mask`).
SSIM is computed on luma, so it gains from this first-order, while RGB RMSE loses only second-order. The
term is added only to a candidate that could become the best or the runner-up, so every search bound stays
valid. At γ 0.25 the equal-SSIM gap to XUBC7 (bc7e level 2) falls from +16.2% to +12.9% and the equal-PSNR
lead moves from −8.5% to −8.3%. Every image gains (kodim02 +11.8% → +7.5%, kodim15 +6.2% → +3.1%, kodim20
−3.7% → −9.1%), but XUBC7 is still smaller at equal SSIM on 23 of 24. Against Oodle every cell stays ahead:
equal RMSE λ 20 goes from −1.0% to −0.7%, the thinnest, and the equal-SSIM lead widens by 0.4-0.9 points
(next section). More weight trades further along the same line: γ 0.3 gives +12.4% with λ 20 at −0.6%
on RMSE, and γ 0.5 gives +10.4% with λ 20 at −0.1%. A contrast-loss term, stronger or sharper smoothness
weights and per-block error caps bought a third as much SSIM or less per point of the Oodle lead
(DEADENDS.md). The remaining gap sits in near-flat windows: windows with luma variance below 8 are a fifth
of all windows and hold 69% of the SSIM loss.

The term moves each λ to a higher quality (λ 1: 41.64 → 42.06 dB, 3.868 → 4.086 bpt), so it is timed at
equal quality. γ 0.25 at λ 1.19 / 4 is compared against γ 0 at λ 1 / 3.36, the λ values that match PSNR and
Zstd size on the Kodak-24 sweep. On all 24 images, on one pinned P-core with the builds interleaved and the
best of 3 per image, it takes 1.037× / 1.054× the time. At the same λ it takes 1.08-1.13× at λ 1 and
0.95-1.05× at λ 4 (6-8 images, busy machine). mipple's times in the committed rows and in the table
below are from before the term; only sizes and quality were re-encoded.

### Time

The machine was busy with other jobs, so the full run measured each encoder process's CPU time (user plus
kernel, `--cpu-time`) rather than wall time. That time includes PNG load and file write and is robust to
time-slicing. It is not robust to a busy SMT sibling, which inflates every number here relative to the quiet
pinned-core tables above. For example, mipple's λ 0 base encode is about 0.055 s per image there and 0.27 s
here.

The table below is a separate check on the four development images (kodim01, 03, 13 and 23).
Every encoder was pinned to one P-core (logical CPU 2), the configs were interleaved, and each kept the
fastest of three runs. Times are CPU seconds per image.

| Encoder | No RDO | RDO sweep |
| --- | --- | --- |
| mipple e5 | 0.27 (λ 0) | 1.6-4.8 (λ 16 → 0.25) |
| Basis UASTC | 4.4 | 9.8-11.8 (λ 8 → 0.5) |
| Basis XUBC7 bc7f q100 | 0.94 (rdo 0) | 1.7-1.9 |
| Basis XUBC7 bc7e2 q100 | 1.9 (rdo 0) | 2.7-2.9 |
| Basis XUBC7 bc7f q75 | 2.1 (rdo 0) | 2.8 |

On the same four images, the 24-image run's times were up to 7.5% higher for mipple and 10-33% higher for
basisu, with the ordering unchanged. The committed rows therefore favor mipple slightly on time; prefer the
table above. UASTC's RDO takes 9.8-11.8 s per image against mipple's 1.6-4.8 s. XUBC7 is about as fast as
mipple's middle λ (λ 1: 2.3 s; XUBC7 bc7e2 rdo 50: 2.9 s) and faster than mipple's light settings
(λ 0.25-0.5: 4.7-4.8 s). Treat all of these times as ±25%.

Exploration on those four images (not in the committed rows):

- UASTC with a 16 KiB RDO dictionary moves along about the same curve as the 4096-byte default (λ 1: 1.1%
  smaller, 0.15 dB lower).
- XUBC7 with 1 stripe instead of the default 8 is 0.4-0.8% smaller.
- Effort 7 is no better than effort 3 (bc7e2 rdo 0: 5.693 against 5.658 bpt).
- XUBC7 at q90 is already 3.4 dB below q100 at rdo 0 (bc7f: 40.89 against 44.30 dB, 4.86 against 5.56 bpt).

**bc7g** is not released (checked 2026-11-16; see [research/bc7f-bc7g.md](research/bc7f-bc7g.md)). **NVTT 3**
is a proprietary download behind an NVIDIA developer login, so it is not automated.

## vs Oodle Texture 2.9.16 (Unreal Engine 5.8)

Oodle Texture is closed source, so `scripts/ue_oodle/run_ue_oodle.py`
drives a stock UE 5.8.4 install as a user would (no engine source, no direct DLL calls): it imports each PNG
once per format and Lossy Compression Amount (None, Lowest, Low, Medium, High, Highest = Oodle λ 0, 1, 10, 20,
30, 40), cooks with Final encode speed and RDO on, and takes Oodle's blocks from the cook's
`-OodleDebugDumpFilter` dumps, checked bit-exact against the cooked package and against the PNG Oodle was
given. The DDC key confirms SDK 2.9.16. `scripts/ue_oodle/compare.py` then scores those blocks and a mipple
-e 5 λ sweep on the same pixels, sizes Zstd-19 over the block data for both, and writes
`bench-results/oodle-ue/` (per-image rows in `results.json`). BC1 and BC7 inputs are the PNGs unaltered; BC4
is the R channel; BC5 goes through UE's normal-map import (renormalized XY as R16G16), so that R16G16 input is
the reference and mipple encodes it rounded to 8 bits.

mipple's size at Oodle's quality, relative to Oodle's size (Kodak-24 totals; negative = mipple smaller):

| Oodle λ | BC1 at RMSE | BC1 at SSIM | BC7 at RMSE | BC7 at SSIM | BC4 at RMSE | BC4 at SSIM | BC5 at RMSE | BC5 at SSIM |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | −18.9% | −9.4% | −6.5% (15/24 images) | −2.6% (4/24) | −13.4% | −5.4% | −31.1% | −11.3% |
| 1 | −15.1% | −6.8% | −2.9% (12/24) | −0.8% (4/24) | −4.0% | −1.7% | −4.5% | −1.8% |
| 10 | −11.5% | −7.5% | −1.6% | −4.1% | −6.6% | −2.1% | −10.7% | −4.2% |
| 20 | −11.0% | −7.3% | −0.7% | −3.1% | −9.6% | −3.2% | −15.7% | −5.7% |
| 30 | −11.3% | −6.9% | −1.2% | −2.2% | −12.6% | −4.3% | −19.0% | −6.3% |
| 40 | −11.5% | −6.7% | −2.0% | −1.6% | −15.1% | −5.1% | −21.2% | −6.7% |

All four are rerun at 4e8547d (the near-tie deferred decision, the neighbourhood smoothness weight, BC1
lookahead and the BC7 far-search gate; before it BC1 was −8.8% / −4.0% to −5.7% at λ 10-40 and BC7 −0.4% to
−1.3% at RMSE, −0.2% to −3.0% at SSIM), on top of mipple 0.9.18's smooth weight (4×, halved at channel std
12), BC4/BC5 RDO from an effort-8 base, and the BC7 speedups below. BC5's R16G16 input is rebuilt from the
PNG as UE's import does (XY of the normalized RGB, to 16 bits), which reproduces the recorded λ 0 scores exactly.
BC7 is rerun at d242683 (cheaper BC7 RDO defaults: near search 8 deep, refinement 128, deferral margin 0.15),
which gave up 0.1-0.4 points of each lead (from −1.9% / −1.7% / −2.3% / −3.2% at RMSE and −3.9% / −2.8% /
−2.4% / −2.0% at SSIM) for 1.22× / 1.24× / 1.21× less time at λ 1 / 10 / 30 (GitHub-hosted AMD EPYC 7763,
one thread, `ab.yml`). It is rerun again for the ranker's 512-block reach (was 2048; see the ROADMAP's BC7
speed item), which gave up 0.1-0.4 points more (from −1.8% / −1.4% / −2.0% / −2.8% at RMSE and −3.8% / −2.6%
/ −2.2% / −1.8% at SSIM) for 1.17× / 1.21× less time at λ 1 / 10. It is rerun once more for the luma error term
(`--luma-weight`, default 0.25 for BC7; see the XUBC7 section), which gives up 0.0-0.4 points at RMSE for 0.4-0.9 more
at SSIM (from −1.6% / −1.0% / −1.6% / −2.3% at RMSE and −3.6% / −2.2% / −1.7% / −1.2% at SSIM, after the
64-block field-swap refinement).

Speed (BC7, Core Ultra 7 155H): Oodle's CPU time is UE's process CPU time for a cook with an empty DDC minus
the same cook with a full one, about 62 CPU-seconds for Kodak-24 at λ 30 (High). mipple reaches that quality
at λ 1-1.5. BC7 RDO at λ 1 took about 104 CPU-seconds for Kodak-24 at 9c2eacf (one process per image on 6
pinned P-cores) and about 89 with the lighter second base (`bc7::rdo_alt_cfg`: the effort-8 plain search
kept beside the entropy-reduced base polishes one candidate per mode, not two, refines once, not twice, and
stretches uber index sets by one step, not three). The polish and refine cuts left the table above unchanged
at every λ (about 10% of the CPU time), the uber cut moved λ 30 at RMSE and λ 20 at SSIM by 0.1% (another 5%).
The ranker's prefilter then took eight candidate blocks at a time (byte-identical output, about 6% less, to
about 83 CPU-seconds; both builds encoding each image back to back on the same core), and mode-6 re-indexing
rejected more candidates on a per-texel lower bound before choosing indices (byte-identical, about 3% less).
The multi-subset rejection then stopped at the first subset that decides it, dequantizing only the endpoints it
reaches (byte-identical, about 5% less, to about 77). The far search's prefilter then also screened
multi-subset candidates before re-indexing them: the endpoint channels fully inside a candidate's first 64 bits
(the rest come from the current block) pin each endpoint to within half a quantization step, and a subset's
mean texel too far from that segment rejects (byte-identical, about 2% less), and mode-6 re-indexing chose
all 16 indices at once with AVX2 (the same projection, window and ties; byte-identical, about 2.5% less, to
about 80). BC7 block packing, unpacking and decoding then got one copy per mode, so each mode's bit layout
unrolls to fixed shifts (byte-identical, about 5% less, to about 75). The fast encoder's 2- and 3-subset
candidates (the plain alternate base) then encoded their subsets in lockstep, one indexing pass for all of them
(byte-identical, about 2.5% less), and BC7 skipped the Zstd check for chunks the rate model says shrank to 0.6
or less, which on Kodak-24 never fail it (byte-identical there; about 1% less at λ 1, to about 73, and 5% at λ 10).
The far search then re-indexed only mode-6 heads, dropping the multi-subset prefilter above: those far
re-indexings almost never won, and every Kodak-24 delta against Oodle moved by 0.02% or less (only λ 20 at
RMSE changes in the table, −0.4% to −0.3%), for about 5% less CPU at λ 1 and 1% at λ 10. Each mode-6 block's
two palettes are now also built once, when it is finished, not for every later block that re-indexes it
(byte-identical). The Zstd check then looked at level 1 first, skipping level 19 for chunks at most 0.9 of the
base's size there (byte-identical, about 2.5% less at λ 1), and the ranker's prefilter passed it 8 blocks per
key, not 16 (λ 20 SSIM, λ 30 SSIM and λ 40 change by 0.1% in the table, each cell by at most 0.07%; about 2%
less). The second base then skipped mode 3, which survives RDO in about 1 block in 12 of those it wins there:
no cell moved by more than 0.05%, the larger moves gains (λ 20 at RMSE −0.3% to −0.4%), for about 6.5% less
CPU at λ 1 and 3.7% at λ 4. It then refit endpoints once, not twice, and skipped mode 5 (no cell moved by
more than 0.01% at RMSE, SSIM gains up to 0.03%; 3.4% less at λ 1). Three cuts followed, byte-identical on Kodak-24: index-copy
and layout-copy refits whose least-squares bound already exceeds the error bound are skipped (about 2% less at
λ 1), the far search re-indexes only each distinct head's latest block in its range (3.5% less), and the final
one-field refinement of a mode-6 block no longer tries other modes' first halves over its indices (about 3% less
at λ 1; those never won on Kodak-24 or the Poly Haven test set, and one synthetic mask moved by 0.2% in size at a 0.01 dB
better PSNR). Chained, these paired measurements
seemed to put mipple at or slightly under Oodle's CPU time for the same size and quality (about 0.92-1.01× of the 62
CPU-seconds above), but a fresh side-by-side run at b99ffa3 does not bear that out. Oodle's share of a cook, taken as a
texture-only cook (fresh imports, shaders already in the DDC) minus a cook with a full DDC, was about 47 CPU-seconds
(four texture-only cooks at 90.6-101.9 against 46.6-53.6 with a full DDC) and 2-3 s of wall time. mipple at λ 1 and
1.5, which bracket Oodle's λ 30 RMSE, used 55-64 CPU-seconds encoding the 24 images one after another with all
threads (13-14 s wall), 76-98 encoding all 24 at once with one thread each (5.7-6.9 s wall), and 33 s on one pinned
P-core. CPU-seconds on this hybrid, hyperthreaded CPU depend heavily on load, so both were then measured on one
pinned P-core (core 16; UE widens its own affinity at start, so its processes were re-pinned every 100 ms). The
texture-only cooks took 51.5-55.4 CPU-seconds against 31.8-33.9 with a full DDC, which puts Oodle's share at about
20-22 CPU-seconds (three pairs). mipple encoding the 24 images at λ 1 (`encode`, one process per image, KTX2
output) took 34.2-34.6. On the same core, mipple's BC7 RDO costs about 1.6× Oodle's CPU for smaller files at the
same quality. Both bases then kept mode 6 for near-flat blocks: the plain effort-8 base skips the multi-subset
search and mode-6 polish when the block's mode-6 error is at most 384 (`busy_err`), and the entropy-reduced base
when its weighted mode-6 score is at most 512. Those blocks then share one mode-6 layout that later blocks can
copy, so files shrink and the quality table gains (λ 20-40 at SSIM −2.5%, −1.8%, −1.5% to −2.8%, −2.4%, −2.0%;
λ 40 at RMSE −3.1% to −3.2%; λ 10 and below unchanged, since the gates only apply at λ ≥ 1). On 8 Kodak images
at λ 1 on one pinned P-core (interleaved, per-image best of five) the bases took 1.04 s instead of 2.71 and the
whole encode 10.48 s instead of 12.00 (−12.6%), with 2.3% smaller files. Kodak-24 at λ 1 (`encode`, one
process per image) now takes 31.2-31.6 CPU-seconds on one pinned P-core, about 1.5× Oodle's share.
Later byte-identical speedups and cheaper defaults have not been re-timed side by side (the UE install is
gone and the dev machine is under load), but paired cloud A/Bs on Kodak-24 put them at 1.557× less time
than that build at λ 1 (7c3c514 against 941af16), and refinement 64 deep instead of 128 at another 1.080×.
Scaled from the 31.2-31.6 CPU-seconds, λ 1 was about 18.5 CPU-seconds against Oodle's 20-22. The BC7
luma term since then takes 1.082× the time at λ 1 (cloud A/B, at higher quality per λ), which puts it at
about 20: level with Oodle. An estimate, not a side-by-side measurement.

mipple is now smaller at both of Oodle's quality measures at every λ in every format. Per image it is not a
sweep: BC1 loses 1 of 288 image/λ/metric cells (kodim05 at λ 40 on SSIM, +0.3%); BC7 at λ 10-40 is behind on
RMSE for 21 of 96 image/λ pairs (up to +2.8%, kodim09 at λ 20) and on SSIM for 19 (up to +5.1%, kodim20 at λ 30,
and +4.5% on kodim07: Oodle's RDO keeps more structure in their flat areas at λ 30-40); BC4 and BC5 are behind on RMSE
in 4 and 2 of 120 image/λ pairs at λ 1-40 (up to +0.4%, +0.6%), but on SSIM in 14 and 22: BC4 most on kodim18
and kodim05 (about +4% at λ 20-40), BC5 on kodim02 and kodim13 (up to +6.7%).

BC5 λ 1 was the last cell behind (+4.0%, then +2.6% after the smooth-weight change, 22 of 24 images reaching
Oodle's RMSE): Oodle's λ 1 is more accurate than its own λ 0, and mipple's -e 5 base (endpoint window radius 1)
could not reach it. RDO now starts BC4/BC5 from an effort-8 base (radius 4, `base_block_opts` in
`src/pipeline.rs`), as BC7 does: BC5 λ 1 −4.1% (all 24 images), λ 10-40 −9.8% to −20.1%; BC4 λ 1 −1.1% →
−3.8%. Kodak-24 BD-rate BC4 −0.95%, BC5 −1.20% at the same encode time (the base is a few ms next to RDO).

BC7 was the gap: Oodle 2.9.16 held 2-3% fewer bits at equal RMSE at λ 10-40 until the smooth-weight change
(last item below). Its non-RDO BC7 is more
accurate than mipple's -e 5 base, so at λ 0-1 mipple's curve only reaches Oodle's RMSE on some images (counts
in brackets); at -e 8 or 9 without RDO mipple is as accurate as Oodle's λ 0 on 5 of those 6 images and
smaller on all 6 (kodim03: RMSE 1.650 at 6.85 bpt vs Oodle's 1.623 at 7.30), so the gap is in the RDO search,
not the base encoder. It was 7-9% (+8.2%,
+8.8%, +8.3%, +6.9%) before four RDO changes, which the first 6 images (kodim03, 05, 08, 13, 18, 24) sized:

- the plain search runs at effort 8 or more whenever RDO is on (`RDO_BASE_EFFORT` in `src/pipeline.rs`; base
  time 11 ms → 241 ms per Kodak image, next to 1.5-3 s of RDO). At λ below 1 (no entropy reduction) that is
  the base itself, and Oodle's λ 10 quality sits there: +11.0% → +2.0% at λ 10. Most of the gain.
- with an entropy-reduced base (λ ≥ 1), that plain encoding is a second base: each block starts from
  whichever of the two scores better (+9.4% → +8.5% at -e 5 before the effort floor).
- a cross-mode candidate: an earlier block (within 8, or the one above) whose mode or partition differs is
  tried with this block's endpoints refit to its layout, so mode, partition and index bits match exactly
  (+8.5% → +8.3%; every earlier block gives +8.1% at a quarter more RDO time at λ 1).
- a mode-6-only encode (`bc7::mode6_cfg`, 2 refits) as a further base, since Oodle's RDO output is 70-80% mode
  6: on the 6 images +3.9% → +2.8% at RMSE and −2.4% → −3.5% at SSIM (with the learned ranker); Kodak-24
  BD-rate −0.22% vs the ranker alone at 1.06× the time. 128 KiB chunks would add another −1.0% to −1.3% BD-rate
  at the same CPU time (6-image Oodle subset: +2.1% / −4.5%), but fewer, longer chunks leave threads idle: 1.4-1.5×
  the wall time at 20 threads, so BC7 chunks stay 64 KiB. Re-measured with every level's chunks in one pool:
  −0.86% / −1.03% BD-rate (RMSE-PSNR / SSIM, Kodak-24 total curves; per image −0.4% to −1.8%),
  equal-RMSE λ 10-40 −1.6/−1.0/−1.6/−2.3% → −1.9/−1.5/−2.1/−2.9%, equal SSIM −3.6/−2.2/−1.7/−1.2% →
  −4.2/−2.9/−2.4/−2.0%. All 24 images in one process take the same wall time (best of 5: 7.88 s against 7.85 s,
  median 8.40 against 8.12 s, 22 threads, λ 1 with mips), but one image at a time is still 1.6× (best per image
  23.6 s against 14.5 s for the 24): an image's work is mostly three top-level chunks of 8192 blocks and one of
  6144 at level 1, so most threads idle. So 64 KiB stays the default.
- the smooth-block weight's shape (all formats). The linear ramp gave flat blocks 5× and blocks at channel std
  14 or more nothing, so gradual-but-textured blocks got no protection while the flattest were over-weighted:
  turning the weight down only moved error from SSIM to RMSE (`--smooth-scale 1` +2.1% / +7.5%, 2.5 +2.0% /
  +1.6% on the 6 images). `1 + (scale − 1) · k² / (k² + σ²)` keeps a tail of protection for busy blocks:
  Kodak-24 BC7 λ 10-40 at RMSE went from +2.0%, +3.1%, +2.5%, +1.8% to −1.1%, −0.6%, −0.9%, −1.5%, at SSIM
  from −5.1% … −2.3% to −3.1% … −0.3%; BC1 and BC4 gain 2-4% at RMSE and give back up to 1.7% of their SSIM
  lead. Scale 3 at k 12 is further ahead on RMSE but behind on SSIM at λ 30-40 (+0.4%, +0.8%); 4 is the
  lowest that stays ahead on both. `mipple check`: BD-rate −1.2% to −2.8% (synthetic), −3.8% (BC7 Kodak-4)
  at the same speed.

On those 6 images, mean of λ 10-40 at RMSE / SSIM: -e 5 +9.4% / −2.7% before the first four changes, +4.0% /
−2.2% after; -e 9 is the same as -e 5.

At λ 1 Oodle comes out both more accurate and smaller than its own λ 0 (BC5: RMSE 2.687 at 5.69 bpt vs 2.756
at 7.43); mipple's BC4 and BC5 are now ahead there too (−3.8% / −1.1%, −4.1% / −0.7%).
The crop of kodim18 alone (0.9.16 numbers) is behind at equal SSIM for BC4 and BC5:
kodim18 (512×512 crop), at RMSE / at SSIM: BC4 −5.2% / −1.5% at λ 0, then +2.8% / +4.8%, +1.2% / +6.2%,
+0.2% / +6.7%, −0.6% / +6.6% at λ 10-40; BC5 −28.9% / −8.7% at λ 0, +5.6% / +3.8% at λ 1, then −7.3% / +2.3%,
−12.2% / +1.0%, −14.8% / +0.4%, −17.2% / +0.7% at λ 10-40. BC1 kodim18: −7.3% at λ 0, −0.8% to −1.8% at λ 10-40 on
RMSE, −2.6% to −4.8% on SSIM.

## BC3 vs bc7enc_rdo

Twelve RGBA images: Kodak 1, 3, 5, 13, 18 and 23, each with alpha from the next Kodak image's luma (detailed
alpha) and with a smooth radial alpha that reaches 0 in the corners. mipple effort 5 with `--alpha data` (so all
four channels are scored against the source) against bc7enc_rdo `-3` (rgbcx L18 color, BC4 alpha at radius 5)
with `-zc8192`, all threads, top level only. PSNR is over RGBA, bits/texel after Zstd-19 and Deflate-9, and time
is the total wall time over the twelve images, process start included:

| λ | mipple PSNR | mipple Zstd bpt | mipple Deflate bpt | mipple s | bc7enc_rdo PSNR | bc7enc_rdo Zstd bpt | bc7enc_rdo Deflate bpt | bc7enc_rdo s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 36.674 | 6.032 | 6.282 | 2.4 | 36.687 | 5.994 | 6.258 | 4.8 |
| 0.5 | 36.517 | 4.846 | 5.059 | 6.7 | 36.397 | 5.134 | 5.283 | 35.3 |
| 1 | 36.320 | 4.465 | 4.668 | 6.9 | 35.948 | 4.808 | 4.950 | 35.3 |
| 2 | 35.893 | 3.999 | 4.200 | 6.7 | 34.980 | 4.422 | 4.563 | 32.9 |
| 4 | 35.043 | 3.417 | 3.620 | 6.6 | 33.251 | 4.016 | 4.154 | 30.5 |

BD-rate of the mean curves: −16.0% after Zstd-19, −14.4% after Deflate-9. Every mipple RDO point is both smaller
and closer to the source than the bc7enc_rdo point at the same λ, at about a fifth of the time. (The times are
from an earlier run of the same commands on an earlier build of these twelve images; sizes and PSNR are from the
current images.) The base encoders are even: the color half is the BC1 encoder held to 4-color blocks (a BC3
color half never decodes in 3-color mode), the alpha half the BC4 encoder.

## BC6H vs ISPC, texconv, Compressonator

Twenty Poly Haven HDR environment maps at 1024×512 (`scripts/fetch_polyhaven.py --hdris --step 50`), unsigned
BC6H (`BC6H_UF16`), one thread for every encoder, pinned to one P-core, `python scripts/compete_bc6h.py
test-images/polyhaven-hdri --threads 1 --cpu 14 --runs 3 --efforts 0,1,2,5,9`. Scores are in linear light:
mPSNR is multi-exposure PSNR (Munkberg et al. 2006) over each image's exposure range, log2 RMSE is roughly
the error in stops. Means per image; time is the total over the twenty images (best of three per image for
mipple's encode loop and the ISPC harness's block loop; process wall time for texconv and Compressonator,
which includes loading and writing):

| encoder | mPSNR dB | log2 RMSE | Zstd-19 bpt | time s |
| --- | ---: | ---: | ---: | ---: |
| mipple effort 0 | 44.951 | 0.04983 | 7.215 | 0.120 |
| mipple effort 1 | 46.713 | 0.03980 | 7.388 | 0.475 |
| mipple effort 2 | 46.873 | 0.03901 | 7.387 | 0.784 |
| mipple effort 5 | 47.022 | 0.03830 | 7.392 | 2.14 |
| mipple effort 9 | 47.117 | 0.03785 | 7.400 | 64.1 |
| ISPC veryfast | 44.857 | 0.05035 | 7.213 | 0.127 |
| ISPC fast | 46.586 | 0.04034 | 7.370 | 0.897 |
| ISPC basic | 46.745 | 0.03959 | 7.359 | 2.21 |
| ISPC slow | 46.838 | 0.03916 | 7.402 | 9.26 |
| ISPC veryslow | 46.848 | 0.03912 | 7.409 | 19.7 |
| texconv | 44.555 | 0.05390 | 7.334 | 5.27 |
| Compressonator q0.1\* | 45.944 | 0.04552 | 7.239 | 161 |
| Compressonator q1.0\* | 45.944 | 0.04552 | 7.243 | 2985 |

\* Too slow to bother using. These rows are from an earlier, unpinned run with the same images, and
`compete_bc6h.py` now runs Compressonator only when given `--cmp`.

mipple encodes eight blocks at a time, one per SIMD lane, as ISPC does. Every ISPC profile has a mipple
effort that is both better and faster:

- Effort 0 (one region, the refit keeps the indices) is 0.09 dB ahead of ISPC veryfast, and 6% faster.
- Effort 1 is 0.13 dB ahead of ISPC fast in 53% of its time.
- Effort 2 is 0.13 dB ahead of ISPC basic in 35% of its time, and ahead of ISPC veryslow (+0.03 dB) in 4% of
  its time.
- Effort 5 is 0.17 dB ahead of every competitor setting, in 2.1 s.

Without RDO, Zstd sizes are within 0.5% of each other for every encoder at matching quality.

### BC6H RDO

`--lambda` runs an RDO pass over the effort-5 blocks. Distortion is the squared error of the half-float bit
patterns, which is close to relative (log) error, so a λ means the same at every exposure. Each block tries
whole-block copies from the last 256 blocks, the endpoints of the last 256 blocks with its own indices
re-chosen, and the indices of the last 32 blocks with least-squares endpoints refit to them. It scores each
candidate with the same LZ rate model as the 8-bit formats. Same twenty images, totals of the Zstd-19 KTX2
files (no mips), `--zstd 19` so the pass checks its chunks against Zstd:

| λ | file bytes | vs λ 0 | mPSNR dB | log2 RMSE |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 9,693,737 | | 47.022 | 0.03830 |
| 1 | 6,209,485 | -35.9% | 45.782 | 0.04260 |
| 2 | 5,570,380 | -42.5% | 44.980 | 0.04646 |
| 4 | 4,870,733 | -49.8% | 43.854 | 0.05284 |
| 8 | 4,149,481 | -57.2% | 42.382 | 0.06290 |
| 16 | 3,455,613 | -64.4% | 40.570 | 0.07824 |

At λ 2 the files are 42% smaller than plain effort 5, at the quality of effort 0 or ISPC veryfast. None of the
other BC6H encoders here has an RDO mode. Refitting endpoints to borrowed indices is worth 3.2% BD-rate
against re-indexing alone. Looking back 32 blocks for refits instead of 256 costs 0.3% BD-rate for about half
the RDO time.

### Signed BC6H (`bc6h-sf16`)

No standard signed HDR test set exists, so this one is made from the same twenty HDRIs: each image minus a 9×9
box blur of itself (a detail layer, both signs everywhere, HDR magnitudes at highlights), saved as `.pfm`.
One thread pinned to one P-core, best of three, `python scripts/compete_bc6h.py <dir> --signed --threads 1
--cpu 14 --runs 3 --efforts 0,1,2,5`. The metrics score signed values: mPSNR tone maps each value by its
magnitude and keeps its sign, and log2 RMSE measures stops above 2^-14 with the sign, so a sign error costs the
distance through zero. Mostly small values of both signs make this a much harsher test than the table above;
compare rows, not the two tables:

| encoder | mPSNR dB | log2 RMSE | Zstd-19 bpt | time s |
| --- | ---: | ---: | ---: | ---: |
| mipple effort 0 | 13.257 | 1.98839 | 7.429 | 0.683 |
| mipple effort 1 | 16.571 | 1.36070 | 7.545 | 3.58 |
| mipple effort 2 | 16.653 | 1.34914 | 7.527 | 5.81 |
| mipple effort 5 | 16.730 | 1.33797 | 7.525 | 14.6 |
| texconv | 5.816 | 5.40843 | 6.120 | 17.5 |

Compressonator\* is left out of the table. \* Too slow to bother using, so it was not run on all twenty: on
the first two images, `-fd BC6H_SF` scored 15.471 dB in 229 s at quality 0.1 and 15.843 dB in 2,503 s at
quality 1.0, against 17.230 dB in 0.51 s for mipple effort 2.

texconv's `BC6H_SF16` encoder clamps every negative value to zero, even from a half-float DDS. Its decoder
agrees with mipple's on every block, so the encoder alone is to blame, and its smaller files are mostly the
zeros. ISPC has no signed mode. Every mipple effort is far ahead of the only other encoder that runs in useful
time.

- On images without negative values, signed blocks score 0.01-0.13 dB below unsigned ones: each endpoint
  spends a bit on the sign.
- Modes with 6-bit endpoints keep only 5 bits of magnitude when signed, so their endpoint levels sit about a
  factor of two apart. Compressonator's output has the same coarse blocks.
- RDO works the same way signed. On one natural HDRI (art_studio) read as signed, λ 2 shrinks the Zstd stream
  from 7.40 to 4.55 bpt (−39%) at 42.29 dB, against 7.64 to 4.57 bpt at 42.29 dB unsigned. On the detail
  layers λ 2 saves only 1.3-1.7%: they are close to noise.

## ETC2 and EAC vs Compressonator

ETC2 RGB on all 24 Kodak images, one thread for every encoder, top level only, `python scripts/compete_etc.py
test-images/kodak --threads 1`. Compressonator 4.6.0 (`-fd ETC2_RGB`; `-Quality` and `-CompressionSpeed`
change nothing on its ETC2 path) is the only ETC2 encoder on disk: etcpak and etc2comp could not be built
here, so they are not graded. PSNR is over RGB, bits/texel after Zstd-19, and time is the total over the 24
images (mipple's encode loop; process wall time for Compressonator, which includes loading and writing):

| encoder | PSNR dB | SSIM | Zstd-19 bpt | time s |
| --- | ---: | ---: | ---: | ---: |
| mipple effort 0 | 37.074 | 0.97850 | 3.056 | 4.4 |
| mipple effort 1 | 37.640 | 0.98090 | 3.066 | 18.2 |
| mipple effort 2 | 37.660 | 0.98091 | 3.068 | 20.7 |
| mipple effort 3 | 37.697 | 0.98096 | 3.071 | 31.2 |
| mipple effort 5 | 37.728 | 0.98099 | 3.073 | 43.1 |
| mipple effort 7 | 37.756 | 0.98102 | 3.079 | 77.1 |
| mipple effort 9 | 37.776 | 0.98104 | 3.081 | 131 |
| Compressonator | 36.700 | 0.98103 | 3.013 | 33.6 |

These times predate the faster ETC2 encoder (byte-identical output; ROADMAP). A cloud A/B (Xeon 6973P-C, Kodak
1-12, one pinned thread) measured 1.327× less time at the default effort and 1.740× at λ 1; locally it is
1.6-2.0× less at efforts 1-9. The table is not re-run.

Effort 0 (ETC1 modes and planar only, two exact candidates per subblock) is 0.37 dB ahead of Compressonator
in an eighth of its time; effort 1 adds T and H and is 0.94 dB ahead in half its time. Compressonator's blocks
are 1.4% smaller after Zstd at λ = 0, which the RDO pass more than recovers (below).

ETC2 RGBA8 on the twelve RGBA images of the BC3 section (Kodak 1, 3, 5, 13, 18 and 23 with luma and radial
alpha), `--alpha data`, one thread, RGBA PSNR:

| encoder | PSNR dB | SSIM | Zstd-19 bpt | time s |
| --- | ---: | ---: | ---: | ---: |
| mipple effort 0 | 37.254 | 0.97968 | 5.898 | 3.3 |
| mipple effort 1 | 37.818 | 0.98184 | 5.905 | 12.8 |
| mipple effort 5 | 37.903 | 0.98195 | 5.909 | 26.0 |
| mipple effort 9 | 37.951 | 0.98201 | 5.916 | 73.4 |
| Compressonator | 36.793 | 0.98154 | 5.339 | 20.7 |

Here Compressonator's files are 10% smaller at λ = 0: mipple's alpha blocks are closer (radial alpha comes
out exact, against 50 dB) but pick their table and multiplier per block, which Zstd finds less repetitive.

RDO (effort 5, all threads, same images and scoring; bits/texel after Zstd-19 and Deflate-9):

| λ | ETC2 RGB PSNR | ETC2 RGB Zstd bpt | ETC2 RGB Deflate bpt | ETC2 RGBA PSNR | ETC2 RGBA Zstd bpt | ETC2 RGBA Deflate bpt |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 37.728 | 3.073 | 3.164 | 37.903 | 5.909 | 6.123 |
| 0.25 | 37.718 | 2.989 | 3.080 | 37.821 | 5.238 | 5.422 |
| 0.5 | 37.675 | 2.882 | 2.972 | 37.713 | 4.948 | 5.126 |
| 1 | 37.545 | 2.703 | 2.792 | 37.478 | 4.600 | 4.782 |
| 2 | 37.225 | 2.479 | 2.569 | 36.988 | 4.188 | 4.370 |
| 4 | 36.497 | 2.174 | 2.266 | 36.053 | 3.693 | 3.870 |
| Compressonator | 36.700 | 3.013 | 3.111 | 36.793 | 5.339 | 5.610 |

At Compressonator's PSNR, mipple's files are about 25% smaller for ETC2 RGB and 23% smaller for ETC2 RGBA after
Zstd, 25% and 24% after Deflate (interpolating between λ 2 and 4); at λ 0.5 ETC2 RGB is 4% smaller and 1 dB
closer. The RDO candidates per 8-byte half are whole-block and whole-half copies, another block's colors with
the selectors re-chosen, another block's selectors with the base colors and tables refit (individual and
differential blocks), and for EAC another block's base, multiplier and table re-indexed or its indices refit.
The RDO pass took about 1.7 s per Kodak image on all threads (a machine shared with other builds), several
times BC1's, when every ETC candidate was scored by a full decode. Re-chosen selectors and refit colors now
return their error and skip the decode, refits use per-selector moments, and EAC refits score unclamped
palettes in closed form (byte-identical output). Not re-run at full scale: on one pinned core of a busy
machine, by process CPU time on Kodak 1-6 (encode, scoring and compression together, best of 3, runs spread
up to 35%), effort 5 at λ 1 / 4 took 1.85x / 1.92x less for ETC2 RGB and 1.84x / 1.75x for RGBA8 before the
last two of those steps; ETC2 RGB's refit across tables then took 1.12x less at λ 1, and EAC R11 at λ 1 took
1.44x less in all.

EAC R11 and RG11 have no competitor on disk (Compressonator has no EAC-only formats). On Kodak, one thread,
R11 from the red channel and RG11 from red and green, PSNR over those channels:

| encoder | R11 PSNR | R11 bpt | R11 s | RG11 PSNR | RG11 bpt | RG11 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| mipple effort 0 | 44.412 | 3.694 | 2.2 | 44.372 | 7.114 | 4.0 |
| mipple effort 2 | 45.011 | 3.694 | 8.4 | 44.974 | 7.130 | 15.7 |
| mipple effort 5 | 45.106 | 3.698 | 13.2 | 45.066 | 7.138 | 25.1 |
| mipple effort 9 | 45.208 | 3.701 | 23.2 | 45.167 | 7.157 | 47.2 |

## ASTC vs ISPC and astcenc

ASTC LDR on all 24 Kodak images, top level only, on a throwaway GitHub Actions runner (AMD EPYC 9V74, 4
logical CPUs) with every encoder on one thread pinned to CPU 1: `gh workflow run bench.yml -f head=<full sha>
-f efforts=0,1,2,3,4,5,6,7 -f astcenc=fast,medium,thorough -f runs=3` (`.github/workflows/bench.yml`, which
runs `scripts/compete_astc.py`). The encoders are mipple 6727c53; Intel's ISPC Texture Compressor 79ddbc9 built
with ISPC 1.32.0 (`ispc_astc:<W>x<H>` in `bcn_harness`, its fast profile, the only ASTC profile it has for
opaque images); and Arm's astcenc 5.7.1 (the prebuilt AVX2 release, `-cl`, presets fast / medium / thorough).
All encode for linear decoding and are scored by `mipple score --astc-linear`: PSNR over RGB, luma SSIM, and
bits/texel after Zstd-19 and Deflate-9. Time is the sum over the images of each image's best of three runs:
mipple's encode loop, the harness's block loop, astcenc's own "Coding time" line (none include loading or
writing files). Runner CPUs differ from run to run, so times compare only within one run; this table is one
run:

| footprint | encoder | PSNR dB | SSIM | Zstd-19 bpt | Deflate-9 bpt | time s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 4x4 | ISPC fast | 43.790 | 0.99677 | 7.286 | 7.431 | 1.80 |
| 4x4 | astcenc fast | 44.580 | 0.99722 | 7.216 | 7.362 | 2.45 |
| 4x4 | astcenc medium | 45.223 | 0.99750 | 7.363 | 7.483 | 11.03 |
| 4x4 | astcenc thorough | 45.472 | 0.99759 | 7.453 | 7.560 | 29.12 |
| 4x4 | mipple effort 0 | 44.555 | 0.99746 | 7.091 | 7.260 | 1.63 |
| 4x4 | mipple effort 1 | 44.784 | 0.99753 | 7.196 | 7.348 | 2.04 |
| 4x4 | mipple effort 2 | 45.417 | 0.99778 | 7.400 | 7.521 | 8.27 |
| 4x4 | mipple effort 3 | 45.526 | 0.99781 | 7.426 | 7.542 | 12.68 |
| 4x4 | mipple effort 4 | 45.661 | 0.99784 | 7.436 | 7.549 | 23.00 |
| 4x4 | mipple effort 5 | 45.741 | 0.99786 | 7.463 | 7.570 | 38.47 |
| 4x4 | mipple effort 6 | 45.779 | 0.99787 | 7.470 | 7.575 | 49.37 |
| 4x4 | mipple effort 7 | 45.825 | 0.99789 | 7.488 | 7.589 | 82.53 |
| 6x6 | ISPC fast | 37.563 | 0.98365 | 3.332 | 3.368 | 1.37 |
| 6x6 | astcenc fast | 37.657 | 0.98337 | 3.205 | 3.262 | 2.00 |
| 6x6 | astcenc medium | 38.218 | 0.98561 | 3.298 | 3.340 | 6.87 |
| 6x6 | astcenc thorough | 38.451 | 0.98664 | 3.384 | 3.407 | 30.63 |
| 6x6 | mipple effort 0 | 37.659 | 0.98530 | 3.287 | 3.334 | 1.22 |
| 6x6 | mipple effort 1 | 38.019 | 0.98608 | 3.370 | 3.399 | 2.26 |
| 6x6 | mipple effort 2 | 38.324 | 0.98681 | 3.386 | 3.411 | 5.73 |
| 6x6 | mipple effort 3 | 38.397 | 0.98697 | 3.392 | 3.415 | 8.46 |
| 6x6 | mipple effort 4 | 38.455 | 0.98709 | 3.393 | 3.416 | 12.50 |
| 6x6 | mipple effort 5 | 38.561 | 0.98722 | 3.398 | 3.420 | 25.88 |
| 6x6 | mipple effort 6 | 38.592 | 0.98726 | 3.400 | 3.422 | 32.42 |
| 6x6 | mipple effort 7 | 38.628 | 0.98729 | 3.405 | 3.425 | 50.50 |
| 8x8 | ISPC fast | 33.650 | 0.95837 | 1.881 | 1.896 | 1.24 |
| 8x8 | astcenc fast | 33.682 | 0.95448 | 1.742 | 1.770 | 2.43 |
| 8x8 | astcenc medium | 34.404 | 0.96329 | 1.843 | 1.863 | 8.83 |
| 8x8 | astcenc thorough | 34.635 | 0.96589 | 1.917 | 1.926 | 32.87 |
| 8x8 | mipple effort 0 | 33.913 | 0.96247 | 1.879 | 1.897 | 1.12 |
| 8x8 | mipple effort 1 | 34.178 | 0.96369 | 1.912 | 1.923 | 1.96 |
| 8x8 | mipple effort 2 | 34.430 | 0.96520 | 1.919 | 1.928 | 4.88 |
| 8x8 | mipple effort 3 | 34.499 | 0.96564 | 1.922 | 1.931 | 7.11 |
| 8x8 | mipple effort 4 | 34.557 | 0.96600 | 1.923 | 1.931 | 10.48 |
| 8x8 | mipple effort 5 | 34.643 | 0.96635 | 1.925 | 1.933 | 21.28 |
| 8x8 | mipple effort 6 | 34.671 | 0.96651 | 1.926 | 1.934 | 26.27 |
| 8x8 | mipple effort 7 | 34.709 | 0.96666 | 1.928 | 1.935 | 39.12 |

Every astcenc preset is beaten in PSNR and SSIM, in less time, at every footprint:

- **fast:** effort 1 at 4x4 (+0.20 dB in 83% of its time; effort 0 was 0.025 dB behind in 67% with files 1.7%
  smaller, and is ahead since 168a9a3, see below), effort 0 at 6x6 (+0.002 dB, SSIM 0.98530 against 0.98337, in 61%) and at 8x8 (+0.23 dB in 46%).
  astcenc fast's files are smaller after Zstd at 6x6 and 8x8 (effort 0's are 2.6% / 7.9% larger), until a
  light RDO pass is added (below).
- **medium:** effort 2, +0.19 / +0.11 / +0.03 dB at 4x4 / 6x6 / 8x8 in 75% / 83% / 55% of its time; files
  0.5% / 2.7% / 4.1% larger after Zstd, again until light RDO is added.
- **thorough:** effort 3 at 4x4 (+0.05 dB in 44% of its time, files 0.4% smaller), effort 4 at 6x6 (+0.004
  dB in 41%, files 0.3% larger) and effort 5 at 8x8 (+0.008 dB in 65%, files 0.4% larger). Effort 7 leads it
  by 0.35 / 0.18 / 0.07 dB in 2.8× / 1.6× / 1.2× its time.

Against ISPC fast, effort 0 is 0.77 / 0.10 / 0.26 dB ahead at 4x4 / 6x6 / 8x8 with higher SSIM, in 91% / 89%
/ 90% of its time, with files 2.7% / 1.4% / 0.1% smaller after Zstd.

### Better on every metric at once: light RDO

The effort settings above win on PSNR, SSIM and time but give up file size at 6x6 and 8x8. A light RDO
pass that looks only one block back (`-l <λ> --max-back 1 --refine-back 1`: a whole copy of the previous
block, its head with the weights re-chosen, and its weights with the endpoints refitted) closes that gap.
With it, some setting beats each astcenc preset on all five columns at once (PSNR, SSIM, Zstd-19 and
Deflate-9 bits/texel, and time) at every footprint. Same setup as above, mipple 8598d56. Each comparison is
one cloud run with both encoders on the same runner, so the times compare within a row:

| footprint | astcenc preset | mipple setting | PSNR dB | SSIM | Zstd-19 bpt | Deflate-9 bpt | time s | runner |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4x4 | fast | effort 1 `-l 0` | 44.784 / 44.580 | 0.99753 / 0.99722 | 7.196 / 7.216 | 7.348 / 7.362 | 1.75 / 2.27 | Xeon 6975P-C |
| 4x4 | medium | effort 2 `-l 0.03` | 45.414 / 45.223 | 0.99777 / 0.99750 | 7.346 / 7.363 | 7.462 / 7.483 | 8.43 / 10.08 | Xeon 6975P-C |
| 4x4 | thorough | effort 3 `-l 0` | 45.526 / 45.472 | 0.99781 / 0.99759 | 7.426 / 7.453 | 7.542 / 7.560 | 15.31 / 36.19 | EPYC 7763 |
| 6x6 | fast | effort 0 `-l 0.3` | 37.664 / 37.657 | 0.98517 / 0.98337 | 3.198 / 3.205 | 3.240 / 3.262 | 2.37 / 2.47 | EPYC 7763 |
| 6x6 | medium | effort 2 `-l 0.35` | 38.297 / 38.218 | 0.98654 / 0.98561 | 3.265 / 3.298 | 3.287 / 3.340 | 7.92 / 8.58 | EPYC 7763 |
| 6x6 | thorough | effort 4 `-l 0.1` | 38.454 / 38.451 | 0.98709 / 0.98664 | 3.368 / 3.384 | 3.388 / 3.407 | 13.14 / 30.55 | EPYC 9V74 |
| 8x8 | fast | effort 0 `-l 1.5` | 33.875 / 33.682 | 0.96073 / 0.95448 | 1.708 / 1.742 | 1.723 / 1.770 | 1.30 / 1.73 | EPYC 9V45 |
| 8x8 | medium | effort 2 `-l 0.8` | 34.412 / 34.404 | 0.96468 / 0.96329 | 1.828 / 1.843 | 1.837 / 1.863 | 3.99 / 6.22 | EPYC 9V45 |
| 8x8 | thorough | effort 6 `-l 0.5` | 34.665 / 34.635 | 0.96635 / 0.96589 | 1.881 / 1.917 | 1.889 / 1.926 | 27.13 / 32.76 | EPYC 9V74 |

Each cell is mipple / astcenc; rows with a positive `-l` also pass `--max-back 1 --refine-back 1`, and `-l 0`
rows ran without RDO (these runs predate the default light pass below, so they passed no `-l`). In the same runs, with
more room on some columns:

- **4x4:** effort 2 `-l 0.06` beats medium by 0.17 dB with files 1.2% smaller (7.277 bpt, 8.46 s), and
  effort 3 `-l 0.03` beats thorough by 0.05 dB with files 1.1% smaller (45.522 dB, 7.373 bpt, 16.67 s).
- **6x6:** effort 5 `-l 0.3` beats thorough by 0.09 dB with files 2.4% smaller (38.540 dB, 3.302 bpt, 26.92 s
  against 30.55 s).
- **8x8:** effort 2 `-l 1` matches medium's PSNR within 0.003 dB (34.401) with files 2.3% smaller (1.801 bpt),
  and effort 3 `-l 1` beats it by 0.07 dB with files 2.1% smaller (34.469 dB, 1.805 bpt, 5.61 s against 6.22
  s). Effort 5 `-l 1` gives up 0.02 dB to thorough for files 5.5% smaller (34.611 dB, 1.812 bpt). In a
  second run (EPYC 9V74, astcenc thorough 32.76 s), effort 6 `-l 0.7` beats thorough by 0.02 dB with SSIM
  0.96613 and files 3.3% / 3.3% smaller after Zstd / Deflate (34.657 dB, 1.854 bpt) in 83% of its time.

The 6x6 fast win is narrow on time (96%). The widest is 4x4 thorough: 42% of the time, files 0.4% smaller,
+0.05 dB. In the 8x8 thorough row's run, effort 5 `-l 0.5` led thorough by only 0.001 dB
(34.636 dB, 22.14 s).

**The default.** Since bd55f3d `encode` runs the light pass for ASTC unless told otherwise: λ 0.03, 0.3
and 0.5 at 4x4, 6x6 and 8x8 with `--max-back 1 --refine-back 1` (`-l 0` turns it off; an explicit λ keeps
the 256-block search). Measured locally on the same 24 images at the default effort 5, against effort 5 with
no RDO:

| footprint | λ | PSNR dB | SSIM | Zstd-19 bpt | Deflate-9 bpt |
| --- | --- | --- | --- | --- | --- |
| 4x4 | 0.03 | 45.736 / 45.741 | 0.99786 / 0.99786 | 7.412 / 7.463 (−0.7%) | 7.513 / 7.570 (−0.8%) |
| 6x6 | 0.3 | 38.540 / 38.561 | 0.98704 / 0.98722 | 3.302 / 3.398 (−2.8%) | 3.321 / 3.420 (−2.9%) |
| 8x8 | 0.5 | 34.636 / 34.643 | 0.96619 / 0.96635 | 1.880 / 1.925 (−2.3%) | 1.887 / 1.933 (−2.4%) |

The pass itself adds about 2 / 1.1 / 0.9 s to the corpus (measured at effort 0, where the base encode
takes 1.6-2.3 s) against 28-70 s for effort 5 single-threaded. Doubling λ (0.06 / 0.5 / 1) costs 0.028 / 0.062 / 0.032 dB for files 1.6% / 5.4% / 5.9% smaller.
Against astcenc in one cloud run (6ef9866, Xeon 6975P-C, one thread; mipple / astcenc thorough):

| footprint | PSNR dB | SSIM | Zstd-19 bpt | Deflate-9 bpt | time s |
| --- | --- | --- | --- | --- | --- |
| 4x4 | 45.736 / 45.472 | 0.99786 / 0.99759 | 7.412 / 7.453 | 7.513 / 7.560 | 35.56 / 26.27 |
| 6x6 | 38.540 / 38.451 | 0.98704 / 0.98664 | 3.302 / 3.384 | 3.321 / 3.407 | 24.05 / 24.70 |
| 8x8 | 34.636 / 34.635 | 0.96619 / 0.96589 | 1.880 / 1.917 | 1.887 / 1.926 | 19.85 / 23.45 |

At 6x6 and 8x8 the default beats thorough on all five columns (at 8x8 by only 0.001 dB). At 4x4 it is
0.26 dB better with smaller files but takes 35% longer; effort 4 (45.657 dB, 0.99783, 7.384 / 7.490 bpt,
21.28 s) and effort 3 (45.522 dB, 0.99780, 7.373 / 7.483 bpt, 12.62 s) beat thorough on all five.

Against astcenc's slowest preset, `-exhaustive`, at 523a4ee (default light RDO; each row is one cloud run,
both encoders on the same runner; mipple / astcenc exhaustive):

| footprint | mipple setting | PSNR dB | SSIM | Zstd-19 bpt | Deflate-9 bpt | time s | runner |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 4x4 | effort 5 (the default) | 45.736 / 45.636 | 0.99786 / 0.99765 | 7.412 / 7.489 | 7.513 / 7.588 | 48.27 / 192.66 | EPYC 7763 |
| 6x6 | effort 6 | 38.570 / 38.549 | 0.98708 / 0.98684 | 3.304 / 3.397 | 3.323 / 3.417 | 40.10 / 197.08 | EPYC 7763 |
| 8x8 | effort 9 | 34.761 / 34.740 | 0.96680 / 0.96646 | 1.887 / 1.925 | 1.894 / 1.933 | 98.93 / 202.53 | EPYC 9V74 |

Each beats exhaustive on all five columns, in 25% / 20% / 49% of its time. The default effort 5 falls
short of it at 6x6 (38.540 dB) and 8x8 (34.636 dB); at 8x8 efforts 7 and 8 do too (34.702 / 34.721 dB,
38.45 / 46.56 s), and effort 9 with no RDO (`-l 0`) gives 34.768 dB with files the size of exhaustive's.

At the fast end, a re-run at cfb4cc1 (EPYC 7763, after the cheaper per-process setup) puts effort 0 at
6x6 at 97% of astcenc fast's time (2.40 / 2.47 s), with the same quality and sizes as the table above.
At 4x4 the light pass costs more than effort 0 and 1 save: by default they take 3.26 / 3.73 s against
astcenc fast's 3.04 s, so the 4x4 fast row stays at `-l 0` (1.94 s).

At 168a9a3 effort 0 searches harder up to 30 texels (4x4 to 6x5): a line-fit rerank of 16 partition patterns
and three polish passes. Re-run on one cloud EPYC 7763 (Kodak 24, one pinned thread, `bench.yml -f
footprints=4x4,5x4,5x5,6x5 -f efforts='0,0:-l 0,1:-l 0' -f astcenc=fast`). Each cell is PSNR dB / SSIM /
Zstd-19 bpt / Deflate-9 bpt / time s:

| footprint | astcenc fast | mipple effort 0 `-l 0` | mipple effort 0 (light RDO) |
| --- | --- | --- | --- |
| 4x4 | 44.580 / .99722 / 7.216 / 7.362 / 3.05 | 44.685 / .99748 / 7.094 / 7.263 / 2.06 | 44.688 / .99748 / 7.044 / 7.208 / 3.38 |
| 5x4 | 42.679 / .99577 / 5.853 / 5.974 / 2.31 | 42.741 / .99610 / 5.720 / 5.840 / 1.86 | 42.737 / .99607 / 5.649 / 5.762 / 2.99 |
| 5x5 | 40.697 / .99290 / 4.625 / 4.719 / 2.15 | 40.799 / .99347 / 4.645 / 4.728 / 1.83 | 40.794 / .99341 / 4.562 / 4.639 / 2.87 |
| 6x5 | 39.080 / .98912 / 3.825 / 3.902 / 2.21 | 39.224 / .99016 / 3.911 / 3.976 / 1.80 | 39.223 / .99008 / 3.837 / 3.898 / 2.77 |

- **4x4 and 5x4:** effort 0 `-l 0` beats fast on all five columns: +0.11 / +0.06 dB, files 1.7% / 2.3%
  smaller after Zstd, in 68% / 81% of its time (the previous effort 0 was 0.02 / 0.17 dB behind).
- **5x5 and 6x5:** `-l 0` is +0.10 / +0.14 dB with higher SSIM in 85% / 81% of the time, but its files are 0.4%
  / 2.2% larger after Zstd. The default light RDO makes them 1.4% smaller / 0.3% larger, at 1.3× fast's time:
  the light pass costs about 1 s here, more than effort 0 saves.

### The other footprints

Every other 2D LDR footprint at a419177, same setup (Kodak 24, one pinned thread, best of three, default
light RDO), against astcenc 5.7.1 fast / medium / thorough. Each group of rows is one cloud run: 5x4-6x5 on a
Xeon Platinum 8573C, 8x5-10x6 on an EPYC 9V74, 10x8-12x12 on an EPYC 9V45. Each cell is PSNR dB / SSIM /
Zstd-19 bpt / Deflate-9 bpt / time s:

| footprint | astcenc fast | astcenc medium | astcenc thorough | mipple effort 0 | mipple effort 5 |
| --- | --- | --- | --- | --- | --- |
| 5x4 | 42.679 / .99577 / 5.853 / 5.974 / 2.26 | 43.291 / .99614 / 5.967 / 6.058 / 10.52 | 43.497 / .99619 / 6.020 / 6.093 / 32.39 | 42.507 / .99594 / 5.634 / 5.751 / 2.46 | 43.696 / .99659 / 5.940 / 6.001 / 40.27 |
| 5x5 | 40.697 / .99290 / 4.625 / 4.719 / 2.07 | 41.320 / .99372 / 4.764 / 4.830 / 9.21 | 41.499 / .99379 / 4.827 / 4.879 / 32.81 | 40.640 / .99326 / 4.553 / 4.632 / 2.37 | 41.646 / .99420 / 4.745 / 4.786 / 36.60 |
| 6x5 | 39.080 / .98912 / 3.825 / 3.902 / 2.08 | 39.725 / .99059 / 3.953 / 4.007 / 8.06 | 39.915 / .99081 / 4.040 / 4.077 / 32.42 | 39.109 / .98992 / 3.831 / 3.892 / 2.30 | 40.030 / .99121 / 3.967 / 3.996 / 33.15 |
| 8x5 | 36.807 / .97961 / 2.772 / 2.826 / 2.00 | 37.440 / .98302 / 2.919 / 2.959 / 6.50 | 37.674 / .98389 / 3.049 / 3.069 / 30.29 | 36.900 / .98224 / 2.905 / 2.942 / 1.90 | 37.760 / .98434 / 2.996 / 3.012 / 25.64 |
| 8x6 | 35.525 / .96978 / 2.343 / 2.380 / 2.27 | 36.187 / .97616 / 2.456 / 2.484 / 7.23 | 36.420 / .97794 / 2.548 / 2.564 / 31.17 | 35.668 / .97554 / 2.434 / 2.460 / 1.84 | 36.472 / .97840 / 2.505 / 2.517 / 24.08 |
| 10x5 | 35.144 / .96202 / 2.162 / 2.200 / 2.53 | 35.863 / .97266 / 2.301 / 2.329 / 7.73 | 36.128 / .97600 / 2.457 / 2.470 / 32.15 | 35.364 / .97331 / 2.355 / 2.380 / 1.83 | 36.170 / .97647 / 2.422 / 2.433 / 24.04 |
| 10x6 | 34.021 / .95117 / 1.838 / 1.866 / 2.68 | 34.772 / .96451 / 1.942 / 1.961 / 8.33 | 35.032 / .96854 / 2.054 / 2.063 / 33.35 | 34.307 / .96512 / 1.968 / 1.985 / 1.76 | 35.043 / .96894 / 2.021 / 2.030 / 22.52 |
| 10x8 | 32.455 / .93851 / 1.399 / 1.421 / 1.63 | 33.175 / .95003 / 1.477 / 1.492 / 6.29 | 33.438 / .95402 / 1.543 / 1.550 / 24.53 | 32.778 / .94947 / 1.486 / 1.497 / 1.20 | 33.421 / .95432 / 1.523 / 1.529 / 14.11 |
| 10x10 | 31.339 / .91944 / 1.097 / 1.114 / 1.52 | 32.094 / .93418 / 1.172 / 1.183 / 6.51 | 32.361 / .93992 / 1.239 / 1.243 / 25.96 | 31.763 / .93425 / 1.197 / 1.203 / 1.12 | 32.328 / .93999 / 1.226 / 1.229 / 13.01 |
| 12x10 | 30.460 / .90286 / 0.907 / 0.920 / 1.35 | 31.264 / .91985 / 0.969 / 0.977 / 6.25 | 31.565 / .92737 / 1.030 / 1.032 / 26.47 | 30.983 / .92061 / 1.001 / 1.005 / 1.05 | 31.519 / .92719 / 1.025 / 1.026 / 12.07 |
| 12x12 | 29.581 / .88273 / 0.744 / 0.755 / 1.22 | 30.450 / .90310 / 0.797 / 0.804 / 6.13 | 30.796 / .91305 / 0.853 / 0.856 / 27.37 | 30.219 / .90504 / 0.835 / 0.837 / 1.02 | 30.742 / .91260 / 0.855 / 0.856 / 11.57 |

- **8x5 to 10x6:** the default effort 5 beats thorough on all five columns (+0.01 to +0.09 dB, files 1.4-1.7%
  smaller after Zstd, in 68-85% of its time).
- **5x4 to 6x5:** effort 5 is 0.12-0.20 dB ahead of thorough with files 1.3-1.8% smaller, but 2-24% slower.
- **10x8 to 12x12:** effort 5 is 0.02-0.05 dB behind thorough in 42-58% of its time, with SSIM ahead at
  10x8 and 10x10 and files smaller except at 12x12 (0.855 against 0.853 bpt).
- **Effort 0 against fast:** from 8x5 up it is 0.09-0.64 dB ahead with higher SSIM in 66-95% of the time,
  but its files are 4-12% larger: the default λ (8x8's 0.5 for every footprint past 8x8) is too light there.
  At 5x4 and 5x5 it has smaller files and higher SSIM but lower PSNR; at 6x5 it is level. At all three it is
  9-14% slower. (Since 168a9a3 effort 0 is ahead of fast in PSNR at all three; see the re-run above.)

At e570c02 the default λ keeps rising past 8x8 (2 / 3.5 / 4.5 / 6 at 10x8 / 10x10 / 12x10 / 12x12). Re-run on
one cloud EPYC 9V74 (same setup, against fast and thorough):

| footprint | astcenc fast | astcenc thorough | mipple effort 0 | mipple effort 5 |
| --- | --- | --- | --- | --- |
| 10x8 | 32.455 / .93851 / 1.399 / 1.421 / 2.35 | 33.438 / .95402 / 1.543 / 1.550 / 35.29 | 32.735 / .94765 / 1.374 / 1.384 / 1.67 | 33.367 / .95245 / 1.403 / 1.410 / 20.61 |
| 10x10 | 31.339 / .91944 / 1.097 / 1.114 / 2.21 | 32.361 / .93992 / 1.239 / 1.243 / 38.09 | 31.698 / .93081 / 1.053 / 1.059 / 1.60 | 32.250 / .93648 / 1.077 / 1.082 / 19.25 |
| 12x10 | 30.460 / .90286 / 0.907 / 0.920 / 1.97 | 31.565 / .92737 / 1.030 / 1.032 / 39.14 | 30.928 / .91719 / 0.881 / 0.884 / 1.51 | 31.453 / .92367 / 0.898 / 0.901 / 18.01 |
| 12x12 | 29.581 / .88273 / 0.744 / 0.755 / 1.80 | 30.796 / .91305 / 0.853 / 0.856 / 40.82 | 30.164 / .90136 / 0.724 / 0.727 / 1.47 | 30.675 / .90888 / 0.741 / 0.744 / 17.25 |

- **Effort 0 against fast:** now ahead on all five columns at all four footprints: +0.28 to +0.58 dB, higher
  SSIM, files 1.8-4.0% smaller after Zstd (2.6-4.9% after Deflate), in 71-82% of its time.
- **Effort 5 against thorough:** 0.07-0.12 dB behind with lower SSIM, but files 9-13% smaller after Zstd, in
  42-58% of its time.

At b7b5f20 effort 0 gets its own floor on the default λ from 8x5 to 10x8 (0.85 at 8x5, 1.5 at 8x6, 2 at 10x5,
3 at 10x6, 8x8 and 10x8; effort 1 and up keep the table above). Re-run on one cloud EPYC 9V45 (same setup)
against fast, with the previous default as the base row:

| footprint | astcenc fast | mipple effort 0 | effort 0 before (3ab7142) |
| --- | --- | --- | --- |
| 6x6 | 37.657 / .98337 / 3.205 / 3.262 / 1.48 | 37.664 / .98517 / 3.198 / 3.240 / 1.51 | (unchanged) |
| 8x5 | 36.807 / .97961 / 2.772 / 2.826 / 1.47 | 36.832 / .98109 / 2.766 / 2.799 / 1.48 | 36.900 / .98224 / 2.905 / 2.942 / 1.43 |
| 8x6 | 35.525 / .96978 / 2.343 / 2.380 / 1.68 | 35.547 / .97288 / 2.213 / 2.235 / 1.45 | 35.668 / .97554 / 2.434 / 2.460 / 1.40 |
| 10x5 | 35.144 / .96202 / 2.162 / 2.200 / 1.83 | 35.206 / .96939 / 2.096 / 2.116 / 1.43 | 35.364 / .97331 / 2.355 / 2.380 / 1.37 |
| 10x6 | 34.021 / .95117 / 1.838 / 1.866 / 1.93 | 34.118 / .95911 / 1.684 / 1.698 / 1.37 | 34.307 / .96512 / 1.968 / 1.985 / 1.31 |
| 8x8 | 33.682 / .95448 / 1.742 / 1.770 / 1.74 | 33.753 / .95655 / 1.562 / 1.575 / 1.35 | 33.926 / .96235 / 1.830 / 1.846 / 1.30 |
| 10x8 | 32.455 / .93851 / 1.399 / 1.421 / 1.72 | 32.689 / .94558 / 1.309 / 1.318 / 1.29 | 32.735 / .94765 / 1.374 / 1.384 / 1.28 |

- **8x6 to 10x8:** effort 0 is now ahead of fast on all five columns: +0.02 to +0.23 dB, higher SSIM, files
  3.1-10.3% smaller after Zstd (3.8-11.0% after Deflate), in 71-86% of its time. Before, its files were
  1.9-12.6% larger after Zstd.
- **8x5 and 6x6:** ahead on PSNR, SSIM and both sizes (Zstd 0.2% smaller), level on time (1.48 vs 1.47 s and
  1.51 vs 1.48 s here; 2.40 vs 2.48 s for 8x5 on an EPYC 7763).

At a10e278 (effort 0's λ 0.26 at 6x5; light-pass candidates skipped before they are written or polished; no
refitted candidate below 36 texels at effort 0) the light pass is cheap enough that effort 0 beats astcenc
fast on all five columns from 4x4 to 6x6. Two runs on cloud EPYC 7763s (same setup, with Intel ISPC fast): 4x4
and 6x6 at de44939 (44239094693, identical output at those footprints) and 5x4-6x5 at a10e278 (44241505134):

| footprint | ISPC fast | astcenc fast | mipple effort 0 |
| --- | --- | --- | --- |
| 4x4 | 43.790 / .99677 / 7.286 / 7.431 / 2.21 | 44.580 / .99722 / 7.216 / 7.362 / 3.06 | 44.685 / .99747 / 7.039 / 7.201 / 2.35 |
| 5x4 | 42.204 / .99521 / 5.884 / 5.984 / 2.11 | 42.679 / .99577 / 5.853 / 5.974 / 2.32 | 42.732 / .99605 / 5.636 / 5.748 / 2.12 |
| 5x5 | 40.532 / .99236 / 4.741 / 4.813 / 2.02 | 40.697 / .99290 / 4.625 / 4.719 / 2.15 | 40.788 / .99338 / 4.550 / 4.626 / 2.11 |
| 6x5 | 39.010 / .98873 / 3.976 / 4.028 / 1.82 | 39.080 / .98912 / 3.825 / 3.902 / 2.21 | 39.211 / .98999 / 3.812 / 3.871 / 2.08 |
| 6x6 | 37.563 / .98365 / 3.332 / 3.368 / 1.69 | 37.657 / .98337 / 3.205 / 3.262 / 2.48 | 37.660 / .98512 / 3.189 / 3.230 / 1.93 |

- **Against astcenc fast:** +0.003 to +0.13 dB, higher SSIM, files 0.3-3.7% smaller after Zstd (0.8-3.8% after
  Deflate), in 77-98% of its time.
- **Against ISPC fast:** +0.10 to +0.90 dB with files 3.4-4.3% smaller after Zstd (3.1-4.1% after Deflate),
  but 1.00-1.14× its time.

At e697b25 effort 0 at 6x5 takes the settings above 30 texels (16% less time with RDO off, 0.117 dB), and
effort 0 with RDO off (`-l 0`) beats ISPC fast on all five columns from 4x4 to 6x6, while the default effort
0 (light pass on) still beats astcenc fast on all five. Cloud bench 44248429239 (EPYC 9V74, same setup):

| footprint | ISPC fast | astcenc fast | mipple effort 0 | mipple effort 0 `-l 0` |
| --- | --- | --- | --- | --- |
| 4x4 | 43.790 / .99677 / 7.286 / 7.431 / 1.80 | 44.580 / .99722 / 7.216 / 7.362 / 2.45 | 44.685 / .99747 / 7.039 / 7.201 / 1.89 | 44.685 / .99748 / 7.094 / 7.263 / 1.53 |
| 5x4 | 42.204 / .99521 / 5.884 / 5.984 / 1.72 | 42.679 / .99577 / 5.853 / 5.974 / 1.87 | 42.732 / .99605 / 5.636 / 5.748 / 1.71 | 42.741 / .99610 / 5.720 / 5.840 / 1.38 |
| 5x5 | 40.532 / .99236 / 4.741 / 4.813 / 1.65 | 40.697 / .99290 / 4.625 / 4.719 / 1.75 | 40.788 / .99338 / 4.550 / 4.626 / 1.69 | 40.799 / .99347 / 4.645 / 4.728 / 1.37 |
| 6x5 | 39.010 / .98873 / 3.976 / 4.028 / 1.48 | 39.080 / .98912 / 3.825 / 3.902 / 1.80 | 39.097 / .98983 / 3.806 / 3.866 / 1.46 | 39.107 / .98999 / 3.905 / 3.972 / 1.14 |
| 6x6 | 37.563 / .98365 / 3.332 / 3.368 / 1.37 | 37.657 / .98337 / 3.205 / 3.262 / 2.01 | 37.660 / .98512 / 3.189 / 3.230 / 1.53 | 37.659 / .98530 / 3.287 / 3.334 / 1.14 |

- **`-l 0` against ISPC fast:** +0.10 to +0.90 dB, higher SSIM, files 1.4-2.8% smaller after Zstd (1.0-2.4%
  after Deflate), in 77-85% of its time.
- **Effort 0 against astcenc fast:** +0.003 to +0.105 dB (6x5 now +0.017), higher SSIM, files 0.5-3.7%
  smaller after Zstd (0.9-3.8% after Deflate), in 76-97% of its time. Against ISPC fast it is 0.09-0.90 dB
  ahead with files 3.4-4.3% smaller after Zstd, in 99-112% of its time.
- Timing margins depend on the CPU. On a Zen 5 EPYC 9V45 at 2ab9c93 (cloud bench 44243279811, before this
  change) ISPC fast took 0.97 s at 6x5 against `-l 0`'s 1.01 s, and effort 0 at 5x5 was level with astcenc
  fast (1.27 / 1.26 s); 5x5 is unchanged by e697b25.

Some setting beats astcenc thorough on all five columns at every footprint but 12x12 at ee1dd5a (same setup,
one run each). At 5x4 and 5x5 that is effort 4 with the default λ (cloud bench 44259632980, EPYC 7763). At
10x8 to 12x10 it is effort 6 or 7 with the light pass at λ 0.5 (`-l 0.5 --max-back 1 --refine-back 1`,
cloud bench 44264020722, EPYC 7763). Each cell is mipple / thorough:

| footprint | mipple setting | PSNR dB | SSIM | Zstd-19 bpt | Deflate-9 bpt | time s |
| --- | --- | --- | --- | --- | --- | --- |
| 5x4 | effort 4 | 43.520 / 43.497 | .99654 / .99619 | 5.907 / 6.020 | 5.969 / 6.093 | 19.58 / 34.95 |
| 5x5 | effort 4 | 41.505 / 41.499 | .99413 / .99379 | 4.722 / 4.827 | 4.763 / 4.879 | 18.16 / 36.61 |
| 10x8 | effort 6 light λ 0.5 | 33.449 / 33.438 | .95454 / .95402 | 1.525 / 1.543 | 1.530 / 1.550 | 31.39 / 44.80 |
| 10x10 | effort 7 light λ 0.5 | 32.386 / 32.361 | .94049 / .93992 | 1.228 / 1.239 | 1.231 / 1.243 | 43.07 / 48.41 |
| 12x10 | effort 7 light λ 0.5 | 31.579 / 31.565 | .92785 / .92737 | 1.027 / 1.030 | 1.028 / 1.032 | 40.27 / 49.88 |

- **5x4 and 5x5:** effort 4 takes 50-56% of thorough's time, with files 1.9-2.2% smaller after Zstd. Effort 3
  was 0.03-0.05 dB short of thorough, in 34-38% of its time.
- **10x8 to 12x10:** effort 7 takes 81-89% of thorough's time and effort 6 takes 70%. Effort 6 at λ 0.5 was
  0.007-0.019 dB short at 10x10 and 12x10. The default λ there (2 / 3.5 / 4.5) costs effort 5 0.06-0.08 dB.
  The full pass at a low λ is short too: effort 5 `-l 0.5` was 0.011-0.047 dB short in 72-96% of the time (EPYC
  9V45, 44259644556). Even `-l 0` was 0.015-0.055 dB short, so the gap there is the base encode.
- **12x12:** effort 7 with the light pass at λ 1 beats thorough on PSNR, SSIM and both sizes (30.800 /
  30.796 dB, .91332 / .91305, Zstd 0.843 / 0.853 bpt) but took 110% of its time on a Xeon 6975P (28.41 /
  25.75 s, 44266863592). On the EPYC 7763 run above, effort 7 at λ 0.5 took 74% of thorough's time (38.37 /
  51.79 s), with Zstd files 0.5% larger (0.857 / 0.853 bpt). How much time thorough takes, compared with
  mipple, depends on the CPU.

Scoring astcenc's files exposed a decoder bug, fixed: a 2- or 3-partition block may spell equal CEMs the long
way (with the extra CEM bits below the weights), and the decoder read those blocks with the short form's
endpoint range. mipple's own files never use the long form, so earlier mipple scores are unchanged.

RDO at effort 0 on the same images, run locally on one thread pinned to one P-core of the Core Ultra 7 155H
(logical CPU 4), `mipple bench test-images/kodak -f astc-4x4,astc-6x6,astc-8x8 -l 0,0.5,1,2,4,8 -e 0` with
`RAYON_NUM_THREADS=1`. Each cell is PSNR dB / SSIM / Zstd-19 bpt; the RDO pass's time is its total over the 24
images. The λ 0 rows are the cloud table's effort-0 encodes:

| λ | 4x4 | 6x6 | 8x8 |
| --- | --- | --- | --- |
| 0 | 44.56 / 0.99746 / 7.091 | 37.66 / 0.98530 / 3.287 | 33.91 / 0.96247 / 1.879 |
| 0.5 | 43.07 / 0.99357 / 5.121 (65 s) | 37.67 / 0.98444 / 2.940 (40 s) | 33.99 / 0.96256 / 1.755 (34 s) |
| 1 | 41.56 / 0.98669 / 4.262 (60 s) | 37.48 / 0.98185 / 2.667 (41 s) | 33.96 / 0.96147 / 1.656 (35 s) |
| 2 | 39.76 / 0.97634 / 3.566 (59 s) | 37.02 / 0.97464 / 2.276 (38 s) | 33.86 / 0.95828 / 1.497 (33 s) |
| 4 | 37.57 / 0.95926 / 2.952 (58 s) | 36.30 / 0.96467 / 1.954 (37 s) | 33.65 / 0.95195 / 1.323 (31 s) |
| 8 | 34.87 / 0.92572 / 2.276 (55 s) | 35.09 / 0.94662 / 1.638 (36 s) | 33.24 / 0.94127 / 1.159 (31 s) |
| ISPC fast | 43.79 / 0.99677 / 7.286 | 37.56 / 0.98365 / 3.332 | 33.65 / 0.95837 / 1.881 |

At ISPC fast's PSNR (interpolated in log rate along each λ curve's Pareto front), mipple's files are 17.7% /
16.6% / 29.8% smaller at 4x4 / 6x6 / 8x8 after Zstd-19, and 17.1% / 16.1% / 29.1% smaller after Deflate-9
(ISPC fast: 7.431 / 3.368 / 1.896 bpt; mipple λ 0: 7.260 / 3.334 / 1.897; λ 1: 4.429 / 2.715 / 1.676). At 6x6
and 8x8, light RDO raises PSNR (0.01 / 0.08 dB at λ 0.5): a polished re-weight or refit of an earlier block is
sometimes closer to the source than effort 0's own encode of the block. The candidates are whole copies of the
previous 256 blocks, an earlier block's head (mode, partition, endpoints) with the weights re-chosen, and an
earlier block's weights with the endpoints refitted, each polished once. Exact error lower bounds skip most of
them without changing the output: from each channel's summed difference for a whole copy, from the texels'
mean distance to each endpoint segment for a re-weight, and from the per-pair least-squares residual for a
refit, whose weight sums are cached with the earlier block. The pass runs on whole surfaces (blocks are not
4x4) and costs 0.8-4.1 s per Kodak image on one thread, 13-46× the effort-0 encode; it parallelizes across 16
KiB chunks like the other formats' passes. The cached refit sums and the re-weight and copy bounds made it
1.22-1.43× / 1.49-1.75× / 1.70-1.89× faster at 4x4 / 6x6 / 8x8 over the λ range, output byte-identical (6727c53
on the same core: 67-93 s / 54-71 s / 52-64 s).

## Normal-map RDO (`--role normal`)

BC5 and EAC RG11 store a normal's X and Y; the shader rebuilds Z = sqrt(1 - X² - Y²). An XY error
moves the rebuilt unit normal by |Δn|² ≈ ΔX² + ΔY² + (XΔX + YΔY)²/Z², so the same XY error costs more
on tilted texels. With `--role normal` (or `--normal`) the RDO scales each block's error by
1 + w·mean((X² + Y²)/2Z²) (Z² floored at 1/16; `--normal-weight w`, default 4). `bench` and `encode`
report **lit PSNR** for normal maps: Lambert shading n·l averaged over every light direction has
mean squared error |Δn|²/3, so lit PSNR is -10·log10(mean |Δn|²/3), a renderer's view of the error
rather than the stored channels'.

32 Poly Haven normal maps (`test-images/polyhaven-test/*_nor*.png`, 1024², CC0), `mipple bench`
default effort, λ 0 to 16; Zstd-19 bits per texel, lit PSNR and RG PSNR on pooled MSE, mean angular
error:

| Format | λ | w = 0: bpt | lit dB | RG dB | ° | w = 4: bpt | lit dB | RG dB | ° |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| BC5 | 1 | 5.518 | 35.62 | 40.53 | 1.30 | 5.601 | 35.86 | 40.75 | 1.26 |
| BC5 | 4 | 3.928 | 30.97 | 35.63 | 2.23 | 4.127 | 31.78 | 36.35 | 2.07 |
| BC5 | 16 | 1.708 | 24.03 | 28.69 | 4.70 | 2.112 | 25.49 | 29.96 | 4.03 |
| EAC RG11 | 1 | 5.516 | 35.86 | 40.80 | 1.26 | 5.598 | 36.13 | 41.04 | 1.23 |
| EAC RG11 | 4 | 3.975 | 31.16 | 35.84 | 2.19 | 4.165 | 31.98 | 36.56 | 2.03 |
| EAC RG11 | 16 | 1.754 | 24.14 | 28.80 | 4.72 | 2.142 | 25.53 | 30.00 | 4.06 |

At a given λ the weighted RDO spends more bits on tilted blocks and keeps more quality; on the
rate-quality curve (BD-rate against w = 0, λ 0 to 16) it is better on all three metrics:

| Weight | BC5 lit | BC5 RG PSNR | BC5 angle | EAC RG11 lit | EAC RG11 RG PSNR | EAC RG11 angle |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | -0.84% | -0.20% | -0.35% | | | |
| 2 | -1.07% | -0.22% | -0.48% | -1.06% | -0.21% | -0.47% |
| **4** (default) | **-1.16%** | **-0.14%** | **-0.57%** | **-1.17%** | **-0.15%** | **-0.58%** |
| 8 | -1.07% | +0.06% | -0.58% | | | |

λ 0 (no RDO) is unchanged. The weight only applies to BC5 and EAC RG11 (signed or not) with
`--role normal`; other formats store Z or a swizzle and keep plain RGB error.

## Lossless field split (`--prep`)

`src/prep.rs` applies a BC7Prep-style lossless transform before Zstd: blocks are split into byte streams of
like fields (BC1: endpoint low/high bytes and selectors; BC4/BC5: endpoints and indices; BC7: mode, shape,
endpoints with p-bits folded in, shared p-bits, bit-packed indices and an escape stream for invalid blocks).
`mipple encode --zstd N --prep on|auto` stores it in KTX2 with a `mipple.prep` key/value entry (one flag per
level) and `ktx2::read` undoes it; `auto` keeps whichever layout compresses smaller per level. Loaders that
do not know the key must not read prepped files. Zstd-19 size change on Kodak 1-6 (`cargo test --release
prep::tests::sizes -- --ignored --nocapture`):

| Format | λ 0 | λ 1 | λ 4 |
| --- | --- | --- | --- |
| BC1 | −4.95% | −3.18% | +2.12% |
| BC4 | −3.88% | −1.08% | +3.23% |
| BC5 | −5.03% | −1.75% | +2.95% |
| BC7 | −7.66% | −5.21% | −1.09% |

At high λ, RDO has optimized whole-block repeats in the interleaved layout and splitting them loses; an RDO
rate model for the split layout is on the roadmap.

## v0.9: entropy-reduced BC7 base encoding

v0.9 ports bc7enc's `-e` ("reduced entropy") into the BC7 base encoder, behind `--entropy off|on|auto`
(default **auto** = on for λ ≥ 1). It was measured with `mipple check`; the competitor tables above include it.

What it does, per block:

- **Mode-6 preference:** another mode must have less than 0.4× mode 6's error (0.6× with alpha) to win.
  Mode 6 is one subset, 4-bit indices and a fixed layout, so neighbouring blocks share more bytes.
- **Mode-6 endpoint quantization:** each 7-bit endpoint is limited to 64 evenly spaced levels with fixed
  p-bits ((1, 1) for opaque blocks so alpha stays exactly 255; (0, 1) with alpha, as bc7enc does).
- **RDO keeps it:** when RDO refits endpoints onto a neighbour's indices, it uses the same restricted
  quantization (worth a further −0.4% BD-rate on Kodak).

Which parts help, as BD-rate vs plain encoding (Zstd-19, λ 0–4, `--entropy on`; negative = smaller at equal PSNR):

| Variant | Kodak first 4 | Synthetic 256² |
| --- | --- | --- |
| bc7enc `-e` as published (weights 0.4/0.6, mode-6 quant, p-bit-1 weight 1.3) | −3.35% | −3.09% |
| p-bit-1 weight 1.3 only | +0.53% | −0.19% |
| mode-6 quantization only | +5.00% | +1.30% |
| mode weights 0.4/0.6 only | −3.25% | −1.67% |
| weights 0.6/0.8 + quant + p-bit | −1.08% | −0.24% |
| weights 0.8/0.9 + quant + p-bit | +2.19% | +0.76% |
| weights 0.25/0.4 + quant + p-bit | −3.34% | −4.44% |
| **weights 0.4/0.6 + quant, RDO keeps the quantization (shipped)** | **−3.70%** | **−4.88%** |

The mode weights do most of the work. The quantization only pays when the mode weights put enough blocks in
mode 6, and when RDO keeps it. The p-bit weighting made no difference once mode 6 was quantized, so it is not
included.

**When it pays.** Dense λ sweep (0, 0.25, 0.5, 1, 2, 4, 8), pooled PSNR vs Zstd-19 bytes:

| λ | Kodak-4 off | Kodak-4 on | Synthetic off | Synthetic on |
| --- | --- | --- | --- | --- |
| 0 | 45.78 dB, 1,418,246 B | 44.12 dB, 1,237,481 B | 45.13 dB, 266,899 B | 43.23 dB, 243,328 B |
| 0.5 | 44.74 dB, 1,235,712 B | 43.50 dB, 1,099,729 B | 44.65 dB, 228,517 B | 42.95 dB, 214,088 B |
| 1 | 43.36 dB, 1,086,535 B | 42.39 dB, 966,324 B | 44.16 dB, 221,299 B | 42.51 dB, 206,230 B |
| 2 | 41.61 dB, 943,867 B | 40.75 dB, 821,604 B | 42.87 dB, 209,834 B | 41.37 dB, 190,525 B |
| 4 | 40.21 dB, 859,850 B | 39.09 dB, 714,449 B | 40.63 dB, 193,037 B | 39.21 dB, 164,876 B |
| 8 | 39.33 dB, 830,667 B | 37.95 dB, 666,483 B | 38.15 dB, 167,913 B | 37.17 dB, 139,298 B |

Over the whole sweep, entropy reduction is −5.1% (Kodak-4) and −5.3% (synthetic) BD-rate. The curves
cross, though. Above about 43.5 dB (Kodak) or 42.5 dB (synthetic), plain encoding plus light RDO reaches the
same PSNR in fewer bytes. For example, plain λ = 0.5 gives 44.74 dB in 1.236 MB, while reduced λ = 0 gives
44.12 dB in 1.237 MB. Below the crossover, entropy reduction wins by 4–15%. Hence `auto` switches it on
from λ = 1:

- λ < 1 output is byte-identical to v0.2.
- On the check corpora, `auto` is −3.2% (Kodak-4) and −3.3% (synthetic) BD-rate vs v0.2, with λ 0–4.
  BD-rate understates it a little, because the cubic fit has to bend through the switch point.

**λ means something different from v0.2 at λ ≥ 1.** On Kodak-4, λ = 1 is now 42.4 dB (it was 43.4 dB) at
11% fewer bytes. Use `--entropy off` for v0.2 behaviour.

## Regression check (`mipple check`)

`mipple check` is the fast guard to run instead of re-running this page. It encodes the six synthetic images at
256² in all four formats at λ 0, 0.5, 1, 2 and 4 (about 27 s on the 2-core dev VM; Kodak first 4 for BC7: about
36 s). It compares the result with `bench-results/check-baseline.csv` (or `check-baseline-kodak.csv`) and
fails on:

- BD-rate > +1%;
- λ = 0 pooled PSNR −0.1 dB, or λ = 0 size +1%;
- any single image −0.5 dB at λ = 0.

How the check judged this release's changes:

| Change | Check result |
| --- | --- |
| `BlockOpts` refactor (effort + flag threaded through every encoder) | PASS, 120/120 encodes byte-identical |
| `--entropy on` | FAIL: λ = 0 PSNR −1.9 dB (gate working as intended: base quality traded for size), BD-rate −3.1% |
| `--entropy auto` (shipped default) | PASS: BC1/BC4/BC5 identical, BC7 BD-rate −3.28% (synthetic) / −3.23% (Kodak-4), λ < 1 identical |

Both baselines were then regenerated with `--update-baseline`.

Later changes:

| Change | Check result |
| --- | --- |
| BC1 batch search for every effort (default effort 5 moves to it) | PASS: BC1 λ = 0 +0.10 dB, BD-rate −0.55%; baseline updated |
| BC7 fast encoder for efforts 0-6 (default effort 5 moves to it) | FAIL on one gate: BC7 λ = 0 Zstd size +1.86% at equal PSNR (45.93 dB), while BD-rate is −0.50%, λ = 0.5 is 2.8% smaller and the check runs 2.3× faster. Accepted (the new default picks mode 1 more often); baseline updated |
| RDO wide search by default (256 back-references, spans from the nearest 8) with the faster candidate loop | PASS: BD-rate BC1 −3.21%, BC4 −5.25%, BC5 −2.83%, BC7 −3.70% (synthetic; Kodak-4: −9.25%, −5.50%, −4.21%, −4.74%), λ = 0 identical, total time 0.82× (Kodak-4: 0.89×); both baselines updated |
| Field-swap refinement (`--refine-back`, 256 for BC1/BC4, off for BC5/BC7) | PASS: BD-rate BC1 −0.56%, BC4 −0.25% (synthetic; Kodak-24: −0.98%, −0.39%), BC5/BC7 identical; both baselines updated (they predated `--min-rms` and the BC7 window change) |
| Literal costs frozen per chunk from the base encoding (BC4/BC5/BC7) | PASS: BD-rate BC4 −0.87%, BC5 +0.15%, BC7 +0.13%, BC1 identical (synthetic; Kodak-24: BC4 −1.24%, BC5 −0.44%; Kodak-4 BC7 −0.06%); both baselines updated |
| Far endpoints for BC1 (`--far-back` 1024, BC1 window 8 KiB) | PASS: BD-rate BC1 −0.72%, BC4/BC5/BC7 identical, total time 0.94× (synthetic; Kodak-24: BC1 −0.55%, 1.02× total for BC1/BC4/BC5); synthetic baseline updated |
| Exact candidate errors, half memo, deferred re-index, bounded LZ cost | PASS: all identical (Kodak-24 BC1/BC4/BC5 360/360), Kodak-24 time BC5 0.80×, BC4 0.94× |
| BC5 window 3072 (192-deep search) | PASS: BD-rate BC5 −0.63%, others identical, total time 1.15× (synthetic; Kodak-24: BC5 −2.81%); synthetic baseline updated |
| BC7 re-index bound before palettes, no index unpack, cached subset means | PASS: all identical, Kodak-8 BC7 time 0.94× |
| BC7 far search (`--far-back` 1024, BC7 window 16 KiB) | PASS: BD-rate BC7 −1.62%, others identical, total time 1.33× (synthetic; Kodak-8: BC7 −1.46%, 1.18× vs before the bound changes); synthetic baseline updated |
| Localized BC5 search (`--local` 16: nearest 128 + 16 network-ranked + row above) | PASS: BD-rate BC5 −1.05%, others identical, total time 1.08× (synthetic; Kodak-24: BC5 −0.76%, 1.02×); synthetic baseline updated |
| Field-swap refinement for BC7 (`--refine-back` 256, mode-6 halves scored without a decode) | PASS: BD-rate BC7 −1.13% Zstd / −0.87% Deflate, BC1/BC4/BC5 identical, total time 1.07× (synthetic; Kodak-24 −0.12%); both baselines updated |
| Smooth weight `1 + 3 · 12² / (12² + σ²)` (was 5× ramping linearly to 1 at σ 14) | PASS: BD-rate Zstd BC1 −1.15%, BC4 −2.34%, BC5 −2.79%, BC7 −1.43% (synthetic), BC7 −3.82% (Kodak-4); same speed head-to-head on Kodak; both baselines updated |
| BC4/BC5 RDO from an effort-8 base (`base_block_opts`) | PASS: BD-rate Zstd BC4 −0.48%, BC5 −0.49%, BC1/BC7 identical (synthetic; Kodak-24: BC4 −0.95%, BC5 −1.20%), total time 0.98× (Kodak-24 1.00×); synthetic baseline updated |
| Deferred decision (`--defer` 0.3), neighbourhood smooth weight (`--smooth-window-std` 16), BC1 lookahead, BC7 far-search gate | PASS: BD-rate Zstd BC1 −1.67%, BC3 −1.41%, BC4 −0.18%, BC5 −1.00%, BC6H −1.00%, BC7 −1.10%, ETC2 RGB / RGBA −1.39% / −1.43%, EAC R11 / RG11 −0.21% / −0.94% (synthetic; Kodak-4 BC7 −1.47%), λ = 0 and ASTC identical, total time 1.01-1.03× (loaded machine); both baselines updated |
| BC4/BC5 far endpoints (`--far-back` 1024 past the window, exact errors, 8 lowest LZ-costed, gate 0.7) | PASS: BD-rate Zstd BC4 −0.15%, BC5 −0.14% (Deflate −0.18% / −0.13%; synthetic; Kodak-24: BC4 −0.20%, BC5 −0.56%), λ = 0 and other formats identical; synthetic baseline BC4/BC5 rows updated |

## How to reproduce

```sh
cargo build --release
./target/release/mipple check                                         # regression check, ~30 s
./target/release/mipple check --corpus test-images/kodak --limit 4 -f bc7
./target/release/mipple check -f bc7 --entropy off                    # v0.9 vs v0.2 behaviour, same baseline
scripts/fetch-kodak.sh
./target/release/mipple bench test-images/kodak -f bc1,bc7 -l 0,0.5,1,2,4
python3 scripts/compete.py --corpus test-images/kodak --out bench-results/compete-kodak \
    --mipple target/release/mipple --bc7enc /path/to/bc7enc_rdo/build/bc7enc \
    --harness /path/to/bcn_harness       # add --threads 1 --base-only --affinity MASK for per-core tables
python3 scripts/compete_astc.py test-images/kodak --harness /path/to/bcn_harness --runs 3 --efforts 0,2
```

The Basis rows (`bench-results/compete-kodak24-basis/`) come from basisu built from basis_universal 073eb6a
(`cmake -S . -B build && cmake --build build --config Release`; the exe lands in `bin/`). They were run as 5
shards (`--shard i/5`, each pinned with `--affinity` to its own P-core, rows concatenated), then the summary
was rebuilt with `--summary-only --threads 1 --cpu-time`:

```sh
python3 scripts/compete.py --corpus test-images/kodak --out bench-results/compete-kodak24-basis --formats bc7 \
    --threads 1 --cpu-time --interleave --repeat 2 --affinity 0x4 --only "mipple e5 λ,basis" \
    --mipple target/release/mipple --basisu /path/to/basis_universal/bin/basisu \
    --mipple-lambdas 0,0.25,0.5,1,2,4,8,16 --basisu-xubc7-args "-xubc7_num_stripes 1"
```

Building bc7enc_rdo with recent GCC needs `-DCMAKE_CXX_FLAGS="-include cstdint"`; add `-DSUPPORT_BC7E=TRUE`
with ISPC on your PATH for its full-strength base encoder.
