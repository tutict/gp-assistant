use crate::agent::context::{agent_news_data, agent_watchlist_codes, agent_watchlist_data};
use crate::agent::intent::{extract_codes, extract_date, heuristic_criteria};
use crate::agent::response::{agent_finalize_response, mode_reply};
use crate::agent::types::{AgentContext, AgentResponse};
use crate::backtest::request::{
    default_backtest_benchmark, default_backtest_source, default_backtest_strategy_mode,
    default_initial_cash, default_rebalance_frequency, default_top_n, default_transaction_cost_bps,
};
use crate::backtest::{backtest_with_source, BacktestRequest};
use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::dates::{current_system_date_yyyymmdd, default_end_date, default_start_date};
use crate::demo_data::MockDataSource;
use crate::error::CoreResult;
use crate::observation::{observe_with_source, StockObserveRequest};
use crate::screening::screen_with_source;
use crate::text::contains_any;
use crate::trend::types::default_series_limit;
use crate::trend_screen::{trend_screen_with_source, TrendScreenRequest};

pub fn run_agent_with_mock(message: &str) -> CoreResult<AgentResponse> {
    run_agent_with_source(&MockDataSource, message, None)
}

pub fn run_agent_with_data(data: &CoreDataSet, message: &str) -> CoreResult<AgentResponse> {
    run_agent_with_data_and_context(data, message, &AgentContext::default())
}

pub fn run_agent_with_data_and_context(
    data: &CoreDataSet,
    message: &str,
    context: &AgentContext,
) -> CoreResult<AgentResponse> {
    let source = StaticDataSource::new(data);
    run_agent_with_source_and_context(&source, message, None, context)
}

pub fn run_agent_with_source(
    source: &impl MarketDataSource,
    message: &str,
    mode: Option<&str>,
) -> CoreResult<AgentResponse> {
    run_agent_with_source_and_context(source, message, mode, &AgentContext::default())
}

pub fn run_agent_with_source_and_context(
    source: &impl MarketDataSource,
    message: &str,
    mode: Option<&str>,
    context: &AgentContext,
) -> CoreResult<AgentResponse> {
    let lower = message.to_lowercase();
    let criteria = heuristic_criteria(message);
    let codes = extract_codes(message);
    let watchlist_codes = agent_watchlist_codes(context);
    let wants_watchlist = contains_any(&lower, &["自选", "自选股", "观察池", "watchlist"]);
    let wants_portfolio = contains_any(
        &lower,
        &[
            "组合",
            "模拟组合",
            "组合观察",
            "回测",
            "backtest",
            "收益曲线",
        ],
    );

    if wants_watchlist && !wants_portfolio {
        let data = agent_watchlist_data(context);
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已读取本地自选股观察池。"),
                action: "watchlist_action".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if wants_portfolio {
        let default_end_date = current_system_date_yyyymmdd();
        let use_watchlist = wants_watchlist || !watchlist_codes.is_empty();
        let backtest = BacktestRequest {
            algorithm_version: None,
            parameter_set_id: None,
            scoring_parameters: None,
            criteria,
            source: if use_watchlist {
                "watchlist".to_string()
            } else {
                default_backtest_source()
            },
            strategy_mode: default_backtest_strategy_mode(),
            stock_codes: if use_watchlist {
                watchlist_codes
            } else {
                Vec::new()
            },
            start_date: extract_date(message, "20200101", true),
            end_date: extract_date(message, &default_end_date, false),
            top_n: default_top_n(),
            initial_cash: default_initial_cash(),
            rebalance_frequency: default_rebalance_frequency(),
            transaction_cost_bps: default_transaction_cost_bps(),
            benchmark: default_backtest_benchmark(),
        };
        let data = serde_json::to_value(backtest_with_source(source, &backtest)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已基于本地数据完成组合观察/回测。"),
                action: "backtest".to_string(),
                criteria: None,
                backtest: Some(backtest),
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "利好", "利空", "新闻", "资讯", "消息", "公告", "舆情", "news", "rag",
        ],
    ) {
        let data = agent_news_data(source, message, &codes)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已整理本地可用的资讯线索与风险边界。"),
                action: "news_rag".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "财务",
            "资金",
            "资金面",
            "个股",
            "速览",
            "分析",
            "看一下",
            "观察",
            "snapshot",
            "observe",
        ],
    ) && !codes.is_empty()
    {
        let code = codes.first().cloned().unwrap_or_default();
        let observe_request = StockObserveRequest {
            code,
            start_date: extract_date(message, &default_start_date(), true),
            end_date: extract_date(message, &default_end_date(), false),
            series_limit: default_series_limit(),
            include_order_book: false,
        };
        let data = serde_json::to_value(observe_with_source(source, &observe_request)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已生成个股本地观察摘要。"),
                action: "observe_stock".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "趋势",
            "趋势股",
            "上升趋势",
            "技术",
            "量价",
            "短线",
            "主力",
            "支撑",
            "阻力",
            "swl",
            "sws",
            "trend",
        ],
    ) {
        let default_end_date = current_system_date_yyyymmdd();
        let trend_request = TrendScreenRequest {
            criteria,
            start_date: extract_date(message, "20200101", true),
            end_date: extract_date(message, &default_end_date, false),
            limit: 10,
        };
        let data = serde_json::to_value(trend_screen_with_source(source, &trend_request)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已按趋势指标完成候选股排序。"),
                action: "trend_screen".to_string(),
                criteria: None,
                backtest: None,
                graph_screen: None,
                trend_screen: Some(trend_request),
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &[
            "板块",
            "行业",
            "主题",
            "概念",
            "半导体",
            "新能源",
            "白酒",
            "银行",
            "sector",
        ],
    ) {
        let data = serde_json::to_value(screen_with_source(source, &criteria)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已按板块/主题条件完成本地候选筛选。"),
                action: "screen".to_string(),
                criteria: Some(criteria),
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    if contains_any(
        &lower,
        &["选股", "筛选", "挑股", "候选", "roe", "pe", "pb", "screen"],
    ) {
        let data = serde_json::to_value(screen_with_source(source, &criteria)?)?;
        return Ok(agent_finalize_response(
            AgentResponse {
                intent: None,
                tool_calls: Vec::new(),
                evidence_summary: Vec::new(),
                answer_sections: Vec::new(),
                warnings: Vec::new(),
                next_actions: Vec::new(),
                reply: mode_reply(mode, "已按描述筛选候选股票。"),
                action: "screen".to_string(),
                criteria: Some(criteria),
                backtest: None,
                graph_screen: None,
                trend_screen: None,
                data: Some(data),
            },
            message,
            mode,
        ));
    }

    Ok(agent_finalize_response(
        AgentResponse {
            intent: None,
            tool_calls: Vec::new(),
            evidence_summary: Vec::new(),
            answer_sections: Vec::new(),
            warnings: Vec::new(),
            next_actions: Vec::new(),
            reply: mode_reply(
                mode,
                "请说明要看个股、资讯、趋势筛选、自选股组合观察，还是普通选股。",
            ),
            action: "clarify".to_string(),
            criteria: None,
            backtest: None,
            graph_screen: None,
            trend_screen: None,
            data: None,
        },
        message,
        mode,
    ))
}
