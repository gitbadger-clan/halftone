//! WAVES-style distortion suite. Each distortion is a pure function on bytes so
//! the harness can run sources on the distorted output exactly as a user would.
//!
//! Two kinds of transform live here:
//! - **Synthetic**: decode → operate → encode with the `image` crate. Deterministic
//!   and fast, but the JPEG/WebP encoders are not libjpeg/libwebp, so a `jpeg_q80`
//!   from here is not byte-identical to Photoshop's or Pillow's q80. For metadata
//!   survival that is irrelevant — an encoder either carries the segment or it does
//!   not, and `image` carries none — and the methodology says so.
//! - **Captured**: files a real application produced (WhatsApp, a screenshot, a
//!   CDN). They are not computed here; [`Distortion::Captured`] only names them so
//!   they take a column next to the synthetic ones.
//!
//! Every synthetic transform drops all metadata by construction (the `image` crate
//! writes none), so the container after a synthetic transform is always a clean
//! re-encode. That is the honest baseline: it is what most pipelines do.

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageEncoder, ImageFormat};
use serde::{Deserialize, Serialize};

/// Error from a transform.
#[derive(Debug, thiserror::Error)]
pub enum DistortError {
    /// The input could not be decoded (unsupported container, truncated data).
    #[error("decode: {0}")]
    Decode(String),
    /// The output could not be encoded.
    #[error("encode: {0}")]
    Encode(String),
    /// A parameter is outside its valid range.
    #[error("invalid parameter: {0}")]
    Param(&'static str),
}

/// A named distortion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Distortion {
    /// Identity: the bytes pass through untouched (no re-encode).
    None,
    /// Re-encode as JPEG at this quality (1–100), whatever the input container.
    Jpeg(u8),
    /// Resize by factor (0 < f ≤ 4), keeping the input container.
    Resize(f32),
    /// Centre crop, removing this fraction (0 ≤ f < 1) of each dimension, keeping
    /// the input container.
    Crop(f32),
    /// Gaussian blur with this sigma, keeping the input container.
    Blur(f32),
    /// Additive noise with this sigma on the 0–255 scale, keeping the input container.
    Noise(f32),
    /// Re-encode as PNG (lossless), whatever the input container.
    Png,
    /// Re-encode as lossless WebP, whatever the input container.
    Webp,
    /// A file produced by a real application; `label` is the route it took
    /// (`whatsapp_photo`, `telegram_file`, `screenshot_macos`, …).
    Captured(String),
}

/// The bytes a transform produced and the container they are in.
#[derive(Debug, Clone)]
pub struct Derivative {
    /// Encoded file.
    pub bytes: Vec<u8>,
    /// File extension for the container (`jpg`, `png`, `webp`).
    pub ext: &'static str,
}

impl Distortion {
    /// Stable name for tables and file names.
    pub fn name(&self) -> String {
        match self {
            Self::None => "none".into(),
            Self::Jpeg(q) => format!("jpeg_q{q}"),
            Self::Resize(f) => format!("resize_{f}"),
            Self::Crop(f) => format!("crop_{f}"),
            Self::Blur(s) => format!("blur_{s}"),
            Self::Noise(s) => format!("noise_{s}"),
            Self::Png => "png".into(),
            Self::Webp => "webp".into(),
            Self::Captured(label) => format!("captured:{label}"),
        }
    }

