//! Robust statistics for anomaly and outlier detection: median, MAD, k from sensitivity.

pub fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() { return None; }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// Median absolute deviation scaled to a normal σ (×1.4826), with a floor so flat baselines still
/// leave room for noise: 5 % of the median or half the Poisson noise (√median), whichever is larger.
pub fn mad(values: &[f64], med: f64) -> f64 {
    let mut d: Vec<f64> = values.iter().map(|x| (x - med).abs()).collect();
    let m = median(&mut d).unwrap_or(0.0) * 1.4826;
    m.max(med.abs() * 0.05).max(med.abs().sqrt() * 0.5).max(1e-9)
}

/// σ multiplier for a 1..5 sensitivity (1 = very loose, 5 = tight).
pub fn k_for(sensitivity: i32) -> f64 {
    match sensitivity.clamp(1, 5) { 1 => 6.0, 2 => 4.5, 3 => 3.5, 4 => 2.5, _ => 2.0 }
}

/// Anomaly verdict: deviation of `value` from the history in σ units; None when no baseline.
pub fn anomaly(history: &[f64], value: f64, sensitivity: i32, min_value: f64) -> Option<(bool, f64, f64)> {
    if history.len() < 3 { return None; }
    let mut h = history.to_vec();
    let med = median(&mut h)?;
    let band = mad(history, med) * k_for(sensitivity);
    let fired = (value - med).abs() > band && value.abs() >= min_value;
    Some((fired, med, band))
}

/// Outlier verdict per group: which groups sit beyond k·MAD of all groups' values.
pub fn outliers(values: &[(String, f64)], sensitivity: i32) -> Vec<(String, f64, f64)> {
    if values.len() < 3 { return vec![]; }
    let mut all: Vec<f64> = values.iter().map(|(_, v)| *v).collect();
    let Some(med) = median(&mut all) else { return vec![] };
    let raw: Vec<f64> = values.iter().map(|(_, v)| *v).collect();
    let band = mad(&raw, med) * k_for(sensitivity);
    values.iter().filter(|(_, v)| (v - med).abs() > band).map(|(k, v)| (k.clone(), *v, med)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn median_and_mad() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [1.0, 2.0, 3.0, 4.0]), Some(2.5));
        assert!(mad(&[10.0, 10.0, 10.0], 10.0) >= 1.0);
    }
    #[test]
    fn anomaly_fires_on_spike_not_noise() {
        let hist = [100.0, 104.0, 98.0, 101.0, 99.0, 103.0, 97.0];
        assert!(anomaly(&hist, 400.0, 3, 0.0).unwrap().0);
        assert!(!anomaly(&hist, 106.0, 3, 0.0).unwrap().0);
        assert!(!anomaly(&hist, 400.0, 3, 1000.0).unwrap().0, "min_value gate");
        assert!(anomaly(&[1.0, 2.0], 50.0, 3, 0.0).is_none(), "needs history");
    }
    #[test]
    fn outlier_group() {
        let v = vec![("a".into(), 0.01), ("b".into(), 0.02), ("c".into(), 0.015), ("chaos".into(), 0.9), ("e".into(), 0.012)];
        let o = outliers(&v, 3);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].0, "chaos");
    }
}
