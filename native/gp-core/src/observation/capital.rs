use crate::capital_evidence::{CapitalEvidenceItem, CapitalEvidenceResult, CapitalEvidenceSection};
use crate::dates::capital_trade_date;
use crate::market_data::{normalize_stock_code, StockItem};
use crate::numeric::round2;
use crate::observation::financial::{format_number, format_percent};
use crate::observation::types::FinancialIndicators;
use crate::trend::types::{TrendIndicatorPoint, TrendIndicatorResult, TrendIndicatorSignal};
use chrono::Local;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub(super) fn build_observation_capital_evidence(
    stock: &StockItem,
    _financial_indicators: &FinancialIndicators,
    trend: Option<&TrendIndicatorResult>,
    end_date: &str,
    provided: Option<CapitalEvidenceResult>,
) -> CapitalEvidenceResult {
    const FUND_FLOW_WEIGHT: f64 = 0.35;
    const INSTITUTION_WEIGHT: f64 = 0.25;
    const NEWS_WEIGHT: f64 = 0.15;
    const TECHNICAL_WEIGHT: f64 = 0.25;

    let as_of_trade_date = provided
        .as_ref()
        .and_then(|evidence| evidence.as_of_trade_date.as_deref())
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| capital_trade_date(value).or_else(|| Some(value.trim().to_string())))
        .or_else(|| capital_trade_date(end_date))
        .or_else(|| trend.map(|trend| trend.signal.date.clone()))
        .unwrap_or_else(|| Local::now().date_naive().format("%Y-%m-%d").to_string());

    let mut items = provided
        .as_ref()
        .map(|evidence| evidence.items.clone())
        .unwrap_or_default();
    items
        .retain(|item| item.category != "external_status" && item.category != "technical_behavior");

    let technical_score = trend.map(technical_capital_score);
    if let Some(trend) = trend {
        if !capital_bucket_has_evidence(&items, ["fund_flow", "", ""]) {
            if let Some(item) = local_fund_flow_proxy_evidence_item(trend) {
                items.push(item);
            }
        }
        items.push(technical_capital_evidence_item(
            trend,
            technical_score.unwrap_or(50.0),
        ));
    }
    if !capital_bucket_has_evidence(&items, ["institution_lhb", "institution_lhb_status", ""]) {
        items.push(institution_status_evidence_item(&as_of_trade_date));
    }
    if !capital_bucket_has_evidence(&items, ["news_rag", "community_sentiment", ""]) {
        items.push(message_sentiment_status_evidence_item(&as_of_trade_date));
    }
    dedup_capital_items(&mut items);

    let buckets = [
        (
            "资金流",
            FUND_FLOW_WEIGHT,
            ["fund_flow", "", ""],
            ["fund_flow", "", ""],
        ),
        (
            "机构席位",
            INSTITUTION_WEIGHT,
            ["institution_lhb", "institution_lhb_status", ""],
            ["institution_lhb", "", ""],
        ),
        (
            "消息情绪",
            NEWS_WEIGHT,
            [
                "news_rag",
                "community_sentiment",
                "message_sentiment_status",
            ],
            ["news_rag", "community_sentiment", ""],
        ),
        (
            "技术推断",
            TECHNICAL_WEIGHT,
            ["technical_behavior", "", ""],
            ["technical_behavior", "", ""],
        ),
    ];
    let mut weighted_sum = 0.0;
    let mut total_weight: f64 = 0.0;
    let mut contributions = BTreeMap::new();
    let mut scored_labels = Vec::new();
    let mut evidence_labels = Vec::new();
    for (label, weight, score_categories, evidence_categories) in buckets {
        let score = capital_bucket_score(&items, score_categories);
        let available = capital_bucket_has_evidence(&items, evidence_categories);
        if score.is_some() {
            scored_labels.push(label.to_string());
        }
        if available {
            evidence_labels.push(label.to_string());
        }
        let bucket_score = score.unwrap_or(50.0);
        weighted_sum += bucket_score * weight;
        total_weight += weight;
        contributions.insert(
            label.to_string(),
            json!({
                "score": score.map(round2),
                "weight": weight,
                "available": available,
            }),
        );
    }
    let composite_score = round2(weighted_sum / total_weight.max(0.01));
    let model_used = provided
        .as_ref()
        .map(|evidence| evidence.model_used)
        .unwrap_or(false);
    let confidence = capital_evidence_confidence(&items, technical_score.is_some());
    let sections = build_capital_evidence_sections(&items, &contributions);
    let freshness = provided
        .as_ref()
        .map(|evidence| evidence.freshness.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("refreshed")
        .to_string();
    let mut notes = Vec::new();
    if let Some(provided) = provided.as_ref() {
        for note in &provided.notes {
            push_capital_note(&mut notes, note);
        }
    }
    if !model_used {
        push_capital_note(
            &mut notes,
            "未调用模型，综合资金证据分由本地规则按资金流、机构席位、消息情绪和技术推断四类权重生成。",
        );
    }
    if !capital_bucket_has_evidence(&items, ["fund_flow", "", ""]) {
        push_capital_note(
            &mut notes,
            "资金流外部证据暂缺，本次按中性权重保留该项，不把缺失数据当作流入或流出结论。",
        );
    }
    if !items.iter().any(|item| item.category == "institution_lhb") {
        push_capital_note(
            &mut notes,
            "龙虎榜机构席位未命中或未提供，仅表示当前证据源没有可展示记录，不代表机构没有交易。",
        );
    }
    if !capital_bucket_has_evidence(&items, ["news_rag", "community_sentiment", ""]) {
        push_capital_note(
            &mut notes,
            "消息情绪证据暂缺，当前不对新闻或社区情绪加减分。",
        );
    }

    CapitalEvidenceResult {
        stock_code: normalize_stock_code(&stock.code),
        generated_at: Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        composite_score: Some(composite_score),
        confidence,
        model_used,
        as_of_trade_date: Some(as_of_trade_date),
        freshness,
        contributions,
        summary: Some(capital_evidence_summary(
            composite_score,
            &scored_labels,
            &evidence_labels,
        )),
        sections,
        items,
        notes,
    }
}

