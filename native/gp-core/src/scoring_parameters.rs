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
