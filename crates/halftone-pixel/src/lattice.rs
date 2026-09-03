//! Period-8 residual lattice: the decoder-seam / block-grid statistic.
//!
//! Why it works: latent-diffusion decoders synthesise the image from a latent at 1/8
//! resolution, so every 8×8 pixel tile comes from one latent pixel and the decoder
//! leaves a faint periodic seam (Corvi et al. 2023, arXiv:2211.00680). In the
//! high-pass residual, the rows and columns on the 8-grid are systematically *louder*
//! than the rest. Real screenshots, renders and never-compressed exports have no
//! global 8-pixel energy rhythm — with two named exceptions this source is honest
//! about: an image JPEG-compressed earlier and re-saved losslessly carries the JPEG
//! block grid (same period), and nearest-neighbour 8× upscales are periodic by
//! construction. `Present` therefore names all causes; none of them is a camera-native
//! photograph.
//!
//! The statistic, per tile of a 4×4 grid, per axis: mean |residual| per column (row),
//! the **median** of those within each x-mod-8 (y-mod-8) residue class — median, so a
//! few straight UI lines that happen to sit on the 8-grid cannot lift a class, while a
//! true grid lifts every column in its class — then the profile contrast
//! `(max − median) / (median + ε)`. A tile's score is the *minimum* over the two axes
//! (a grid needs both); the headline statistic is the median tile score, and the
//! fraction of consistent tiles is reported. A screenshot merely *containing* a JPEG
//! photo has the grid only in that region and fails the consistency requirement.

use halftone_core::{
    Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Statistic, Status,
};
use serde::Serialize;

use crate::residual::{self, Plane};

/// Tile grid dimension (4 → 16 tiles).
const GRID: usize = 4;
/// Minimum image dimension; below this there are too few periods per tile.
const MIN_DIM: usize = 256;
/// Stabiliser added to the class median in the contrast denominator (gray levels).
const EPS: f64 = 0.1;
/// A tile counts as "consistent" above this contrast.
const CONSISTENT_MARGIN: f64 = 0.04;
/// Present additionally requires this fraction of consistent tiles.
const CONSISTENT_FRACTION: f64 = 0.75;

/// Per-tile result.
#[derive(Debug, Clone, Serialize)]
pub struct TileScore {
    /// Tile column and row in the 4×4 grid.
    pub tile: (usize, usize),
    /// min(x, y) profile contrast for this tile.
    pub contrast: f64,
    /// Per-axis contrasts.
    pub x: f64,
    /// See `x`.
    pub y: f64,
}

/// Full lattice statistics.
#[derive(Debug, Clone, Serialize)]
pub struct LatticeStats {
    /// Per-tile contrasts.
    pub tiles: Vec<TileScore>,
    /// Median tile contrast — the headline statistic.
    pub median: f64,
    /// Fraction of tiles above [`CONSISTENT_MARGIN`].
    pub consistent_tiles: f64,
    /// Median per-axis contrasts, for diagnosing single-axis resizes.
    pub median_x: f64,
    /// See `median_x`.
    pub median_y: f64,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Profile contrast along one axis of a region: per-line mean |r|, class medians over
/// line index mod 8, `(max class − median class) / (median class + EPS)`.
fn axis_profile_contrast(r: &Plane, x0: usize, y0: usize, w: usize, h: usize, x_axis: bool) -> f64 {
    let lines = if x_axis { w } else { h };
    let per_line = if x_axis { h } else { w };
    let mut means = vec![0f64; lines];
    for (li, m) in means.iter_mut().enumerate() {
        let mut s = 0f64;
        for p in 0..per_line {
            let (x, y) = if x_axis {
                (x0 + li, y0 + p)
            } else {
                (x0 + p, y0 + li)
            };
            s += r.data[y * r.w + x].abs() as f64;
        }
        *m = s / per_line as f64;
    }
    let mut classes = [0f64; 8];
    for (k, c) in classes.iter_mut().enumerate() {
        let mut vals: Vec<f64> = means
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 8 == k)
            .map(|(_, &v)| v)
            .collect();
        *c = median(&mut vals);
    }
    let mut sorted = classes;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = sorted[4];
    let max = sorted[7];
    (max - med) / (med + EPS)
}