fn dedup_capital_items(items: &mut Vec<CapitalEvidenceItem>) {
    let mut seen = HashSet::new();
    items.retain(|item| {
        let key = format!(
            "{}|{}|{}|{}",
            item.category,
            item.source,
            item.title,
            item.date.as_deref().unwrap_or("")
        );
        seen.insert(key)
    });
}

fn capital_item_matches<const N: usize>(item: &CapitalEvidenceItem, categories: [&str; N]) -> bool {
    categories
        .iter()
        .any(|category| !category.is_empty() && *category == item.category.as_str())
}

fn capital_bucket_has_evidence<const N: usize>(
    items: &[CapitalEvidenceItem],
    categories: [&str; N],
) -> bool {
    items
        .iter()
        .any(|item| capital_item_matches(item, categories))
}

fn capital_bucket_score<const N: usize>(
    items: &[CapitalEvidenceItem],
    categories: [&str; N],
) -> Option<f64> {
    let mut total = 0.0;
    let mut count = 0.0;
    for item in items
        .iter()
        .filter(|item| capital_item_matches(item, categories))
    {
        if let Some(score) = item.score.filter(|value| value.is_finite()) {
            total += score;
            count += 1.0;
        }
    }
    if count > 0.0 {
        Some(round2(total / count))
    } else {
        None
    }
}

fn capital_evidence_confidence(items: &[CapitalEvidenceItem], has_technical: bool) -> String {
    let scored_external_count = [
        ["fund_flow", "", ""],
        ["institution_lhb", "", ""],
        [
            "news_rag",
            "community_sentiment",
            "message_sentiment_status",
        ],
    ]
    .into_iter()
    .filter(|categories| capital_bucket_score(items, *categories).is_some())
    .count();
    if scored_external_count >= 3 {
        "高".to_string()
    } else if scored_external_count >= 1 || has_technical {
        "中".to_string()
    } else {
        "低".to_string()
    }
}

