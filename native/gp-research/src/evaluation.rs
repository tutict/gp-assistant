use crate::*;
use chrono::{Datelike, NaiveDate};
use gp_core::{AdaptiveRecentExposure, ScoringParameters, ScreenCriteria};
use std::{
    collections::{BTreeMap, HashSet},
    sync::OnceLock,
};

pub fn research_implementation_fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            gp_core::source_text_fingerprint(
                b"gp-research-implementation/v2-lf\0",
                &[
                    ("models.rs", include_bytes!("models.rs").as_slice()),
                    ("dataset.rs", include_bytes!("dataset.rs").as_slice()),
                    ("import.rs", include_bytes!("import.rs").as_slice()),
                    ("evaluation.rs", include_bytes!("evaluation.rs").as_slice()),
                    ("lib.rs", include_bytes!("lib.rs").as_slice()),
                    (
                        "calibration.rs",
                        include_bytes!("calibration.rs").as_slice(),
                    ),
                    (
                        "diagnostics.rs",
                        include_bytes!("diagnostics.rs").as_slice(),
                    ),
                    (
                        "release_gate.rs",
                        include_bytes!("release_gate.rs").as_slice(),
                    ),
                    ("registry.rs", include_bytes!("registry.rs").as_slice()),
                    ("cli.rs", include_bytes!("cli.rs").as_slice()),
                    ("main.rs", include_bytes!("main.rs").as_slice()),
                    ("Cargo.toml", include_bytes!("../Cargo.toml").as_slice()),
                    ("Cargo.lock", include_bytes!("../Cargo.lock").as_slice()),
                ],
            )
        })
        .as_str()
}
struct Holding {
    shares: f64,
    price: f64,
}
struct Receivable {
    amount: f64,
    pay_date: NaiveDate,
}
struct Portfolio {
    cash: f64,
    holdings: BTreeMap<String, Holding>,
    receivables: Vec<Receivable>,
    total_cost: f64,
    turnover: f64,
}
impl Portfolio {
    fn new(cash: f64) -> Self {
        Self {
            cash,
            holdings: BTreeMap::new(),
            receivables: vec![],
            total_cost: 0.0,
            turnover: 0.0,
        }
    }
    fn nav(&self) -> f64 {
        self.cash
            + self
                .holdings
                .values()
                .map(|h| h.shares * h.price)
                .sum::<f64>()
            + self.receivables.iter().map(|r| r.amount).sum::<f64>()
    }
    fn actions(&mut self, d: &DatasetReader, date: NaiveDate) -> ResearchResult<()> {
        let actions = d.actions_on(date)?;
        let previous_shares = self
            .holdings
            .iter()
            .map(|(code, h)| (code.clone(), h.shares))
            .collect::<BTreeMap<_, _>>();
        // A missing ex-date bar retains the prior valuation, not the detached
        // cash entitlement. Adjust cash in pre-split units before transforms.
        // Exact raw quotes replace the mark later and must not be discounted.
        let mut cash_adjustments = BTreeMap::<String, f64>::new();
        for action in &actions {
            if action.kind == "cash_dividend" && previous_shares.contains_key(&action.code) {
                *cash_adjustments.entry(action.code.clone()).or_default() +=
                    action.cash_per_share.ok_or_else(|| {
                        ResearchError::new("action", "missing cash dividend evidence")
                    })?;
            }
        }
        for (code, cash_per_share) in cash_adjustments {
            if d.bar(&code, date)?.is_none() {
                let holding = self.holdings.get_mut(&code).expect("held share snapshot");
                holding.price -= cash_per_share;
                if !holding.price.is_finite() || holding.price < 0.0 {
                    return Err(ResearchError::new(
                        "action",
                        "cash adjustment exceeds retained holding valuation; exact quote required",
                    ));
                }
            }
        }
        for a in actions {
            if let Some(h) = self.holdings.get_mut(&a.code) {
                match a.kind.as_str() {
                    "cash_dividend" => self.receivables.push(Receivable {
                        amount: previous_shares[&a.code]
                            * a.cash_per_share.ok_or_else(|| {
                                ResearchError::new("action", "missing cash dividend evidence")
                            })?,
                        pay_date: a.pay_date,
                    }),
                    "share_split" => {
                        let multiplier = a.share_multiplier.ok_or_else(|| {
                            ResearchError::new("action", "missing split evidence")
                        })?;
                        h.shares *= multiplier;
                        h.price /= multiplier;
                    }
                    "delisting_settlement" => {
                        self.receivables.push(Receivable {
                            amount: previous_shares[&a.code]
                                * a.cash_per_share.ok_or_else(|| {
                                    ResearchError::new(
                                        "action",
                                        "missing delisting settlement evidence",
                                    )
                                })?,
                            pay_date: a.pay_date,
                        });
                        h.shares = 0.0;
                    }
                    _ => return Err(ResearchError::new("action", "unsupported realized action")),
                }
            }
        }
        self.holdings.retain(|_, h| h.shares > 1e-12);
        self.receivables.retain(|r| {
            if r.pay_date <= date {
                self.cash += r.amount;
                false
            } else {
                true
            }
        });
        Ok(())
    }
    fn mark(&mut self, d: &DatasetReader, date: NaiveDate) -> ResearchResult<()> {
        for (code, h) in &mut self.holdings {
            if let Some(bar) = d.bar(code, date)? {
                h.price = bar.close;
            }
        }
        Ok(())
    }
    // Explicit execution evidence and accounting context avoid ambient dates,
    // prices or fee defaults in this internal order boundary.
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &mut self,
        d: &DatasetReader,
        date: NaiveDate,
        code: &str,
        side: &str,
        shares: f64,
        fee: f64,
        benchmark: bool,
        trades: &mut Vec<TradeRecord>,
    ) -> ResearchResult<()> {
        if !shares.is_finite() {
            return Err(ResearchError::new(
                "numeric",
                "execution share intent must be finite",
            ));
        }
        if shares <= 1e-10 {
            return Ok(());
        }
        let bar = d.bar(code, date)?;
        let status = d.status(code, date)?;
        let can_trade = status
            .as_ref()
            .is_some_and(|s| s.is_listed && if side == "buy" { s.can_buy } else { s.can_sell });
        if !can_trade || bar.is_none() {
            trades.push(TradeRecord {
                date,
                code: code.into(),
                side: side.into(),
                shares,
                price: bar.map(|b| b.close),
                notional: 0.0,
                cost: 0.0,
                executed: false,
                reason: Some(
                    if status.is_none() {
                        "missing exact daily status"
                    } else if !can_trade {
                        "daily trading restriction"
                    } else {
                        "missing exact execution bar"
                    }
                    .into(),
                ),
                benchmark,
            });
            return Ok(());
        }
        let price = bar.unwrap().close;
        let requested_shares = shares;
        let shares = if side == "buy" {
            shares.min(self.cash / (price * (1.0 + fee)))
        } else {
            shares.min(self.holdings.get(code).map(|h| h.shares).unwrap_or(0.0))
        };
        if shares <= 1e-10 {
            trades.push(TradeRecord {
                date,
                code: code.into(),
                side: side.into(),
                shares: requested_shares,
                price: Some(price),
                notional: 0.0,
                cost: 0.0,
                executed: false,
                reason: Some(
                    if side == "buy" {
                        "insufficient spendable cash"
                    } else {
                        "insufficient held shares"
                    }
                    .into(),
                ),
                benchmark,
            });
            return Ok(());
        }
        let nav = self.nav();
        let notional = shares * price;
        let cost = notional * fee;
        if side == "buy" {
            self.cash -= notional + cost;
            let h = self
                .holdings
                .entry(code.into())
                .or_insert(Holding { shares: 0.0, price });
            h.shares += shares;
            h.price = price;
        } else {
            self.cash += notional - cost;
            if let Some(h) = self.holdings.get_mut(code) {
                h.shares -= shares;
                h.price = price;
            }
        }
        if self.cash.abs() < 1e-10 {
            self.cash = 0.0;
        }
        self.holdings.retain(|_, h| h.shares > 1e-10);
        self.total_cost += cost;
        self.turnover += notional / nav.max(1e-9);
        trades.push(TradeRecord {
            date,
            code: code.into(),
            side: side.into(),
            shares,
            price: Some(price),
            notional,
            cost,
            executed: true,
            reason: None,
            benchmark,
        });
        if requested_shares - shares > 1e-10 {
            trades.push(TradeRecord {
                date,
                code: code.into(),
                side: side.into(),
                shares: requested_shares - shares,
                price: Some(price),
                notional: 0.0,
                cost: 0.0,
                executed: false,
                reason: Some(
                    if side == "buy" {
                        "insufficient spendable cash"
                    } else {
                        "insufficient held shares"
                    }
                    .into(),
                ),
                benchmark,
            });
        }
        Ok(())
    }
    fn rebalance(
        &mut self,
        d: &DatasetReader,
        date: NaiveDate,
        targets: &[String],
        fee: f64,
        benchmark: bool,
        trades: &mut Vec<TradeRecord>,
    ) -> ResearchResult<()> {
        let target_set: HashSet<&str> = targets.iter().map(String::as_str).collect();
        let exits = self
            .holdings
            .iter()
            .filter(|(code, _)| !target_set.contains(code.as_str()))
            .map(|(code, h)| (code.clone(), h.shares))
            .collect::<Vec<_>>();
        for (code, shares) in exits {
            self.execute(d, date, &code, "sell", shares, fee, benchmark, trades)?;
        }
        if targets.is_empty() {
            return Ok(());
        }
        // Locked non-target positions remain in NAV and consume exposure; receivables
        // count in NAV but do not increase the money available for target orders.
        let budget = self.cash
            + self
                .holdings
                .iter()
                .filter(|(code, _)| target_set.contains(code.as_str()))
                .map(|(_, h)| h.shares * h.price)
                .sum::<f64>();
        // Solve the post-cost equal weight, rather than letting the last buy
        // absorb all fees and produce an unequal candidate benchmark.
        let mut order_states = Vec::with_capacity(targets.len());
        for code in targets {
            let current = self
                .holdings
                .get(code)
                .map(|h| h.shares * h.price)
                .unwrap_or(0.0);
            let status = d.status(code, date)?;
            let exact = d.bar(code, date)?.is_some();
            let can_buy = exact && status.as_ref().is_some_and(|s| s.is_listed && s.can_buy);
            let can_sell = exact && status.as_ref().is_some_and(|s| s.is_listed && s.can_sell);
            order_states.push((current, can_buy, can_sell));
        }
        let mut lower = 0.0;
        let mut upper = budget / targets.len() as f64;
        for _ in 0..64 {
            let value = (lower + upper) / 2.0;
            let cost = fee
                * order_states
                    .iter()
                    .map(|(current, buy, sell)| {
                        if (value > *current && *buy) || (value < *current && *sell) {
                            (value - current).abs()
                        } else {
                            0.0
                        }
                    })
                    .sum::<f64>();
            let allocations = order_states
                .iter()
                .map(|(current, _, sell)| {
                    // Unsellable overweight target shares are a funding floor,
                    // not cash which peers can consume in the nominal weight.
                    if !sell && *current > value {
                        *current
                    } else {
                        value
                    }
                })
                .sum::<f64>();
            if allocations + cost > budget {
                upper = value;
            } else {
                lower = value;
            }
        }
        let target_value = (lower + upper) / 2.0;
        for code in targets {
            if let Some(h) = self.holdings.get(code) {
                let excess = h.shares * h.price - target_value;
                if excess > 1e-8 {
                    self.execute(
                        d,
                        date,
                        code,
                        "sell",
                        excess / h.price,
                        fee,
                        benchmark,
                        trades,
                    )?;
                }
            }
        }
        for code in targets {
            let current = self
                .holdings
                .get(code)
                .map(|h| h.shares * h.price)
                .unwrap_or(0.0);
            let wanted = (target_value - current).max(0.0);
            if wanted > 1e-8 {
                let exact_price = d
                    .bar(code, date)?
                    .map(|bar| bar.close)
                    .filter(|price| price.is_finite() && *price > 0.0);
                let quantity = exact_price
                    .map(|price| wanted / price)
                    .filter(|shares| shares.is_finite());
                let Some(quantity) = quantity else {
                    // Retained valuation (including zero after cash detachment)
                    // is not an execution quote. Preserve the intended cash slot
                    // with explicitly unavailable quantity, without dividing it.
                    trades.push(TradeRecord { date, code: code.clone(), side: "buy".into(), shares: 0.0, price: exact_price, notional: wanted, cost: 0.0, executed: false, reason: Some("positive exact execution quote or share quantity unavailable; quantity unavailable".into()), benchmark });
                    continue;
                };
                self.execute(d, date, code, "buy", quantity, fee, benchmark, trades)?;
            }
        }
        Ok(())
    }
    fn exposures(
        &self,
        d: &DatasetReader,
        date: NaiveDate,
        benchmark: bool,
        records: &mut Vec<ExposureRecord>,
    ) -> ResearchResult<()> {
        for (code, h) in &self.holdings {
            let status = d.status(code, date)?;
            let exact = d.bar(code, date)?.is_some();
            records.push(ExposureRecord {
                date,
                code: code.clone(),
                shares: h.shares,
                last_price: h.price,
                locked: !exact || !status.is_some_and(|s| s.can_sell),
                benchmark,
            });
        }
        Ok(())
    }
}

