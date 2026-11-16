//! HDR images for BC6H: a linear float RGB surface ([`HdrSurface`]), loading (Radiance `.hdr`,
//! Portable Float Map `.pfm` (which holds negative values, for BC6H_SF16), or 8/16-bit images
//! decoded from sRGB), box-filtered mips, and the quality metrics HDR codecs are compared with:
//! multi-exposure PSNR ([`mpsnr`]) and log-space RMSE ([`log_rmse`]). OpenEXR is not read: the
//! `image` crate's EXR codec instantiates in every crate that saves an image and slows builds by
//! tens of minutes.

use std::path::Path;

use anyhow::{Context, Result};

use crate::bc6h::{HdrBlock, f32_to_bc6h, f32_to_uf16, half_to_f32};

/// A linear RGB float image, row-major. Negative and NaN values are stored as given; unsigned
/// BC6H encodes them as 0, signed BC6H keeps negative values (NaN as 0).
#[derive(Clone, Debug, PartialEq)]
pub struct HdrSurface {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 3]>,
}

impl HdrSurface {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![[0.0; 3]; width as usize * height as usize],
        }
    }

    /// Loads an image as linear light: float formats (Radiance `.hdr`, `.pfm`) as stored, 8- and
    /// 16-bit images decoded from sRGB unless `linear` (values then read as 0-1 fractions).
    pub fn load(path: &Path, linear: bool) -> Result<Self> {
        Self::load_as(path, linear, false)
    }

    /// [`HdrSurface::load`], except that when `signed` 8- and 16-bit images are read as signed
    /// normalized values (`v` in 0-1 as `2v - 1`, so 8-bit `u` is `(2u - 255) / 255`, the crate's
    /// convention for signed formats) rather than decoded from sRGB.
    pub fn load_as(path: &Path, linear: bool, signed: bool) -> Result<Self> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("pfm") {
            let bytes = std::fs::read(path).with_context(|| format!("loading {}", path.display()))?;
            return read_pfm(&bytes).with_context(|| format!("loading {}", path.display()));
        }
        let img = image::open(path).with_context(|| format!("loading {}", path.display()))?;
        let float = matches!(img.color(), image::ColorType::Rgb32F | image::ColorType::Rgba32F);
        let rgb = img.to_rgb32f();
        let decode = !float && !linear && !signed;
        let snorm = !float && signed;
        let pixels = rgb
            .pixels()
            .map(|p| {
                p.0.map(|v| {
                    if snorm {
                        2.0 * v - 1.0
                    } else if decode {
                        srgb_to_linear_f(v)
                    } else {
                        v
                    }
                })
            })
            .collect();
        Ok(Self {
            width: rgb.width(),
            height: rgb.height(),
            pixels,
        })
    }

    /// An 8-bit surface's RGB as linear light, decoded from sRGB when `srgb`.
    pub fn from_ldr(s: &crate::surface::Surface, srgb: bool) -> Self {
        let f = |v: u8| {
            if srgb {
                crate::color::srgb_to_linear(v)
            } else {
                f32::from(v) / 255.0
            }
        };
        Self {
            width: s.width,
            height: s.height,
            pixels: s.pixels.iter().map(|p| [f(p[0]), f(p[1]), f(p[2])]).collect(),
        }
    }

    /// Writes a Radiance `.hdr`, a `.pfm` (which keeps negative values), or, clamped to 8 bits, any
    /// LDR format the extension names.
    pub fn save(&self, path: &Path) -> Result<()> {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pfm")) {
            return std::fs::write(path, self.to_pfm()).with_context(|| format!("writing {}", path.display()));
        }
        let flat: Vec<f32> = self.pixels.iter().flatten().copied().collect();
        let img = image::Rgb32FImage::from_raw(self.width, self.height, flat).context("image size")?;
        image::DynamicImage::ImageRgb32F(img)
            .save(path)
            .with_context(|| format!("writing {}", path.display()))
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [f32; 3] {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn blocks_x(&self) -> u32 {
        self.width.div_ceil(4)
    }

    pub fn blocks_y(&self) -> u32 {
        self.height.div_ceil(4)
    }

    pub fn block_count(&self) -> usize {
        self.blocks_x() as usize * self.blocks_y() as usize
    }

    /// The 4x4 block at block coordinates (`bx`, `by`) as BC6H targets (unsigned half floats),
    /// replicating the last row and column past the edges.
    pub fn read_block(&self, bx: u32, by: u32) -> HdrBlock {
        self.read_block_as(bx, by, false)
    }

    /// [`HdrSurface::read_block`] as signed half floats when `signed`
    /// ([`crate::bc6h::f32_to_sf16`]).
    pub fn read_block_as(&self, bx: u32, by: u32, signed: bool) -> HdrBlock {
        let mut out = [[0u16; 3]; 16];
        for (i, o) in out.iter_mut().enumerate() {
            let x = (bx * 4 + (i % 4) as u32).min(self.width - 1);
            let y = (by * 4 + (i / 4) as u32).min(self.height - 1);
            *o = self.get(x, y).map(|v| f32_to_bc6h(v, signed));
        }
        out
    }

    /// Every texel as BC6H targets ([`f32_to_uf16`] of each channel), row-major. Uses F16C where
    /// the CPU has it (eight channels per instruction; the same values).
    pub fn to_uf16(&self) -> Vec<[u16; 3]> {
        use rayon::prelude::*;
        let src = self.pixels.as_flattened();
        let mut out = vec![[0u16; 3]; self.pixels.len()];
        out.as_flattened_mut()
            .par_chunks_mut(1 << 14)
            .zip(src.par_chunks(1 << 14))
            .for_each(|(o, s)| {
                #[cfg(target_arch = "x86_64")]
                if f16c() {
                    // SAFETY: the CPU reports F16C (and AVX, which it implies).
                    unsafe { uf16_f16c(s, o) };
                    return;
                }
                for (o, &s) in o.iter_mut().zip(s) {
                    *o = f32_to_uf16(s);
                }
            });
        out
    }

    /// The image of decoded half-float blocks in raster order (each `block` covers 4x4 texels,
    /// cropped at the edges).
    pub fn from_blocks(width: u32, height: u32, blocks: impl IntoIterator<Item = HdrBlock>) -> Self {
        let mut s = Self::new(width, height);
        let bw = width.div_ceil(4);
        for (bi, b) in blocks.into_iter().enumerate() {
            let (bx, by) = (bi as u32 % bw, bi as u32 / bw);
            for (i, t) in b.iter().enumerate() {
                let (x, y) = (bx * 4 + (i % 4) as u32, by * 4 + (i / 4) as u32);
                if x < width && y < height {
                    s.pixels[(y * width + x) as usize] = t.map(half_to_f32);
                }
            }
        }
        s
    }

    /// The next mip level: a 2x2 box filter (edges clamped for odd sizes), as [`crate::mips`].
    pub fn downsample(&self) -> Self {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut out = Self::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let xs = [(2 * x).min(self.width - 1), (2 * x + 1).min(self.width - 1)];
                let ys = [(2 * y).min(self.height - 1), (2 * y + 1).min(self.height - 1)];
                let mut c = [0f32; 3];
                for yy in ys {
                    for xx in xs {
                        let p = self.get(xx, yy);
                        for k in 0..3 {
                            c[k] += p[k];
                        }
                    }
                }
                out.pixels[(y * w + x) as usize] = c.map(|v| v * 0.25);
            }
        }
        out
    }

    /// Full mip chain down to 1x1, starting with a copy of `self`.
    pub fn chain(&self) -> Vec<Self> {
        let mut levels = vec![self.clone()];
        while let Some(last) = levels.last() {
            if last.width == 1 && last.height == 1 {
                break;
            }
            let next = last.downsample();
            levels.push(next);
        }
        levels
    }

    /// The values BC6H can store: clamped to [0, 65504] (NaN as 0) and rounded to half precision.
    pub fn representable(&self) -> Self {
        self.representable_as(false)
    }

    /// [`HdrSurface::representable`], or when `signed` the values signed BC6H can store (clamped
    /// to [-65504, 65504]).
    pub fn representable_as(&self, signed: bool) -> Self {
        Self {
            width: self.width,
            height: self.height,
            pixels: self
                .pixels
                .iter()
                .map(|p| p.map(|v| half_to_f32(f32_to_bc6h(v, signed))))
                .collect(),
        }
    }

    /// The image as a little-endian RGB Portable Float Map (rows bottom to top).
    pub fn to_pfm(&self) -> Vec<u8> {
        let mut out = format!("PF\n{} {}\n-1.0\n", self.width, self.height).into_bytes();
        for row in self.pixels.chunks(self.width as usize).rev() {
            for v in row.iter().flatten() {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }
}

/// Parses a Portable Float Map: `PF` (RGB) or `Pf` (gray, read into all three channels), width
/// and height, then a scale whose sign gives the byte order (negative: little-endian), then rows
/// of 32-bit floats from the bottom up.
pub fn read_pfm(bytes: &[u8]) -> Result<HdrSurface> {
    // Three whitespace-separated header tokens after the magic, then exactly one whitespace byte.
    let mut pos = 0;
    let mut token = || -> Result<&str> {
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        let start = pos;
        while bytes.get(pos).is_some_and(|b| !b.is_ascii_whitespace()) {
            pos += 1;
        }
        anyhow::ensure!(pos > start && pos < bytes.len(), "truncated PFM header");
        Ok(std::str::from_utf8(&bytes[start..pos])?)
    };
    let chans = match token()? {
        "PF" => 3,
        "Pf" => 1,
        m => anyhow::bail!("not a PFM file (magic {m:?})"),
    };
    let width: u32 = token()?.parse().context("PFM width")?;
    let height: u32 = token()?.parse().context("PFM height")?;
    let scale: f32 = token()?.parse().context("PFM scale")?;
    anyhow::ensure!(width > 0 && height > 0, "empty PFM image");
    let data = &bytes[pos + 1..];
    let n = (width as usize)
        .checked_mul(height as usize)
        .and_then(|t| t.checked_mul(chans * 4))
        .context("PFM size")?;
    anyhow::ensure!(data.len() >= n, "truncated PFM data ({} of {n} bytes)", data.len());
    let read = |i: usize| {
        let b = [data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]];
        if scale < 0.0 {
            f32::from_le_bytes(b)
        } else {
            f32::from_be_bytes(b)
        }
    };
    let mut s = HdrSurface::new(width, height);
    for y in 0..height as usize {
        let src_row = height as usize - 1 - y;
        for x in 0..width as usize {
            let t = (src_row * width as usize + x) * chans;
            s.pixels[y * width as usize + x] = if chans == 3 {
                [read(t), read(t + 1), read(t + 2)]
            } else {
                [read(t); 3]
            };
        }
    }
    Ok(s)
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn f16c() -> bool {
    static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *F.get_or_init(|| {
        crate::simd::level() != crate::simd::Level::Generic && std::arch::is_x86_feature_detected!("f16c")
    })
}

/// [`f32_to_uf16`] of each of `src` into `dst` (the same length). Clamping to [0, 65504] first
/// (`max` returns its second operand, 0, for NaN; -0 also becomes 0) leaves only finite,
/// non-negative values, which the round-to-nearest-even conversion maps as `f32_to_half` does.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,f16c")]
unsafe fn uf16_f16c(src: &[f32], dst: &mut [u16]) {
    use std::arch::x86_64::*;
    let n = src.len() / 8 * 8;
    let zero = _mm256_setzero_ps();
    let top = _mm256_set1_ps(65504.0);
    for i in (0..n).step_by(8) {
        // SAFETY: i + 8 <= n <= both lengths.
        unsafe {
            let x = _mm256_loadu_ps(src.as_ptr().add(i));
            let x = _mm256_min_ps(_mm256_max_ps(x, zero), top);
            let h = _mm256_cvtps_ph::<_MM_FROUND_TO_NEAREST_INT>(x);
            _mm_storeu_si128(dst.as_mut_ptr().add(i).cast(), h);
        }
    }
    for i in n..src.len() {
        dst[i] = f32_to_uf16(src[i]);
    }
}

/// sRGB-encoded 0-1 to linear 0-1.
fn srgb_to_linear_f(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Clean value for the metrics: NaN and negatives as 0 (what BC6H unsigned stores).
#[inline]
fn clean(v: f32) -> f32 {
    if v > 0.0 { v.min(65504.0) } else { 0.0 }
}

/// Whether any texel of `a` or `b` has a negative channel: the metrics then score signed values
/// (see [`mpsnr`]).
fn any_negative(a: &HdrSurface, b: &HdrSurface) -> bool {
    a.pixels.iter().chain(&b.pixels).flatten().any(|&v| v < 0.0)
}

/// The exposure range (whole stops) [`mpsnr`] uses for an image, from its brightest and darkest
/// texels: from the stop that brings the 99.9th percentile of max(R, G, B) to 1 to the stop that
/// brings the 1st percentile of the positive values to 1, at most 20 stops (dropping the darkest).
/// Magnitudes when the image has negative values.
pub fn exposure_range(s: &HdrSurface) -> (i32, i32) {
    let signed = any_negative(s, s);
    let m = |v: f32| if signed { clean(v.abs()) } else { clean(v) };
    let mut v: Vec<f32> = s
        .pixels
        .iter()
        .map(|p| m(p[0]).max(m(p[1])).max(m(p[2])))
        .filter(|&m| m > 0.0)
        .collect();
    if v.is_empty() {
        return (0, 0);
    }
    v.sort_unstable_by(f32::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    let lo = (-at(0.999).log2()).round() as i32;
    let hi = (-at(0.01).log2()).round() as i32;
    (lo, hi.clamp(lo, lo + 20))
}

/// Multi-exposure PSNR (Munkberg et al. 2006, "High dynamic range texture compression for graphics
/// hardware"): each image is exposed at every whole stop `c` in `stops`, tone mapped as
/// round(255 * (2^c * v)^(1/2.2)) clamped to 0-255, and the squared error is averaged over all
/// exposures, texels and RGB channels. In dB against 255. When either image has a negative value
/// (signed BC6H), each value is tone mapped by its magnitude and keeps its sign, so -x and x are
/// two tone-mapped values apart; images without negative values score as before.
pub fn mpsnr(a: &HdrSurface, b: &HdrSurface, stops: (i32, i32)) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height), "size mismatch");
    let signed = any_negative(a, b);
    let tm1 = |v: f32, scale: f32| ((255.0 * (scale * clean(v)).powf(1.0 / 2.2)).round()).clamp(0.0, 255.0);
    let tm = |v: f32, scale: f32| {
        if signed && v < 0.0 {
            -tm1(-v, scale)
        } else {
            tm1(v, scale)
        }
    };
    let mut sum = 0f64;
    let mut n = 0u64;
    for c in stops.0..=stops.1 {
        let scale = 2f32.powi(c);
        for (p, q) in a.pixels.iter().zip(&b.pixels) {
            for k in 0..3 {
                let d = f64::from(tm(p[k], scale) - tm(q[k], scale));
                sum += d * d;
            }
        }
        n += a.pixels.len() as u64 * 3;
    }
    let mse = sum / n.max(1) as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / mse).log10()
    }
}

