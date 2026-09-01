//! Double JPEG compression: periodicity in quantized-coefficient histograms.
//!
//! Why it works (Popescu & Farid 2004; Lukáš & Fridrich 2003): a JPEG that was decoded
//! and re-encoded was quantized twice, with two different step sizes. The composition
//! of two quantizers is not a quantizer — it leaves a comb of periodically empty or
//! doubly-populated bins in the histogram of each DCT coefficient. A single
//! compression produces a smooth, unimodal histogram that is monotone outward from
//! zero. We walk the histograms of the first six luma AC coefficients outward and count
//! bins that dip far below a populated bin further out (gaps) or jump far above their
//! inner neighbour (spikes); a single quantization produces neither.
//!
//! What it catches: "this was edited/re-saved" — orthogonal to which encoder was used.
//! Blind spots, stated in the rationale: re-saving at the *same* quality is nearly
//! idempotent; a coarser second quantization hides the first; heavily textured or
//! very small images give noisy histograms; progressive JPEGs are not decoded yet.

use halftone_core::{
    Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Statistic, Status,
};
use serde::Serialize;

use crate::jpeg::coeffs::{self, ComponentCoeffs};

/// Half-width of the coefficient histogram (bins cover `-H..=H`).
const H: i32 = 64;
/// Minimum number of blocks for a usable histogram.
const MIN_BLOCKS: usize = 1024;
/// Zigzag positions examined (first six AC coefficients).
const POSITIONS: [usize; 6] = [1, 2, 3, 4, 5, 6];
/// A bin only counts as a candidate when the outward reference count is at least this,
/// so Poisson noise in the far tails does not register as gaps.
const N_MIN: f64 = 30.0;
/// How far outward to look for a rebound.
const WINDOW: usize = 3;
/// Minimum candidates per position for the fraction to be meaningful.
const MIN_CANDIDATES: usize = 6;

/// Provisional decision threshold on the anomaly fraction. Set from synthetic
/// single- vs double-compressed sets during development (singles ≤ 0.12, finer-second
/// doubles ≥ 0.7 on those sets); **not** a calibrated false-positive rate.
pub const PROVISIONAL_THRESHOLD: f64 = 0.3;

/// Per-position result.
#[derive(Debug, Clone, Serialize)]
pub struct PositionScore {
    /// Zigzag position.
    pub position: usize,
    /// Bins examined (outward reference ≥ `N_MIN`).
    pub candidates: usize,
    /// Bins that dip below 25% of a populated bin further out (comb gaps).
    pub gaps: usize,
    /// Bins that rise above 2.5× their inner neighbour (comb peaks).
    pub spikes: usize,
    /// `(gaps + spikes) / candidates`, or `None` if too few candidates.
    pub fraction: Option<f64>,
}

/// Summary statistic over positions.
#[derive(Debug, Clone, Serialize)]
pub struct DqStats {
    /// Per-position scores.
    pub positions: Vec<PositionScore>,
    /// Aggregate: mean of the three highest per-position fractions.
    pub score: f64,
    /// Blocks examined.
    pub blocks: usize,
}

/// Walk one side of a symmetric histogram outward from the zero bin. A single
/// quantization gives a monotone tail, so any bin that dips well below a populated bin
/// further out (a gap), or rises well above its inner neighbour (a spike), is an
/// anomaly. Returns (candidates, gaps, spikes).
fn side_anomalies(side: &[f64]) -> (usize, usize, usize) {
    let (mut cand, mut gaps, mut spikes) = (0usize, 0usize, 0usize);
    for k in 1..side.len().saturating_sub(1) {
        let outer = side[k + 1..(k + 1 + WINDOW).min(side.len())]
            .iter()
            .cloned()
            .fold(0.0, f64::max);
        let inner = side[k - 1];
        if outer >= N_MIN {
            cand += 1;
            if side[k] < 0.25 * outer {
                gaps += 1;
            }
        }
        if k >= 2 && inner >= N_MIN && side[k] > 2.5 * inner {
            spikes += 1;
        }
    }
    (cand, gaps, spikes)
}

/// Compute the double-quantization statistic for one component.
pub fn dq_stats(c: &ComponentCoeffs) -> DqStats {
    let n_bins = (2 * H + 1) as usize;
    let mut positions = Vec::with_capacity(POSITIONS.len());
    for &p in &POSITIONS {
        let mut hist = vec![0f64; n_bins];
        for blk in &c.blocks {
            let v = blk[p] as i32;
            if (-H..=H).contains(&v) {
                hist[(v + H) as usize] += 1.0;
            }
        }
        let zero = H as usize;
        let pos: Vec<f64> = hist[zero..].to_vec();
        let neg: Vec<f64> = hist[..=zero].iter().rev().cloned().collect();
        let (c1, g1, s1) = side_anomalies(&pos);
        let (c2, g2, s2) = side_anomalies(&neg);
        let (candidates, gaps, spikes) = (c1 + c2, g1 + g2, s1 + s2);
        let fraction = if candidates >= MIN_CANDIDATES {
            Some((gaps + spikes) as f64 / candidates as f64)
        } else {
            None
        };
        positions.push(PositionScore {
            position: p,
            candidates,
            gaps,
            spikes,
            fraction,
        });
    }
    let mut fr: Vec<f64> = positions.iter().filter_map(|p| p.fraction).collect();
    fr.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<f64> = fr.into_iter().take(3).collect();
    let score = if top.is_empty() {
        0.0
    } else {
        top.iter().sum::<f64>() / top.len() as f64
    };
    DqStats {
        positions,
        score,
        blocks: c.blocks.len(),
    }
}