pub fn evaluate(
    d: &DatasetReader,
    parameters: &ScoringParameters,
    c: &EvaluationConfig,
) -> ResearchResult<EvaluationReport> {
    crate::dataset::validate_config(c)?;
    gp_core::validate_scoring_parameters(parameters)?;
    let quality = d.validate(c)?;
    evaluate_audited(d, parameters, c, quality)
}
/// Only calibration calls this after auditing its immutable full window once.
pub(crate) fn evaluate_audited(
    d: &DatasetReader,
    parameters: &ScoringParameters,
    c: &EvaluationConfig,
    quality: QualityReport,
) -> ResearchResult<EvaluationReport> {
    crate::dataset::validate_config(c)?;
    gp_core::validate_scoring_parameters(parameters)?;
    if !quality.passed {
        return Err(ResearchError {
            code: "data_quality".into(),
            message: "strict PIT validation failed before evaluation".into(),
            details: quality
                .issues
                .iter()
                .chain(quality.missing_details.iter())
                .cloned()
                .collect(),
        });
    }
    let first = d.calendar().partition_point(|date| *date < c.start_date);
    let end = d.calendar().partition_point(|date| *date <= c.end_date);
    let parameter_fingerprint =
        crate::import::digest(serde_json::to_string(parameters)?.as_bytes());
    let run_id=crate::import::digest(serde_json::to_vec(&serde_json::json!({"format":"strict-pit-evaluation/v1","data":d.record().data_fingerprint,"core":gp_core::scoring_implementation_fingerprint(),"research":research_implementation_fingerprint(),"parameters":parameter_fingerprint,"config":c}))?.as_slice());
    let mut portfolio = Portfolio::new(c.initial_cash);
    let mut benchmark = Portfolio::new(c.initial_cash);
    let mut equity_curve = Vec::new();
    let mut benchmark_curve = Vec::new();
    let mut trades = Vec::new();
    let mut exposures = Vec::new();
    let mut folds: Vec<MonthlyFold> = Vec::new();
    let mut fold_base = c.initial_cash;
    let mut benchmark_base = c.initial_cash;
    let mut recent = Vec::<AdaptiveRecentExposure>::new();
    for index in first..end {
        let date = d.calendar()[index];
        let rebalance = index == first
            || date.month() != d.calendar()[index - 1].month()
            || date.year() != d.calendar()[index - 1].year();
        if rebalance {
            if let Some(fold) = folds.last_mut() {
                fold.evaluation_end_date = d.calendar()[index - 1];
                fold.net_return = period_return(portfolio.nav(), fold_base);
                fold.benchmark_return = period_return(benchmark.nav(), benchmark_base);
                fold.complete = month_complete(d, fold.selection_date, fold.evaluation_end_date, c);
            }
            fold_base = portfolio.nav();
            benchmark_base = benchmark.nav();
        }
        portfolio.actions(d, date)?;
        benchmark.actions(d, date)?;
        portfolio.mark(d, date)?;
        benchmark.mark(d, date)?;
        if rebalance {
            let signal = d.calendar()[index - 1];
            if c.signal_cutoff.is_some_and(|cutoff| signal > cutoff) {
                // A purged validation month retains the live portfolios and
                // monthly accounting, but obtains no new signals or labels.
                folds.push(MonthlyFold {
                    signal_date: signal,
                    selection_date: date,
                    evaluation_end_date: date,
                    selected_symbols: portfolio.holdings.keys().cloned().collect(),
                    eligible_symbol_count: benchmark.holdings.len(),
                    net_return: 0.0,
                    benchmark_return: 0.0,
                    precision_at_n: None,
                    complete: false,
                });
            } else {
                let features = d.features_on(signal, c)?;
                let benchmark_targets = features
                    .candidates
                    .iter()
                    .map(|r| r.stock.code.clone())
                    .collect::<Vec<_>>();
                let (universe, histories, indices) = d.frame(signal, c)?;
                let targets = if c.strategy == "adaptive" {
                    gp_core::adaptive_screen_stocks_with_parameters(
                        &universe,
                        &histories,
                        &indices,
                        &recent,
                        &d.request(signal, c),
                        parameters,
                    )?
                    .items
                    .iter()
                    .map(|s| s.stock.code.clone())
                    .collect::<Vec<_>>()
                } else {
                    let candidate_codes: HashSet<&str> =
                        benchmark_targets.iter().map(String::as_str).collect();
                    let eligible = universe
                        .into_iter()
                        .filter(|s| candidate_codes.contains(s.code.as_str()))
                        .collect::<Vec<_>>();
                    let criteria = ScreenCriteria {
                        limit: eligible.len(),
                        score_profile: "balanced".into(),
                        ..c.criteria.clone()
                    };
                    let screened = gp_core::screen_stocks(&eligible, &criteria);
                    let mut counts = BTreeMap::<String, usize>::new();
                    let mut targets = Vec::new();
                    for row in screened.items {
                        let count = counts.entry(row.stock.industry.clone()).or_default();
                        if *count < 3 {
                            *count += 1;
                            targets.push(row.stock.code);
                            if targets.len() == c.top_n {
                                break;
                            }
                        }
                    }
                    if targets.len() < c.top_n {
                        return Err(ResearchError::new(
                            "selection",
                            "legacy balanced cannot meet requested industry-diversified top N",
                        ));
                    }
                    targets
                };
                let labels = targets
                    .iter()
                    .map(|code| d.forward_return(code, signal, 20))
                    .collect::<ResearchResult<Vec<_>>>()?;
                let precision = if labels.iter().all(Option::is_some) && !labels.is_empty() {
                    Some(
                        labels.iter().filter(|r| r.is_some_and(|r| r > 0.0)).count() as f64
                            / labels.len() as f64,
                    )
                } else {
                    None
                };
                portfolio.rebalance(
                    d,
                    date,
                    &targets,
                    c.transaction_cost_bps / 10000.0,
                    false,
                    &mut trades,
                )?;
                benchmark.rebalance(
                    d,
                    date,
                    &benchmark_targets,
                    c.transaction_cost_bps / 10000.0,
                    true,
                    &mut trades,
                )?;
                recent = targets
                    .iter()
                    .map(|code| AdaptiveRecentExposure {
                        code: code.clone(),
                        trade_date: date.to_string(),
                        bucket: "primary".into(),
                    })
                    .collect();
                folds.push(MonthlyFold {
                    signal_date: signal,
                    selection_date: date,
                    evaluation_end_date: date,
                    selected_symbols: targets,
                    eligible_symbol_count: benchmark_targets.len(),
                    net_return: 0.0,
                    benchmark_return: 0.0,
                    precision_at_n: precision,
                    complete: false,
                });
            }
        }
        equity_curve.push(EquityPoint {
            date,
            equity: portfolio.nav(),
        });
        benchmark_curve.push(EquityPoint {
            date,
            equity: benchmark.nav(),
        });
        portfolio.exposures(d, date, false, &mut exposures)?;
        benchmark.exposures(d, date, true, &mut exposures)?;
    }
    if let Some(fold) = folds.last_mut() {
        fold.evaluation_end_date = d.calendar()[end - 1];
        fold.net_return = period_return(portfolio.nav(), fold_base);
        fold.benchmark_return = period_return(benchmark.nav(), benchmark_base);
        fold.complete = month_complete(d, fold.selection_date, fold.evaluation_end_date, c);
    }
    let metrics = portfolio_metrics(
        &equity_curve,
        c.initial_cash,
        portfolio.total_cost,
        portfolio.turnover,
    );
    let benchmark_metrics = portfolio_metrics(
        &benchmark_curve,
        c.initial_cash,
        benchmark.total_cost,
        benchmark.turnover,
    );
    let report = EvaluationReport{run_id,dataset_id:d.record().dataset_id.clone(),data_fingerprint:d.record().data_fingerprint.clone(),implementation_fingerprint:gp_core::scoring_implementation_fingerprint().into(),research_fingerprint:research_implementation_fingerprint().into(),parameter_fingerprint,parameters:parameters.clone(),config:c.clone(),data_quality:quality,metrics,benchmark_metrics,monthly_folds:folds,equity_curve,benchmark_curve,notes:vec!["Strict PIT research report; evaluation success does not qualify release/promotion.".into(),"Candidate equal-weight benchmark uses the identical historical core feature pool, raw prices/actions, trade restrictions and costs.".into(),"Fractional shares model proportional close-price orders; no exchange lot-size or impact/slippage model.".into(),"Precision is the fraction of selected symbols with positive matured total return at 20 sessions; unavailable endpoints remain null.".into()],trades,exposures};
    validate_report_numbers(&report)?;
    Ok(report)
}
fn period_return(value: f64, base: f64) -> f64 {
    if base > 0.0 {
        value / base - 1.0
    } else {
        0.0
    }
}