    /// Parse a name produced by [`Self::name`].
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        if let Some(l) = name.strip_prefix("captured:") {
            return Some(Self::Captured(l.to_string()));
        }
        match name {
            "none" | "identity" => return Some(Self::None),
            "png" => return Some(Self::Png),
            "webp" => return Some(Self::Webp),
            _ => {}
        }
        let (kind, arg) = name.split_once('_')?;
        match kind {
            "jpeg" => arg
                .strip_prefix('q')?
                .parse::<u8>()
                .ok()
                .filter(|q| (1..=100).contains(q))
                .map(Self::Jpeg),
            "resize" => arg.parse().ok().map(Self::Resize),
            "crop" => arg.parse().ok().map(Self::Crop),
            "blur" => arg.parse().ok().map(Self::Blur),
            "noise" => arg.parse().ok().map(Self::Noise),
            _ => None,
        }
    }

    /// Whether this transform needs the decoded image (everything but identity
    /// and captured files).
    pub fn needs_decode(&self) -> bool {
        !matches!(self, Self::None | Self::Captured(_))
    }

    /// The original WAVES-style robustness suite.
    pub fn default_suite() -> Vec<Self> {
        vec![
            Self::None,
            Self::Jpeg(50),
            Self::Jpeg(75),
            Self::Resize(0.5),
            Self::Crop(0.1),
            Self::Blur(1.0),
            Self::Noise(5.0),
        ]
    }

    /// The metadata-survival suite: what ordinary delivery paths do to a file.
    pub fn survival_suite() -> Vec<Self> {
        vec![
            Self::None,
            Self::Jpeg(95),
            Self::Jpeg(80),
            Self::Jpeg(70),
            Self::Resize(0.5),
            Self::Crop(0.1),
            Self::Png,
            Self::Webp,
        ]
    }

    /// Apply to encoded bytes: decodes (if needed), operates, encodes.
    /// For several transforms on one input prefer [`decode`] once and
    /// [`Self::apply_decoded`] per transform.
    pub fn apply(&self, bytes: &[u8]) -> Result<Derivative, DistortError> {
        if !self.needs_decode() {
            return self.apply_decoded(None, bytes);
        }
        let img = decode(bytes)?;
        self.apply_decoded(Some(&img), bytes)
    }

    /// Apply with a pre-decoded image. `decoded` may be `None` only for transforms
    /// that do not need it ([`Self::needs_decode`]).
    pub fn apply_decoded(
        &self,
        decoded: Option<&Decoded>,
        original: &[u8],
    ) -> Result<Derivative, DistortError> {
        match self {
            Self::None => {
                let format = sniff_format(original)
                    .ok_or_else(|| DistortError::Decode("unrecognised container".into()))?;
                return Ok(Derivative {
                    bytes: original.to_vec(),
                    ext: ext_of(format),
                });
            }
            Self::Captured(_) => {
                return Err(DistortError::Param(
                    "captured transforms are files, not computations",
                ))
            }
            _ => {}
        }
        let d = decoded.ok_or(DistortError::Param("transform needs a decoded image"))?;
        match self {
            Self::Jpeg(q) => encode_jpeg(&d.image, *q),
            Self::Png => encode_png(&d.image),
            Self::Webp => encode_webp(&d.image),
            Self::Resize(f) => {
                if f.is_nan() || *f <= 0.0 || *f > 4.0 {
                    return Err(DistortError::Param("resize factor must be in (0, 4]"));
                }
                let (w, h) = (d.image.width(), d.image.height());
                let nw = ((w as f32 * f).round() as u32).max(1);
                let nh = ((h as f32 * f).round() as u32).max(1);
                let out = d
                    .image
                    .resize_exact(nw, nh, image::imageops::FilterType::Triangle);
                encode_same(&out, d.format)
            }
            Self::Crop(f) => {
                if f.is_nan() || *f < 0.0 || *f >= 1.0 {
                    return Err(DistortError::Param("crop fraction must be in [0, 1)"));
                }
                let (w, h) = (d.image.width(), d.image.height());
                let nw = ((w as f32 * (1.0 - f)).round() as u32).clamp(1, w);
                let nh = ((h as f32 * (1.0 - f)).round() as u32).clamp(1, h);
                let x = (w - nw) / 2;
                let y = (h - nh) / 2;
                let out = d.image.crop_imm(x, y, nw, nh);
                encode_same(&out, d.format)
            }
            Self::Blur(sigma) => {
                if sigma.is_nan() || *sigma <= 0.0 {
                    return Err(DistortError::Param("blur sigma must be > 0"));
                }
                encode_same(&d.image.blur(*sigma), d.format)
            }
            Self::Noise(sigma) => {
                if sigma.is_nan() || *sigma < 0.0 {
                    return Err(DistortError::Param("noise sigma must be >= 0"));
                }
                encode_same(&add_noise(&d.image, *sigma), d.format)
            }
            Self::None | Self::Captured(_) => unreachable!("handled above"),
        }
    }
}

/// Parse a comma-separated suite; unknown names are returned as errors.
pub fn parse_suite(spec: &str) -> Result<Vec<Distortion>, String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Distortion::parse(s).ok_or_else(|| format!("unknown transform `{s}`")))
        .collect()
}

/// A decoded input, kept alongside the container it came from so same-container
/// transforms know what to write back.
#[derive(Debug, Clone)]
pub struct Decoded {
    /// Pixels.
    pub image: DynamicImage,
    /// Container of the original bytes.
    pub format: ImageFormat,
}

/// Decode once; share across transforms.
pub fn decode(bytes: &[u8]) -> Result<Decoded, DistortError> {
    let format =
        sniff_format(bytes).ok_or_else(|| DistortError::Decode("unrecognised container".into()))?;
    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| DistortError::Decode(e.to_string()))?;
    Ok(Decoded { image, format })
}

