use crate::market_data::StockItem;
use crate::rates::{RateRatio, RateUnit};
use crate::screening::classification::{industry_matches, market_scope_for_stock};
use crate::screening::types::ScreenCriteria;

pub(crate) fn deducted_profit_rule_notes(
    universe: &[StockItem],
    criteria: &ScreenCriteria,
) -> Vec<String> {
    if criteria.min_deducted_net_profit_billion.is_none()
        && criteria.min_deducted_net_profit_margin.is_none()
        && criteria.min_deducted_net_profit_growth_rate.is_none()
    {
        return Vec::new();
    }
    let with_metrics = universe
        .iter()
        .filter(|stock| has_deducted_profit_rule_metrics(stock, criteria))
        .count();
    if with_metrics < universe.len() {
        vec![format!("扣非净利润规则已启用；当前股票池 {with_metrics}/{} 只股票带扣非财务字段，缺字段股票按不达标处理。", universe.len())]
    } else {
        Vec::new()
    }
}

fn has_deducted_profit_rule_metrics(stock: &StockItem, criteria: &ScreenCriteria) -> bool {
    if criteria.min_deducted_net_profit_billion.is_some()
        && stock.deducted_net_profit_billion.is_none()
    {
        return false;
    }
    if criteria.min_deducted_net_profit_margin.is_some()
        && stock.deducted_net_profit_margin.is_none()
    {
        return false;
    }
    if criteria.min_deducted_net_profit_growth_rate.is_some()
        && stock.deducted_net_profit_growth_rate.is_none()
    {
        return false;
    }
    true
}

pub(super) fn finite_positive(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

fn meets_min_rate(value: Option<f64>, minimum: f64, unit: RateUnit, inclusive: bool) -> bool {
    let Some(value) = value.and_then(|v| RateRatio::from_unit(v, unit).ok()) else {
        return false;
    };
    let Ok(minimum) = RateRatio::from_unit(minimum, unit) else {
        return false;
    };
    if inclusive {
        value.as_ratio() >= minimum.as_ratio()
    } else {
        value.as_ratio() > minimum.as_ratio()
    }
}

pub(crate) fn matches_stock(stock: &StockItem, criteria: &ScreenCriteria) -> Option<Vec<String>> {
    let mut reasons = Vec::new();
    if !stock.price.is_finite() || stock.price <= 0.0 {
        return None;
    }
    if !criteria.include_st && stock.is_st {
        return None;
    }

    if let Some(industry) = criteria.industry.as_ref() {
        let stock_value = stock.industry.trim();
        let selected_value = industry.trim();
        if !selected_value.is_empty()
            && (stock_value.is_empty() || !industry_matches(stock_value, selected_value))
        {
            return None;
        }
    }

    if let Some(market_scope) = criteria.market_scope.as_ref() {
        let selected_value = market_scope.trim();
        if !selected_value.is_empty() && market_scope_for_stock(stock) != selected_value {
            return None;
        }
    }

    if let Some(min_roe) = criteria.min_roe {
        if !meets_min_rate(stock.roe, min_roe, RateUnit::Ratio, true) {
            return None;
        }
        reasons.push("roe_ok".to_string());
    }

    if let Some(max_pe) = criteria.max_pe {
        if !max_pe.is_finite()
            || max_pe <= 0.0
            || finite_positive(stock.pe)
                .map(|pe| pe > max_pe)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("pe_ok".to_string());
    }

    if let Some(max_pb) = criteria.max_pb {
        if !max_pb.is_finite()
            || max_pb <= 0.0
            || finite_positive(stock.pb)
                .map(|pb| pb > max_pb)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("pb_ok".to_string());
    }

    if let Some(min_market_cap) = criteria.min_market_cap_billion {
        if !min_market_cap.is_finite()
            || min_market_cap < 0.0
            || finite_positive(stock.market_cap_billion)
                .map(|market_cap| market_cap < min_market_cap)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("mcap_ok".to_string());
    }

    if let Some(min_profit) = criteria.min_deducted_net_profit_billion {
        if !min_profit.is_finite()
            || stock
                .deducted_net_profit_billion
                .filter(|profit| profit.is_finite())
                .map(|profit| profit <= min_profit)
                .unwrap_or(true)
        {
            return None;
        }
        reasons.push("deducted_net_profit_ok".to_string());
    }

    if let Some(min_margin) = criteria.min_deducted_net_profit_margin {
        if !meets_min_rate(
            stock.deducted_net_profit_margin,
            min_margin,
            RateUnit::PercentagePoints,
            false,
        ) {
            return None;
        }
        reasons.push("deducted_net_profit_margin_ok".to_string());
    }

    if let Some(min_growth) = criteria.min_deducted_net_profit_growth_rate {
        if !meets_min_rate(
            stock.deducted_net_profit_growth_rate,
            min_growth,
            RateUnit::PercentagePoints,
            false,
        ) {
            return None;
        }
        reasons.push("deducted_net_profit_growth_rate_ok".to_string());
    }

    Some(reasons)
}
