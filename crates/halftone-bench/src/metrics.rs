//! ROC / TPR@FPR from raw statistics. Pure functions, easy to test.

/// Threshold giving at most `fpr` false positives on `negatives` (higher = more suspicious).
pub fn threshold_at_fpr(negatives: &[f64], fpr: f64) -> f64 {
    let mut n: Vec<f64> = negatives.to_vec();
    n.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = ((n.len() as f64) * fpr).floor() as usize;
    // Threshold just above the k-th highest negative.
    match n.get(k) {
        Some(&v) => v + f64::EPSILON,
        None => f64::INFINITY,
    }
}

/// Fraction of `positives` at or above `threshold`.
pub fn tpr(positives: &[f64], threshold: f64) -> f64 {
    if positives.is_empty() {
        return 0.0;
    }
    positives.iter().filter(|&&p| p >= threshold).count() as f64 / positives.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpr_at_fpr_separable() {
        let neg: Vec<f64> = (0..100).map(|i| i as f64 / 100.0).collect();
        let pos: Vec<f64> = (0..100).map(|i| 1.0 + i as f64 / 100.0).collect();
        let t = threshold_at_fpr(&neg, 0.01);
        assert!(t > 0.98 && t <= 1.0);
        assert_eq!(tpr(&pos, t), 1.0);
    }
}