/// Container by magic bytes, restricted to what the suite can write back.
pub fn sniff_format(b: &[u8]) -> Option<ImageFormat> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(ImageFormat::Png)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

/// File extension for a container.
pub fn ext_of(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        _ => "bin",
    }
}

/// JPEG quality used when a same-container transform writes a JPEG back.
pub const SAME_CONTAINER_JPEG_QUALITY: u8 = 90;

fn encode_same(img: &DynamicImage, format: ImageFormat) -> Result<Derivative, DistortError> {
    match format {
        ImageFormat::Jpeg => encode_jpeg(img, SAME_CONTAINER_JPEG_QUALITY),
        ImageFormat::Png => encode_png(img),
        ImageFormat::WebP => encode_webp(img),
        _ => Err(DistortError::Encode("unsupported container".into())),
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Derivative, DistortError> {
    if !(1..=100).contains(&quality) {
        return Err(DistortError::Param("jpeg quality must be 1..=100"));
    }
    let rgb = img.to_rgb8();
    let mut out = Vec::with_capacity((rgb.len() / 8).max(4096));
    JpegEncoder::new_with_quality(&mut out, quality)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| DistortError::Encode(e.to_string()))?;
    Ok(Derivative {
        bytes: out,
        ext: "jpg",
    })
}

fn encode_png(img: &DynamicImage) -> Result<Derivative, DistortError> {
    let mut out = Vec::with_capacity(img.as_bytes().len() / 2);
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        PngEncoder::new(&mut out)
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| DistortError::Encode(e.to_string()))?;
    } else {
        let rgb = img.to_rgb8();
        PngEncoder::new(&mut out)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| DistortError::Encode(e.to_string()))?;
    }
    Ok(Derivative {
        bytes: out,
        ext: "png",
    })
}

fn encode_webp(img: &DynamicImage) -> Result<Derivative, DistortError> {
    let mut out = Vec::with_capacity(img.as_bytes().len() / 2);
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        WebPEncoder::new_lossless(&mut out)
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| DistortError::Encode(e.to_string()))?;
    } else {
        let rgb = img.to_rgb8();
        WebPEncoder::new_lossless(&mut out)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| DistortError::Encode(e.to_string()))?;
    }
    Ok(Derivative {
        bytes: out,
        ext: "webp",
    })
}

