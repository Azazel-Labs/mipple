# Competitive benchmark

Corpus: 24 images from `test-images\kodak`; threads per encoder: 1. Sizes are corpus totals after each back end; PSNR is the per-image mean over the format's channels; time is total CPU seconds of the encoder processes (user + kernel, with --cpu-time).


## BC7

| Encoder config | PSNR (dB) | Zstd-19 bpt | Deflate-9 bpt | Time (s) | Mpix/s | Zstd size vs mipple RDO curve at equal PSNR |
| --- | --- | --- | --- | --- | --- | --- |
| mipple e5 λ=0 | 45.37 | 7.175 | 7.276 | 6.84 | 1.38 | +0.0% |
| mipple e5 λ=0.25 | 45.38 | 6.177 | 6.294 | 116.09 | 0.08 | -0.0% |
| mipple e5 λ=0.5 | 44.28 | 5.299 | 5.448 | 118.20 | 0.08 | +0.0% |
| mipple e5 λ=1 | 42.06 | 4.086 | 4.270 | 60.05 | 0.16 | +0.0% |
| mipple e5 λ=2 | 40.18 | 3.308 | 3.493 | 54.98 | 0.17 | +0.0% |
| mipple e5 λ=4 | 38.03 | 2.671 | 2.852 | 50.78 | 0.19 | +0.0% |
| mipple e5 λ=8 | 35.62 | 2.097 | 2.260 | 49.25 | 0.19 | +0.0% |
| mipple e5 λ=16 | 32.97 | 1.534 | 1.663 | 42.94 | 0.22 | +0.0% |
| basis UASTC λ=0 | 43.22 | 7.418 | 7.496 | 118.27 | 0.08 | +58.6% |
| basis UASTC λ=0 as BC7 | 43.22 | 6.889 | 7.034 | 118.27 | 0.08 | +47.2% |
| basis UASTC λ=0.5 | 42.32 | 6.814 | 6.882 | 321.00 | 0.03 | +61.8% |
| basis UASTC λ=0.5 as BC7 | 42.32 | 6.342 | 6.479 | 321.00 | 0.03 | +50.6% |
| basis UASTC λ=1 | 41.01 | 6.355 | 6.419 | 313.78 | 0.03 | +75.1% |
| basis UASTC λ=1 as BC7 | 41.01 | 5.900 | 6.031 | 313.78 | 0.03 | +62.6% |
| basis UASTC λ=2 | 38.64 | 5.909 | 5.972 | 302.19 | 0.03 | +108.3% |
| basis UASTC λ=2 as BC7 | 38.64 | 5.447 | 5.575 | 302.19 | 0.03 | +92.0% |
| basis UASTC λ=3 | 36.69 | 5.615 | 5.678 | 297.98 | 0.03 | +140.5% |
| basis UASTC λ=3 as BC7 | 36.69 | 5.143 | 5.270 | 297.98 | 0.03 | +120.3% |
| basis UASTC λ=5 | 34.15 | 5.198 | 5.267 | 288.00 | 0.03 | +194.9% |
| basis UASTC λ=5 as BC7 | 34.15 | 4.698 | 4.825 | 288.00 | 0.03 | +166.5% |
| basis UASTC λ=8 | 32.18 | 4.826 | 4.906 | 273.31 | 0.03 | — |
| basis UASTC λ=8 as BC7 | 32.18 | 4.290 | 4.424 | 273.31 | 0.03 | — |
| basis XUBC7 bc7f q100 rdo0 | 44.25 | 5.533 | 5.533 | 27.80 | 0.34 | +4.8% |
| basis XUBC7 bc7f q100 rdo25 | 43.48 | 5.070 | 5.070 | 51.64 | 0.18 | +5.1% |
| basis XUBC7 bc7f q100 rdo50 | 41.92 | 4.571 | 4.571 | 52.05 | 0.18 | +13.7% |
| basis XUBC7 bc7f q100 rdo75 | 40.63 | 4.147 | 4.147 | 51.69 | 0.18 | +19.1% |
| basis XUBC7 bc7f q100 rdo100 | 39.67 | 3.789 | 3.789 | 51.55 | 0.18 | +20.5% |
| basis XUBC7 bc7e2 q100 rdo0 | 45.32 | 5.597 | 5.597 | 54.45 | 0.17 | -20.9% |
| basis XUBC7 bc7e2 q100 rdo25 | 44.39 | 5.312 | 5.312 | 74.78 | 0.13 | -2.8% |
| basis XUBC7 bc7e2 q100 rdo50 | 42.35 | 4.757 | 4.757 | 78.00 | 0.12 | +12.5% |
| basis XUBC7 bc7e2 q100 rdo75 | 40.82 | 4.238 | 4.238 | 81.88 | 0.12 | +19.3% |
| basis XUBC7 bc7e2 q100 rdo100 | 39.76 | 3.833 | 3.833 | 79.11 | 0.12 | +20.8% |
| basis XUBC7 bc7f q75 rdo0 | 36.37 | 3.901 | 3.901 | 55.06 | 0.17 | +72.5% |
| basis XUBC7 bc7f q75 rdo25 | 36.07 | 3.362 | 3.362 | 77.61 | 0.12 | +53.3% |
| basis XUBC7 bc7f q75 rdo50 | 35.58 | 2.894 | 2.894 | 77.98 | 0.12 | +38.7% |

BD-rate of **mipple RDO** vs each RDO family (negative = mipple needs fewer bits at equal PSNR, or at equal SSIM in the last column; for Basis rows the bits are what ships, see scripts/compete.py), and each family's total time range over its sweep (mipple RDO: 6.8-118.2 s):

| vs family | Zstd-19 | Deflate-9 | Zstd-19, equal SSIM | family time (s) |
| --- | --- | --- | --- | --- |
| basis UASTC RDO | -54.3% | -51.9% | -45.0% | 118.3-321.0 |
| basis UASTC RDO as BC7 | -50.2% | -48.3% | -40.3% | 118.3-321.0 |
| basis XUBC7 bc7f q100 | -11.3% | -7.3% | +12.6% | 27.8-52.0 |
| basis XUBC7 bc7e2 q100 | -8.3% | -4.6% | +12.9% | 54.5-81.9 |
| basis XUBC7 bc7f q75 | -34.3% | -29.3% | -29.8% | 55.1-78.0 |
