use crate::industry_rank::industry_rank_values;
use crate::market_data::StockItem;
use crate::numeric::round6;
use crate::rates::{percentage_points, ratio_percentage_points};
use crate::screening::classification::{
    concept_group_for_stock, is_cold_sector, theme_match_for_stock,
};
use crate::screening::criteria::finite_positive;
use crate::screening::types::{ScoreContribution, ScreenedStock};
use std::collections::{BTreeMap, HashMap};

pub(crate) const SCREEN_SCORE_SCALE: f64 = 20.0;

pub(crate) fn score_stock(
    stock: &StockItem,
    reasons: &[String],
    score_profile: &str,
) -> ScreenedStock {
    let theme = theme_match_for_stock(stock);
    let cold = is_cold_sector(&stock.industry);
    let quality_profile = normalized_score_profile(score_profile) == "quality";
    let cold_penalty = cold && !quality_profile;
    let theme_score = theme.as_ref().map(|(_, _, score)| *score).unwrap_or(0.35);
    let fundamental = fundamental_score(stock);
    let valuation = valuation_score(stock);
    let size = size_score(stock);
    let risk = risk_score(stock, cold_penalty);
    let data_quality = data_quality_score(stock);
    let market_heat = market_heat_score(stock);
    let overheat_penalty = overheat_penalty_score(stock);

    let scoring_factors = BTreeMap::from([
        ("theme".to_string(), theme_score),
        ("fundamental".to_string(), fundamental),
        ("valuation".to_string(), valuation),
        ("size".to_string(), size),
        ("risk".to_string(), risk),
        ("data_quality".to_string(), data_quality),
        ("market_heat".to_string(), market_heat),
        ("overheat_penalty".to_string(), overheat_penalty),
    ]);

    let quality_score = profile_score(&scoring_factors, "quality");
    let trend_score = profile_score(&scoring_factors, "trend");
    let balanced_score = profile_score(&scoring_factors, "balanced");
    let selected_score = profile_score(&scoring_factors, score_profile);
    let score = (selected_score * SCREEN_SCORE_SCALE).clamp(0.0, SCREEN_SCORE_SCALE);

    let mut public_factors = BTreeMap::from([
        ("theme".to_string(), theme_score),
        ("fundamental".to_string(), fundamental),
        ("valuation".to_string(), valuation),
        ("size".to_string(), size),
        ("risk".to_string(), risk),
        ("data_quality".to_string(), data_quality),
        ("quality_score".to_string(), quality_score),
        ("trend_score".to_string(), trend_score),
        ("balanced_score".to_string(), balanced_score),
    ]);
    if uses_market_heat_score_profile(score_profile) {
        public_factors.insert("market_heat".to_string(), market_heat);
        public_factors.insert("overheat_penalty".to_string(), overheat_penalty);
    }

    let profile_theme = if quality_profile {
        None
    } else {
        theme.as_ref()
    };
    let mut all_reasons = reasons.to_vec();
    all_reasons.extend(factor_reasons(
        profile_theme,
        cold_penalty,
        &scoring_factors,
    ));
    let reason_tags = build_reason_tags(profile_theme, &scoring_factors);
    let risk_tags = build_risk_tags(stock, cold_penalty, &scoring_factors);
    let suitable_periods = suitable_periods_for_scores(quality_score, trend_score, risk);
    let score_breakdown = profile_score_breakdown(score_profile, &scoring_factors);
    let score_explanation = explain_score(&score_breakdown);
    let rounded_scores = public_factors
        .into_iter()
        .map(|(key, value)| (key, round6(value)))
        .collect();

    ScreenedStock {
        industry_sample_count: None,
        stock: stock.clone(),
        score: round6(score),
        reasons: all_reasons,
        quality_score: round6(quality_score * SCREEN_SCORE_SCALE),
        trend_score: round6(trend_score * SCREEN_SCORE_SCALE),
        risk_score: round6(risk * SCREEN_SCORE_SCALE),
        balanced_score: round6(balanced_score * SCREEN_SCORE_SCALE),
        factor_scores: rounded_scores,
        score_breakdown,
        score_explanation,
        reason_tags,
        risk_tags,
        suitable_periods,
        concept: Some(concept_group_for_stock(stock)),
        theme_category: theme.map(|(key, _, _)| key.to_string()),
    }
}