/// Deterministic additive noise: fixed-seed xorshift, approximately Gaussian via
/// a sum of four uniforms, per channel, clamped. Same input, same output, so a
/// row can be reproduced.
fn add_noise(img: &DynamicImage, sigma: f32) -> DynamicImage {
    let mut rgb = img.to_rgb8();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    // Irwin–Hall with n = 4: variance n/12, so scale to unit variance.
    let scale = sigma * (12.0f32 / 4.0).sqrt();
    for px in rgb.pixels_mut() {
        for c in px.0.iter_mut() {
            let mut acc = 0.0f32;
            for _ in 0..4 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                acc += (state >> 40) as f32 / (1u64 << 24) as f32; // uniform [0,1)
            }
            let n = (acc - 2.0) * scale;
            *c = (*c as f32 + n).round().clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgb8(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn synthetic(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([
                (x * 7 % 256) as u8,
                (y * 13 % 256) as u8,
                ((x ^ y) % 256) as u8,
            ])
        }))
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        encode_png(&synthetic(w, h)).unwrap().bytes
    }

    fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
        encode_jpeg(&synthetic(w, h), 90).unwrap().bytes
    }

    #[test]
    fn names_round_trip_through_parse() {
        for d in Distortion::survival_suite()
            .into_iter()
            .chain(Distortion::default_suite())
            .chain([Distortion::Captured("whatsapp_photo".into())])
        {
            assert_eq!(
                Distortion::parse(&d.name()),
                Some(d.clone()),
                "{}",
                d.name()
            );
        }
        assert!(Distortion::parse("jpeg_q0").is_none());
        assert!(Distortion::parse("jpeg_q101").is_none());
        assert!(Distortion::parse("bogus").is_none());
        assert_eq!(
            parse_suite("none, jpeg_q80,png").unwrap(),
            vec![Distortion::None, Distortion::Jpeg(80), Distortion::Png]
        );
        assert!(parse_suite("none,what").is_err());
    }

    #[test]
    fn identity_passes_bytes_through_untouched() {
        let b = jpeg_bytes(32, 24);
        let d = Distortion::None.apply(&b).unwrap();
        assert_eq!(d.bytes, b);
        assert_eq!(d.ext, "jpg");
    }

    #[test]
    fn every_synthetic_transform_produces_a_decodable_file_in_the_right_container() {
        let src = png_bytes(64, 48);
        let dec = decode(&src).unwrap();
        for d in Distortion::survival_suite()
            .into_iter()
            .chain(Distortion::default_suite())
        {
            if !d.needs_decode() {
                continue;
            }
            let out = d.apply_decoded(Some(&dec), &src).unwrap();
            let back = decode(&out.bytes).unwrap_or_else(|e| panic!("{}: {e}", d.name()));
            let expect_fmt = match d {
                Distortion::Jpeg(_) => ImageFormat::Jpeg,
                Distortion::Webp => ImageFormat::WebP,
                _ => ImageFormat::Png, // same-container from a PNG source
            };
            assert_eq!(back.format, expect_fmt, "{}", d.name());
            assert_eq!(sniff_format(&out.bytes), Some(expect_fmt));
        }
    }

    #[test]
    fn resize_and_crop_change_dimensions_as_specified() {
        let src = png_bytes(100, 60);
        let dec = decode(&src).unwrap();
        let r = Distortion::Resize(0.5)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        let ri = decode(&r.bytes).unwrap().image;
        assert_eq!((ri.width(), ri.height()), (50, 30));
        let c = Distortion::Crop(0.1)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        let ci = decode(&c.bytes).unwrap().image;
        assert_eq!((ci.width(), ci.height()), (90, 54));
        assert!(Distortion::Resize(0.0)
            .apply_decoded(Some(&dec), &src)
            .is_err());
        assert!(Distortion::Crop(1.0)
            .apply_decoded(Some(&dec), &src)
            .is_err());
    }

    #[test]
    fn jpeg_quality_orders_file_size() {
        let src = png_bytes(96, 96);
        let dec = decode(&src).unwrap();
        let q95 = Distortion::Jpeg(95)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        let q70 = Distortion::Jpeg(70)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        assert!(q70.bytes.len() < q95.bytes.len());
        assert!(Distortion::Jpeg(0).apply_decoded(Some(&dec), &src).is_err());
    }

    #[test]
    fn same_container_transforms_keep_the_container() {
        let jpg = jpeg_bytes(40, 40);
        let dec = decode(&jpg).unwrap();
        let out = Distortion::Blur(1.0)
            .apply_decoded(Some(&dec), &jpg)
            .unwrap();
        assert_eq!(out.ext, "jpg");
        let web = Distortion::Webp.apply(&jpg).unwrap();
        let dec2 = decode(&web.bytes).unwrap();
        let out2 = Distortion::Noise(3.0)
            .apply_decoded(Some(&dec2), &web.bytes)
            .unwrap();
        assert_eq!(out2.ext, "webp");
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let src = png_bytes(32, 32);
        let dec = decode(&src).unwrap();
        let a = Distortion::Noise(5.0)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        let b = Distortion::Noise(5.0)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        assert_eq!(a.bytes, b.bytes, "same seed, same bytes");
        let zero = Distortion::Noise(0.0)
            .apply_decoded(Some(&dec), &src)
            .unwrap();
        assert_eq!(
            decode(&zero.bytes).unwrap().image.to_rgb8().as_raw(),
            dec.image.to_rgb8().as_raw(),
            "sigma 0 is a no-op on pixels"
        );
    }

    #[test]
    fn synthetic_transforms_drop_embedded_metadata() {
        // A PNG with a text chunk that looks like marking: the image crate's
        // encoders write no metadata, so any transform strips it. This is the
        // property the survival table relies on for its synthetic columns.
        let mut src = png_bytes(16, 16);
        let marker = b"DigitalSourceType";
        // Splice a tEXt chunk after IHDR (8 sig + 25 IHDR bytes).
        let data = [&b"Comment\0"[..], &marker[..]].concat();
        let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(&data);
        chunk.extend_from_slice(&[0, 0, 0, 0]);
        src.splice(33..33, chunk);
        assert!(src.windows(marker.len()).any(|w| w == marker));
        let out = Distortion::Png.apply(&src).unwrap();
        assert!(!out.bytes.windows(marker.len()).any(|w| w == marker));
    }

    #[test]
    fn captured_is_a_label_not_a_computation() {
        let src = png_bytes(8, 8);
        assert!(Distortion::Captured("x".into()).apply(&src).is_err());
        assert_eq!(
            Distortion::Captured("whatsapp".into()).name(),
            "captured:whatsapp"
        );
    }

    #[test]
    fn garbage_input_is_an_error_not_a_panic() {
        assert!(decode(b"not an image").is_err());
        assert!(Distortion::Jpeg(80)
            .apply(&[0xFF, 0xD8, 0xFF, 0x00])
            .is_err());
        assert!(Distortion::None.apply(b"nope").is_err());
    }
}
