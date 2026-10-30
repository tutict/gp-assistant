use crate::*;
use chrono::{Datelike, Months, NaiveDate, Weekday};
use gp_core::ScoringParameters;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Instant};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CalibrationOptions {
    pub end_date: Option<NaiveDate>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationSplit {
    pub train_start: NaiveDate,
    pub train_end: NaiveDate,
    pub validation_start: NaiveDate,
    pub validation_end: NaiveDate,
    pub test_start: NaiveDate,
    pub test_end: NaiveDate,
    pub train_last_signal: NaiveDate,
    pub validation_last_signal: NaiveDate,
    pub purge_sessions: usize,
    pub training_months: usize,
    pub validation_months: usize,
    pub test_months: usize,
    pub warmup_bars: usize,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModeCalibration {
    pub calibrated: bool,
    pub effective_signal_days: usize,
    pub available_labels: usize,
    pub iterations: usize,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationCandidate {
    pub parameters: ScoringParameters,
    pub training_objective: Option<f64>,
    pub baseline_distance: f64,
    pub feasible: bool,
    pub annualized_excess_over_fixed: Option<f64>,
    pub evaluation: EvaluationReport,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub run_id: String,
    pub dataset_id: String,
    pub status: String,
    pub parameters: ScoringParameters,
    pub split: Option<CalibrationSplit>,
    pub data_fingerprint: String,
    pub implementation_fingerprint: String,
    pub research_fingerprint: String,
    pub parameter_fingerprint: String,
    pub issues: Vec<String>,
    pub data_quality: QualityReport,
    pub mode_calibration: BTreeMap<String, ModeCalibration>,
    pub diagnostics: DiagnosticsReport,
    pub validation_candidates: Vec<ValidationCandidate>,
    pub test: Option<EvaluationReport>,
    pub fixed_baseline: Option<EvaluationReport>,
    pub legacy_baseline: Option<EvaluationReport>,
    pub stress_tests: Vec<EvaluationReport>,
    pub latency: LatencyEvidence,
    pub gate: GateReport,
}
fn month_start(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()
}
fn month_end(d: NaiveDate) -> NaiveDate {
    month_start(d)
        .checked_add_months(Months::new(1))
        .unwrap()
        .pred_opt()
        .unwrap()
}
fn last_weekday(d: NaiveDate) -> NaiveDate {
    let mut d = month_end(d);
    while matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
        d = d.pred_opt().unwrap();
    }
    d
}
fn month_count(a: NaiveDate, b: NaiveDate) -> usize {
    ((b.year() - a.year()) * 12 + b.month() as i32 - a.month() as i32 + 1).max(0) as usize
}
/// Calendar cutoffs never shorten validation/test to accommodate short data.
pub fn compute_split(
    calendar: &[NaiveDate],
    end_date: Option<NaiveDate>,
) -> ResearchResult<CalibrationSplit> {
    let latest = *calendar
        .last()
        .ok_or_else(|| ResearchError::new("insufficient_data", "empty calendar"))?;
    let latest_complete = if latest >= last_weekday(latest) {
        month_end(latest)
    } else {
        month_start(latest).pred_opt().unwrap()
    };
    let cap = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
    let end = end_date.unwrap_or(latest_complete.min(cap));
    if end != month_end(end) || end > latest_complete {
        return Err(ResearchError::new(
            "config",
            "end_date must be a complete supported month end",
        ));
    }
    let test_start = month_start(end)
        .checked_sub_months(Months::new(59))
        .ok_or_else(|| ResearchError::new("config", "date underflow"))?;
    let validation_start = test_start.checked_sub_months(Months::new(12)).unwrap();
    let validation_end = test_start.pred_opt().unwrap();
    let first = *calendar.get(120).ok_or_else(|| {
        ResearchError::new(
            "insufficient_data",
            "at least120 prior warmup bars required",
        )
    })?;
    let train_start = if first.day() == 1 {
        first
    } else {
        month_start(first)
            .checked_add_months(Months::new(1))
            .unwrap()
    };
    let train_end = validation_start.pred_opt().unwrap();
    let training_months = month_count(train_start, train_end);
    if training_months < 36 {
        return Err(ResearchError::new("insufficient_data",format!("requires >=36 TRAIN +12 VALIDATION +60 TEST complete months and120 prior warmup bars; available training months {training_months}")));
    }
    let purged = |boundary: NaiveDate| -> ResearchResult<NaiveDate> {
        let at = calendar.partition_point(|d| *d < boundary);
        let index = at.checked_sub(32).ok_or_else(|| {
            ResearchError::new(
                "insufficient_data",
                "31-session purge has no eligible prior signal",
            )
        })?;
        Ok(calendar[index])
    };
    Ok(CalibrationSplit {
        train_start,
        train_end,
        validation_start,
        validation_end,
        test_start,
        test_end: end,
        train_last_signal: purged(validation_start)?,
        validation_last_signal: purged(test_start)?,
        purge_sessions: 31,
        training_months,
        validation_months: 12,
        test_months: 60,
        warmup_bars: 120,
    })
}
pub fn baseline_distance(p: &ScoringParameters) -> f64 {
    let b = gp_core::fixed_scoring_parameters();
    p.modes
        .iter()
        .map(|(name, m)| {
            m.weights
                .iter()
                .map(|(k, v)| (v - b.modes[name].weights[k]).powi(2))
                .sum::<f64>()
                + (m.overheat_penalty - b.modes[name].overheat_penalty).powi(2)
        })
        .sum()
}
fn weight_distance(p: &ScoringParameters) -> f64 {
    let b = gp_core::fixed_scoring_parameters();
    p.modes
        .iter()
        .map(|(name, m)| {
            m.weights
                .iter()
                .map(|(k, v)| (v - b.modes[name].weights[k]).powi(2))
                .sum::<f64>()
        })
        .sum()
}
fn vector(p: &ScoringParameters) -> Vec<f64> {
    p.modes
        .values()
        .flat_map(|m| {
            m.weights
                .values()
                .copied()
                .chain(std::iter::once(m.overheat_penalty))
        })
        .collect()
}
fn compare_vectors(a: &ScoringParameters, b: &ScoringParameters) -> std::cmp::Ordering {
    vector(a)
        .iter()
        .zip(vector(b))
        .map(|(a, b)| a.total_cmp(&b))
        .find(|o| !o.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}
fn candidate_id(mut p: ScoringParameters, context: &str) -> ResearchResult<ScoringParameters> {
    p.algorithm_version = "adaptive_swing_v2".into();
    p.parameter_set_id = format!(
        "calibrated-{}",
        crate::import::digest(&serde_json::to_vec(
            &serde_json::json!({"context":context,"modes":p.modes})
        )?)
    );
    gp_core::validate_scoring_parameters(&p)?;
    Ok(p)
}
/// Every feasible one-cent transfer / penalty move; canonical order, no RNG.
pub fn feasible_neighbors(
    p: &ScoringParameters,
    modes: &[String],
) -> ResearchResult<Vec<ScoringParameters>> {
    gp_core::validate_scoring_parameters(p)?;
    let mut out = Vec::new();
    for name in modes {
        let m = p
            .modes
            .get(name)
            .ok_or_else(|| ResearchError::new("parameters", "unsupported calibration mode"))?;
        let keys: Vec<String> = m
            .weights
            .iter()
            .filter(|(_, v)| **v > 0.0)
            .map(|(k, _)| k.clone())
            .collect();
        for from in &keys {
            for to in &keys {
                if from == to {
                    continue;
                }
                let mut n = p.clone();
                let weights = &mut n.modes.get_mut(name).unwrap().weights;
                *weights.get_mut(from).unwrap() = ((weights[from] - 0.01) * 1e12).round() / 1e12;
                *weights.get_mut(to).unwrap() = ((weights[to] + 0.01) * 1e12).round() / 1e12;
                n.algorithm_version = "adaptive_swing_v2".into();
                n.parameter_set_id = "neighbor".into();
                if gp_core::validate_scoring_parameters(&n).is_ok() {
                    out.push(candidate_id(n, "neighbor")?);
                }
            }
        }
        for delta in [-0.01, 0.01] {
            let mut n = p.clone();
            let penalty = n.modes[name].overheat_penalty + delta;
            n.modes.get_mut(name).unwrap().overheat_penalty = (penalty * 1e12).round() / 1e12;
            // Validate before assigning ID so out-of-bound neighbors are omitted.
            n.algorithm_version = "adaptive_swing_v2".into();
            n.parameter_set_id = "neighbor".into();
            if gp_core::validate_scoring_parameters(&n).is_ok() {
                out.push(candidate_id(n, "neighbor")?);
            }
        }
    }
    out.sort_by(compare_vectors);
    out.dedup_by(|a, b| a.modes == b.modes);
    Ok(out)
}
#[derive(Clone)]
struct LabelRow {
    factors: BTreeMap<String, f64>,
    labels: [Option<f64>; 3],
}
fn cache_days(cache: &Connection) -> ResearchResult<Vec<(String, String)>> {
    let mut stmt =
        cache.prepare("SELECT date,mode FROM samples GROUP BY date,mode ORDER BY date")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}
fn cache_rows(cache: &Connection, date: &str) -> ResearchResult<Vec<LabelRow>> {
    let mut stmt =
        cache.prepare("SELECT factors,h10,h20,h30 FROM samples WHERE date=? ORDER BY code")?;
    let rows = stmt.query_map([date], |r| {
        Ok((
            r.get::<_, String>(0)?,
            [r.get::<_, Option<f64>>(1)?, r.get(2)?, r.get(3)?],
        ))
    })?;
    rows.map(|r| {
        let (factors, labels) = r?;
        Ok(LabelRow {
            factors: serde_json::from_str(&factors)?,
            labels,
        })
    })
    .collect()
}
fn objective(
    cache: &Connection,
    p: &ScoringParameters,
    modes: &[String],
) -> ResearchResult<Option<f64>> {
    Ok(objectives(cache, std::slice::from_ref(p), modes)?.remove(0))
}
// Evaluate all neighbors in a single bounded per-date SQLite scan. Parameter
// and factor contracts are checked once; arithmetic preserves core's ordered
// weighted sum, penalty, floor and20-point clamp exactly.
fn objectives(
    cache: &Connection,
    candidates: &[ScoringParameters],
    modes: &[String],
) -> ResearchResult<Vec<Option<f64>>> {
    for p in candidates {
        gp_core::validate_scoring_parameters(p)?;
    }
    let mut sums = vec![0.0; candidates.len()];
    let mut counts = vec![0usize; candidates.len()];
    for (date, mode) in cache_days(cache)? {
        if !modes.contains(&mode) {
            continue;
        }
        let rows = cache_rows(cache, &date)?;
        let rows: Vec<_> = rows.iter().filter(|r| r.labels[1].is_some()).collect();
        if rows.len() < 30 {
            continue;
        }
        let labels: Vec<_> = rows.iter().map(|r| r.labels[1].unwrap()).collect();
        if labels.iter().any(|v| !v.is_finite()) {
            return Err(ResearchError::new("numeric", "nonfinite training labels"));
        }
        for row in &rows {
            gp_core::adaptive_score_factor_vector(
                &row.factors,
                &mode,
                &gp_core::fixed_scoring_parameters(),
            )?;
        }
        for (i, p) in candidates.iter().enumerate() {
            let m = &p.modes[&mode];
            let scores: Vec<f64> = rows
                .iter()
                .map(|r| {
                    let mut terms: BTreeMap<&str, f64> = m
                        .weights
                        .iter()
                        .filter(|(_, w)| **w > 0.0)
                        .map(|(k, w)| (k.as_str(), r.factors[k] * w * 20.0))
                        .collect();
                    terms.insert(
                        "overheat_penalty",
                        -r.factors["overheat_penalty"] * m.overheat_penalty * 20.0,
                    );
                    let sum = terms.values().sum::<f64>();
                    if sum < 0.0 {
                        terms.insert("score_floor_adjustment", -sum);
                    }
                    terms.values().sum::<f64>().clamp(0.0, 20.0)
                })
                .collect();
            if let Some(ic) = spearman(&scores, &labels) {
                sums[i] += ic;
                counts[i] += 1;
            }
        }
    }
    Ok(candidates
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (counts[i] > 0).then(|| sums[i] / counts[i] as f64 - 0.1 * weight_distance(p))
        })
        .collect())
}
fn build_train_cache(
    store: &ResearchStore,
    d: &DatasetReader,
    split: &CalibrationSplit,
    context: &str,
) -> ResearchResult<(Connection, BTreeMap<String, ModeCalibration>)> {
    let path = store
        .root()
        .join("datasets")
        .join(&d.record().dataset_id)
        .join(format!(
            "cache-{}-{}.sqlite",
            &crate::import::digest(context.as_bytes())[..16],
            std::process::id()
        ));
    // Cache holds bounded feature/label rows, never raw full OHLC history or
    // millions of records in the lightweight registry/report payload.
    let cache = Connection::open(&path).map_err(|e| ResearchError {
        code: "sqlite".into(),
        message: e.to_string(),
        details: vec![path.display().to_string()],
    })?;
    cache.execute_batch("CREATE TABLE IF NOT EXISTS samples(date TEXT NOT NULL,code TEXT NOT NULL,mode TEXT NOT NULL,factors TEXT NOT NULL,h10 REAL,h20 REAL,h30 REAL,PRIMARY KEY(date,code)); BEGIN IMMEDIATE; DELETE FROM samples;")?;
    let mut summaries: BTreeMap<String, ModeCalibration> = ["range", "trend", "defensive"]
        .into_iter()
        .map(|m| (m.into(), ModeCalibration::default()))
        .collect();
    let config = EvaluationConfig {
        start_date: split.train_start,
        end_date: split.train_end,
        ..Default::default()
    };
    for date in d
        .calendar()
        .iter()
        .filter(|date| **date >= split.train_start && **date <= split.train_last_signal)
    {
        let f = d.features_on(*date, &config)?;
        let mode = f.market_regime.effective;
        let mut labels20 = 0;
        let signal = d.calendar().binary_search(date).expect("calendar signal");
        for row in f.candidates {
            let mut labels = [None; 3];
            for (i, horizon) in [10, 20, 30].into_iter().enumerate() {
                if d.calendar()
                    .get(signal + 1 + horizon)
                    .is_some_and(|end| *end <= split.train_end)
                {
                    labels[i] = d.forward_return(&row.stock.code, *date, horizon)?;
                }
            }
            if labels.iter().flatten().any(|v| !v.is_finite()) {
                return Err(ResearchError::new("numeric", "nonfinite factor label"));
            }
            if labels[1].is_some() {
                labels20 += 1;
            }
            cache.execute(
                "INSERT INTO samples VALUES(?,?,?,?,?,?,?)",
                params![
                    date.to_string(),
                    row.stock.code,
                    mode,
                    serde_json::to_string(&row.factors)?,
                    labels[0],
                    labels[1],
                    labels[2]
                ],
            )?;
        }
        if let Some(s) = summaries.get_mut(&mode) {
            s.available_labels += labels20;
            if labels20 >= 30 {
                s.effective_signal_days += 1;
            }
        }
    }
    cache.execute_batch("COMMIT;")?;
    for s in summaries.values_mut() {
        if s.effective_signal_days < 120 {
            s.reason=Some("sparse mode: requires120 effective signal days, each >=30 mature20-day labels; builtin weights retained".into());
        }
    }
    Ok((cache, summaries))
}
type TrainingCandidates = Vec<(ScoringParameters, Option<f64>)>;
fn retain_train_candidates(
    seen: &mut TrainingCandidates,
    fixed: &ScoringParameters,
    baseline: Option<f64>,
) {
    seen.sort_by(|(a, x), (b, y)| {
        y.unwrap_or(f64::NEG_INFINITY)
            .total_cmp(&x.unwrap_or(f64::NEG_INFINITY))
            .then_with(|| baseline_distance(a).total_cmp(&baseline_distance(b)))
            .then_with(|| compare_vectors(a, b))
    });
    seen.dedup_by(|(a, _), (b, _)| a.modes == b.modes);
    seen.retain(|(p, _)| p.parameter_set_id != "fixed-v1");
    seen.truncate(4);
    seen.push((fixed.clone(), baseline));
}
fn train_candidates(
    cache: &Connection,
    modes: &[String],
    context: &str,
) -> ResearchResult<(TrainingCandidates, usize)> {
    let fixed = gp_core::fixed_scoring_parameters();
    let base = objective(cache, &fixed, modes)?;
    let mut current = fixed.clone();
    let mut current_score = base;
    let mut seen = vec![(fixed.clone(), base)];
    let mut iterations = 0;
    for _ in 0..20 {
        let Some(value) = current_score else {
            break;
        };
        let mut best: Option<(ScoringParameters, f64)> = None;
        let neighbors = feasible_neighbors(&current, modes)?
            .into_iter()
            .map(|p| candidate_id(p, context))
            .collect::<ResearchResult<Vec<_>>>()?;
        let scores = objectives(cache, &neighbors, modes)?;
        for (neighbor, score) in neighbors.into_iter().zip(scores) {
            if let Some(score) = score {
                if base.is_some_and(|baseline| score > baseline + 1e-12) {
                    seen.push((neighbor.clone(), Some(score)));
                    retain_train_candidates(&mut seen, &fixed, base);
                }
                if score <= value + 1e-12 {
                    continue;
                }
                let better = best.as_ref().is_none_or(|(p, s)| {
                    score > *s + 1e-12
                        || ((score - *s).abs() <= 1e-12
                            && (baseline_distance(&neighbor)
                                .total_cmp(&baseline_distance(p))
                                .then_with(|| compare_vectors(&neighbor, p)))
                            .is_lt())
                });
                if better {
                    best = Some((neighbor, score));
                }
            }
        }
        let Some((p, score)) = best else {
            break;
        };
        current = p;
        current_score = Some(score);
        iterations += 1;
    }
    retain_train_candidates(&mut seen, &fixed, base);
    Ok((seen, iterations))
}
fn summarize(
    cache: &Connection,
    p: &ScoringParameters,
    prefix: &str,
) -> ResearchResult<DiagnosticsReport> {
    let mut acc = crate::diagnostics::DiagnosticAccumulator::default();
    for (date, mode) in cache_days(cache)? {
        let rows = cache_rows(cache, &date)?;
        let Some(first) = rows.first() else {
            continue;
        };
        let mut keys: Vec<String> = first.factors.keys().cloned().collect();
        keys.push("composite".into());
        for (h, index) in [(10, 0), (20, 1), (30, 2)] {
            let available: Vec<_> = rows.iter().filter(|r| r.labels[index].is_some()).collect();
            let labels: Vec<_> = available.iter().map(|r| r.labels[index].unwrap()).collect();
            for key in &keys {
                let scores = available
                    .iter()
                    .map(|r| {
                        if key == "composite" {
                            gp_core::adaptive_score_factor_vector(&r.factors, &mode, p)
                                .map_err(ResearchError::from)
                        } else {
                            Ok(r.factors[key])
                        }
                    })
                    .collect::<ResearchResult<Vec<_>>>()?;
                acc.date(
                    &format!("{prefix}/{key}/h{h}"),
                    &scores,
                    &labels,
                    rows.len() - available.len(),
                );
            }
        }
        let keys: Vec<_> = first.factors.keys().collect();
        for a in 0..keys.len() {
            for b in a + 1..keys.len() {
                acc.pair(
                    &format!("{prefix}/{}/{}", keys[a], keys[b]),
                    &rows.iter().map(|r| r.factors[keys[a]]).collect::<Vec<_>>(),
                    &rows.iter().map(|r| r.factors[keys[b]]).collect::<Vec<_>>(),
                );
            }
        }
    }
    Ok(acc.finish())
}
fn append_window_diagnostics(
    store: &ResearchStore,
    d: &DatasetReader,
    p: &ScoringParameters,
    split: &CalibrationSplit,
    prefix: &str,
    context: &str,
    diagnostics: &mut DiagnosticsReport,
) -> ResearchResult<()> {
    let mut window = split.clone();
    if prefix == "validation" {
        window.train_start = split.validation_start;
        window.train_end = split.validation_end;
        window.train_last_signal = split.validation_last_signal;
    } else {
        window.train_start = split.test_start;
        window.train_end = split.test_end;
        window.train_last_signal = split.test_end;
    }
    let (cache, _) = build_train_cache(store, d, &window, &format!("{context}-{prefix}"))?;
    let summary = summarize(&cache, p, prefix)?;
    diagnostics.factors.extend(summary.factors);
    diagnostics
        .pairwise_factor_correlation
        .extend(summary.pairwise_factor_correlation);
    Ok(())
}
fn canonical_config(split: &CalibrationSplit, cost: f64, strategy: &str) -> EvaluationConfig {
    EvaluationConfig {
        start_date: split.test_start,
        end_date: split.test_end,
        transaction_cost_bps: cost,
        strategy: strategy.into(),
        ..Default::default()
    }
}
fn same_config(a: &EvaluationConfig, b: &EvaluationConfig) -> ResearchResult<bool> {
    Ok(serde_json::to_value(a)? == serde_json::to_value(b)?)
}
pub(crate) fn gate_evidence(r: &CalibrationReport) -> ResearchResult<GateEvidence> {
    let mut e = GateEvidence {
        real_data: r.data_quality.real_data,
        full_universe: r.data_quality.full_universe,
        quality_passed: r.data_quality.passed,
        latency: r.latency.clone(),
        ..Default::default()
    };
    let (Some(t), Some(f), Some(l), Some(s)) =
        (&r.test, &r.fixed_baseline, &r.legacy_baseline, &r.split)
    else {
        return Ok(e);
    };
    e.identities_match = r.implementation_fingerprint
        == gp_core::scoring_implementation_fingerprint()
        && r.research_fingerprint == research_implementation_fingerprint()
        && r.parameter_fingerprint == crate::registry::parameter_digest(&r.parameters)?;
    let reports = std::iter::once(t)
        .chain(std::iter::once(f))
        .chain(std::iter::once(l))
        .chain(r.stress_tests.iter());
    for report in reports {
        e.identities_match &= report.dataset_id == r.dataset_id
            && report.data_fingerprint == r.data_fingerprint
            && report.implementation_fingerprint == r.implementation_fingerprint
            && report.research_fingerprint == r.research_fingerprint
            && report.parameter_fingerprint
                == crate::registry::parameter_digest(&report.parameters)?
            && gp_core::validate_scoring_parameters(&report.parameters).is_ok()
            && report.data_quality.passed
            && report.data_quality.real_data
            && report.data_quality.full_universe;
    }
    e.frozen_config = r.parameters.algorithm_version == "adaptive_swing_v2"
        && same_config(&t.config, &canonical_config(s, 10., "adaptive"))?
        && same_config(&f.config, &canonical_config(s, 10., "adaptive"))?
        && same_config(&l.config, &canonical_config(s, 10., "legacy_balanced"))?
        && f.parameters.parameter_set_id == "fixed-v1"
        && l.parameters.parameter_set_id == "fixed-v1"
        && crate::registry::parameter_digest(&t.parameters)? == r.parameter_fingerprint
        && s.training_months >= 36
        && s.test_months == 60
        && s.validation_months == 12
        && s.purge_sessions == 31
        && s.warmup_bars == 120;
    e.complete_months = t.monthly_folds.iter().filter(|m| m.complete).count();
    e.candidate_annualized = t.metrics.annualized_return;
    e.fixed_annualized = f.metrics.annualized_return;
    e.candidate_drawdown = t.metrics.max_drawdown;
    e.fixed_drawdown = f.metrics.max_drawdown;
    e.candidate_turnover = Some(t.metrics.annualized_turnover);
    e.fixed_turnover = Some(f.metrics.annualized_turnover);
    e.cost_excess[0] = t
        .metrics
        .annualized_return
        .zip(t.benchmark_metrics.annualized_return)
        .map(|(a, b)| a - b);
    for (i, cost) in [25., 50.].iter().enumerate() {
        let matching: Vec<_> = r
            .stress_tests
            .iter()
            .filter(|test| test.config.transaction_cost_bps == *cost)
            .collect();
        if matching.len() == 1 {
            let test = matching[0];
            e.frozen_config &= same_config(&test.config, &canonical_config(s, *cost, "adaptive"))?
                && crate::registry::parameter_digest(&test.parameters)? == r.parameter_fingerprint;
            e.cost_excess[i + 1] = test
                .metrics
                .annualized_return
                .zip(test.benchmark_metrics.annualized_return)
                .map(|(a, b)| a - b);
        } else {
            e.frozen_config = false;
        }
    }
    e.frozen_config &= r.stress_tests.len() == 2;
    // Contiguous complete monthly pairing, exactly the frozen 60-month span.
    let folds = &t.monthly_folds;
    let mut contiguous = folds.len() == 60;
    for (i, fold) in folds.iter().enumerate() {
        let expected = s
            .test_start
            .checked_add_months(Months::new(i as u32))
            .unwrap();
        contiguous &= fold.complete && month_start(fold.selection_date) == expected;
    }
    if contiguous {
        e.monthly_excess = folds
            .iter()
            .map(|m| m.net_return - m.benchmark_return)
            .collect();
        for chunk in folds.as_chunks::<12>().0 {
            let candidate = chunk.iter().fold(1.0, |v, m| v * (1.0 + m.net_return));
            let benchmark = chunk
                .iter()
                .fold(1.0, |v, m| v * (1.0 + m.benchmark_return));
            e.complete_year_excess.push(candidate - benchmark);
        }
    } else {
        e.frozen_config = false;
    }
    Ok(e)
}
fn persist(store: &ResearchStore, report: &CalibrationReport) -> ResearchResult<()> {
    store.conn.execute_batch("BEGIN IMMEDIATE;")?;
    let result = (|| {
        store.save_report("calibration", &serde_json::to_value(report)?)?;
        store.calibration_receipt(report)
    })();
    match result {
        Ok(()) => {
            store.conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(e) => {
            store.conn.execute_batch("ROLLBACK;")?;
            Err(e)
        }
    }
}
pub fn calibrate(
    store: &ResearchStore,
    dataset_id: &str,
    options: &CalibrationOptions,
) -> ResearchResult<CalibrationReport> {
    store.require_write()?;
    let d = store.dataset(dataset_id)?;
    let context = crate::import::digest(&serde_json::to_vec(
        &serde_json::json!({"format":"calibration/v2","dataset":dataset_id,"core":gp_core::scoring_implementation_fingerprint(),"research":research_implementation_fingerprint(),"options":options}),
    )?);
    let run_id = format!("calibration-{context}");
    let existing = match store.report(&run_id) {
        Ok(value) => Some(value),
        Err(e) if e.code == "not_found" => None,
        Err(e) => return Err(e),
    };
    if let Some(old) = existing {
        let raw: String = store.conn.query_row(
            "SELECT report FROM reports WHERE run_id=?",
            [&run_id],
            |r| r.get(0),
        )?;
        let receipt: Option<String> = store
            .conn
            .query_row(
                "SELECT payload FROM calibration_receipts WHERE run_id=?",
                [&run_id],
                |r| r.get(0),
            )
            .optional()?;
        let receipt = receipt.ok_or_else(|| {
            ResearchError::new(
                "immutable_conflict",
                "generic report occupies calibration run id without owned receipt",
            )
        })?;
        let receipt: crate::registry::CalibrationReceipt = serde_json::from_str(&receipt)?;
        if receipt.report_digest != crate::import::digest(raw.as_bytes())
            || receipt.core != gp_core::scoring_implementation_fingerprint()
            || receipt.research != research_implementation_fingerprint()
        {
            return Err(ResearchError::new(
                "immutable_conflict",
                "existing calibration evidence differs",
            ));
        }
        return Ok(serde_json::from_value(old)?);
    }
    let fixed = gp_core::fixed_scoring_parameters();
    let mut report = CalibrationReport {
        run_id,
        dataset_id: dataset_id.into(),
        status: "insufficient_data".into(),
        parameters: fixed.clone(),
        split: None,
        data_fingerprint: d.record().data_fingerprint.clone(),
        implementation_fingerprint: gp_core::scoring_implementation_fingerprint().into(),
        research_fingerprint: research_implementation_fingerprint().into(),
        parameter_fingerprint: crate::registry::parameter_digest(&fixed)?,
        issues: vec![],
        data_quality: QualityReport {
            real_data: d.record().real_data,
            full_universe: d.record().full_universe,
            ..Default::default()
        },
        mode_calibration: ["range", "trend", "defensive"]
            .into_iter()
            .map(|mode| {
                (
                    mode.into(),
                    ModeCalibration {
                        reason: Some(
                            "insufficient split/quality evidence; builtin weights retained".into(),
                        ),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        diagnostics: DiagnosticsReport::default(),
        validation_candidates: vec![],
        test: None,
        fixed_baseline: None,
        legacy_baseline: None,
        stress_tests: vec![],
        latency: LatencyEvidence::default(),
        gate: GateReport::default(),
    };
    let split = match compute_split(d.calendar(), options.end_date) {
        Ok(s) => s,
        Err(e) if e.code == "insufficient_data" => {
            report.issues.push(e.message);
            report.data_quality.issues = report.issues.clone();
            report.gate = release_gate(&gate_evidence(&report)?);
            persist(store, &report)?;
            return Ok(report);
        }
        Err(e) => return Err(e),
    };
    report.split = Some(split.clone());
    let context = crate::import::digest(&serde_json::to_vec(
        &serde_json::json!({"origin":context,"split":split}),
    )?);
    let audit = EvaluationConfig {
        start_date: split.train_start,
        end_date: split.test_end,
        ..Default::default()
    };
    report.data_quality = d.validate(&audit)?;
    if !report.data_quality.passed {
        report.issues = report.data_quality.issues.clone();
        report.gate = release_gate(&gate_evidence(&report)?);
        persist(store, &report)?;
        return Ok(report);
    }
    let frozen: Option<(String,String,String)> = store
        .conn
        .query_row(
            "SELECT parameters,signature,split FROM test_locks WHERE dataset_id=? AND test_start=? AND test_end=?",
            params![
                dataset_id,
                split.test_start.to_string(),
                split.test_end.to_string()
            ],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        )
        .optional()?;
    if let Some((frozen, signature, locked_split)) = frozen {
        // Same weight signature can be reverified after code changes, but the
        // previous test outcome can never trigger a new training search.
        let frozen: ScoringParameters = serde_json::from_str(&frozen)?;
        if crate::registry::weight_signature(&frozen)? != signature
            || locked_split != serde_json::to_string(&split)?
        {
            return Err(ResearchError::new(
                "test_locked",
                "existing test-window lock is inconsistent",
            ));
        }
        let mut fresh = fixed.clone();
        fresh.modes = frozen.modes;
        report.parameters = if fresh.modes == fixed.modes {
            fixed.clone()
        } else {
            candidate_id(fresh, &context)?
        };
        report.issues.push(
            "existing dataset/test-window freeze reused; no training or validation reselection"
                .into(),
        );
        for mode in ["range", "trend", "defensive"] {
            report.mode_calibration.insert(
                mode.into(),
                ModeCalibration {
                    calibrated: report.parameters.modes[mode] != fixed.modes[mode],
                    reason: Some("frozen weights reused for re-verification".into()),
                    ..Default::default()
                },
            );
        }
    } else {
        let (cache, mut modes) = build_train_cache(store, &d, &split, &context)?;
        let eligible: Vec<String> = modes
            .iter()
            .filter(|(_, s)| s.effective_signal_days >= 120)
            .map(|(m, _)| m.clone())
            .collect();
        let (candidates, iterations) = train_candidates(&cache, &eligible, &context)?;
        let validation = EvaluationConfig {
            start_date: split.validation_start,
            end_date: split.validation_end,
            signal_cutoff: Some(split.validation_last_signal),
            ..Default::default()
        };
        // Keep all12 validation months and the continuous portfolio, while
        // suppressing the last31 signals and all their forward labels.
        let baseline = crate::evaluation::evaluate_audited(
            &d,
            &fixed,
            &validation,
            report.data_quality.clone(),
        )?;
        for (p, objective) in candidates {
            let evaluation = if p.parameter_set_id == "fixed-v1" {
                baseline.clone()
            } else {
                crate::evaluation::evaluate_audited(
                    &d,
                    &p,
                    &validation,
                    report.data_quality.clone(),
                )?
            };
            let gain = evaluation
                .metrics
                .annualized_return
                .zip(baseline.metrics.annualized_return)
                .map(|(a, b)| a - b);
            let feasible = gain.is_some_and(|v| v.is_finite() && v > 1e-12)
                && evaluation
                    .metrics
                    .max_drawdown
                    .zip(baseline.metrics.max_drawdown)
                    .is_some_and(|(a, b)| a.abs() <= b.abs() + 0.005)
                && evaluation.metrics.annualized_turnover
                    <= baseline.metrics.annualized_turnover * 1.10;
            report.validation_candidates.push(ValidationCandidate {
                baseline_distance: baseline_distance(&p),
                parameters: p,
                training_objective: objective,
                feasible,
                annualized_excess_over_fixed: gain,
                evaluation,
            });
        }
        let mut choices: Vec<_> = report
            .validation_candidates
            .iter()
            .filter(|c| c.feasible && c.parameters.parameter_set_id != "fixed-v1")
            .collect();
        choices.sort_by(|a, b| {
            b.annualized_excess_over_fixed
                .unwrap()
                .total_cmp(&a.annualized_excess_over_fixed.unwrap())
                .then_with(|| {
                    a.evaluation
                        .metrics
                        .max_drawdown
                        .unwrap()
                        .abs()
                        .total_cmp(&b.evaluation.metrics.max_drawdown.unwrap().abs())
                })
                .then_with(|| a.baseline_distance.total_cmp(&b.baseline_distance))
                .then_with(|| compare_vectors(&a.parameters, &b.parameters))
        });
        if let Some(chosen) = choices.first() {
            report.parameters = chosen.parameters.clone();
        } else {
            report.issues.push("no distinct feasible validation gain; corrected fixed retained, no candidate qualification".into());
        }
        for (name, s) in &mut modes {
            s.iterations = if eligible.contains(name) {
                iterations
            } else {
                0
            };
            s.calibrated = report.parameters.modes[name] != fixed.modes[name];
            if !s.calibrated && s.reason.is_none() {
                s.reason = Some("validation retained builtin mode weights".into());
            }
        }
        report.mode_calibration = modes;
        report.diagnostics = summarize(&cache, &report.parameters, "train")?;
        append_window_diagnostics(
            store,
            &d,
            &report.parameters,
            &split,
            "validation",
            &context,
            &mut report.diagnostics,
        )?;
        // Registration and test lock precede ANY test label or metric query.
        if report.parameters.algorithm_version == "adaptive_swing_v2" {
            store.register_parameters(&report.parameters, dataset_id)?;
        }
        store.freeze_test_parameters(dataset_id, &report.parameters, &split)?;
    }
    if report.parameters.algorithm_version == "adaptive_swing_v2" {
        store.register_parameters(&report.parameters, dataset_id)?;
    }
    report.parameter_fingerprint = crate::registry::parameter_digest(&report.parameters)?;
    // Test diagnostic labels are first obtained only after the chosen weights
    // and window lock have been persisted. They never affect selection.
    append_window_diagnostics(
        store,
        &d,
        &report.parameters,
        &split,
        "test",
        &context,
        &mut report.diagnostics,
    )?;
    report.test = Some(crate::evaluation::evaluate_audited(
        &d,
        &report.parameters,
        &canonical_config(&split, 10., "adaptive"),
        report.data_quality.clone(),
    )?);
    report.fixed_baseline = Some(crate::evaluation::evaluate_audited(
        &d,
        &fixed,
        &canonical_config(&split, 10., "adaptive"),
        report.data_quality.clone(),
    )?);
    report.legacy_baseline = Some(crate::evaluation::evaluate_audited(
        &d,
        &fixed,
        &canonical_config(&split, 10., "legacy_balanced"),
        report.data_quality.clone(),
    )?);
    for cost in [25., 50.] {
        report
            .stress_tests
            .push(crate::evaluation::evaluate_audited(
                &d,
                &report.parameters,
                &canonical_config(&split, cost, "adaptive"),
                report.data_quality.clone(),
            )?);
    }
    let signal = d.calendar()[d
        .calendar()
        .partition_point(|date| *date < split.test_start)
        - 1];
    let request = gp_core::AdaptiveScreenRequest {
        as_of_date: Some(signal.to_string()),
        ..Default::default()
    };
    let start = Instant::now();
    let cold = store.dataset(dataset_id)?;
    let selection = cold.screen_on(signal, &request, &report.parameters)?;
    let cold_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut cached_ms = 0.0f64;
    for _ in 0..3 {
        let start = Instant::now();
        cold.screen_on(signal, &request, &report.parameters)?;
        cached_ms = cached_ms.max(start.elapsed().as_secs_f64() * 1000.);
    }
    let mut industries = BTreeMap::<String, usize>::new();
    for item in &selection.items {
        *industries.entry(item.stock.industry.clone()).or_default() += 1;
    }
    report.latency = LatencyEvidence {
        label: "local PIT replay".into(),
        cold_ms: Some(cold_ms),
        cached_ms: Some(cached_ms),
        repeated_samples: 3,
        industry_cap: if industries.values().all(|n| *n <= 3) {
            3
        } else {
            0
        },
    };
    let folds = &report.test.as_ref().unwrap().monthly_folds;
    let jaccard: Vec<f64> = folds
        .windows(2)
        .map(|w| {
            let a: &Vec<String> = &w[0].selected_symbols;
            let b = &w[1].selected_symbols;
            let intersection = a.iter().filter(|code| b.contains(code)).count();
            let union = a.len() + b.len() - intersection;
            if union == 0 {
                1.0
            } else {
                intersection as f64 / union as f64
            }
        })
        .collect();
    report.diagnostics.mean_selection_jaccard =
        (!jaccard.is_empty()).then(|| jaccard.iter().sum::<f64>() / jaccard.len() as f64);
    report.gate = release_gate(&gate_evidence(&report)?);
    report.status = if !d.record().real_data {
        "insufficient_data"
    } else if report.gate.passed {
        "complete"
    } else {
        "rejected"
    }
    .into();
    if !d.record().real_data {
        report
            .issues
            .push("synthetic/fixture provenance cannot qualify release".into());
    }
    persist(store, &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Days;
    fn calendar() -> Vec<NaiveDate> {
        let start = NaiveDate::from_ymd_opt(2016, 1, 1).unwrap();
        (0..4000)
            .map(|i| start.checked_add_days(Days::new(i)).unwrap())
            .filter(|d| !matches!(d.weekday(), Weekday::Sat | Weekday::Sun))
            .collect()
    }
    #[test]
    fn exact_split_and_purge() {
        let c = calendar();
        let s = compute_split(&c, None).unwrap();
        assert_eq!(s.test_start, NaiveDate::from_ymd_opt(2021, 10, 1).unwrap());
        assert_eq!(s.test_end, NaiveDate::from_ymd_opt(2026, 9, 30).unwrap());
        assert_eq!(
            s.validation_start,
            NaiveDate::from_ymd_opt(2020, 10, 1).unwrap()
        );
        let at = c.partition_point(|d| *d < s.validation_start);
        assert_eq!(s.train_last_signal, c[at - 32]);
        assert!(c[c.binary_search(&s.train_last_signal).unwrap() + 31] < s.validation_start);
        assert_eq!(
            compute_split(&c[..200], None).unwrap_err().code,
            "insufficient_data"
        );
        assert_eq!(
            compute_split(&c, Some(NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()))
                .unwrap_err()
                .code,
            "config"
        );
    }
    #[test]
    fn support_bounds_neighbors() {
        let p = gp_core::fixed_scoring_parameters();
        let n = feasible_neighbors(&p, &["defensive".into()]).unwrap();
        assert!(!n.is_empty());
        for item in n {
            gp_core::validate_scoring_parameters(&item).unwrap();
            assert_eq!(item.modes["defensive"].weights["trend"], 0.);
            assert!(baseline_distance(&item) > 0.);
            assert_eq!(item.modes["range"], p.modes["range"]);
        }
    }
    fn synthetic_cache() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE samples(date TEXT,code TEXT,mode TEXT,factors TEXT,h10 REAL,h20 REAL,h30 REAL);").unwrap();
        for date in ["2019-01-02", "2019-01-03"] {
            for i in 0..40 {
                let x = i as f64 / 40.;
                let f: BTreeMap<String, f64> = [
                    ("quality", x),
                    ("valuation", 1. - x),
                    ("range_setup", ((i * 17) % 40) as f64 / 40.),
                    ("trend", 0.3),
                    ("relative_strength", ((i * 7) % 40) as f64 / 40.),
                    ("liquidity", 0.5),
                    ("risk", ((i * 13) % 40) as f64 / 40.),
                    ("stability", 0.5),
                    ("overheat_penalty", ((i * 19) % 40) as f64 / 40.),
                ]
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect();
                conn.execute(
                    "INSERT INTO samples VALUES(?,?,?,?,?,?,?)",
                    params![
                        date,
                        format!("{i:06}"),
                        "range",
                        serde_json::to_string(&f).unwrap(),
                        x,
                        x,
                        x
                    ],
                )
                .unwrap();
            }
        }
        conn
    }
    #[test]
    fn deterministic_search_penalty_bounds_and_baseline() {
        let cache = synthetic_cache();
        let modes = ["range".into()];
        let (a, iterations) = train_candidates(&cache, &modes, "dataset-a").unwrap();
        let (b, repeated) = train_candidates(&cache, &modes, "dataset-a").unwrap();
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
        assert_eq!(iterations, repeated);
        assert!(iterations > 0 && iterations <= 20);
        assert!(a.len() <= 5);
        assert!(a.iter().any(|(p, _)| p.parameter_set_id == "fixed-v1"));
        let base = objective(&cache, &gp_core::fixed_scoring_parameters(), &modes)
            .unwrap()
            .unwrap();
        assert!(a.iter().any(|(_, o)| o.unwrap() > base));
        for (p, _) in &a {
            gp_core::validate_scoring_parameters(p).unwrap();
        }
        let (c, _) = train_candidates(&cache, &modes, "dataset-b").unwrap();
        assert_eq!(a[0].0.modes, c[0].0.modes);
        assert_ne!(a[0].0.parameter_set_id, c[0].0.parameter_set_id);
    }
    #[test]
    fn constant_labels_never_select_code_order_or_changed_weights() {
        let cache = synthetic_cache();
        cache.execute("UPDATE samples SET h20=0.5", []).unwrap();
        let (candidates, iterations) =
            train_candidates(&cache, &["range".into()], "constant").unwrap();
        assert_eq!(iterations, 0);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0.parameter_set_id, "fixed-v1");
        assert_eq!(candidates[0].1, None);
    }
    #[test]
    fn train_label_cutoff_never_queries_future_outcomes() {
        let app = std::env::temp_dir().join(format!(
            "gp-cal-label-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ResearchStore::open(&app).unwrap();
        let record = store
            .import_bundle(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny"),
            )
            .unwrap();
        let d = store.dataset(&record.dataset_id).unwrap();
        let c = d.calendar();
        let split = CalibrationSplit {
            train_start: c[60],
            train_end: c[70],
            train_last_signal: c[61],
            validation_start: c[71],
            validation_end: c[79],
            validation_last_signal: c[75],
            test_start: c[80],
            test_end: c[89],
            training_months: 0,
            validation_months: 0,
            test_months: 0,
            purge_sessions: 31,
            warmup_bars: 120,
        };
        let (cache, modes) = build_train_cache(&store, &d, &split, "cutoff").unwrap();
        let labels: i64 = cache
            .query_row(
                "SELECT COUNT(*) FROM samples WHERE h20 IS NOT NULL OR h30 IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(labels,0,"future labels beyond the supplied train cutoff must be unavailable even if the dataset contains their prices");
        assert!(modes.values().all(|m| !m.calibrated && m.reason.is_some()));
        type CachedRows = Vec<(String, Option<f64>, Option<f64>, Option<f64>)>;
        let before: CachedRows = {
            let mut q = cache
                .prepare("SELECT factors,h10,h20,h30 FROM samples ORDER BY date,code")
                .unwrap();
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let mutable = Connection::open(
            store
                .root()
                .join("datasets")
                .join(&record.dataset_id)
                .join("dataset.sqlite"),
        )
        .unwrap();
        mutable.execute("UPDATE bars SET payload=json_set(payload,'$.open',50.0,'$.high',50.0,'$.low',50.0,'$.close',50.0) WHERE date>?",[split.train_end.to_string()]).unwrap();
        drop(mutable);
        let changed = store.dataset(&record.dataset_id).unwrap();
        let (future_cache, _) =
            build_train_cache(&store, &changed, &split, "future-mutated").unwrap();
        let after: CachedRows = {
            let mut q = future_cache
                .prepare("SELECT factors,h10,h20,h30 FROM samples ORDER BY date,code")
                .unwrap();
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            before, after,
            "future/test price changes cannot alter training factors or eligible labels"
        );
    }
}