pub(crate) fn fundamental_score(stock: &StockItem) -> f64 {
    let roe_percent = stock.roe.and_then(ratio_percentage_points);
    let roe_score = match roe_percent {
        None => 0.5,
        Some(value) if value >= 15.0 => 1.0,
        Some(value) if value >= 8.0 => 0.72 + (value - 8.0) / 7.0 * 0.18,
        Some(value) if value >= 0.0 => 0.42 + value / 8.0 * 0.25,
        Some(_) => 0.2,
    };
    let dividend = stock.dividend_yield.and_then(ratio_percentage_points);
    let dividend_bonus = dividend
        .map(|value| value.max(0.0).min(6.0) / 6.0 * 0.12)
        .unwrap_or(0.0);
    let mut quality_bonus = 0.0;
    if stock
        .deducted_net_profit_billion
        .map(|value| value > 0.0)
        .unwrap_or(false)
    {
        quality_bonus += 0.08;
    }
    if stock
        .deducted_net_profit_growth_rate
        .and_then(percentage_points)
        .map(|value| value >= 10.0)
        .unwrap_or(false)
    {
        quality_bonus += 0.08;
    }
    (roe_score + dividend_bonus + quality_bonus).clamp(0.0, 1.0)
}

fn valuation_score(stock: &StockItem) -> f64 {
    ((pe_score(stock.pe) + pb_score(stock.pb)) / 2.0).clamp(0.0, 1.0)
}

pub(crate) fn pe_score(value: Option<f64>) -> f64 {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return 0.52;
    };
    match value {
        value if value <= 0.0 => 0.25,
        value if value <= 15.0 => 1.0,
        value if value <= 30.0 => 0.72,
        value if value <= 60.0 => 0.45,
        _ => 0.28,
    }
}

pub(crate) fn pb_score(value: Option<f64>) -> f64 {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return 0.52;
    };
    match value {
        value if value <= 0.0 => 0.25,
        value if value <= 1.5 => 1.0,
        value if value <= 3.0 => 0.74,
        value if value <= 6.0 => 0.5,
        value if value <= 10.0 => 0.34,
        _ => 0.22,
    }
}

fn size_score(stock: &StockItem) -> f64 {
    match finite_positive(stock.market_cap_billion) {
        None => 0.55,
        Some(value) if value < 20.0 => 0.36,
        Some(value) if value < 100.0 => 0.68,
        Some(value) if value < 500.0 => 0.88,
        Some(value) if value < 2000.0 => 0.78,
        Some(_) => 0.62,
    }
}

pub(crate) fn data_quality_score(stock: &StockItem) -> f64 {
    if let Some(coverage) = &stock.financial_coverage {
        return (4 - coverage.missing_fields().len()) as f64 / 4.0;
    }
    let mut score: f64 = 0.0;
    if stock.roe.and_then(ratio_percentage_points).is_some() {
        score += 0.30;
    }
    if finite_positive(stock.pe).is_some() {
        score += 0.20;
    }
    if finite_positive(stock.pb).is_some() {
        score += 0.15;
    }
    if finite_positive(stock.market_cap_billion).is_some() {
        score += 0.10;
    }
    if stock
        .deducted_net_profit_billion
        .filter(|value| value.is_finite())
        .is_some()
    {
        score += 0.10;
    }
    if stock
        .deducted_net_profit_growth_rate
        .and_then(percentage_points)
        .is_some()
    {
        score += 0.10;
    }
    if stock
        .dividend_yield
        .and_then(ratio_percentage_points)
        .filter(|value| *value >= 0.0)
        .is_some()
    {
        score += 0.05;
    }
    round6(score.clamp(0.0, 1.0))
}

fn risk_score(stock: &StockItem, cold: bool) -> f64 {
    if stock.is_st {
        0.05
    } else if cold {
        0.36
    } else {
        1.0
    }
}

