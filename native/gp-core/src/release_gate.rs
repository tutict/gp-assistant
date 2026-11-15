use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AdaptiveReleaseGateInput {
    pub release_configuration_qualified: Option<bool>,
    pub legacy_annualized_return: Option<f64>,
    pub adaptive_annualized_return: Option<f64>,
    pub legacy_max_drawdown: Option<f64>,
    pub adaptive_max_drawdown: Option<f64>,
    pub legacy_precision_at_10: Option<f64>,
    pub adaptive_precision_at_10: Option<f64>,
    pub max_primary_industry_count: Option<usize>,
    pub average_adjacent_jaccard: Option<f64>,
    pub five_run_unique_coverage: Option<usize>,
    pub first_run_millis: Option<u64>,
    pub cached_run_millis: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveReleaseGateCheck {
    pub key: String,
    pub passed: bool,
    pub actual: Option<f64>,
    pub requirement: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveReleaseGateReport {
    pub passed: bool,
    pub checks: Vec<AdaptiveReleaseGateCheck>,
}

pub fn evaluate_adaptive_release_gate(
    input: &AdaptiveReleaseGateInput,
) -> AdaptiveReleaseGateReport {
    let mut checks = Vec::new();
    let mut push = |key: &str, actual: Option<f64>, passed: bool, requirement: &str| {
        checks.push(AdaptiveReleaseGateCheck {
            key: key.to_string(),
            passed,
            actual,
            requirement: requirement.to_string(),
        });
    };
    push(
        "release_configuration_qualified",
        input
            .release_configuration_qualified
            .map(|value| if value { 1.0 } else { 0.0 }),
        input.release_configuration_qualified == Some(true),
        "auto mode, full universe, top 10, monthly, >=60 strict out-of-sample folds",
    );
    let return_delta = input
        .adaptive_annualized_return
        .zip(input.legacy_annualized_return)
        .map(|(adaptive, legacy)| adaptive - legacy);
    push(
        "annualized_return_delta",
        return_delta,
        return_delta.is_some_and(|value| value + 1e-12 >= -0.01),
        "adaptive annualized return no worse than legacy by more than 1 percentage point",
    );
    let drawdown_delta = input
        .adaptive_max_drawdown
        .zip(input.legacy_max_drawdown)
        .map(|(adaptive, legacy)| adaptive - legacy);
    push(
        "max_drawdown_delta",
        drawdown_delta,
        drawdown_delta.is_some_and(|value| value + 1e-12 >= -0.01),
        "adaptive drawdown no worse than legacy by more than 1 percentage point",
    );
    let precision_delta = input
        .adaptive_precision_at_10
        .zip(input.legacy_precision_at_10)
        .map(|(adaptive, legacy)| adaptive - legacy);
    push(
        "precision_at_10_delta",
        precision_delta,
        precision_delta.is_some_and(|value| value + 1e-12 >= -0.03),
        "adaptive Precision@10 no worse than legacy by more than 3 percentage points",
    );
    push(
        "max_primary_industry_count",
        input.max_primary_industry_count.map(|value| value as f64),
        input
            .max_primary_industry_count
            .is_some_and(|value| value <= 3),
        "no primary industry exceeds 3/10",
    );
    push(
        "average_adjacent_jaccard",
        input.average_adjacent_jaccard,
        input
            .average_adjacent_jaccard
            .is_some_and(|value| value <= 0.70),
        "average adjacent primary Jaccard <= 0.70",
    );
    let coverage = input.five_run_unique_coverage.map(|value| value as f64);
    push(
        "five_run_unique_coverage",
        coverage,
        input
            .five_run_unique_coverage
            .is_some_and(|value| value >= 20),
        "five consecutive primary+exploration runs cover at least 20 symbols",
    );
    let first_run = input.first_run_millis.map(|value| value as f64);
    push(
        "first_run_millis",
        first_run,
        input.first_run_millis.is_some_and(|value| value <= 20_000),
        "first network run <= 20000 ms",
    );
    let cached_run = input.cached_run_millis.map(|value| value as f64);
    push(
        "cached_run_millis",
        cached_run,
        input.cached_run_millis.is_some_and(|value| value <= 2_000),
        "same-day cached run <= 2000 ms",
    );
    AdaptiveReleaseGateReport {
        passed: checks.iter().all(|check| check.passed),
        checks,
    }
}