fn capital_evidence_summary(
    composite_score: f64,
    scored_labels: &[String],
    evidence_labels: &[String],
) -> String {
    if !scored_labels.is_empty() {
        return format!(
            "综合资金证据分 {}，已纳入{}；缺失分数的证据项按中性权重处理。",
            format_number(composite_score),
            scored_labels.join("、")
        );
    }
    if !evidence_labels.is_empty() {
        return format!(
            "综合资金证据分 {}，当前只有{}状态证据；缺失分数的证据项按中性权重处理。",
            format_number(composite_score),
            evidence_labels.join("、")
        );
    }
    format!(
        "综合资金证据分 {}，外部资金证据暂缺，当前仅保留中性权重。",
        format_number(composite_score)
    )
}

fn push_capital_note(notes: &mut Vec<String>, note: impl AsRef<str>) {
    let clean = note.as_ref().trim();
    if clean.is_empty() || notes.iter().any(|existing| existing == clean) {
        return;
    }
    notes.push(clean.to_string());
}

fn local_fund_flow_proxy_evidence_item(
    trend: &TrendIndicatorResult,
) -> Option<CapitalEvidenceItem> {
    let signal = &trend.signal;
    let latest = trend.series.last()?;
    let score = local_fund_flow_proxy_score(latest);
    let mut metrics = BTreeMap::new();
    metrics.insert("隐性资金代理分".to_string(), format_number(score));
    metrics.insert(
        "推断方向".to_string(),
        fund_flow_proxy_direction(score).to_string(),
    );
    metrics.insert("收盘价".to_string(), format_number(signal.close));
    if let Some(change_pct) = signal.close_change_pct {
        metrics.insert("涨跌幅".to_string(), format_percent(change_pct));
    }
    let numeric_metrics = [
        ("量价热度", latest.volume_price_heat),
        ("吸筹强度", latest.accumulation_strength),
        ("吸筹指标", latest.accumulation_index),
        ("趋势热度", latest.trend_heat),
        ("异动热度", latest.anomaly_heat),
        ("人气热度", latest.popularity_heat),
    ];
    for (label, value) in numeric_metrics {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            metrics.insert(label.to_string(), format_number(value));
        }
    }
    metrics.insert("证据类型".to_string(), "本地日线量价代理".to_string());
    Some(CapitalEvidenceItem {
        category: "fund_flow".to_string(),
        source: "Tauri/Rust 日线量价".to_string(),
        title: "本地量价资金代理".to_string(),
        date: Some(signal.date.clone()),
        metrics,
        sentiment: score_sentiment(score).to_string(),
        weight: 0.35,
        confidence: "中".to_string(),
        url: None,
        score: Some(score),
        note: Some(format!(
            "资金流代理分 {}：由量价热度、吸筹强度、趋势热度和承接指标合成；它不是外部主力净流入数据。",
            format_number(score)
        )),
    })
}

fn local_fund_flow_proxy_score(point: &TrendIndicatorPoint) -> f64 {
    let volume_price = point.volume_price_heat.unwrap_or(50.0).clamp(0.0, 100.0);
    let accumulation_strength = point
        .accumulation_strength
        .unwrap_or(50.0)
        .clamp(0.0, 100.0);
    let trend_heat = point.trend_heat.unwrap_or(50.0).clamp(0.0, 100.0);
    let rebound = point.rebound_signal.unwrap_or(50.0).clamp(0.0, 100.0);
    let accumulation_index = point
        .accumulation_index
        .map(|value| (50.0 + value * 2.0).clamp(0.0, 100.0))
        .unwrap_or(50.0);
    round2(
        volume_price * 0.35
            + accumulation_strength * 0.25
            + trend_heat * 0.20
            + accumulation_index * 0.15
            + rebound * 0.05,
    )
}

