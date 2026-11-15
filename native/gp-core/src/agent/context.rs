use crate::agent::types::AgentContext;
use crate::data_source::MarketDataSource;
use crate::error::CoreResult;
use crate::market_data::normalize_stock_code;
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) fn agent_watchlist_codes(context: &AgentContext) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut codes = Vec::new();
    for item in &context.watchlist {
        let code = normalize_stock_code(&item.code);
        if !code.is_empty() && seen.insert(code.clone()) {
            codes.push(code);
        }
    }
    codes
}

pub(super) fn agent_watchlist_data(context: &AgentContext) -> Value {
    let codes = agent_watchlist_codes(context);
    let items = context
        .watchlist
        .iter()
        .map(|item| {
            json!({
                "code": normalize_stock_code(&item.code),
                "name": item.name,
                "industry": item.industry,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "source": "local_watchlist",
        "total": items.len(),
        "returned": items.len(),
        "codes": codes,
        "items": items,
        "notes": ["只读取本地自选股观察池，不执行真实交易。"],
    })
}

pub(super) fn agent_news_data(
    source: &impl MarketDataSource,
    message: &str,
    codes: &[String],
) -> CoreResult<Value> {
    let universe = source.stocks()?;
    let mut items = Vec::new();
    for code in codes.iter().take(5) {
        let stock = universe
            .iter()
            .find(|item| normalize_stock_code(&item.code) == normalize_stock_code(code));
        items.push(json!({
            "code": code,
            "name": stock.map(|item| item.name.clone()).unwrap_or_default(),
            "source": "local_cache",
            "level": "degraded",
            "summary": "当前核心层未接入外部新闻抓取；请结合桌面新闻 RAG 或导入的移动 RAG 包复核。",
        }));
    }
    if items.is_empty() {
        items.push(json!({
            "source": "local_cache",
            "level": "degraded",
            "summary": "未识别到股票代码，已保留为待验证资讯研究问题。",
        }));
    }
    Ok(json!({
        "query": message,
        "source": "local_agent_news_stub",
        "returned": items.len(),
        "items": items,
        "notes": ["社区或未验证来源只作为待验证线索；不输出买卖、目标价或收益承诺。"],
    }))
}