/// Double-compression evidence source.
///
/// `Present` = periodic comb detected above the threshold; `Absent` = no comb;
/// `Inconclusive` = not decodable (progressive/arithmetic) or too small.
#[derive(Debug)]
pub struct DoubleCompression {
    /// Decision threshold on the aggregate anomaly fraction.
    pub threshold: f64,
}

impl Default for DoubleCompression {
    fn default() -> Self {
        Self {
            threshold: PROVISIONAL_THRESHOLD,
        }
    }
}

impl EvidenceSource for DoubleCompression {
    fn id(&self) -> SourceId {
        SourceId {
            name: "jpeg_double".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image && a.mime == "image/jpeg"
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
        let coeffs = match coeffs::decode(&a.bytes) {
            Ok(c) => c,
            Err(e) => {
                return Ok(inconclusive(
                    format!(
                    "Coefficient extraction not possible ({e}); double-compression test skipped."
                ),
                    serde_json::json!({ "error": e }),
                ))
            }
        };
        let luma = match coeffs.luma() {
            Some(l) if l.blocks.len() >= MIN_BLOCKS => l,
            _ => {
                return Ok(inconclusive(
                    "Image too small for a stable coefficient histogram; double-compression test skipped."
                        .into(),
                    serde_json::json!({ "blocks": coeffs.luma().map(|l| l.blocks.len()) }),
                ))
            }
        };
        let stats = dq_stats(luma);
        let present = stats.score >= self.threshold;
        let status = if present {
            Status::Present
        } else {
            Status::Absent
        };
        let rationale = if present {
            format!(
                "Luma DCT-coefficient histograms show a periodic comb (anomaly fraction {:.2} ≥ {:.2}): \
                 the image was JPEG-compressed at least twice with different quantizers — it was \
                 decoded and re-saved after its first encoding. Says nothing about what the edit \
                 was. Threshold is provisional, not a calibrated false-positive rate.",
                stats.score, self.threshold
            )
        } else {
            format!(
                "No double-quantization comb in luma coefficient histograms (anomaly fraction {:.2} < \
                 {:.2}). Consistent with a single encoding — but a re-save at the same quality, or \
                 a coarser second quantization, would also look like this.",
                stats.score, self.threshold
            )
        };
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: Some(Statistic {
                name: "dq_anomaly_fraction".into(),
                value: stats.score,
                null_model: "single-quantized baseline JPEG: luma coefficient histograms are \
                             unimodal and monotone outward from zero (fraction ≈ 0–0.12 on dev sets)"
                    .into(),
                p_value: None,
                threshold: Some(self.threshold),
            }),
            calibration: None,
            rationale,
            details: serde_json::json!({
                "score": stats.score,
                "blocks": stats.blocks,
                "positions": stats.positions,
                "width": coeffs.width,
                "height": coeffs.height,
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(blocks: Vec<[i16; 64]>) -> ComponentCoeffs {
        ComponentCoeffs {
            id: 1,
            h: 1,
            v: 1,
            blocks_w: 1,
            blocks_h: blocks.len(),
            blocks,
        }
    }

    /// Laplacian-ish single-quantized draws vs. the same values quantized a second time
    /// with a different step, which empties every other bin.
    #[test]
    fn synthetic_double_quantization_scores_higher() {
        let mut seed = 12345u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let draw = |r: u64| -> i32 {
            // Symmetric geometric-ish distribution, width ~8.
            let mag = ((r % 1000) as f64 / 1000.0).ln().abs() * 6.0;
            let s = if r & 1 == 0 { 1 } else { -1 };
            s * mag.round() as i32
        };
        let mut single = Vec::new();
        let mut double = Vec::new();
        for _ in 0..5000 {
            let mut a = [0i16; 64];
            let mut b = [0i16; 64];
            for p in 1..=6 {
                let v = draw(rnd());
                a[p] = v as i16;
                // First quantized with step 2 (values*2 in the finer domain), then with step 1.
                b[p] = ((v as f64 / 2.0).round() * 2.0) as i16;
            }
            single.push(a);
            double.push(b);
        }
        let s1 = dq_stats(&comp(single)).score;
        let s2 = dq_stats(&comp(double)).score;
        assert!(s1 < 0.15, "single {s1}");
        assert!(s2 > 0.4 && s2 > 3.0 * s1, "double {s2} vs single {s1}");
    }
}