fn fund_flow_proxy_direction(score: f64) -> &'static str {
    if score >= 60.0 {
        "偏流入"
    } else if score <= 40.0 {
        "偏流出"
    } else {
        "中性"
    }
}

fn technical_capital_score(trend: &TrendIndicatorResult) -> f64 {
    let signal = &trend.signal;
    let quant_max = signal.quant_score_max.max(1) as f64;
    let pattern_max = signal.pattern_score_max.max(1) as f64;
    let quant = (signal.quant_score as f64 / quant_max * 100.0).clamp(0.0, 100.0);
    let pattern = (signal.pattern_score as f64 / pattern_max * 100.0).clamp(0.0, 100.0);
    let mut score = quant * 0.55 + pattern * 0.45;
    if signal.short_buy {
        score += 8.0;
    }
    if signal.kdj_golden_cross {
        score += 6.0;
    }
    if signal.kdj_oversold {
        score += 5.0;
    }
    if signal.red_hold {
        score += 4.0;
    }
    if signal.swl_above_sws {
        score += 3.0;
    }
    if signal.white_exit {
        score -= 12.0;
    }
    if signal.kdj_dead_cross {
        score -= 10.0;
    }
    if signal.kdj_overbought {
        score -= 8.0;
    }
    if signal.cyan_watch {
        score -= 4.0;
    }
    round2(score.clamp(0.0, 100.0))
}

fn technical_capital_evidence_item(
    trend: &TrendIndicatorResult,
    score: f64,
) -> CapitalEvidenceItem {
    let signal = &trend.signal;
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "\u{6536}\u{76d8}\u{4ef7}".to_string(),
        format_number(signal.close),
    );
    if let Some(change_pct) = signal.close_change_pct {
        metrics.insert(
            "\u{6da8}\u{8dcc}\u{5e45}".to_string(),
            format_percent(change_pct),
        );
    }
    metrics.insert(
        "\u{91cf}\u{5316}\u{5206}".to_string(),
        format!("{}/{}", signal.quant_score, signal.quant_score_max),
    );
    metrics.insert(
        "\u{5f62}\u{6001}\u{5206}".to_string(),
        format!("{}/{}", signal.pattern_score, signal.pattern_score_max),
    );
    if let (Some(k), Some(d), Some(j)) = (signal.k, signal.d, signal.j) {
        metrics.insert(
            "KDJ".to_string(),
            format!(
                "K {} / D {} / J {}",
                format_number(k),
                format_number(d),
                format_number(j)
            ),
        );
    }
    if let (Some(swl), Some(sws)) = (signal.swl, signal.sws) {
        metrics.insert(
            "SWL/SWS".to_string(),
            format!("{} / {}", format_number(swl), format_number(sws)),
        );
    }
    metrics.insert("\u{72b6}\u{6001}".to_string(), signal.status.clone());
    if let Some(point) = trend.series.last() {
        let point_metrics = [
            ("吸筹指标", point.accumulation_index),
            ("吸筹强度", point.accumulation_strength),
            ("波段机会", point.swing_opportunity),
            ("绝地反击", point.rebound_signal),
            ("趋势热度", point.trend_heat),
            ("量价热度", point.volume_price_heat),
            ("异动热度", point.anomaly_heat),
            ("人气热度", point.popularity_heat),
        ];
        for (label, value) in point_metrics {
            if let Some(value) = value.filter(|value| value.is_finite()) {
                metrics.insert(label.to_string(), format_number(value));
            }
        }
    }
    metrics.insert(
        "\u{77ed}\u{7ebf}\u{4e70}\u{70b9}".to_string(),
        if signal.short_buy {
            "\u{662f}"
        } else {
            "\u{5426}"
        }
        .to_string(),
    );
    let risk_labels = capital_technical_risk_labels(signal);
    if !risk_labels.is_empty() {
        metrics.insert(
            "\u{98ce}\u{9669}\u{4fe1}\u{53f7}".to_string(),
            risk_labels.join("\u{3001}"),
        );
    }
    CapitalEvidenceItem {
        category: "technical_behavior".to_string(),
        source: "Tauri/Rust \u{6280}\u{672f}\u{6307}\u{6807}".to_string(),
        title: "\u{6280}\u{672f}\u{63a8}\u{65ad}\u{8d44}\u{91d1}\u{627f}\u{63a5}".to_string(),
        date: Some(signal.date.clone()),
        metrics,
        sentiment: score_sentiment(score).to_string(),
        weight: 0.25,
        confidence: "\u{4e2d}".to_string(),
        url: None,
        score: Some(score),
        note: Some(technical_capital_note(signal, score)),
    }
}

