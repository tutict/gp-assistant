use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSourceItem {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source_tier: String,
    #[serde(default)]
    pub source_name: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillRequest {
    pub stock_code: String,
    #[serde(default)]
    pub stock_name: String,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub sources: Vec<MobileStockSourceItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillFinding {
    pub label: String,
    pub title: String,
    pub summary: String,
    pub source_tier: String,
    pub source_name: String,
    #[serde(default)]
    pub published_at: Option<String>,
    pub evidence: String,
    pub confidence: f64,
    pub risk_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillOverview {
    pub stock_code: String,
    pub stock_name: String,
    pub overall_label: String,
    pub summary: String,
    pub positive_count: usize,
    pub negative_count: usize,
    pub neutral_count: usize,
    pub unverified_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobileStockSkillResult {
    pub overview: MobileStockSkillOverview,
    #[serde(default)]
    pub positive_factors: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub negative_factors: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub neutral_information: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub unverified_leads: Vec<MobileStockSkillFinding>,
    #[serde(default)]
    pub notes: Vec<String>,
}