pub(super) fn normalized_score_profile(score_profile: &str) -> &'static str {
    match score_profile.trim().to_ascii_lowercase().as_str() {
        "rotation" => "rotation",
        "trend" | "trend_swing" => "trend",
        "quality" => "quality",
        _ => "balanced",
    }
}

fn uses_market_heat_score_profile(score_profile: &str) -> bool {
    matches!(
        normalized_score_profile(score_profile),
        "balanced" | "trend" | "rotation"
    )
}

pub(crate) fn profile_score(factor_scores: &BTreeMap<String, f64>, score_profile: &str) -> f64 {
    let theme = factor_scores.get("theme").copied().unwrap_or(0.35);
    let fundamental = factor_scores.get("fundamental").copied().unwrap_or(0.5);
    let valuation = factor_scores.get("valuation").copied().unwrap_or(0.5);
    let size = factor_scores.get("size").copied().unwrap_or(0.55);
    let risk = factor_scores.get("risk").copied().unwrap_or(1.0);
    let data_quality = factor_scores.get("data_quality").copied().unwrap_or(0.0);
    let market_heat = factor_scores.get("market_heat").copied().unwrap_or(0.5);
    let overheat_penalty = factor_scores
        .get("overheat_penalty")
        .copied()
        .unwrap_or(0.0);

    let raw = match normalized_score_profile(score_profile) {
        "rotation" => {
            theme * 0.18
                + fundamental * 0.18
                + valuation * 0.16
                + market_heat * 0.22
                + size * 0.08
                + risk * 0.18
                - overheat_penalty * 0.10
        }
        "trend" => {
            theme * 0.16
                + fundamental * 0.16
                + valuation * 0.12
                + market_heat * 0.30
                + size * 0.08
                + risk * 0.18
                - overheat_penalty * 0.16
        }
        "quality" => {
            fundamental * 0.40 + valuation * 0.28 + size * 0.10 + risk * 0.10 + data_quality * 0.12
        }
        _ => {
            theme * 0.16
                + fundamental * 0.26
                + valuation * 0.20
                + market_heat * 0.16
                + size * 0.08
                + risk * 0.22
                - overheat_penalty * 0.08
        }
    };
    raw.clamp(0.0, 1.0)
}

fn market_heat_score(stock: &StockItem) -> f64 {
    let change_pct = stock
        .change_pct
        .and_then(ratio_percentage_points)
        .unwrap_or(0.0);
    let positive_momentum = if change_pct >= 0.0 {
        (change_pct.min(10.0) / 10.0).clamp(0.0, 1.0)
    } else {
        (0.45 + change_pct.max(-10.0) / 20.0).clamp(0.0, 0.45)
    };
    let volume_ratio = stock
        .volume_ratio
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| ((value.min(3.0) - 1.0).max(0.0) / 2.0).clamp(0.0, 1.0))
        .unwrap_or(0.45);
    let turnover = stock
        .turnover_rate
        .and_then(ratio_percentage_points)
        .map(|value| (value.max(0.0).min(8.0) / 8.0).clamp(0.0, 1.0))
        .unwrap_or(0.45);
    let amount = stock
        .amount
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| ((value / 1_000_000_000.0).min(1.0)).clamp(0.0, 1.0))
        .unwrap_or(0.45);
    (positive_momentum * 0.46 + volume_ratio * 0.24 + turnover * 0.18 + amount * 0.12)
        .clamp(0.0, 1.0)
}