fn institution_status_evidence_item(as_of_trade_date: &str) -> CapitalEvidenceItem {
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "状态".to_string(),
        "当前没有可展示的龙虎榜机构席位记录".to_string(),
    );
    metrics.insert("查询窗口".to_string(), as_of_trade_date.to_string());
    metrics.insert(
        "已尝试信源".to_string(),
        "本地缓存 / 已提供外部证据".to_string(),
    );
    CapitalEvidenceItem {
        category: "institution_lhb_status".to_string(),
        source: "Tauri/Rust".to_string(),
        title: "机构席位暂无外部证据".to_string(),
        date: Some(as_of_trade_date.to_string()),
        metrics,
        sentiment: "uncertain".to_string(),
        weight: 0.25,
        confidence: "低".to_string(),
        url: None,
        score: None,
        note: Some(
            "当前窗口没有可展示的龙虎榜机构席位证据；这只是证据缺口，不代表机构没有买卖。"
                .to_string(),
        ),
    }
}

fn message_sentiment_status_evidence_item(as_of_trade_date: &str) -> CapitalEvidenceItem {
    let mut metrics = BTreeMap::new();
    metrics.insert("状态".to_string(), "本地消息缓存暂无可用证据".to_string());
    metrics.insert("查询窗口".to_string(), as_of_trade_date.to_string());
    metrics.insert(
        "已尝试信源".to_string(),
        "本地新闻缓存 / 已提供外部证据".to_string(),
    );
    CapitalEvidenceItem {
        category: "message_sentiment_status".to_string(),
        source: "Tauri/Rust".to_string(),
        title: "消息情绪暂无外部证据".to_string(),
        date: Some(as_of_trade_date.to_string()),
        metrics,
        sentiment: "uncertain".to_string(),
        weight: 0.15,
        confidence: "低".to_string(),
        url: None,
        score: None,
        note: Some("当前窗口没有可展示的新闻或社区情绪证据；该项仅作为证据缺口提示。".to_string()),
    }
}