/// Compute lattice statistics for a residual plane.
pub fn lattice_stats(r: &Plane) -> LatticeStats {
    let (tw, th) = (r.w / GRID, r.h / GRID);
    let mut tiles = Vec::with_capacity(GRID * GRID);
    for ty in 0..GRID {
        for tx in 0..GRID {
            let cx = axis_profile_contrast(r, tx * tw, ty * th, tw, th, true);
            let cy = axis_profile_contrast(r, tx * tw, ty * th, tw, th, false);
            tiles.push(TileScore {
                tile: (tx, ty),
                contrast: cx.min(cy),
                x: cx,
                y: cy,
            });
        }
    }
    let mut c: Vec<f64> = tiles.iter().map(|t| t.contrast).collect();
    let mut xs: Vec<f64> = tiles.iter().map(|t| t.x).collect();
    let mut ys: Vec<f64> = tiles.iter().map(|t| t.y).collect();
    let consistent_tiles =
        c.iter().filter(|&&v| v > CONSISTENT_MARGIN).count() as f64 / c.len() as f64;
    LatticeStats {
        median: median(&mut c),
        consistent_tiles,
        median_x: median(&mut xs),
        median_y: median(&mut ys),
        tiles,
    }
}

/// Decoder-lattice evidence source.
///
/// Dark mode (`threshold: None`, the default): always `Inconclusive`, statistic
/// reported — run it like this until a calibration exists. With a threshold:
/// `Present` above it (if enough tiles agree), `Absent` below.
#[derive(Debug, Default)]
pub struct LatticeSource {
    /// Decision threshold on the median tile contrast. `None` = dark mode.
    pub threshold: Option<f64>,
}

/// Cheap losslessness check for WebP: is there a VP8L chunk near the front?
fn webp_is_lossless(b: &[u8]) -> bool {
    b.len() > 16 && b[12..b.len().min(64)].windows(4).any(|w| w == b"VP8L")
}

impl EvidenceSource for LatticeSource {
    fn id(&self) -> SourceId {
        SourceId {
            name: "pixel_lattice".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image
            && (a.mime == "image/png" || (a.mime == "image/webp" && webp_is_lossless(&a.bytes)))
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let inconclusive = |why: String, details: serde_json::Value| Evidence {
            layer: self.layer(),
            source: self.id(),
            status: Status::Inconclusive,
            statistic: None,
            calibration: None,
            rationale: why,
            details,
            duration_ms: 0,
        };
        let plane = match residual::luma(&a.bytes) {
            Ok(p) => p,
            Err(e) => {
                return Ok(inconclusive(
                    format!("Could not decode image ({e}); lattice test skipped."),
                    serde_json::json!({ "error": e }),
                ))
            }
        };
        if plane.w.min(plane.h) < MIN_DIM {
            return Ok(inconclusive(
                format!(
                    "Image is {}×{}; below {MIN_DIM}px there are too few 8-pixel periods for a \
                     stable estimate.",
                    plane.w, plane.h
                ),
                serde_json::json!({ "width": plane.w, "height": plane.h }),
            ));
        }
        let r = residual::highpass(&plane);
        let energy: f64 =
            r.data.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / r.data.len() as f64;
        if energy < 0.05 {
            return Ok(inconclusive(
                "Residual is nearly empty (flat or synthetic-solid image); lattice test not \
                 meaningful."
                    .into(),
                serde_json::json!({ "residual_energy": energy }),
            ));
        }
        let stats = lattice_stats(&r);

        let statistic = Statistic {
            name: "lattice_period8_profile".into(),
            value: stats.median,
            null_model: "real lossless sources (screenshots, renders, PNG exports of \
                         never-JPEG'd images): no global period-8 residual energy rhythm"
                .into(),
            p_value: None,
            threshold: self.threshold,
        };
        let details = serde_json::json!({
            "median": stats.median,
            "median_x": stats.median_x,
            "median_y": stats.median_y,
            "consistent_tiles": stats.consistent_tiles,
            "tiles": stats.tiles,
            "residual_energy": energy,
            "width": plane.w,
            "height": plane.h,
        });

        let (status, rationale) = match self.threshold {
            None => (
                Status::Inconclusive,
                format!(
                    "Dark mode: period-8 lattice contrast is {:.4} ({}% of tiles consistent). No \
                     verdict — this statistic has no calibrated threshold yet.",
                    stats.median,
                    (stats.consistent_tiles * 100.0).round()
                ),
            ),
            Some(t) if stats.median >= t && stats.consistent_tiles >= CONSISTENT_FRACTION => (
                Status::Present,
                format!(
                    "Global 8-pixel periodic residual structure (contrast {:.4} ≥ {:.4}, {}% of \
                     tiles consistent). Characteristic of latent-decoder output at native \
                     resolution — but an earlier JPEG compression re-saved losslessly, or a \
                     nearest-neighbour 8× upscale, leaves the same lattice. None of these is a \
                     camera-native photograph; which one this is, this source cannot say.",
                    stats.median,
                    t,
                    (stats.consistent_tiles * 100.0).round()
                ),
            ),
            Some(t) => (
                Status::Absent,
                format!(
                    "No global period-8 residual lattice (contrast {:.4} < {:.4}). Expected to \
                     vanish after any resize or re-compression, so absence says nothing about \
                     origin.",
                    stats.median, t
                ),
            ),
        };

        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: Some(statistic),
            calibration: None,
            rationale,
            details,
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: &mut u64) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        ((*seed % 1000) as f32 / 1000.0 - 0.5) * 8.0
    }

