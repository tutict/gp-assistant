use crate::market_data::{StockFinancialSnapshot, StockItem};
use crate::observation::types::{FinancialIndicatorItem, FinancialIndicators};
use crate::rates::{percentage_points, ratio_percentage_points};
use std::collections::HashMap;

pub(super) fn build_observation_financial_indicators(
    stock: &StockItem,
    financial: Option<&StockFinancialSnapshot>,
) -> FinancialIndicators {
    let mut items = Vec::new();
    let mut notes = Vec::new();
    let mut source_parts = vec!["Tauri/Rust".to_string()];
    let mut period = "\u{672c}\u{5730}\u{884c}\u{60c5}\u{5feb}\u{7167}".to_string();

    if let Some(snapshot) = financial {
        if let Some(source) = snapshot
            .source
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            source_parts.push(source.clone());
        }
        if let Some(snapshot_period) = snapshot
            .period
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            period = snapshot_period.clone();
        }
    }
    let snapshot_period = financial.and_then(|snapshot| snapshot.period.as_deref());

    push_indicator(
        &mut items,
        "\u{5e02}\u{76c8}\u{7387}(TTM)",
        stock.pe,
        |value| format_number(value),
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{5e02}\u{51c0}\u{7387}",
        stock.pb,
        |value| format_number(value),
        "neutral",
    );
    let snapshot_roe = financial.and_then(|snapshot| snapshot.roe);
    if snapshot_roe.is_some() {
        push_indicator_with_meta(
            &mut items,
            "ROE",
            snapshot_roe,
            format_percentage_points,
            indicator_tone(snapshot_roe),
            Some("%"),
            Some("roe"),
            snapshot_period,
        );
    } else {
        push_indicator_with_meta(
            &mut items,
            "ROE",
            stock.roe,
            format_percent,
            indicator_tone(stock.roe),
            Some("%"),
            Some("roe"),
            None,
        );
    }
    push_indicator(
        &mut items,
        "\u{5e02}\u{503c}",
        stock.market_cap_billion,
        |value| format!("{}\u{4ebf}", format_number(value)),
        "neutral",
    );
    let snapshot_dividend_yield = financial.and_then(|snapshot| snapshot.dividend_yield);
    if snapshot_dividend_yield.is_some() {
        push_indicator_with_meta(
            &mut items,
            "\u{80a1}\u{606f}\u{7387}",
            snapshot_dividend_yield,
            format_percentage_points,
            "neutral",
            Some("%"),
            Some("dividend_yield"),
            financial.and_then(|snapshot| snapshot.dividend_period.as_deref()),
        );
    } else {
        push_indicator_with_meta(
            &mut items,
            "\u{80a1}\u{606f}\u{7387}",
            stock.dividend_yield,
            format_percent,
            "neutral",
            Some("%"),
            Some("dividend_yield"),
            None,
        );
    }
    push_indicator_with_meta(
        &mut items,
        "营业总收入",
        financial.and_then(|snapshot| snapshot.operating_revenue_billion),
        |value| format!("{}亿", format_number(value)),
        "neutral",
        Some("亿"),
        Some("operating_revenue"),
        snapshot_period,
    );
    push_indicator_with_meta(
        &mut items,
        "总营收同比",
        financial.and_then(|snapshot| snapshot.operating_revenue_yoy),
        format_percentage_points,
        indicator_tone(financial.and_then(|snapshot| snapshot.operating_revenue_yoy)),
        Some("%"),
        Some("operating_revenue_yoy"),
        snapshot_period,
    );
    push_indicator_with_meta(
        &mut items,
        "归母净利润",
        financial.and_then(|snapshot| snapshot.parent_net_profit_billion),
        |value| format!("{}亿", format_number(value)),
        indicator_tone(financial.and_then(|snapshot| snapshot.parent_net_profit_billion)),
        Some("亿"),
        Some("parent_net_profit"),
        snapshot_period,
    );
    push_indicator_with_meta(
        &mut items,
        "归母净利同比",
        financial.and_then(|snapshot| snapshot.parent_net_profit_yoy),
        format_percentage_points,
        indicator_tone(financial.and_then(|snapshot| snapshot.parent_net_profit_yoy)),
        Some("%"),
        Some("parent_net_profit_yoy"),
        snapshot_period,
    );
    for (label, metric_key, value, metric_period) in [
        (
            "毛利率",
            "gross_margin",
            financial.and_then(|snapshot| snapshot.gross_margin),
            snapshot_period,
        ),
        (
            "净利率",
            "net_margin",
            financial.and_then(|snapshot| snapshot.net_margin),
            snapshot_period,
        ),
        (
            "资产负债率",
            "asset_liability_ratio",
            financial.and_then(|snapshot| snapshot.asset_liability_ratio),
            snapshot_period,
        ),
        (
            "商誉净资产比",
            "goodwill_to_net_assets",
            financial.and_then(|snapshot| snapshot.goodwill_to_net_assets),
            financial.and_then(|snapshot| snapshot.goodwill_period.as_deref()),
        ),
        (
            "质押总股本比",
            "pledged_share_ratio",
            financial.and_then(|snapshot| snapshot.pledged_share_ratio),
            financial.and_then(|snapshot| snapshot.pledged_share_period.as_deref()),
        ),
        (
            "股利支付率(静)",
            "dividend_payout_ratio",
            financial.and_then(|snapshot| snapshot.dividend_payout_ratio),
            financial.and_then(|snapshot| snapshot.dividend_period.as_deref()),
        ),
    ] {
        push_indicator_with_meta(
            &mut items,
            label,
            value,
            format_percentage_points,
            "neutral",
            Some("%"),
            Some(metric_key),
            metric_period,
        );
    }
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{6da6}",
        stock.deducted_net_profit_billion,
        |value| format!("{}\u{4ebf}", format_number(value)),
        indicator_tone(stock.deducted_net_profit_billion),
    );
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{7387}",
        stock.deducted_net_profit_margin,
        format_percentage_points,
        "neutral",
    );
    push_indicator(
        &mut items,
        "\u{6263}\u{975e}\u{51c0}\u{5229}\u{6da6}\u{589e}\u{957f}\u{7387}",
        stock.deducted_net_profit_growth_rate,
        format_percentage_points,
        indicator_tone(stock.deducted_net_profit_growth_rate),
    );

    let latest_eps = financial.and_then(|snapshot| snapshot.latest_eps);
    if latest_eps.is_some() {
        push_indicator_with_meta(
            &mut items,
            "\u{6700}\u{65b0}\u{6bcf}\u{80a1}\u{6536}\u{76ca}",
            latest_eps,
            |value| format!("{}\u{5143}", format_number(value)),
            indicator_tone(latest_eps),
            Some("\u{5143}"),
            Some("latest_eps"),
            snapshot_period,
        );
    } else if let Some(pe) = stock.pe.filter(|value| value.abs() > f64::EPSILON) {
        let eps = stock.price / pe;
        push_indicator_with_meta(
            &mut items,
            "\u{6bcf}\u{80a1}\u{6536}\u{76ca}(\u{4f30}\u{7b97})",
            Some(eps),
            |value| format!("{}\u{5143}", format_number(value)),
            indicator_tone(Some(eps)),
            Some("\u{5143}"),
            Some("estimated_eps"),
            None,
        );
    }

    let latest_bps = financial.and_then(|snapshot| snapshot.latest_bps);
    if latest_bps.is_some() {
        push_indicator_with_meta(
            &mut items,
            "\u{6bcf}\u{80a1}\u{51c0}\u{8d44}\u{4ea7}",
            latest_bps,
            |value| format!("{}\u{5143}", format_number(value)),
            "neutral",
            Some("\u{5143}"),
            Some("latest_bps"),
            snapshot_period,
        );
    } else if let Some(pb) = stock.pb.filter(|value| value.abs() > f64::EPSILON) {
        let bps = stock.price / pb;
        push_indicator_with_meta(
            &mut items,
            "\u{6bcf}\u{80a1}\u{51c0}\u{8d44}\u{4ea7}(\u{4f30}\u{7b97})",
            Some(bps),
            |value| format!("{}\u{5143}", format_number(value)),
            "neutral",
            Some("\u{5143}"),
            Some("estimated_bps"),
            None,
        );
    }

    if let Some(snapshot) = financial {
        notes.extend(snapshot.notes.iter().filter_map(|note| {
            let trimmed = note.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }));
        let mut points = snapshot
            .quarterly_eps
            .iter()
            .filter(|point| {
                point.value.is_finite() && normalize_period_key(&point.period).is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        points.sort_by(|left, right| right.period.cmp(&left.period));
        points.dedup_by(|left, right| left.period == right.period);
        let values_by_period: HashMap<String, f64> = points
            .iter()
            .map(|point| (point.period.clone(), point.value))
            .collect();
        for point in points.iter().take(12) {
            let tone = previous_year_period(&point.period)
                .and_then(|previous| values_by_period.get(&previous).copied())
                .map(|previous| indicator_tone(Some(point.value - previous)))
                .unwrap_or("neutral");
            push_indicator_with_meta(
                &mut items,
                &format!("{} \u{6bcf}\u{80a1}\u{6536}\u{76ca}", point.period),
                Some(point.value),
                |value| format!("{}\u{5143}", format_number(value)),
                tone,
                Some("\u{5143}"),
                Some("quarterly_eps"),
                Some(point.period.as_str()),
            );
        }
        if points.is_empty() {
            notes
                .push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
        }
    } else {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }

    FinancialIndicators {
        title: "\u{6700}\u{65b0}\u{6307}\u{6807}".to_string(),
        period,
        source: source_parts.join(" / "),
        items,
        notes,
    }
}

fn previous_year_period(period: &str) -> Option<String> {
    let normalized = normalize_period_key(period)?;
    let year = normalized.get(0..4)?.parse::<i32>().ok()?;
    let quarter = normalized.get(5..6)?;
    Some(format!("{:04}Q{}", year - 1, quarter))
}

fn normalize_period_key(period: &str) -> Option<String> {
    let raw = period.trim().to_ascii_uppercase();
    let bytes = raw.as_bytes();
    if bytes.len() == 6
        && bytes[0..4].iter().all(|byte| byte.is_ascii_digit())
        && bytes[4] == b'Q'
        && matches!(bytes[5], b'1'..=b'4')
    {
        return Some(raw);
    }
    None
}

fn push_indicator(
    items: &mut Vec<FinancialIndicatorItem>,
    label: &str,
    raw_value: Option<f64>,
    formatter: impl Fn(f64) -> String,
    tone: &str,
) {
    push_indicator_with_meta(items, label, raw_value, formatter, tone, None, None, None);
}

fn push_indicator_with_meta(
    items: &mut Vec<FinancialIndicatorItem>,
    label: &str,
    raw_value: Option<f64>,
    formatter: impl Fn(f64) -> String,
    tone: &str,
    unit: Option<&str>,
    metric_key: Option<&str>,
    period: Option<&str>,
) {
    let Some(value) = raw_value.filter(|value| value.is_finite()) else {
        return;
    };
    items.push(FinancialIndicatorItem {
        label: label.to_string(),
        value: formatter(value),
        raw_value: Some(value),
        unit: unit.map(str::to_string),
        tone: tone.to_string(),
        metric_key: metric_key.map(str::to_string),
        period: period.map(str::to_string),
    });
}

pub(crate) fn format_number(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

pub(crate) fn format_percent(value: f64) -> String {
    match ratio_percentage_points(value) {
        Some(percent) => format!("{}%", format_number(percent)),
        None => "—".to_string(),
    }
}

pub(crate) fn format_percentage_points(value: f64) -> String {
    match percentage_points(value) {
        Some(percent) => format!("{}%", format_number(percent)),
        None => "—".to_string(),
    }
}

fn indicator_tone(value: Option<f64>) -> &'static str {
    match value {
        Some(value) if value.is_finite() && value >= 0.0 => "rise",
        Some(value) if value.is_finite() => "fall",
        _ => "neutral",
    }
}