// Serialization maps nonfinite f64 values to null. Reject those results at the
// public report boundary rather than silently changing finite/optional evidence.
fn validate_report_numbers(report: &EvaluationReport) -> ResearchResult<()> {
    let check = |field: String, value: f64| {
        if value.is_finite() {
            Ok(())
        } else {
            Err(ResearchError {
                code: "numeric".into(),
                message: "strict evaluation produced a nonfinite report value".into(),
                details: vec![field],
            })
        }
    };
    check(
        "data_quality.slow_factor_coverage".into(),
        report.data_quality.slow_factor_coverage,
    )?;
    for (name, metrics) in [
        ("metrics", &report.metrics),
        ("benchmark_metrics", &report.benchmark_metrics),
    ] {
        for (field, value) in [
            ("total_return", metrics.total_return),
            ("annualized_turnover", metrics.annualized_turnover),
            ("total_transaction_cost", metrics.total_transaction_cost),
        ] {
            check(format!("{name}.{field}"), value)?;
        }
        for (field, value) in [
            ("annualized_return", metrics.annualized_return),
            ("max_drawdown", metrics.max_drawdown),
        ] {
            if let Some(value) = value {
                check(format!("{name}.{field}"), value)?;
            }
        }
    }
    for (index, trade) in report.trades.iter().enumerate() {
        for (field, value) in [
            ("shares", trade.shares),
            ("notional", trade.notional),
            ("cost", trade.cost),
        ] {
            check(format!("trades[{index}].{field}"), value)?;
        }
        if let Some(price) = trade.price {
            check(format!("trades[{index}].price"), price)?;
        }
    }
    for (index, exposure) in report.exposures.iter().enumerate() {
        check(format!("exposures[{index}].shares"), exposure.shares)?;
        check(
            format!("exposures[{index}].last_price"),
            exposure.last_price,
        )?;
    }
    for (name, curve) in [
        ("equity_curve", &report.equity_curve),
        ("benchmark_curve", &report.benchmark_curve),
    ] {
        for (index, point) in curve.iter().enumerate() {
            check(format!("{name}[{index}].equity"), point.equity)?;
        }
    }
    for (index, fold) in report.monthly_folds.iter().enumerate() {
        check(
            format!("monthly_folds[{index}].net_return"),
            fold.net_return,
        )?;
        check(
            format!("monthly_folds[{index}].benchmark_return"),
            fold.benchmark_return,
        )?;
        if let Some(precision) = fold.precision_at_n {
            check(format!("monthly_folds[{index}].precision_at_n"), precision)?;
        }
    }
    Ok(())
}

