use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FinancialCoverage {
    pub eps: bool,
    pub roe: bool,
    pub revenue: bool,
    pub net_profit: bool,
}

impl FinancialCoverage {
    pub(crate) fn missing_fields(&self) -> Vec<&'static str> {
        [
            (self.eps, "每股收益"),
            (self.roe, "净资产收益率"),
            (self.revenue, "营业总收入"),
            (self.net_profit, "归母净利润"),
        ]
        .into_iter()
        .filter_map(|(available, label)| (!available).then_some(label))
        .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockItem {
    pub code: String,
    pub name: String,
    pub industry: String,
    #[serde(default)]
    pub is_st: bool,
    pub price: f64,
    #[serde(default)]
    pub pe: Option<f64>,
    #[serde(default)]
    pub pb: Option<f64>,
    #[serde(default)]
    pub roe: Option<f64>,
    #[serde(default)]
    pub market_cap_billion: Option<f64>,
    #[serde(default)]
    pub dividend_yield: Option<f64>,
    #[serde(default)]
    pub latest_eps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub financial_coverage: Option<FinancialCoverage>,
    #[serde(default)]
    pub deducted_net_profit_billion: Option<f64>,
    #[serde(default)]
    pub deducted_net_profit_margin: Option<f64>,
    #[serde(default)]
    pub deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default)]
    pub change_pct: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub turnover_rate: Option<f64>,
    #[serde(default)]
    pub volume_ratio: Option<f64>,
    #[serde(default)]
    pub quote_time: Option<String>,
}

impl Default for StockItem {
    fn default() -> Self {
        Self {
            code: String::new(),
            name: String::new(),
            industry: String::new(),
            is_st: false,
            price: 0.0,
            pe: None,
            pb: None,
            roe: None,
            market_cap_billion: None,
            dividend_yield: None,
            latest_eps: None,
            financial_coverage: None,
            deducted_net_profit_billion: None,
            deducted_net_profit_margin: None,
            deducted_net_profit_growth_rate: None,
            change_pct: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            volume_ratio: None,
            quote_time: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockRelation {
    pub source_code: String,
    pub target_code: String,
    pub relation_type: String,
    #[serde(default = "default_relation_weight")]
    pub weight: f64,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryBar {
    pub date: String,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    pub close: f64,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub capital: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuarterlyEpsPoint {
    pub period: String,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StockFinancialSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_eps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operating_revenue_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operating_revenue_yoy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_net_profit_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_net_profit_yoy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gross_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roe: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_liability_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goodwill_to_net_assets: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pledged_share_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_payout_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goodwill_period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pledged_share_period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_notice_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_fetched_at: Option<String>,
    #[serde(default)]
    pub quarterly_eps: Vec<QuarterlyEpsPoint>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StockFactorSnapshot {
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub industry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_st: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_listed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_tradable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pe: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roe: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_cap_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_billion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnover_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn default_relation_weight() -> f64 {
    1.0
}

pub(crate) fn normalize_stock_code(code: &str) -> String {
    let text = code.trim().to_uppercase();
    if text.is_empty() {
        return String::new();
    }
    if let Some((digits, suffix)) = text.split_once('.') {
        if digits.chars().all(|ch| ch.is_ascii_digit()) && matches!(suffix, "SH" | "SZ" | "BJ") {
            return format!("{:0>6}.{}", digits, suffix);
        }
        return text;
    }
    let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() != 6 {
        return text;
    }
    let suffix = if digits.starts_with('5') || digits.starts_with('6') || digits.starts_with('9') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    };
    format!("{}.{}", digits, suffix)
}
