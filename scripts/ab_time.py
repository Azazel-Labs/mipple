#!/usr/bin/env python3
"""A/B timing of two mipple builds on the same encodes, with an output check.

For each configuration and image, the base and head builds take turns (best of --runs each), so drift
in clock speed lands on both. Time is the encode time mipple prints (`encode Xs`: the base encode plus
RDO, without loading or writing). Every pair of outputs is compared byte for byte; a change meant to be
output-preserving should report all identical.

A configuration is `EXT|ARGS`: the output extension and the arguments after `encode IMG -o OUT`.

Usage:
  python scripts/ab_time.py test-images/kodak --base old/mipple --head target/release/mipple --cpu 1 \
      --config "dds|-f bc7 -l 1 --no-mips" --config "astc|-f astc-6x6 -e 0 -l 1 --linear --no-mips"
"""
import argparse
import ctypes
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def pin(cpu):
    # Child processes inherit the affinity.
    if sys.platform == "win32":
        k = ctypes.windll.kernel32
        k.GetCurrentProcess.restype = ctypes.c_void_p
        if not k.SetProcessAffinityMask(ctypes.c_void_p(k.GetCurrentProcess()), ctypes.c_size_t(1 << cpu)):
            raise SystemExit(f"could not pin to CPU {cpu}")
    else:
        os.sched_setaffinity(0, {cpu})


def encode(exe, img, out, args, env):
    r = subprocess.run([str(exe), "encode", str(img), "-o", str(out), *args], capture_output=True, text=True, env=env)
    if r.returncode != 0:
        raise RuntimeError(f"{exe} failed on {img.name}: {r.stderr.strip() or r.stdout.strip()}")
    m = re.search(r"encode ([0-9.]+)s", r.stdout + r.stderr)
    if not m:
        raise RuntimeError(f"{exe} printed no encode time")
    return float(m.group(1))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("images", type=Path)
    ap.add_argument("--base", type=Path, required=True)
    ap.add_argument("--head", type=Path, required=True)
    ap.add_argument("--config", action="append", required=True, help="EXT|ARGS (repeatable)")
    ap.add_argument("--runs", type=int, default=3, help="best of N timings per image")
    ap.add_argument("--limit", type=int, default=0, help="first N images only")
    ap.add_argument("--cpu", type=int, default=None, help="pin this script and the encoders to this logical CPU")
    ap.add_argument("--out", type=Path, default=ROOT / "bench-results" / "ab")
    a = ap.parse_args()
    if a.cpu is not None:
        pin(a.cpu)
    imgs = sorted(p for p in a.images.iterdir() if p.suffix.lower() in (".png", ".jpg", ".tga"))
    if a.limit:
        imgs = imgs[: a.limit]
    a.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, RAYON_NUM_THREADS="1")
    print("| configuration | base s | head s | base / head | identical |")
    print("|---|---:|---:|---:|---:|")
    for cfg in a.config:
        ext, _, rest = cfg.partition("|")
        args = shlex.split(rest)
        tb = th = 0.0
        same = 0
        for img in imgs:
            ob, oh = a.out / f"base.{ext}", a.out / f"head.{ext}"
            bb = bh = float("inf")
            for _ in range(a.runs):
                bb = min(bb, encode(a.base, img, ob, args, env))
                bh = min(bh, encode(a.head, img, oh, args, env))
            tb += bb
            th += bh
            same += ob.read_bytes() == oh.read_bytes()
        print(f"| `{rest.strip()}` | {tb:.2f} | {th:.2f} | {tb / th:.3f} | {same}/{len(imgs)} |", flush=True)


if __name__ == "__main__":
    main()
