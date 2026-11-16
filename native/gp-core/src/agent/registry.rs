use crate::agent::types::AgentSkill;
use serde_json::json;

pub fn agent_skill_registry() -> Vec<AgentSkill> {
    vec![
        agent_skill(
            "stock_snapshot",
            "Stock Snapshot",
            "Local quote, observe and financial snapshot.",
            "desktop,android",
            "local_cache",
            "Return available observe data or explain missing local cache.",
        ),
        agent_skill(
            "stock_news",
            "Stock News",
            "News and RAG evidence for recent positive and negative signals.",
            "desktop,android",
            "rag_or_news_cache",
            "Return short-link news or imported mobile RAG package when full RAG is unavailable.",
        ),
        agent_skill(
            "stock_screen",
            "Stock Screen",
            "Rule based candidate screening over the local universe.",
            "desktop,android",
            "local_cache",
            "Ask for clearer criteria when the intent cannot be mapped.",
        ),
        agent_skill(
            "trend_analysis",
            "Trend Analysis",
            "Technical trend ranking and local OHLC indicators.",
            "desktop,android",
            "local_history",
            "Return skipped-count warnings when OHLC history is incomplete.",
        ),
        agent_skill(
            "sector_analysis",
            "Sector Analysis",
            "Sector and relation driven candidate discovery.",
            "desktop",
            "local_relations",
            "Fall back to ordinary screening when relation data is unavailable.",
        ),
        agent_skill(
            "watchlist_action",
            "Watchlist Action",
            "Local watchlist-only add, remove and review workflow.",
            "desktop,android",
            "local_state",
            "Never place trades; explain local-only scope.",
        ),
        agent_skill(
            "portfolio_simulation",
            "Portfolio Simulation",
            "Watchlist or candidate portfolio observation and backtest.",
            "desktop",
            "local_backtest",
            "Return research-only backtest or desktop-only downgrade notice.",
        ),
    ]
}

fn agent_skill(
    key: &str,
    title: &str,
    description: &str,
    platforms: &str,
    evidence_level: &str,
    fallback: &str,
) -> AgentSkill {
    AgentSkill {
        key: key.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        input_schema: json!({ "query": "string", "symbols": ["string"], "mode": "quick|expert|research" }),
        output_schema: json!({ "sections": ["summary", "evidence", "risks"], "research_only": true }),
        platforms: platforms.split(',').map(|item| item.to_string()).collect(),
        evidence_level: evidence_level.to_string(),
        fallback: fallback.to_string(),
    }
}
