use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateCheck {
    pub key: String,
    pub passed: bool,
    pub actual: Option<f64>,
    pub requirement: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GateReport {
    pub passed: bool,
    pub checks: Vec<GateCheck>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LatencyEvidence {
    pub label: String,
    pub cold_ms: Option<f64>,
    pub cached_ms: Option<f64>,
    pub repeated_samples: usize,
    pub industry_cap: usize,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GateEvidence {
    pub real_data: bool,
    pub full_universe: bool,
    pub quality_passed: bool,
    pub identities_match: bool,
    pub frozen_config: bool,
    pub complete_months: usize,
    pub candidate_annualized: Option<f64>,
    pub fixed_annualized: Option<f64>,
    pub candidate_drawdown: Option<f64>,
    pub fixed_drawdown: Option<f64>,
    pub candidate_turnover: Option<f64>,
    pub fixed_turnover: Option<f64>,
    pub cost_excess: [Option<f64>; 3],
    pub monthly_excess: Vec<f64>,
    pub complete_year_excess: Vec<f64>,
    pub latency: LatencyEvidence,
}
/// Circular six-month blocks, 2000 draws, central 95% percentile interval.
/// Explicit xorshift64* integer PRNG makes the seed and stream portable.
pub fn bootstrap_lower_bound(excess: &[f64]) -> Option<f64> {
    if excess.len() < 60 || excess.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut state = 20261029u64;
    let mut draws = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let mut sum = 0.0;
        let mut n = 0;
        while n < excess.len() {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let start = (state.wrapping_mul(2685821657736338717) % excess.len() as u64) as usize;
            for offset in 0..6 {
                if n == excess.len() {
                    break;
                }
                sum += excess[(start + offset) % excess.len()];
                n += 1;
            }
        }
        if !sum.is_finite() {
            return None;
        }
        draws.push(sum / excess.len() as f64);
    }
    draws.sort_by(f64::total_cmp);
    let pos = 0.025 * (draws.len() - 1) as f64;
    let i = pos.floor() as usize;
    let bound = draws[i] + (draws[i + 1] - draws[i]) * (pos - i as f64);
    bound.is_finite().then_some(bound)
}
/// Literal evidence can be tested independently. Registry promotion additionally
/// authenticates a calibration-owned receipt and re-derives these facts.
pub fn release_gate(e: &GateEvidence) -> GateReport {
    let mut checks = Vec::new();
    let mut add = |key: &str, passed: bool, actual: Option<f64>, requirement: &str| {
        checks.push(GateCheck {
            key: key.into(),
            passed: passed && actual.is_none_or(f64::is_finite),
            actual: actual.filter(|v| v.is_finite()),
            requirement: requirement.into(),
        });
    };
    add(
        "real_data",
        e.real_data,
        Some(f64::from(e.real_data)),
        "explicit real market provenance; fixtures cannot qualify",
    );
    add(
        "full_universe",
        e.full_universe,
        Some(f64::from(e.full_universe)),
        "full A-share universe evidence",
    );
    add(
        "data_quality",
        e.quality_passed,
        Some(f64::from(e.quality_passed)),
        "strict PIT quality passed",
    );
    add(
        "identities",
        e.identities_match,
        Some(f64::from(e.identities_match)),
        "matching current data/core/research/parameter identities",
    );
    add(
        "frozen_config",
        e.frozen_config,
        Some(f64::from(e.frozen_config)),
        "eligible frozen split and canonical config",
    );
    add(
        "complete_months",
        e.complete_months >= 60,
        Some(e.complete_months as f64),
        ">=60 complete OOS months",
    );
    let finite_pair =
        |a: Option<f64>, b: Option<f64>| a.zip(b).filter(|(a, b)| a.is_finite() && b.is_finite());
    let gain = finite_pair(e.candidate_annualized, e.fixed_annualized).map(|(a, b)| a - b);
    add(
        "annualized_gain",
        finite_pair(e.candidate_annualized, e.fixed_annualized).is_some_and(|(a, b)| a >= b + 0.01),
        gain,
        ">=0.01 net annualized gain over corrected fixed",
    );
    let dd = finite_pair(e.candidate_drawdown, e.fixed_drawdown).map(|(a, b)| a.abs() - b.abs());
    add(
        "drawdown",
        finite_pair(e.candidate_drawdown, e.fixed_drawdown)
            .is_some_and(|(a, b)| a.abs() <= b.abs() + 0.005),
        dd,
        "absolute drawdown degradation <=0.005",
    );
    let turn = finite_pair(e.candidate_turnover, e.fixed_turnover);
    add(
        "turnover",
        turn.is_some_and(|(a, b)| a >= 0.0 && b >= 0.0 && a <= b * 1.10),
        turn.map(|(a, b)| a - b * 1.10),
        "candidate <=fixed*1.10; zero fixed permits zero only",
    );
    for (i, key) in ["excess_10bps", "excess_25bps", "excess_50bps"]
        .iter()
        .enumerate()
    {
        add(
            key,
            e.cost_excess[i].is_some_and(|v| v > 0.0),
            e.cost_excess[i],
            "positive annualized excess over same candidate benchmark",
        );
    }
    let years = (!e.complete_year_excess.is_empty()
        && e.complete_year_excess.iter().all(|v| v.is_finite()))
    .then(|| {
        e.complete_year_excess.iter().filter(|v| **v > 0.0).count() as f64
            / e.complete_year_excess.len() as f64
    });
    add(
        "positive_year_windows",
        years.is_some_and(|v| v >= 0.60) && e.complete_year_excess.len() >= 5,
        years,
        ">=0.60 positive complete nonoverlapping 12-month windows",
    );
    let lower = bootstrap_lower_bound(&e.monthly_excess);
    add(
        "bootstrap",
        lower.is_some_and(|v| v > 0.0),
        lower,
        "circular6 block/2000/seed20261029 central95% lower mean excess >0",
    );
    add(
        "industry_cap",
        e.latency.industry_cap == 3,
        Some(e.latency.industry_cap as f64),
        "target industry cap 3",
    );
    let measured = e.latency.label == "local PIT replay" && e.latency.repeated_samples >= 3;
    add(
        "cold_latency",
        measured
            && e.latency
                .cold_ms
                .is_some_and(|v| (0.0..=20000.0).contains(&v)),
        e.latency.cold_ms,
        "measured same PIT scorer+read cold <=20000ms (local replay)",
    );
    add(
        "cached_latency",
        measured
            && e.latency
                .cached_ms
                .is_some_and(|v| (0.0..=2000.0).contains(&v)),
        e.latency.cached_ms,
        "measured repeated PIT scorer+read max <=2000ms (local replay)",
    );
    GateReport {
        passed: checks.iter().all(|c| c.passed),
        checks,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn good() -> GateEvidence {
        GateEvidence {
            real_data: true,
            full_universe: true,
            quality_passed: true,
            identities_match: true,
            frozen_config: true,
            complete_months: 60,
            candidate_annualized: Some(0.125),
            fixed_annualized: Some(0.1),
            candidate_drawdown: Some(-0.1),
            fixed_drawdown: Some(-0.1),
            candidate_turnover: Some(1.0),
            fixed_turnover: Some(1.0),
            cost_excess: [Some(0.01); 3],
            monthly_excess: vec![0.01; 60],
            complete_year_excess: vec![0.1; 5],
            latency: LatencyEvidence {
                label: "local PIT replay".into(),
                cold_ms: Some(20000.),
                cached_ms: Some(2000.),
                repeated_samples: 3,
                industry_cap: 3,
            },
        }
    }
    #[test]
    fn seeded_blocks_literal() {
        assert!((bootstrap_lower_bound(&[0.01; 60]).unwrap() - 0.01).abs() < 1e-12);
        assert!(bootstrap_lower_bound(&[-0.01; 60]).unwrap() < 0.0);
        let values: Vec<f64> = (0..60).map(|i| if i < 30 { 0.02 } else { -0.01 }).collect();
        assert_eq!(
            bootstrap_lower_bound(&values),
            bootstrap_lower_bound(&values)
        );
        assert_eq!(bootstrap_lower_bound(&[0.1; 59]), None);
    }
    #[test]
    fn every_gate_fails_closed() {
        let e = good();
        assert!(release_gate(&e).passed);
        let mut bads = Vec::new();
        macro_rules! bad {
            ($field:ident,$v:expr) => {{
                let mut b = e.clone();
                b.$field = $v;
                bads.push(b);
            }};
        }
        bad!(real_data, false);
        bad!(full_universe, false);
        bad!(quality_passed, false);
        bad!(identities_match, false);
        bad!(frozen_config, false);
        bad!(complete_months, 59);
        bad!(candidate_annualized, Some(0.109));
        bad!(candidate_drawdown, Some(-0.106));
        bad!(candidate_turnover, Some(1.10001));
        bad!(cost_excess, [Some(0.01), Some(0.0), Some(0.01)]);
        bad!(complete_year_excess, vec![0.1, 0.1, -0.1, -0.1, -0.1]);
        bad!(monthly_excess, vec![-0.1; 60]);
        bad!(candidate_annualized, None);
        bad!(candidate_drawdown, Some(f64::NAN));
        for b in bads {
            assert!(!release_gate(&b).passed);
        }
        let mut b = e;
        b.fixed_turnover = Some(0.);
        b.candidate_turnover = Some(0.);
        assert!(
            release_gate(&b)
                .checks
                .iter()
                .find(|c| c.key == "turnover")
                .unwrap()
                .passed
        );
        b.latency.cold_ms = Some(20000.1);
        assert!(!release_gate(&b).passed);
    }
    #[test]
    fn nonfinite_and_overflowed_evidence_is_never_positive() {
        let base = good();
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            for index in 0..3 {
                let mut e = base.clone();
                e.cost_excess[index] = Some(value);
                assert!(
                    !release_gate(&e)
                        .checks
                        .iter()
                        .find(|c| c.key == ["excess_10bps", "excess_25bps", "excess_50bps"][index])
                        .unwrap()
                        .passed
                );
            }
            let mut e = base.clone();
            e.candidate_annualized = Some(value);
            assert!(!release_gate(&e).passed);
            e = base.clone();
            e.latency.cold_ms = Some(value);
            assert!(!release_gate(&e).passed);
            e = base.clone();
            e.latency.cached_ms = Some(value);
            assert!(!release_gate(&e).passed);
            assert_eq!(bootstrap_lower_bound(&[value; 60]), None);
        }
        assert_eq!(
            bootstrap_lower_bound(&[f64::MAX / 2.; 60]),
            None,
            "finite inputs cannot yield an infinite confidence bound"
        );
    }
    #[test]
    fn inclusive_literal_boundaries_and_seed_stream() {
        let mut e = good();
        e.candidate_annualized = Some(0.11);
        e.candidate_drawdown = Some(-0.105);
        e.candidate_turnover = Some(1.10);
        e.complete_year_excess = vec![0.1, 0.1, 0.1, 0.0, -0.1];
        assert!(release_gate(&e).passed);
        let values: Vec<f64> = (0..60).map(|i| if i < 30 { 0.02 } else { -0.01 }).collect();
        assert!((bootstrap_lower_bound(&values).unwrap() - (-0.004)).abs() < 1e-15);
    }
    #[test]
    fn missing_cost_and_latency_evidence_fail_each_boundary() {
        let base = good();
        for index in 0..3 {
            for value in [None, Some(0.0), Some(-0.00001)] {
                let mut e = base.clone();
                e.cost_excess[index] = value;
                assert!(!release_gate(&e).passed);
            }
        }
        for latency in [
            LatencyEvidence {
                industry_cap: 2,
                ..base.latency.clone()
            },
            LatencyEvidence {
                label: "online".into(),
                ..base.latency.clone()
            },
            LatencyEvidence {
                repeated_samples: 2,
                ..base.latency.clone()
            },
            LatencyEvidence {
                cold_ms: None,
                ..base.latency.clone()
            },
            LatencyEvidence {
                cold_ms: Some(-0.01),
                ..base.latency.clone()
            },
            LatencyEvidence {
                cached_ms: Some(2000.001),
                ..base.latency.clone()
            },
            LatencyEvidence {
                cached_ms: None,
                ..base.latency.clone()
            },
        ] {
            let mut e = base.clone();
            e.latency = latency;
            assert!(!release_gate(&e).passed);
        }
        let mut e = base;
        e.monthly_excess.pop();
        assert!(!release_gate(&e).passed);
    }
}
