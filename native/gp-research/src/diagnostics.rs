use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Average tied ranks; undefined/nonfinite or constant rankings have no IC.
pub fn spearman(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 || x.iter().chain(y).any(|v| !v.is_finite()) {
        return None;
    }
    fn ranks(values: &[f64]) -> Vec<f64> {
        let mut order: Vec<usize> = (0..values.len()).collect();
        order.sort_by(|a, b| values[*a].total_cmp(&values[*b]));
        let mut out = vec![0.0; values.len()];
        let mut i = 0;
        while i < order.len() {
            let mut j = i + 1;
            while j < order.len() && values[order[i]] == values[order[j]] {
                j += 1;
            }
            for k in i..j {
                out[order[k]] = (i + j - 1) as f64 / 2.0;
            }
            i = j;
        }
        out
    }
    let a = ranks(x);
    let b = ranks(y);
    let mean = (x.len() - 1) as f64 / 2.0;
    let aa = a.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
    let bb = b.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
    if aa == 0.0 || bb == 0.0 {
        None
    } else {
        Some(
            a.iter()
                .zip(b)
                .map(|(a, b)| (a - mean) * (b - mean))
                .sum::<f64>()
                / (aa * bb).sqrt(),
        )
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiagnosticSummary {
    pub mean_rank_ic: Option<f64>,
    pub valid_dates: usize,
    pub samples: usize,
    pub unavailable_labels: usize,
    pub quintile_returns: [Option<f64>; 5],
    pub top10_excess: Option<f64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub factors: BTreeMap<String, DiagnosticSummary>,
    pub pairwise_factor_correlation: BTreeMap<String, DiagnosticSummary>,
    pub mean_selection_jaccard: Option<f64>,
}
#[derive(Default)]
pub(crate) struct DiagnosticAccumulator {
    pub report: DiagnosticsReport,
    counts: BTreeMap<String, [usize; 5]>,
    top_counts: BTreeMap<String, usize>,
}
impl DiagnosticAccumulator {
    pub fn date(&mut self, key: &str, scores: &[f64], labels: &[f64], missing: usize) {
        let s = self.report.factors.entry(key.into()).or_default();
        s.samples += labels.len();
        s.unavailable_labels += missing;
        if let Some(ic) = spearman(scores, labels) {
            s.mean_rank_ic = Some(s.mean_rank_ic.unwrap_or(0.0) + ic);
            s.valid_dates += 1;
        }
        if labels.is_empty() {
            return;
        }
        // Tied scores share the same quintile and fractional Top10 membership.
        // This prevents symbol order from manufacturing an apparent excess.
        let mut order: Vec<usize> = (0..scores.len()).collect();
        order.sort_by(|a, b| scores[*a].total_cmp(&scores[*b]));
        let mut qsum = [0.0; 5];
        let mut qn = [0usize; 5];
        let mut top_sum = 0.0;
        let mut top_n = 0.0;
        let mut i = 0;
        while i < order.len() {
            let mut j = i + 1;
            while j < order.len() && scores[order[j]] == scores[order[i]] {
                j += 1;
            }
            let quintile = (((i + j - 1) as f64 / 2.0 * 5.0 / scores.len() as f64) as usize).min(4);
            let weight = ((j as f64 - (scores.len().saturating_sub(10)) as f64)
                .clamp(0.0, (j - i) as f64))
                / (j - i) as f64;
            for k in i..j {
                qsum[quintile] += labels[order[k]];
                qn[quintile] += 1;
                top_sum += labels[order[k]] * weight;
                top_n += weight;
            }
            i = j;
        }
        let counts = self.counts.entry(key.into()).or_default();
        for q in 0..5 {
            if qn[q] > 0 {
                s.quintile_returns[q] =
                    Some(s.quintile_returns[q].unwrap_or(0.0) + qsum[q] / qn[q] as f64);
                counts[q] += 1;
            }
        }
        if top_n > 0.0 {
            s.top10_excess = Some(
                s.top10_excess.unwrap_or(0.0) + top_sum / top_n
                    - labels.iter().sum::<f64>() / labels.len() as f64,
            );
            *self.top_counts.entry(key.into()).or_default() += 1;
        }
    }
    pub fn pair(&mut self, key: &str, a: &[f64], b: &[f64]) {
        let s = self
            .report
            .pairwise_factor_correlation
            .entry(key.into())
            .or_default();
        s.samples += a.len();
        if let Some(ic) = spearman(a, b) {
            s.mean_rank_ic = Some(s.mean_rank_ic.unwrap_or(0.0) + ic);
            s.valid_dates += 1;
        }
    }
    pub fn finish(mut self) -> DiagnosticsReport {
        for (key, s) in &mut self.report.factors {
            if s.valid_dates > 0 {
                s.mean_rank_ic = s.mean_rank_ic.map(|v| v / s.valid_dates as f64);
            }
            for q in 0..5 {
                if let Some(counts) = self.counts.get(key) {
                    if counts[q] > 0 {
                        s.quintile_returns[q] = s.quintile_returns[q].map(|v| v / counts[q] as f64);
                    }
                }
            }
            if let Some(n) = self.top_counts.get(key) {
                s.top10_excess = s.top10_excess.map(|v| v / *n as f64);
            }
        }
        for s in self.report.pairwise_factor_correlation.values_mut() {
            if s.valid_dates > 0 {
                s.mean_rank_ic = s.mean_rank_ic.map(|v| v / s.valid_dates as f64);
            }
        }
        self.report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ties_and_constants() {
        assert_eq!(spearman(&[1., 1., 2.], &[3., 3., 4.]), Some(1.));
        assert_eq!(spearman(&[1., 1.], &[1., 2.]), None);
        assert_eq!(spearman(&[f64::NAN, 1.], &[1., 2.]), None);
    }
    #[test]
    fn equal_date_weight_and_tied_top() {
        let mut a = DiagnosticAccumulator::default();
        a.date("x", &[1., 1., 1.], &[1., 2., 3.], 7);
        a.date("x", &[1., 2.], &[2., 1.], 0);
        let r = a.finish();
        let s = &r.factors["x"];
        assert_eq!(s.samples, 5);
        assert_eq!(s.unavailable_labels, 7);
        assert_eq!(s.valid_dates, 1);
        assert_eq!(s.mean_rank_ic, Some(-1.));
        assert_eq!(s.top10_excess, Some(0.));
    }
}
