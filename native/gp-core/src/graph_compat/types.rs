use crate::market_data::{StockItem, StockRelation};
use crate::screening::types::{ScreenCriteria, SelectionExplanation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GraphCenterContext {
    #[serde(default = "default_graph_center_mode")]
    pub mode: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub codes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScreenRequest {
    #[serde(default)]
    pub criteria: ScreenCriteria,
    #[serde(default)]
    pub seed_codes: Vec<String>,
    #[serde(default)]
    pub seed_query: String,
    #[serde(default = "default_relation_depth")]
    pub relation_depth: usize,
    #[serde(default = "default_relation_weight_param")]
    pub relation_weight: f64,
    #[serde(default = "default_graph_limit")]
    pub limit: usize,
}

impl Default for GraphScreenRequest {
    fn default() -> Self {
        Self {
            criteria: ScreenCriteria::default(),
            seed_codes: Vec::new(),
            seed_query: String::new(),
            relation_depth: default_relation_depth(),
            relation_weight: default_relation_weight_param(),
            limit: default_graph_limit(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphStockSignal {
    pub stock: StockItem,
    pub base_score: f64,
    pub relation_score: f64,
    pub final_score: f64,
    pub suggested_weight: f64,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub related: Vec<StockRelation>,
    #[serde(default)]
    pub explanation: SelectionExplanation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScreenResult {
    pub total: usize,
    pub returned: usize,
    pub relation_count: usize,
    pub items: Vec<GraphStockSignal>,
    #[serde(default)]
    pub center_context: GraphCenterContext,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_relation_depth() -> usize {
    1
}

fn default_relation_weight_param() -> f64 {
    0.35
}

fn default_graph_center_mode() -> String {
    "seed_codes".to_string()
}

fn default_graph_limit() -> usize {
    10
}