fn month_complete(
    d: &DatasetReader,
    start: NaiveDate,
    end: NaiveDate,
    c: &EvaluationConfig,
) -> bool {
    let first = d
        .calendar()
        .iter()
        .find(|date| date.year() == start.year() && date.month() == start.month());
    let last = d
        .calendar()
        .iter()
        .rev()
        .find(|date| date.year() == start.year() && date.month() == start.month());
    let month_end = if start.month() == 12 {
        NaiveDate::from_ymd_opt(start.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1).unwrap()
    }
    .pred_opt()
    .unwrap();
    first == Some(&start)
        && last == Some(&end)
        && c.start_date <= start
        && c.end_date >= end
        && d.record().start_date <= start.with_day(1).unwrap()
        && d.covers_end_date(month_end)
}
fn portfolio_metrics(
    curve: &[EquityPoint],
    initial: f64,
    cost: f64,
    turnover: f64,
) -> PortfolioMetrics {
    let total = curve
        .last()
        .map(|p| p.equity / initial - 1.0)
        .unwrap_or(0.0);
    let mut peak = initial;
    let mut drawdown: f64 = 0.0;
    for p in curve {
        peak = peak.max(p.equity);
        drawdown = drawdown.max((peak - p.equity) / peak);
    }
    let years = curve.len() as f64 / 252.0;
    PortfolioMetrics {
        total_return: total,
        annualized_return: if years > 0.0 && total > -1.0 {
            Some((1.0 + total).powf(1.0 / years) - 1.0)
        } else {
            None
        },
        max_drawdown: if curve.is_empty() {
            None
        } else {
            Some(drawdown)
        },
        annualized_turnover: if years > 0.0 { turnover / years } else { 0.0 },
        total_transaction_cost: cost,
    }
}

#[cfg(test)]
#[path = "../tests/internal/order_accounting.rs"]
mod order_accounting_tests;