fn build_capital_evidence_sections(
    items: &[CapitalEvidenceItem],
    contributions: &BTreeMap<String, Value>,
) -> Vec<CapitalEvidenceSection> {
    let definitions = [
        (
            "fund_flow",
            "资金流",
            "资金流",
            ["fund_flow", "", ""],
            ["fund_flow", "", ""],
        ),
        (
            "institution_lhb",
            "机构席位",
            "机构席位",
            ["institution_lhb", "institution_lhb_status", ""],
            ["institution_lhb", "", ""],
        ),
        (
            "message_sentiment",
            "消息情绪",
            "消息情绪",
            [
                "news_rag",
                "community_sentiment",
                "message_sentiment_status",
            ],
            ["news_rag", "community_sentiment", ""],
        ),
        (
            "technical_behavior",
            "技术推断",
            "技术推断",
            ["technical_behavior", "", ""],
            ["technical_behavior", "", ""],
        ),
    ];
    definitions
        .into_iter()
        .map(
            |(key, title, contribution_key, categories, evidence_categories)| {
                let section_items = items
                    .iter()
                    .filter(|item| {
                        categories
                            .iter()
                            .any(|category| !category.is_empty() && *category == item.category)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let contribution = contributions.get(contribution_key);
                let score = contribution
                    .and_then(|value| value.get("score"))
                    .and_then(Value::as_f64);
                let weight = contribution
                    .and_then(|value| value.get("weight"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let contribution_available = contribution
                    .and_then(|value| value.get("available"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let has_real_evidence = section_items.iter().any(|item| {
                    evidence_categories
                        .iter()
                        .any(|category| !category.is_empty() && *category == item.category)
                });
                let available = contribution_available || has_real_evidence;
                CapitalEvidenceSection {
                    key: key.to_string(),
                    title: title.to_string(),
                    score,
                    weight,
                    available,
                    summary: Some(capital_section_summary(
                        title,
                        score,
                        available,
                        section_items.len(),
                    )),
                    items: section_items,
                }
            },
        )
        .collect()
}

fn capital_section_summary(
    title: &str,
    score: Option<f64>,
    available: bool,
    item_count: usize,
) -> String {
    if available {
        if let Some(score) = score {
            return format!("{}\u{8bc1}\u{636e}\u{5206} {}\u{ff0c}\u{547d}\u{4e2d} {} \u{6761}\u{8bc1}\u{636e}\u{3002}", title, format_number(score), item_count);
        }
        return format!("{}\u{6709} {} \u{6761}\u{72b6}\u{6001}\u{6216}\u{8f85}\u{52a9}\u{8bc1}\u{636e}\u{3002}", title, item_count);
    }
    format!(
        "{}\u{6682}\u{65e0}\u{53ef}\u{7528}\u{8bc1}\u{636e}\u{3002}",
        title
    )
}

fn capital_technical_risk_labels(signal: &TrendIndicatorSignal) -> Vec<String> {
    let mut labels = Vec::new();
    if signal.white_exit {
        labels.push("\u{767d}\u{7ebf}\u{5356}\u{51fa}".to_string());
    }
    if signal.kdj_dead_cross {
        labels.push("KDJ \u{6b7b}\u{53c9}".to_string());
    }
    if signal.kdj_overbought {
        labels.push("KDJ \u{9ad8}\u{4f4d}\u{8d85}\u{4e70}".to_string());
    }
    if signal.cyan_watch {
        labels.push("\u{9752}\u{8272}\u{89c2}\u{671b}".to_string());
    }
    labels
}

fn technical_capital_note(signal: &TrendIndicatorSignal, score: f64) -> String {
    let mut parts = Vec::new();
    if signal.short_buy {
        parts.push("\u{77ed}\u{7ebf}\u{4e70}\u{70b9}\u{6210}\u{7acb}".to_string());
    }
    if signal.kdj_golden_cross {
        parts.push("KDJ \u{91d1}\u{53c9}".to_string());
    }
    if signal.swl_above_sws {
        parts.push("SWL \u{5728} SWS \u{4e0a}\u{65b9}".to_string());
    }
    if signal.white_exit || signal.kdj_dead_cross || signal.kdj_overbought {
        parts.push("\u{5b58}\u{5728}\u{6280}\u{672f}\u{98ce}\u{9669}\u{4fe1}\u{53f7}".to_string());
    }
    if parts.is_empty() {
        parts.push("\u{6280}\u{672f}\u{6307}\u{6807}\u{672a}\u{51fa}\u{73b0}\u{5f3a}\u{52bf}\u{627f}\u{63a5}".to_string());
    }
    format!(
        "\u{6280}\u{672f}\u{63a8}\u{65ad}\u{5206} {}\u{ff1a}{}\u{3002}\u{8be5}\u{9879}\u{53ea}\u{4f5c}\u{8d44}\u{91d1}\u{884c}\u{4e3a}\u{7684}\u{8f85}\u{52a9}\u{89c2}\u{5bdf}\u{ff0c}\u{4e0d}\u{7b49}\u{540c}\u{4e8e}\u{771f}\u{5b9e}\u{8d44}\u{91d1}\u{6d41}\u{3002}",
        format_number(score),
        parts.join("\u{3001}")
    )
}

fn score_sentiment(score: f64) -> &'static str {
    if score >= 60.0 {
        "positive"
    } else if score <= 40.0 {
        "negative"
    } else {
        "neutral"
    }
}