fn overheat_penalty_score(stock: &StockItem) -> f64 {
    let change_pct = stock
        .change_pct
        .and_then(ratio_percentage_points)
        .unwrap_or(0.0);
    let turnover = stock
        .turnover_rate
        .and_then(ratio_percentage_points)
        .unwrap_or(0.0);
    let volume_ratio = finite_positive(stock.volume_ratio).unwrap_or(0.0);
    let change_penalty = if change_pct > 7.0 {
        ((change_pct - 7.0) / 5.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let turnover_penalty = if turnover > 8.0 {
        ((turnover - 8.0) / 8.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let volume_penalty = if volume_ratio > 3.0 {
        ((volume_ratio - 3.0) / 2.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (change_penalty * 0.50 + turnover_penalty * 0.28 + volume_penalty * 0.22).clamp(0.0, 1.0)
}

fn profile_score_breakdown(
    score_profile: &str,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<ScoreContribution> {
    let weights: &[(&str, &str, f64)] = match normalized_score_profile(score_profile) {
        "quality" => &[
            ("fundamental", "质量盈利", 0.40),
            ("valuation", "估值安全", 0.28),
            ("size", "规模流动性", 0.10),
            ("risk", "基础风险", 0.10),
            ("data_quality", "数据完整度", 0.12),
        ],
        "trend" => &[
            ("theme", "主题匹配", 0.16),
            ("fundamental", "质量底座", 0.16),
            ("valuation", "估值约束", 0.12),
            ("market_heat", "趋势热度", 0.30),
            ("size", "规模流动性", 0.08),
            ("risk", "风险控制", 0.18),
            ("overheat_penalty", "过热惩罚", -0.16),
        ],
        "rotation" => &[
            ("theme", "主题匹配", 0.18),
            ("fundamental", "质量底座", 0.18),
            ("valuation", "估值约束", 0.16),
            ("market_heat", "轮动热度", 0.22),
            ("size", "规模流动性", 0.08),
            ("risk", "风险控制", 0.18),
            ("overheat_penalty", "过热惩罚", -0.10),
        ],
        _ => &[
            ("theme", "主题匹配", 0.16),
            ("fundamental", "质量盈利", 0.26),
            ("valuation", "估值安全", 0.20),
            ("market_heat", "趋势热度", 0.16),
            ("size", "规模流动性", 0.08),
            ("risk", "风险控制", 0.22),
            ("overheat_penalty", "过热惩罚", -0.08),
        ],
    };
    let mut breakdown = weights
        .iter()
        .map(|(key, label, weight)| {
            let value = factor_scores
                .get(*key)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let contribution = value * weight * SCREEN_SCORE_SCALE;
            let tone = if *weight < 0.0 {
                if value >= 0.5 {
                    "weak"
                } else {
                    "neutral"
                }
            } else if value >= 0.74 {
                "strong"
            } else if value >= 0.55 {
                "positive"
            } else if value <= 0.35 {
                "weak"
            } else {
                "neutral"
            };
            ScoreContribution {
                key: (*key).to_string(),
                label: (*label).to_string(),
                value: Some(round6(value * SCREEN_SCORE_SCALE)),
                contribution: Some(round6(contribution)),
                tone: tone.to_string(),
            }
        })
        .collect::<Vec<_>>();
    reconcile_score_breakdown(
        &mut breakdown,
        profile_score(factor_scores, score_profile) * SCREEN_SCORE_SCALE,
    );
    breakdown
}

fn build_reason_tags(
    theme: Option<&(&'static str, &'static str, f64)>,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some((_, label, _)) = theme {
        tags.push(format!("主题:{label}"));
    }
    if factor_scores.get("fundamental").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("质量较强".to_string());
    }
    if factor_scores.get("valuation").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("估值有安全边际".to_string());
    }
    if factor_scores.get("market_heat").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("趋势热度较高".to_string());
    }
    if factor_scores.get("risk").copied().unwrap_or(0.0) >= 0.72 {
        tags.push("风险约束通过".to_string());
    }
    if factor_scores.get("data_quality").copied().unwrap_or(0.0) >= 0.85 {
        tags.push("核心数据完整".to_string());
    }
    tags
}

fn build_risk_tags(
    stock: &StockItem,
    cold: bool,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<String> {
    let mut tags = Vec::new();
    if stock.is_st {
        tags.push("ST 风险".to_string());
    }
    if cold {
        tags.push("低热度板块降权".to_string());
    }
    if factor_scores.get("valuation").copied().unwrap_or(0.0) <= 0.38 {
        tags.push("估值偏高".to_string());
    }
    if factor_scores.get("fundamental").copied().unwrap_or(0.0) <= 0.38 {
        tags.push("质量偏弱".to_string());
    }
    if let Some(coverage) = &stock.financial_coverage {
        let missing = coverage.missing_fields();
        if !missing.is_empty() {
            tags.push(format!("财务缺项：{}", missing.join("、")));
        }
        if stock
            .latest_eps
            .is_some_and(|value| value.is_finite() && value < 0.0)
        {
            tags.push("最新报告期亏损".to_owned());
        }
        if stock
            .pe
            .is_some_and(|value| value.is_finite() && value < 0.0)
        {
            tags.push("PE为负，盈利估值不适用".to_owned());
        }
    } else if factor_scores.get("data_quality").copied().unwrap_or(0.0) < 0.70 {
        tags.push("核心财务数据缺失较多".to_string());
    }
    if factor_scores
        .get("overheat_penalty")
        .copied()
        .unwrap_or(0.0)
        >= 0.35
    {
        tags.push("短线过热".to_string());
    }
    if tags.is_empty() {
        tags.push("未识别明确高风险".to_string());
    }
    tags
}

fn suitable_periods_for_scores(quality_score: f64, trend_score: f64, risk: f64) -> Vec<String> {
    let mut periods = Vec::new();
    if trend_score >= 0.62 && risk >= 0.5 {
        periods.push("5-10日观察".to_string());
    }
    if quality_score >= 0.58 && risk >= 0.5 {
        periods.push("20-60日观察".to_string());
    }
    if periods.is_empty() {
        periods.push("仅观察验证".to_string());
    }
    periods
}

fn factor_reasons(
    theme: Option<&(&'static str, &'static str, f64)>,
    cold: bool,
    factor_scores: &BTreeMap<String, f64>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some((_, label, _)) = theme {
        reasons.push(format!("{}{label}", "\u{4e3b}\u{9898}:"));
    }
    if factor_scores.get("valuation").copied().unwrap_or(0.0) >= 0.74 {
        reasons.push("\u{4f30}\u{503c}\u{8f83}\u{4f4e}".to_string());
    } else if factor_scores.get("valuation").copied().unwrap_or(0.0) <= 0.38 {
        reasons.push("\u{4f30}\u{503c}\u{504f}\u{9ad8}".to_string());
    }
    if factor_scores.get("fundamental").copied().unwrap_or(0.0) >= 0.72 {
        reasons.push("\u{57fa}\u{672c}\u{9762}\u{8f83}\u{5f3a}".to_string());
    }
    if factor_scores.get("data_quality").copied().unwrap_or(0.0) >= 0.85 {
        reasons.push("核心财务数据完整".to_string());
    }
    if factor_scores.get("market_heat").copied().unwrap_or(0.0) >= 0.72 {
        reasons.push("\u{8f6e}\u{52a8}\u{70ed}\u{5ea6}\u{8f83}\u{9ad8}".to_string());
    }
    if cold {
        reasons.push("\u{4f4e}\u{70ed}\u{5ea6}\u{964d}\u{6743}".to_string());
    }
    reasons
}

fn explain_score(breakdown: &[ScoreContribution]) -> String {
    let mut contributions = breakdown
        .iter()
        .filter(|entry| {
            entry.key != "score_clamp_adjustment"
                && entry.contribution.is_some_and(|value| value > 0.0)
        })
        .collect::<Vec<_>>();
    contributions.sort_by(|a, b| {
        b.contribution
            .unwrap()
            .total_cmp(&a.contribution.unwrap())
            .then_with(|| a.key.cmp(&b.key))
    });
    let mut parts = contributions
        .into_iter()
        .take(3)
        .map(|entry| format!("{}贡献 {:.2}分", entry.label, entry.contribution.unwrap()))
        .collect::<Vec<_>>();
    parts.extend(
        breakdown
            .iter()
            .filter(|entry| entry.contribution.is_some_and(|value| value < 0.0))
            .map(|entry| format!("{} {:.2}分", entry.label, entry.contribution.unwrap())),
    );
    format!("{}。", parts.join("；"))
}

fn reconcile_score_breakdown(breakdown: &mut Vec<ScoreContribution>, score: f64) {
    let sum = breakdown
        .iter()
        .filter_map(|entry| entry.contribution)
        .sum::<f64>();
    let delta = round6(score) - sum;
    if delta.abs() > 0.00002 {
        breakdown.push(ScoreContribution {
            key: "score_clamp_adjustment".to_string(),
            label: "分数上下限调整".to_string(),
            value: None,
            contribution: Some(delta),
            tone: "neutral".to_string(),
        });
    } else if let Some(last) = breakdown.last_mut() {
        last.contribution = last.contribution.map(|value| value + delta);
    }
}

pub(super) fn apply_industry_relative_scores(screened: &mut [ScreenedStock], score_profile: &str) {
    let quality_values = industry_percentiles(screened, "fundamental");
    let valuation_values = industry_percentiles(screened, "valuation");
    let profile = normalized_score_profile(score_profile);

    for item in screened.iter_mut() {
        let quality_pct = quality_values.get(&item.stock.code).copied().unwrap_or(0.5);
        let valuation_pct = valuation_values
            .get(&item.stock.code)
            .copied()
            .unwrap_or(0.5);
        let relative_score = ((quality_pct + valuation_pct) / 2.0).clamp(0.0, 1.0);
        let old_quality = item.quality_score;
        let old_balanced = item.balanced_score;
        item.quality_score = round6(
            (old_quality * 0.82 + relative_score * SCREEN_SCORE_SCALE * 0.18)
                .clamp(0.0, SCREEN_SCORE_SCALE),
        );
        item.balanced_score = round6(
            (old_balanced * 0.88 + relative_score * SCREEN_SCORE_SCALE * 0.12)
                .clamp(0.0, SCREEN_SCORE_SCALE),
        );
        item.factor_scores
            .insert("industry_quality_pct".to_string(), round6(quality_pct));
        item.factor_scores
            .insert("industry_valuation_pct".to_string(), round6(valuation_pct));
        item.factor_scores.insert(
            "industry_relative_score".to_string(),
            round6(relative_score),
        );
        item.reason_tags.push("行业内相对评分".to_string());

        item.score = match profile {
            "quality" => item.quality_score,
            "balanced" => item.balanced_score,
            _ => item.score,
        };
        if matches!(profile, "quality" | "balanced") {
            let retained = if profile == "quality" { 0.82 } else { 0.88 };
            for entry in &mut item.score_breakdown {
                entry.contribution = entry.contribution.map(|value| round6(value * retained));
            }
            item.score_breakdown.push(ScoreContribution {
                key: "industry_relative_score".to_string(),
                label: "行业内相对评分".to_string(),
                value: Some(round6(relative_score * SCREEN_SCORE_SCALE)),
                contribution: Some(round6(
                    relative_score * SCREEN_SCORE_SCALE * (1.0 - retained),
                )),
                tone: "neutral".to_string(),
            });
            reconcile_score_breakdown(&mut item.score_breakdown, item.score);
            item.score_explanation = explain_score(&item.score_breakdown);
        }
    }
}

fn industry_percentiles(screened: &[ScreenedStock], factor_key: &str) -> HashMap<String, f64> {
    industry_rank_values(screened.iter().map(|item| {
        (
            item.stock.code.clone(),
            item.stock.industry.clone(),
            item.factor_scores.get(factor_key).copied().unwrap_or(0.5),
        )
    }))
    .into_iter()
    .map(|(code, (rank, _))| (code, rank))
    .collect()
}

pub(crate) fn score_contribution(
    key: &str,
    label: &str,
    value: f64,
    contribution: f64,
    strong_threshold: f64,
    positive_threshold: f64,
) -> ScoreContribution {
    let tone = if value >= strong_threshold {
        "strong"
    } else if value >= positive_threshold {
        "positive"
    } else if value <= 0.0 {
        "weak"
    } else {
        "neutral"
    };
    ScoreContribution {
        key: key.to_string(),
        label: label.to_string(),
        value: Some(round6(value)),
        contribution: Some(round6(contribution)),
        tone: tone.to_string(),
    }
}
