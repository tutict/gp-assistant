use crate::backtest::BacktestRequest;
use crate::graph_compat::GraphScreenRequest;
use crate::screening::ScreenCriteria;
use crate::trend_screen::TrendScreenRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSkill {
    pub key: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub platforms: Vec<String>,
    pub evidence_level: String,
    pub fallback: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentIntent {
    pub kind: String,
    pub query: String,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub window: Option<String>,
    pub depth: String,
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentToolCall {
    pub id: String,
    pub tool: String,
    pub label: String,
    pub status: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output_summary: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEvidenceItem {
    pub title: String,
    pub source: String,
    pub level: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentAnswerSection {
    pub title: String,
    #[serde(default)]
    pub bullets: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentContext {
    #[serde(default)]
    pub watchlist: Vec<AgentWatchlistItem>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentWatchlistItem {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub industry: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub reply: String,
    pub action: String,
    #[serde(default)]
    pub intent: Option<AgentIntent>,
    #[serde(default)]
    pub tool_calls: Vec<AgentToolCall>,
    #[serde(default)]
    pub evidence_summary: Vec<AgentEvidenceItem>,
    #[serde(default)]
    pub answer_sections: Vec<AgentAnswerSection>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub criteria: Option<ScreenCriteria>,
    #[serde(default)]
    pub backtest: Option<BacktestRequest>,
    #[serde(default)]
    pub graph_screen: Option<GraphScreenRequest>,
    #[serde(default)]
    pub trend_screen: Option<TrendScreenRequest>,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStreamEvent {
    pub run_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<AgentResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
