use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapitalEvidenceItem {
    pub category: String,
    pub source: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_metrics")]
    pub metrics: BTreeMap<String, String>,
    #[serde(default = "default_uncertain_sentiment")]
    pub sentiment: String,
    #[serde(default)]
    pub weight: f64,
    #[serde(default = "default_low_confidence")]
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn deserialize_string_metrics<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<BTreeMap<String, Value>>::deserialize(deserializer)?;
    let Some(map) = value else {
        return Ok(BTreeMap::new());
    };
    let mut metrics = BTreeMap::new();
    for (key, value) in map {
        let text = match value {
            Value::Null => String::new(),
            Value::String(value) => value,
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            other => other.to_string(),
        };
        metrics.insert(key, text);
    }
    Ok(metrics)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapitalEvidenceSection {
    pub key: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub items: Vec<CapitalEvidenceItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapitalEvidenceResult {
    pub stock_code: String,
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_score: Option<f64>,
    #[serde(default = "default_low_confidence")]
    pub confidence: String,
    #[serde(default)]
    pub model_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of_trade_date: Option<String>,
    #[serde(default = "default_unknown_freshness")]
    pub freshness: String,
    #[serde(default)]
    pub contributions: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub sections: Vec<CapitalEvidenceSection>,
    #[serde(default)]
    pub items: Vec<CapitalEvidenceItem>,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_uncertain_sentiment() -> String {
    "uncertain".to_string()
}

fn default_low_confidence() -> String {
    "\u{4f4e}".to_string()
}

fn default_unknown_freshness() -> String {
    "unknown".to_string()
}