    fn textured(w: usize, h: usize) -> Plane {
        let mut seed = 99u64;
        let data = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                128.0
                    + 40.0 * ((x as f32) / 23.0).sin() * ((y as f32) / 17.0).cos()
                    + noise(&mut seed)
            })
            .collect();
        Plane { w, h, data }
    }

    fn with_fixed_seam(mut p: Plane, amplitude: f32) -> Plane {
        for y in 0..p.h {
            for x in 0..p.w {
                if x % 8 == 0 || y % 8 == 0 {
                    p.data[y * p.w + x] += amplitude;
                }
            }
        }
        p
    }

    fn with_iid_tiles(mut p: Plane, amplitude: f32) -> Plane {
        let mut seed = 7u64;
        let tiles_x = p.w.div_ceil(8);
        let offsets: Vec<f32> = (0..tiles_x * p.h.div_ceil(8))
            .map(|_| noise(&mut seed) / 4.0 * amplitude)
            .collect();
        for y in 0..p.h {
            for x in 0..p.w {
                p.data[y * p.w + x] += offsets[(y / 8) * tiles_x + x / 8];
            }
        }
        p
    }

    fn score(p: &Plane) -> LatticeStats {
        lattice_stats(&crate::residual::highpass(p))
    }

    #[test]
    fn grids_score_above_clean() {
        let clean = score(&textured(320, 320));
        let fixed = score(&with_fixed_seam(textured(320, 320), 2.5));
        let iid = score(&with_iid_tiles(textured(320, 320), 4.0));
        assert!(clean.median < 0.035, "clean {:.4}", clean.median);
        assert!(fixed.median > 0.05, "fixed {:.4}", fixed.median);
        assert!(fixed.consistent_tiles > 0.9);
        assert!(iid.median > 0.04, "iid {:.4}", iid.median);
    }

    #[test]
    fn straight_ui_lines_on_the_grid_do_not_fire() {
        // Flat UI with rules every 40 and 56 px — both multiples of 8, the worst case.
        let mut p = Plane {
            w: 320,
            h: 320,
            data: vec![240.0; 320 * 320],
        };
        for y in 0..p.h {
            for x in 0..p.w {
                if y % 40 == 0 || x % 56 == 0 {
                    p.data[y * p.w + x] = 80.0;
                }
            }
        }
        let s = score(&p);
        assert!(s.median < 0.01, "ui {:.4}", s.median);
        assert!(s.consistent_tiles < 0.2);
    }

    #[test]
    fn partial_grid_fails_consistency() {
        let mut p = textured(320, 320);
        let seamed = with_fixed_seam(textured(160, 160), 2.5);
        for y in 0..160 {
            for x in 0..160 {
                p.data[y * p.w + x] = seamed.data[y * 160 + x];
            }
        }
        let s = score(&p);
        assert!(
            s.consistent_tiles <= 0.5,
            "consistent {:.2}",
            s.consistent_tiles
        );
    }

    #[test]
    fn small_image_is_inconclusive() {
        let img = image::GrayImage::from_pixel(8, 8, image::Luma([200u8]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        let a = Asset::from_bytes(out.into_inner(), None).unwrap();
        let ev = LatticeSource::default().assess(&a).unwrap();
        assert_eq!(ev.status, Status::Inconclusive);
    }
}
