use std::collections::{BTreeMap, HashMap};

pub(crate) fn is_market_board_label(value: &str) -> bool {
    matches!(
        value.trim(),
        "沪市A股" | "深市A股" | "创业板" | "科创板" | "北交所" | "沪主板" | "深主板"
    )
}

fn is_unknown(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "__unknown__" | "unknown" | "未知" | "未知行业" | "未分类" | "其他"
    )
}

/// Average tied percentile, shrunk towards neutral for small industry samples.
/// Non-industries always have neutral ranks; code only stabilizes output ordering.
pub(crate) fn industry_rank_values(
    rows: impl IntoIterator<Item = (String, String, f64)>,
) -> HashMap<String, (f64, usize)> {
    let mut groups = BTreeMap::<String, Vec<(String, f64)>>::new();
    for (code, industry, value) in rows {
        if value.is_finite() {
            groups
                .entry(industry.trim().to_string())
                .or_default()
                .push((code, value));
        }
    }
    let mut result = HashMap::new();
    for (industry, values) in &mut groups {
        values.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let n = values.len();
        let reliability = (n.saturating_sub(1) as f64 / 7.0).min(1.0);
        let neutral = is_unknown(industry) || is_market_board_label(industry) || n == 1;
        let mut start = 0;
        while start < n {
            let mut end = start + 1;
            while end < n && values[end].1 == values[start].1 {
                end += 1;
            }
            let rank = if neutral {
                0.5
            } else {
                let percentile = (start + end - 1) as f64 / 2.0 / (n - 1) as f64;
                0.5 + (percentile - 0.5) * reliability
            };
            for (code, _) in &values[start..end] {
                result.insert(code.clone(), (rank, n));
            }
            start = end;
        }
    }
    result
}
