use std::{collections::BTreeMap, sync::OnceLock};

use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

pub const METRIC_CONTRACT_VERSION: u32 = 2;
pub const SCORING_REVISION: &str = "unit-rank-fix/v1";
pub(crate) const FACTOR_KEYS: [&str; 8] = [
    "quality",
    "valuation",
    "range_setup",
    "trend",
    "relative_strength",
    "liquidity",
    "risk",
    "stability",
];

pub fn scoring_implementation_fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            crate::source_text_fingerprint(
                b"gp-core-scoring-implementation/v2-lf\0",
                &[
                    ("lib.rs", include_bytes!("lib.rs").as_slice()),
                    ("error.rs", include_bytes!("error.rs").as_slice()),
                    ("dates.rs", include_bytes!("dates.rs").as_slice()),
                    ("numeric.rs", include_bytes!("numeric.rs").as_slice()),
                    (
                        "market_data.rs",
                        include_bytes!("market_data.rs").as_slice(),
                    ),
                    (
                        "capital_evidence.rs",
                        include_bytes!("capital_evidence.rs").as_slice(),
                    ),
                    (
                        "capital_evidence/types.rs",
                        include_bytes!("capital_evidence/types.rs").as_slice(),
                    ),
                    (
                        "data_source.rs",
                        include_bytes!("data_source.rs").as_slice(),
                    ),
                    ("demo_data.rs", include_bytes!("demo_data.rs").as_slice()),
                    ("screening.rs", include_bytes!("screening.rs").as_slice()),
                    (
                        "screening/types.rs",
                        include_bytes!("screening/types.rs").as_slice(),
                    ),
                    (
                        "screening/criteria.rs",
                        include_bytes!("screening/criteria.rs").as_slice(),
                    ),
                    (
                        "screening/classification.rs",
                        include_bytes!("screening/classification.rs").as_slice(),
                    ),
                    (
                        "screening/scoring.rs",
                        include_bytes!("screening/scoring.rs").as_slice(),
                    ),
                    (
                        "screening/grouping.rs",
                        include_bytes!("screening/grouping.rs").as_slice(),
                    ),
                    (
                        "screening/service.rs",
                        include_bytes!("screening/service.rs").as_slice(),
                    ),
                    (
                        "graph_compat.rs",
                        include_bytes!("graph_compat.rs").as_slice(),
                    ),
                    (
                        "graph_compat/types.rs",
                        include_bytes!("graph_compat/types.rs").as_slice(),
                    ),
                    (
                        "graph_compat/service.rs",
                        include_bytes!("graph_compat/service.rs").as_slice(),
                    ),
                    ("text.rs", include_bytes!("text.rs").as_slice()),
                    (
                        "backtest/request.rs",
                        include_bytes!("backtest/request.rs").as_slice(),
                    ),
                    (
                        "backtest/types.rs",
                        include_bytes!("backtest/types.rs").as_slice(),
                    ),
                    (
                        "backtest/history.rs",
                        include_bytes!("backtest/history.rs").as_slice(),
                    ),
                    (
                        "backtest/selection.rs",
                        include_bytes!("backtest/selection.rs").as_slice(),
                    ),
                    (
                        "backtest/simulation.rs",
                        include_bytes!("backtest/simulation.rs").as_slice(),
                    ),
                    (
                        "backtest/metrics.rs",
                        include_bytes!("backtest/metrics.rs").as_slice(),
                    ),
                    (
                        "backtest/service.rs",
                        include_bytes!("backtest/service.rs").as_slice(),
                    ),
                    (
                        "release_gate.rs",
                        include_bytes!("release_gate.rs").as_slice(),
                    ),
                    ("backtest.rs", include_bytes!("backtest.rs").as_slice()),
                    (
                        "trend/types.rs",
                        include_bytes!("trend/types.rs").as_slice(),
                    ),
                    (
                        "trend/indicators.rs",
                        include_bytes!("trend/indicators.rs").as_slice(),
                    ),
                    (
                        "trend/service.rs",
                        include_bytes!("trend/service.rs").as_slice(),
                    ),
                    (
                        "trend_screen/types.rs",
                        include_bytes!("trend_screen/types.rs").as_slice(),
                    ),
                    (
                        "trend_screen/rules.rs",
                        include_bytes!("trend_screen/rules.rs").as_slice(),
                    ),
                    (
                        "trend_screen/service.rs",
                        include_bytes!("trend_screen/service.rs").as_slice(),
                    ),
                    (
                        "observation/types.rs",
                        include_bytes!("observation/types.rs").as_slice(),
                    ),
                    (
                        "observation/financial.rs",
                        include_bytes!("observation/financial.rs").as_slice(),
                    ),
                    (
                        "observation/capital.rs",
                        include_bytes!("observation/capital.rs").as_slice(),
                    ),
                    (
                        "observation/service.rs",
                        include_bytes!("observation/service.rs").as_slice(),
                    ),
                    ("trend.rs", include_bytes!("trend.rs").as_slice()),
                    (
                        "trend_screen.rs",
                        include_bytes!("trend_screen.rs").as_slice(),
                    ),
                    (
                        "observation.rs",
                        include_bytes!("observation.rs").as_slice(),
                    ),
                    (
                        "adaptive_screen.rs",
                        include_bytes!("adaptive_screen.rs").as_slice(),
                    ),
                    ("rates.rs", include_bytes!("rates.rs").as_slice()),
                    (
                        "industry_rank.rs",
                        include_bytes!("industry_rank.rs").as_slice(),
                    ),
                    (
                        "scoring_parameters.rs",
                        include_bytes!("scoring_parameters.rs").as_slice(),
                    ),
                    ("volatility.rs", include_bytes!("volatility.rs").as_slice()),
                    (
                        "agent/types.rs",
                        include_bytes!("agent/types.rs").as_slice(),
                    ),
                    (
                        "agent/registry.rs",
                        include_bytes!("agent/registry.rs").as_slice(),
                    ),
                    (
                        "agent/intent.rs",
                        include_bytes!("agent/intent.rs").as_slice(),
                    ),
                    (
                        "agent/response.rs",
                        include_bytes!("agent/response.rs").as_slice(),
                    ),
                    (
                        "agent/stock_summary.rs",
                        include_bytes!("agent/stock_summary.rs").as_slice(),
                    ),
                    (
                        "agent/service.rs",
                        include_bytes!("agent/service.rs").as_slice(),
                    ),
                    (
                        "agent/stream.rs",
                        include_bytes!("agent/stream.rs").as_slice(),
                    ),
                    (
                        "agent/context.rs",
                        include_bytes!("agent/context.rs").as_slice(),
                    ),
                    (
                        "mobile_skill/types.rs",
                        include_bytes!("mobile_skill/types.rs").as_slice(),
                    ),
                    (
                        "mobile_skill/rules.rs",
                        include_bytes!("mobile_skill/rules.rs").as_slice(),
                    ),
                    (
                        "mobile_skill/service.rs",
                        include_bytes!("mobile_skill/service.rs").as_slice(),
                    ),
                    (
                        "json_api/requests.rs",
                        include_bytes!("json_api/requests.rs").as_slice(),
                    ),
                    (
                        "json_api/adapters.rs",
                        include_bytes!("json_api/adapters.rs").as_slice(),
                    ),
                    ("agent.rs", include_bytes!("agent.rs").as_slice()),
                    (
                        "mobile_skill.rs",
                        include_bytes!("mobile_skill.rs").as_slice(),
                    ),
                    ("json_api.rs", include_bytes!("json_api.rs").as_slice()),
                    ("ffi.rs", include_bytes!("ffi.rs").as_slice()),
                    (
                        "source_fingerprint.rs",
                        include_bytes!("source_fingerprint.rs").as_slice(),
                    ),
                    ("Cargo.toml", include_bytes!("../Cargo.toml").as_slice()),
                    ("Cargo.lock", include_bytes!("../Cargo.lock").as_slice()),
                ],
            )
        })
        .as_str()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModeParameters {
    pub weights: BTreeMap<String, f64>,
    pub overheat_penalty: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoringParameters {
    pub algorithm_version: String,
    pub parameter_set_id: String,
    pub metric_contract_version: u32,
    pub scoring_revision: String,
    pub implementation_fingerprint: String,
    pub modes: BTreeMap<String, ModeParameters>,
}

fn builtin_modes() -> BTreeMap<String, ModeParameters> {
    [
        (
            "range",
            [0.24, 0.16, 0.22, 0.0, 0.14, 0.10, 0.14, 0.0],
            0.10,
        ),
        (
            "trend",
            [0.18, 0.10, 0.0, 0.30, 0.18, 0.10, 0.14, 0.0],
            0.10,
        ),
        (
            "defensive",
            [0.30, 0.22, 0.0, 0.0, 0.0, 0.12, 0.24, 0.12],
            0.06,
        ),
    ]
    .into_iter()
    .map(|(mode, weights, overheat_penalty)| {
        (
            mode.to_string(),
            ModeParameters {
                weights: FACTOR_KEYS
                    .into_iter()
                    .zip(weights)
                    .map(|(key, weight)| (key.to_string(), weight))
                    .collect(),
                overheat_penalty,
            },
        )
    })
    .collect()
}

pub fn fixed_scoring_parameters() -> ScoringParameters {
    ScoringParameters {
        algorithm_version: "adaptive_swing_v1".to_string(),
        parameter_set_id: "fixed-v1".to_string(),
        metric_contract_version: METRIC_CONTRACT_VERSION,
        scoring_revision: SCORING_REVISION.to_string(),
        implementation_fingerprint: scoring_implementation_fingerprint().to_string(),
        modes: builtin_modes(),
    }
}

pub fn validate_scoring_parameters(parameters: &ScoringParameters) -> CoreResult<()> {
    let invalid = |reason: &str| CoreError::new(format!("invalid scoring parameters: {reason}"));
    if parameters.metric_contract_version != METRIC_CONTRACT_VERSION {
        return Err(invalid("metric_contract_version mismatch"));
    }
    if parameters.scoring_revision != SCORING_REVISION {
        return Err(invalid("scoring_revision mismatch"));
    }
    if parameters.implementation_fingerprint != scoring_implementation_fingerprint() {
        return Err(invalid("implementation_fingerprint mismatch"));
    }
    let calibrated = match parameters.algorithm_version.as_str() {
        "adaptive_swing_v1" if parameters.parameter_set_id == "fixed-v1" => false,
        "adaptive_swing_v2"
            if !parameters.parameter_set_id.trim().is_empty()
                && parameters.parameter_set_id.trim() != "fixed-v1" =>
        {
            true
        }
        _ => {
            return Err(invalid(
                "algorithm_version or parameter_set_id is unsupported",
            ))
        }
    };
    let builtin = builtin_modes();
    if parameters.modes.len() != builtin.len() || parameters.modes.keys().ne(builtin.keys()) {
        return Err(invalid("exact range/trend/defensive modes required"));
    }
    for (name, mode) in &parameters.modes {
        let base = &builtin[name];
        if mode.weights.len() != base.weights.len() || mode.weights.keys().ne(base.weights.keys()) {
            return Err(invalid(
                "exact eight factor weights required; no unknown factors",
            ));
        }
        if mode.weights.values().any(|v| !v.is_finite() || *v < 0.0)
            || (mode.weights.values().sum::<f64>() - 1.0).abs() > 1e-9
        {
            return Err(invalid(
                "weights must be finite, nonnegative and sum to one",
            ));
        }
        if !mode.overheat_penalty.is_finite() || mode.overheat_penalty < 0.0 {
            return Err(invalid("overheat_penalty must be finite and nonnegative"));
        }
        if !calibrated {
            if mode != base {
                return Err(invalid("fixed-v1 requires builtin values"));
            }
        } else {
            for (key, weight) in &mode.weights {
                let baseline = base.weights[key];
                if (baseline == 0.0 && *weight != 0.0) || (*weight - baseline).abs() > 0.05 + 1e-12
                {
                    return Err(invalid("weight outside baseline support or +/-0.05 bound"));
                }
            }
            if (mode.overheat_penalty - base.overheat_penalty).abs() > 0.02 + 1e-12 {
                return Err(invalid("overheat_penalty outside +/-0.02 bound"));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_factor_vector(factors: &BTreeMap<String, f64>) -> CoreResult<()> {
    if factors.len() != FACTOR_KEYS.len() + 1
        || !FACTOR_KEYS
            .iter()
            .chain(std::iter::once(&"overheat_penalty"))
            .all(|key| factors.contains_key(*key))
        || factors
            .values()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(CoreError::new(
            "factor vector requires exact nine finite normalized factors",
        ));
    }
    Ok(())
}

/// Scores only complete normalized vectors and validated, supported parameter sets.
pub fn adaptive_score_factor_vector(
    factors: &BTreeMap<String, f64>,
    mode: &str,
    parameters: &ScoringParameters,
) -> CoreResult<f64> {
    validate_scoring_parameters(parameters)?;
    validate_factor_vector(factors)?;
    if mode != "transition" && !parameters.modes.contains_key(mode) {
        return Err(CoreError::new("unsupported adaptive scoring mode"));
    }
    Ok(score_factor_vector_validated(factors, mode, parameters))
}

pub(crate) fn score_factor_vector_validated(
    factors: &BTreeMap<String, f64>,
    mode: &str,
    parameters: &ScoringParameters,
) -> f64 {
    factor_contributions(factors, mode, parameters)
        .values()
        .sum::<f64>()
        .clamp(0.0, 20.0)
}

pub(crate) fn factor_contributions(
    factors: &BTreeMap<String, f64>,
    mode: &str,
    parameters: &ScoringParameters,
) -> BTreeMap<String, f64> {
    if mode == "transition" {
        let range = factor_contributions(factors, "range", parameters);
        let defensive = factor_contributions(factors, "defensive", parameters);
        return range
            .keys()
            .chain(defensive.keys())
            .map(|key| {
                (
                    key.clone(),
                    (range.get(key).copied().unwrap_or(0.0)
                        + defensive.get(key).copied().unwrap_or(0.0))
                        / 2.0,
                )
            })
            .collect();
    }
    let weights = &parameters.modes[mode];
    let mut values = weights
        .weights
        .iter()
        .filter(|(_, weight)| **weight > 0.0)
        .map(|(key, weight)| (key.clone(), factors[key] * weight * 20.0))
        .collect::<BTreeMap<_, _>>();
    values.insert(
        "overheat_penalty".to_string(),
        -factors["overheat_penalty"] * weights.overheat_penalty * 20.0,
    );
    let sum = values.values().sum::<f64>();
    if sum < 0.0 {
        values.insert("score_floor_adjustment".to_string(), -sum);
    }
    values
}
