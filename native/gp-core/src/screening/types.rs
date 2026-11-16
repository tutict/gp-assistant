use crate::market_data::StockItem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenCriteria {
    #[serde(default)]
    pub min_roe: Option<f64>,
    #[serde(default)]
    pub max_pe: Option<f64>,
    #[serde(default)]
    pub max_pb: Option<f64>,
    #[serde(default)]
    pub min_market_cap_billion: Option<f64>,
    #[serde(default)]
    pub min_deducted_net_profit_billion: Option<f64>,
    #[serde(default)]
    pub min_deducted_net_profit_margin: Option<f64>,
    #[serde(default)]
    pub min_deducted_net_profit_growth_rate: Option<f64>,
    #[serde(default)]
    pub industry: Option<String>,
    #[serde(default)]
    pub market_scope: Option<String>,
    #[serde(default)]
    pub include_st: bool,
    #[serde(default = "default_screen_limit")]
    pub limit: usize,
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
    #[serde(default = "default_score_profile")]
    pub score_profile: String,
}

impl Default for ScreenCriteria {
    fn default() -> Self {
        Self {
            min_roe: None,
            max_pe: None,
            max_pb: None,
            min_market_cap_billion: None,
            min_deducted_net_profit_billion: None,
            min_deducted_net_profit_margin: None,
            min_deducted_net_profit_growth_rate: None,
            industry: None,
            market_scope: None,
            include_st: false,
            limit: default_screen_limit(),
            sort_by: default_sort_by(),
            sort_dir: default_sort_dir(),
            score_profile: default_score_profile(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenedStock {
    #[serde(default)]
    pub industry_sample_count: Option<usize>,
    pub stock: StockItem,
    pub score: f64,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub quality_score: f64,
    #[serde(default)]
    pub trend_score: f64,
    #[serde(default)]
    pub risk_score: f64,
    #[serde(default)]
    pub balanced_score: f64,
    #[serde(default)]
    pub factor_scores: BTreeMap<String, f64>,
    #[serde(default)]
    pub score_breakdown: Vec<ScoreContribution>,
    #[serde(default)]
    pub score_explanation: String,
    #[serde(default)]
    pub reason_tags: Vec<String>,
    #[serde(default)]
    pub risk_tags: Vec<String>,
    #[serde(default)]
    pub suitable_periods: Vec<String>,
    #[serde(default)]
    pub concept: Option<String>,
    #[serde(default)]
    pub theme_category: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenResultGroup {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub returned: usize,
    #[serde(default)]
    pub items: Vec<ScreenedStock>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenResult {
    pub total: usize,
    pub returned: usize,
    pub items: Vec<ScreenedStock>,
    #[serde(default)]
    pub groups: Vec<ScreenResultGroup>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default = "default_sector_group_limit")]
    pub max_sectors: usize,
    #[serde(default = "default_per_sector_limit")]
    pub per_sector_limit: usize,
    #[serde(default = "default_min_sector_candidates")]
    pub min_sector_candidates: usize,
    #[serde(default = "default_sector_group_by")]
    pub group_by: String,
}

impl Default for SectorScreenRequest {
    fn default() -> Self {
        Self {
            criteria: ScreenCriteria::default(),
            max_sectors: default_sector_group_limit(),
            per_sector_limit: default_per_sector_limit(),
            min_sector_candidates: default_min_sector_candidates(),
            group_by: default_sector_group_by(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenGroup {
    pub sector: String,
    pub total: usize,
    pub returned: usize,
    pub average_score: f64,
    pub items: Vec<ScreenedStock>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectorScreenResult {
    pub total: usize,
    pub returned: usize,
    pub sector_count: usize,
    pub groups: Vec<SectorScreenGroup>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreContribution {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub contribution: Option<f64>,
    #[serde(default = "default_tone")]
    pub tone: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SelectionExplanation {
    #[serde(default)]
    pub basis: Vec<String>,
    #[serde(default)]
    pub score_breakdown: Vec<ScoreContribution>,
    #[serde(default)]
    pub risk_checks: Vec<String>,
    #[serde(default)]
    pub verification: Vec<String>,
}

fn default_screen_limit() -> usize {
    10
}

fn default_sort_by() -> String {
    "score".to_string()
}

fn default_sort_dir() -> String {
    "desc".to_string()
}

fn default_score_profile() -> String {
    "balanced".to_string()
}

pub(crate) fn default_tone() -> String {
    "neutral".to_string()
}

fn default_sector_group_limit() -> usize {
    12
}

fn default_per_sector_limit() -> usize {
    5
}

fn default_min_sector_candidates() -> usize {
    5
}

pub(crate) fn default_sector_group_by() -> String {
    "concept".to_string()
}