/// Root mean square of log2 differences over RGB, each value floored at 2^-14 (the smallest
/// normal half): a relative error, about the error in stops. When either image has a negative
/// value, values are measured as their magnitude's stops above the floor with their sign, so a
/// sign flip costs the stops down to the floor and back.
pub fn log_rmse(a: &HdrSurface, b: &HdrSurface) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height), "size mismatch");
    let floor = 2f32.powi(-14);
    let signed = any_negative(a, b);
    let l = |v: f32| {
        if signed {
            let m = f64::from(clean(v.abs()).max(floor).log2()) + 14.0;
            if v < 0.0 { -m } else { m }
        } else {
            f64::from(clean(v).max(floor).log2())
        }
    };
    let mut sum = 0f64;
    for (p, q) in a.pixels.iter().zip(&b.pixels) {
        for k in 0..3 {
            let d = l(p[k]) - l(q[k]);
            sum += d * d;
        }
    }
    (sum / (a.pixels.len() as f64 * 3.0).max(1.0)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: u32, h: u32) -> HdrSurface {
        let mut s = HdrSurface::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = 2f32.powf(x as f32 / w as f32 * 16.0 - 8.0);
                s.pixels[(y * w + x) as usize] = [v, v * 0.5, v * (1.0 + y as f32 / h as f32)];
            }
        }
        s
    }

    #[test]
    fn metrics_are_exact_on_identity_and_drop_with_error() {
        let a = ramp(64, 32);
        let r = exposure_range(&a);
        assert!(r.0 < r.1 && r.1 - r.0 <= 20, "{r:?}");
        assert_eq!(mpsnr(&a, &a, r), f64::INFINITY);
        assert_eq!(log_rmse(&a, &a), 0.0);
        let mut b = a.clone();
        for p in &mut b.pixels {
            p[0] *= 1.05;
        }
        let mut c = a.clone();
        for p in &mut c.pixels {
            p[0] *= 1.2;
        }
        assert!(mpsnr(&a, &b, r) > mpsnr(&a, &c, r));
        // A uniform 5% error is log2(1.05) on one channel of three.
        let want = (1.05f64.log2().powi(2) / 3.0).sqrt();
        assert!((log_rmse(&a, &b) - want).abs() < 1e-3, "{} vs {want}", log_rmse(&a, &b));
    }

    #[test]
    fn pfm_round_trips_negative_values() {
        let mut a = ramp(7, 5);
        for (i, p) in a.pixels.iter_mut().enumerate() {
            if i % 3 == 0 {
                p[1] = -p[1];
            }
        }
        assert_eq!(read_pfm(&a.to_pfm()).unwrap(), a);
        // Big-endian and gray files, with a comment-free header split over spaces.
        let mut be = b"Pf 2  1\n1.0\n".to_vec();
        for v in [1.5f32, -2.0] {
            be.extend_from_slice(&v.to_be_bytes());
        }
        let g = read_pfm(&be).unwrap();
        assert_eq!(g.pixels, vec![[1.5; 3], [-2.0; 3]]);
        assert!(read_pfm(b"PF\n4 4\n-1.0\n\0\0").is_err());
    }

    #[test]
    fn signed_metrics_see_sign_errors() {
        let a = ramp(16, 8);
        let mut neg = a.clone();
        for p in &mut neg.pixels {
            p[0] = -p[0];
        }
        let r = exposure_range(&neg);
        assert_eq!(r, exposure_range(&a));
        assert_eq!(mpsnr(&neg, &neg, r), f64::INFINITY);
        assert_eq!(log_rmse(&neg, &neg), 0.0);
        // Losing the sign of red costs far more than a 1% error in it.
        let mut flipped = neg.clone();
        let mut off = neg.clone();
        for (f, o) in flipped.pixels.iter_mut().zip(&mut off.pixels) {
            f[0] = -f[0];
            o[0] *= 1.01;
        }
        assert!(mpsnr(&neg, &off, r) > mpsnr(&neg, &flipped, r) + 20.0);
        assert!(log_rmse(&neg, &off) < log_rmse(&neg, &flipped) / 10.0);
    }

    #[test]
    fn to_uf16_matches_scalar() {
        let mut v = vec![
            f32::NAN,
            -f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -0.0,
            -3.0,
            1e-30,
            2f32.powi(-25),
            2f32.powi(-25) * 1.01,
            2f32.powi(-14) * 0.999,
            1.0 + 1.0 / 2048.0,
            1.0 + 3.0 / 2048.0,
            65504.0,
            65519.0,
            65520.0,
            1e9,
        ];
        // Every half's value and the midpoints either side of it, plus pseudo-random floats.
        for h in 0..0x7c00u16 {
            let f = half_to_f32(h);
            v.extend([
                f,
                f32::from_bits(f.to_bits() + 0x1000),
                f32::from_bits(f.to_bits().saturating_sub(0x1000)),
            ]);
        }
        let mut x = 0x1234_5678u32;
        for _ in 0..30000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            v.push(f32::from_bits(x));
        }
        v.truncate(v.len() / 3 * 3);
        let s = HdrSurface {
            width: (v.len() / 3) as u32,
            height: 1,
            pixels: v.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
        };
        let got = s.to_uf16();
        for (p, g) in s.pixels.iter().zip(&got) {
            assert_eq!(p.map(f32_to_uf16), *g, "{p:?}");
        }
    }

    #[test]
    fn blocks_round_trip_and_mips_average() {
        let a = ramp(10, 6).representable();
        let blocks: Vec<HdrBlock> = (0..a.block_count() as u32)
            .map(|i| a.read_block(i % a.blocks_x(), i / a.blocks_x()))
            .collect();
        assert_eq!(HdrSurface::from_blocks(10, 6, blocks), a);
        let chain = a.chain();
        assert_eq!(chain.len(), 4); // 10x6, 5x3, 2x1, 1x1
        let m = &chain[1];
        let want = (a.get(0, 0)[2] + a.get(1, 0)[2] + a.get(0, 1)[2] + a.get(1, 1)[2]) / 4.0;
        assert!((m.get(0, 0)[2] - want).abs() < 1e-6);
    }
}
