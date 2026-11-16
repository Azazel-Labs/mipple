# Competitive benchmark

Corpus: 24 images from `test-images\kodak`; threads per encoder: 1. Sizes are corpus totals after each back end; PSNR is the per-image mean over the format's channels; time is total CPU seconds of the encoder processes (user + kernel, with --cpu-time).


## BC7

| Encoder config | PSNR (dB) | Zstd-19 bpt | Deflate-9 bpt | Time (s) | Mpix/s | Zstd size vs mipple RDO curve at equal PSNR |
| --- | --- | --- | --- | --- | --- | --- |
| mipple e5 λ=0 | 45.37 | 7.175 | 7.276 | 6.84 | 1.38 | +0.0% |
| mipple e5 λ=0.25 | 45.22 | 6.029 | 6.149 | 116.09 | 0.08 | +0.0% |
| mipple e5 λ=0.5 | 43.95 | 5.083 | 5.234 | 118.20 | 0.08 | +0.0% |
| mipple e5 λ=1 | 41.64 | 3.868 | 4.049 | 60.05 | 0.16 | +0.0% |
| mipple e5 λ=2 | 39.70 | 3.140 | 3.320 | 54.98 | 0.17 | +0.0% |
| mipple e5 λ=4 | 37.46 | 2.516 | 2.690 | 50.78 | 0.19 | +0.0% |
| mipple e5 λ=8 | 34.92 | 1.933 | 2.086 | 49.25 | 0.19 | +0.0% |
| mipple e5 λ=16 | 32.14 | 1.365 | 1.485 | 42.94 | 0.22 | +0.0% |
| basis UASTC λ=0 | 43.22 | 7.418 | 7.496 | 118.27 | 0.08 | +59.1% |
| basis UASTC λ=0 as BC7 | 43.22 | 6.889 | 7.034 | 118.27 | 0.08 | +47.7% |
| basis UASTC λ=0.5 | 42.32 | 6.814 | 6.882 | 321.00 | 0.03 | +62.6% |
| basis UASTC λ=0.5 as BC7 | 42.32 | 6.342 | 6.479 | 321.00 | 0.03 | +51.3% |
| basis UASTC λ=1 | 41.01 | 6.355 | 6.419 | 313.78 | 0.03 | +75.9% |
| basis UASTC λ=1 as BC7 | 41.01 | 5.900 | 6.031 | 313.78 | 0.03 | +63.3% |
| basis UASTC λ=2 | 38.64 | 5.909 | 5.972 | 302.19 | 0.03 | +109.1% |
| basis UASTC λ=2 as BC7 | 38.64 | 5.447 | 5.575 | 302.19 | 0.03 | +92.7% |
| basis UASTC λ=3 | 36.69 | 5.615 | 5.678 | 297.98 | 0.03 | +141.8% |
| basis UASTC λ=3 as BC7 | 36.69 | 5.143 | 5.270 | 297.98 | 0.03 | +121.4% |
| basis UASTC λ=5 | 34.15 | 5.198 | 5.267 | 288.00 | 0.03 | +196.2% |
| basis UASTC λ=5 as BC7 | 34.15 | 4.698 | 4.825 | 288.00 | 0.03 | +167.8% |
| basis UASTC λ=8 | 32.18 | 4.826 | 4.906 | 273.31 | 0.03 | +251.7% |
| basis UASTC λ=8 as BC7 | 32.18 | 4.290 | 4.424 | 273.31 | 0.03 | +212.6% |
| basis XUBC7 bc7f q100 rdo0 | 44.25 | 5.533 | 5.533 | 27.80 | 0.34 | +4.5% |
| basis XUBC7 bc7f q100 rdo25 | 43.48 | 5.070 | 5.070 | 51.64 | 0.18 | +5.4% |
| basis XUBC7 bc7f q100 rdo50 | 41.92 | 4.571 | 4.571 | 52.05 | 0.18 | +14.3% |
| basis XUBC7 bc7f q100 rdo75 | 40.63 | 4.147 | 4.147 | 51.69 | 0.18 | +19.5% |
| basis XUBC7 bc7f q100 rdo100 | 39.67 | 3.789 | 3.789 | 51.55 | 0.18 | +21.0% |
| basis XUBC7 bc7e2 q100 rdo0 | 45.32 | 5.597 | 5.597 | 54.45 | 0.17 | -17.0% |
| basis XUBC7 bc7e2 q100 rdo25 | 44.39 | 5.312 | 5.312 | 74.78 | 0.13 | -1.6% |
| basis XUBC7 bc7e2 q100 rdo50 | 42.35 | 4.757 | 4.757 | 78.00 | 0.12 | +13.0% |
| basis XUBC7 bc7e2 q100 rdo75 | 40.82 | 4.238 | 4.238 | 81.88 | 0.12 | +19.7% |
| basis XUBC7 bc7e2 q100 rdo100 | 39.76 | 3.833 | 3.833 | 79.11 | 0.12 | +21.2% |
| basis XUBC7 bc7f q75 rdo0 | 36.37 | 3.901 | 3.901 | 55.06 | 0.17 | +73.6% |
| basis XUBC7 bc7f q75 rdo25 | 36.07 | 3.362 | 3.362 | 77.61 | 0.12 | +54.5% |
| basis XUBC7 bc7f q75 rdo50 | 35.58 | 2.894 | 2.894 | 77.98 | 0.12 | +39.8% |

BD-rate of **mipple RDO** vs each RDO family (negative = mipple needs fewer bits at equal PSNR, or at equal SSIM in the last column; for Basis rows the bits are what ships, see scripts/compete.py), and each family's total time range over its sweep (mipple RDO: 6.8-118.2 s):

| vs family | Zstd-19 | Deflate-9 | Zstd-19, equal SSIM | family time (s) |
| --- | --- | --- | --- | --- |
| basis UASTC RDO | -55.9% | -53.5% | -43.8% | 118.3-321.0 |
| basis UASTC RDO as BC7 | -51.9% | -49.9% | -39.0% | 118.3-321.0 |
| basis XUBC7 bc7f q100 | -11.6% | -7.7% | +15.7% | 27.8-52.0 |
| basis XUBC7 bc7e2 q100 | -8.5% | -4.9% | +16.2% | 54.5-81.9 |
| basis XUBC7 bc7f q75 | -34.8% | -29.9% | -28.6% | 55.1-78.0 |
