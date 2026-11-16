use crate::agent::intent::{
    agent_depth_for_mode, agent_intent_kind, agent_window_hint, extract_codes,
};
use crate::agent::stock_summary::{agent_beginner_stock_sections, agent_data_summary};
use crate::agent::types::{
    AgentAnswerSection, AgentEvidenceItem, AgentIntent, AgentResponse, AgentToolCall,
};
use serde_json::json;

pub(super) fn agent_finalize_response(
    mut response: AgentResponse,
    message: &str,
    mode: Option<&str>,
) -> AgentResponse {
    let mode_text = mode.unwrap_or("quick").to_string();
    let action = response.action.clone();
    let symbols = extract_codes(message);
    response.intent = Some(AgentIntent {
        kind: agent_intent_kind(&action).to_string(),
        query: message.trim().to_string(),
        symbols,
        window: agent_window_hint(message),
        depth: agent_depth_for_mode(&mode_text).to_string(),
        mode: mode_text,
    });
    response.tool_calls = agent_tool_calls_for_response(&response);
    response.evidence_summary = agent_evidence_for_response(&response);
    response.answer_sections = agent_beginner_sections_for_response(&response);
    response.warnings = agent_warnings_for_response(&response);
    response.next_actions = agent_next_actions_for_response(&response);
    response
}

fn agent_tool_calls_for_response(response: &AgentResponse) -> Vec<AgentToolCall> {
    if response.action == "clarify" {
        return vec![AgentToolCall {
            id: "intent_parser".to_string(),
            tool: "agent_plan_request".to_string(),
            label: "Understand request".to_string(),
            status: "needs_input".to_string(),
            input: json!({ "action": response.action }),
            output_summary: Some("Need a clearer stock research task.".to_string()),
            warnings: Vec::new(),
        }];
    }
    let tool = agent_intent_kind(&response.action);
    vec![
        AgentToolCall {
            id: "intent_parser".to_string(),
            tool: "agent_plan_request".to_string(),
            label: "Understand request".to_string(),
            status: "ok".to_string(),
            input: json!({ "action": response.action }),
            output_summary: Some(format!("Routed to {tool}.")),
            warnings: Vec::new(),
        },
        AgentToolCall {
            id: format!("tool_{tool}"),
            tool: tool.to_string(),
            label: agent_tool_label(tool).to_string(),
            status: if response.data.is_some() {
                "ok"
            } else {
                "degraded"
            }
            .to_string(),
            input: json!({ "action": response.action }),
            output_summary: Some(agent_data_summary(response)),
            warnings: agent_warnings_for_response(response),
        },
        AgentToolCall {
            id: "synthesizer".to_string(),
            tool: "agent_synthesize_response".to_string(),
            label: "Structure answer".to_string(),
            status: "ok".to_string(),
            input: json!({ "research_only": true }),
            output_summary: Some("Prepared sections, evidence and risk boundary.".to_string()),
            warnings: Vec::new(),
        },
    ]
}

fn agent_tool_label(tool: &str) -> &'static str {
    match tool {
        "trend_analysis" => "运行趋势筛选",
        "sector_analysis" => "运行自定义板块筛选",
        "portfolio_simulation" => "运行本地组合观察",
        "stock_screen" => "运行本地选股",
        "stock_snapshot" => "生成个股速览",
        "stock_news" => "整理资讯证据",
        "watchlist_action" => "读取本地自选股",
        _ => "运行本地工具",
    }
}

fn agent_evidence_for_response(response: &AgentResponse) -> Vec<AgentEvidenceItem> {
    let source = match response.action.as_str() {
        "trend_screen" => "本地 K 线与趋势指标",
        "graph_screen" => "本地自定义筛选兼容结果",
        "backtest" => "本地回测引擎",
        "screen" => "本地股票池缓存",
        "observe_stock" => "本地行情与观察数据",
        "news_rag" => "本地新闻/RAG 降级结果",
        "watchlist_action" => "本地自选股观察池",
        _ => "本地 Agent 路由",
    };
    vec![AgentEvidenceItem {
        title: agent_tool_label(agent_intent_kind(&response.action)).to_string(),
        source: source.to_string(),
        level: if response.data.is_some() {
            "primary"
        } else {
            "degraded"
        }
        .to_string(),
        summary: agent_data_summary(response),
    }]
}

fn agent_beginner_sections_for_response(response: &AgentResponse) -> Vec<AgentAnswerSection> {
    if response.action == "observe_stock" {
        if let Some(sections) = agent_beginner_stock_sections(response) {
            return sections;
        }
    }
    agent_sections_for_response(response)
}

fn agent_sections_for_response(response: &AgentResponse) -> Vec<AgentAnswerSection> {
    vec![
        AgentAnswerSection {
            title: "结论概览".to_string(),
            bullets: vec![
                response.reply.clone(),
                "仅供选股研究，不构成投资建议。".to_string(),
            ],
        },
        AgentAnswerSection {
            title: "证据摘要".to_string(),
            bullets: agent_evidence_for_response(response)
                .into_iter()
                .map(|item| item.summary)
                .collect(),
        },
        AgentAnswerSection {
            title: "风险边界".to_string(),
            bullets: vec!["不输出买入、卖出、目标价、仓位或收益承诺。".to_string()],
        },
    ]
}

fn agent_warnings_for_response(response: &AgentResponse) -> Vec<String> {
    let mut warnings = vec!["仅供选股研究，不构成投资建议。".to_string()];
    if response.action == "backtest" {
        warnings.push("组合观察只使用本地自选股/候选股数据，不是真实交易。".to_string());
    }
    if response.action == "news_rag" {
        warnings.push("当前新闻能力可能使用本地缓存或降级结果，需复核原始来源。".to_string());
    }
    if response.data.is_none() {
        warnings.push("本次请求没有可用的本地证据载荷。".to_string());
    }
    warnings
}

fn agent_next_actions_for_response(response: &AgentResponse) -> Vec<String> {
    match response.action.as_str() {
        "trend_screen" => vec![
            "查看候选股详情".to_string(),
            "运行本地回测".to_string(),
            "加入自选股观察".to_string(),
        ],
        "graph_screen" => vec![
            "调整自定义筛选条件".to_string(),
            "比较板块趋势强度".to_string(),
        ],
        "backtest" => vec!["查看回撤与换手".to_string(), "调整回测时间窗口".to_string()],
        "screen" => vec!["收紧筛选条件".to_string(), "继续做趋势分析".to_string()],
        "observe_stock" => vec!["查看趋势指标".to_string(), "补充资讯证据".to_string()],
        "news_rag" => vec!["核验原始来源".to_string(), "继续看财务和资金面".to_string()],
        "watchlist_action" => vec![
            "用自选股做组合观察".to_string(),
            "对自选股运行回测".to_string(),
        ],
        _ => vec!["补充股票代码、板块或筛选条件".to_string()],
    }
}

pub(super) fn mode_reply(mode: Option<&str>, text: &str) -> String {
    match mode.unwrap_or("research") {
        "quick" => text.to_string(),
        "expert" => format!(
            "{text}\n\n风险提示：以上为本地策略结果，需结合基本面与市场环境复核。\n下一步：可对候选股做观察或回测验证。"
        ),
        _ => format!("{text} 仅供选股研究，不构成投资建议。"),
    }
}
