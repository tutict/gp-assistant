from __future__ import annotations

import json
import os
import re
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Any, Dict, Optional, TypedDict

from app.providers.base import StockProvider
from app.schemas import (
    AgentResponse,
    BacktestRequest,
    GraphScreenRequest,
    LlmClientConfig,
    NewsRagRequest,
    ScreenCriteria,
    SectorScreenRequest,
    StockObserveRequest,
    TrendScreenRequest,
    current_system_date_yyyymmdd,
)
from app.services.backtest import backtest_hold
from app.services.data_maintenance import data_source_status, prune_cache, refresh_universe
from app.services.news_rag import analyze_supply_chain_news
from app.services.observation import observe_stock
from app.services.screener import screen_stocks, screen_stocks_by_sector, screening_universe
from app.services.stock_graph import graph_screen_stocks
from app.services.trend_indicator import trend_screen_stocks


SYSTEM_PROMPT = """You are an A-share stock assistant. You must respond in JSON.
The current system date is __CURRENT_DATE_YYYYMMDD__. When the user omits an end_date, use this current system date instead of any fixed historical date.
Decide whether the user wants an individual stock observation, basic stock screen, sector-grouped screen, relation-aware graph screen, trend screen, backtest, or clarification.
LangGraph is only the workflow/state orchestration layer. Stock-to-stock relationships must be handled by the graph_screen tool, which uses a stock knowledge graph and GNN-style relation scoring.
Return this shape:
{
  "action": "observe_stock" | "screen" | "sector_screen" | "trend_screen" | "backtest" | "news_rag" | "data_status" | "refresh_data" | "prune_cache" | "clarify",
  "criteria": { ...ScreenCriteria fields... } | null,
  "observe": {
    "code": "000001.SZ",
    "start_date": null,
    "end_date": null,
    "series_limit": 120,
    "minute_period": "1",
    "minute_limit": 160,
    "include_order_book": true
  } | null,
  "sector_screen": {
    "criteria": { ...ScreenCriteria fields... },
    "per_sector_limit": 3,
    "max_sectors": 12,
    "min_sector_candidates": 3
  } | null,
  "graph_screen": {
    "criteria": { ...ScreenCriteria fields... },
    "seed_codes": ["optional stock codes"],
    "relation_depth": 1,
    "relation_weight": 0.35,
    "limit": 10
  } | null,
  "trend_screen": {
    "criteria": { ...ScreenCriteria fields... },
    "start_date": "20200101",
    "end_date": "__CURRENT_DATE_YYYYMMDD__",
    "limit": 10
  } | null,
  "backtest": {
    "criteria": { ...ScreenCriteria fields... },
    "start_date": "20200101",
    "end_date": "__CURRENT_DATE_YYYYMMDD__",
    "top_n": 10,
    "rebalance_frequency": "none" | "monthly" | "quarterly",
    "transaction_cost_bps": 10,
    "benchmark": "candidate_equal_weight" | "none"
  } | null,
  "news_rag": {
    "code": "optional stock code",
    "criteria": { ...ScreenCriteria fields... },
    "seed_codes": ["optional stock codes"],
    "days": 30,
    "max_items": 24
  } | null,
  "reply": "short Chinese reply to the user"
}
Use action "observe_stock" when the user asks about one stock's quote, valuation, PE/PB, order book, intraday bars, daily technical trend, support/resistance, or asks to look at/check/analyze a specific stock code.
Use action "sector_screen" when the user asks to group by sector/industry/board, pick stocks from each sector, or diversify across sectors.
Use action "graph_screen" when the user mentions stock relations, industry chain, upstream/downstream,
supply chain, peer linkage, GNN, knowledge graph, graph learning, or LangGraph-related stock relation analysis.
Use action "trend_screen" when the user asks for uptrend, trend indicator, SWL/SWS, short-buy,
main-force accumulation, red hold, cyan watch, support/resistance, or quantitative score screening.
Use action "news_rag" when the user asks to analyze upstream/downstream positive or negative news, supply-chain catalysts, 利好消息, 利空消息, or evidence-backed industry-chain message analysis.
Use action "data_status" for data source status, stock universe count, cache usage, or freshness questions.
Use action "refresh_data" when the user asks to refresh, sync, or update the stock universe/data source.
Use action "prune_cache" when the user asks to clean, shrink, or free local cache/storage.
If the user request is unclear, use action "clarify" and ask a brief question in reply.
"""

RESEARCH_RISK_NOTE = "仅供选股研究，不构成投资建议。"
FORBIDDEN_ADVICE_PATTERNS = [
    r"必涨",
    r"稳赚",
    r"保本",
    r"无风险",
    r"确定上涨",
    r"稳赚不赔",
    r"建议\s*(立即)?买入",
    r"可以买入",
    r"应该买入",
    r"立即买入",
    r"满仓",
    r"梭哈",
    r"清仓",
    r"必须卖出",
]

DEFAULT_RESEARCH_REPLIES = {
    "observe_stock": "已按选股研究口径整理个股行情、估值、盘口和技术面观察。",
    "screen": "已按选股研究口径完成基础筛选。",
    "sector_screen": "已按选股研究口径完成板块分组选股。",
    "graph_screen": "已按选股研究口径完成关系图选股。",
    "trend_screen": "已按选股研究口径完成趋势指标排序。",
    "backtest": "已按选股研究口径完成历史回测。",
    "news_rag": "已按选股研究口径完成上下游消息证据分析。",
    "data_status": "已读取当前数据源和缓存状态。",
    "refresh_data": "已按选股研究口径触发数据刷新。",
    "prune_cache": "已按轻量缓存策略清理可丢弃缓存。",
    "clarify": "请补充你的研究目标：普通筛选、关系图选股、趋势观察、消息证据分析或回测。",
}

VALID_ACTIONS = {
    "observe_stock",
    "screen",
    "sector_screen",
    "graph_screen",
    "trend_screen",
    "backtest",
    "news_rag",
    "data_status",
    "refresh_data",
    "prune_cache",
    "clarify",
}

OBSERVE_INTENT_KEYWORDS = [
    "看看",
    "看一下",
    "怎么样",
    "如何",
    "分析",
    "观察",
    "评价",
    "诊断",
    "体检",
    "查",
    "行情",
    "报价",
    "估值",
    "pe",
    "pb",
    "盘口",
    "买卖盘",
    "分钟",
    "分时",
    "技术面",
    "日线",
    "支撑",
    "阻力",
    "趋势",
    "能买吗",
    "能买",
    "买入",
    "卖出",
    "持有",
    "值不值得",
    "quote",
    "valuation",
    "order book",
]


@dataclass(frozen=True)
class ResolvedLlmConfig:
    api_key: Optional[str]
    base_url: Optional[str]
    model: str
    temperature: float
    timeout_seconds: float
    json_mode: bool
    organization: Optional[str]
    project: Optional[str]


class AgentState(TypedDict, total=False):
    provider: StockProvider
    message: str
    llm_config: Optional[LlmClientConfig]
    action: str
    reply: str
    criteria: Optional[ScreenCriteria]
    observe: Optional[StockObserveRequest]
    sector_screen: Optional[SectorScreenRequest]
    graph_screen: Optional[GraphScreenRequest]
    trend_screen: Optional[TrendScreenRequest]
    backtest: Optional[BacktestRequest]
    news_rag: Optional[NewsRagRequest]
    data: Optional[dict[str, Any]]


def run_agent(
    provider: StockProvider,
    message: str,
    llm_config: Optional[LlmClientConfig] = None,
) -> AgentResponse:
    initial_state: AgentState = {
        "provider": provider,
        "message": message,
        "llm_config": llm_config,
    }
    workflow = _get_langgraph_workflow()
    if workflow is not None:
        final_state = workflow.invoke(initial_state)
    else:
        final_state = _run_agent_state_machine(initial_state)
    return _state_to_response(final_state)


@lru_cache(maxsize=1)
def _get_langgraph_workflow() -> Any:
    try:
        from langgraph.graph import END, START, StateGraph
    except Exception:
        return None

    builder = StateGraph(AgentState)
    builder.add_node("parse_intent", _parse_intent_node)
    builder.add_node("observe_stock", _observe_stock_node)
    builder.add_node("screen", _screen_node)
    builder.add_node("sector_screen", _sector_screen_node)
    builder.add_node("graph_screen", _graph_screen_node)
    builder.add_node("trend_screen", _trend_screen_node)
    builder.add_node("backtest", _backtest_node)
    builder.add_node("news_rag", _news_rag_node)
    builder.add_node("data_status", _data_status_node)
    builder.add_node("refresh_data", _refresh_data_node)
    builder.add_node("prune_cache", _prune_cache_node)
    builder.add_node("clarify", _clarify_node)

    builder.add_edge(START, "parse_intent")
    builder.add_conditional_edges(
        "parse_intent",
        _route_action,
        {
            "observe_stock": "observe_stock",
            "screen": "screen",
            "sector_screen": "sector_screen",
            "graph_screen": "graph_screen",
            "trend_screen": "trend_screen",
            "backtest": "backtest",
            "news_rag": "news_rag",
            "data_status": "data_status",
            "refresh_data": "refresh_data",
            "prune_cache": "prune_cache",
            "clarify": "clarify",
        },
    )
    for node in (
        "observe_stock",
        "screen",
        "sector_screen",
        "graph_screen",
        "trend_screen",
        "backtest",
        "news_rag",
        "data_status",
        "refresh_data",
        "prune_cache",
        "clarify",
    ):
        builder.add_edge(node, END)
    return builder.compile()


def _run_agent_state_machine(initial_state: AgentState) -> AgentState:
    state: AgentState = dict(initial_state)
    state.update(_parse_intent_node(state))
    action = _route_action(state)
    if action == "observe_stock":
        state.update(_observe_stock_node(state))
    elif action == "screen":
        state.update(_screen_node(state))
    elif action == "sector_screen":
        state.update(_sector_screen_node(state))
    elif action == "graph_screen":
        state.update(_graph_screen_node(state))
    elif action == "trend_screen":
        state.update(_trend_screen_node(state))
    elif action == "backtest":
        state.update(_backtest_node(state))
    elif action == "news_rag":
        state.update(_news_rag_node(state))
    elif action == "data_status":
        state.update(_data_status_node(state))
    elif action == "refresh_data":
        state.update(_refresh_data_node(state))
    elif action == "prune_cache":
        state.update(_prune_cache_node(state))
    else:
        state.update(_clarify_node(state))
    return state


def _parse_intent_node(state: AgentState) -> AgentState:
    message = state.get("message", "")
    response = _call_llm(state.get("message", ""), state.get("llm_config"))
    action = str(response.get("action") or "clarify")
    if action not in VALID_ACTIONS:
        action = "clarify"

    criteria = _parse_criteria(response.get("criteria"))
    observe_request = _parse_observe(response.get("observe"))
    sector_request = _parse_sector_screen(response.get("sector_screen"))
    graph_request = _parse_graph_screen(response.get("graph_screen"))
    trend_request = _parse_trend_screen(response.get("trend_screen"))
    backtest = _parse_backtest(response.get("backtest"))
    news_rag = _parse_news_rag(response.get("news_rag"))

    if sector_request is not None and criteria is None:
        criteria = sector_request.criteria
    if graph_request is not None and criteria is None:
        criteria = graph_request.criteria
    if trend_request is not None and criteria is None:
        criteria = trend_request.criteria
    if backtest is not None and criteria is None:
        criteria = backtest.criteria
    if news_rag is not None and criteria is None:
        criteria = news_rag.criteria

    if action == "observe_stock" and observe_request is None:
        observe_request = _observe_request_from_message(message)
        if observe_request is None:
            action = "clarify"

    if (
        action in {"clarify", "screen", "trend_screen"}
        and observe_request is None
        and _is_observe_intent(message)
    ):
        fallback_observe = _observe_request_from_message(message)
        if fallback_observe is not None:
            action = "observe_stock"
            observe_request = fallback_observe
            response["reply"] = f"已为 {fallback_observe.code} 拉取行情、估值、盘口和技术面观察。"

    if action == "screen" and criteria is None:
        criteria = ScreenCriteria()
    elif action == "sector_screen" and sector_request is None:
        sector_request = SectorScreenRequest(criteria=criteria or ScreenCriteria())
    elif action == "graph_screen" and graph_request is None:
        graph_request = GraphScreenRequest(criteria=criteria or ScreenCriteria())
    elif action == "trend_screen" and trend_request is None:
        trend_request = TrendScreenRequest(criteria=criteria or ScreenCriteria())
    elif action == "backtest" and backtest is None:
        backtest = BacktestRequest(
            criteria=criteria or ScreenCriteria(),
            start_date="20200101",
            end_date=current_system_date_yyyymmdd(),
            rebalance_frequency=_extract_rebalance_frequency(message),
            transaction_cost_bps=_extract_cost_bps(message),
            benchmark=_extract_benchmark(message),
        )
    elif action == "news_rag" and news_rag is None:
        news_rag = NewsRagRequest(
            criteria=criteria or ScreenCriteria(),
            seed_codes=_extract_codes(message),
        )

    return {
        "action": action,
        "reply": str(response.get("reply") or ""),
        "criteria": criteria,
        "observe": observe_request,
        "sector_screen": sector_request,
        "graph_screen": graph_request,
        "trend_screen": trend_request,
        "backtest": backtest,
        "news_rag": news_rag,
        "data": None,
    }


def _route_action(state: AgentState) -> str:
    action = state.get("action") or "clarify"
    return action if action in VALID_ACTIONS else "clarify"


def _observe_stock_node(state: AgentState) -> AgentState:
    request = state.get("observe")
    if request is None or not request.code:
        return {
            "action": "clarify",
            "data": None,
            "reply": state.get("reply") or "请给出要观察的 6 位 A 股代码，例如 000001 或 300750.SZ。",
        }

    try:
        result = observe_stock(state["provider"], request)
    except KeyError:
        return {
            "action": "clarify",
            "observe": request,
            "data": None,
            "reply": f"未找到股票 {request.code}，请确认代码是否正确。",
        }

    stock = result.stock
    return {
        "observe": request,
        "data": result.model_dump(),
        "reply": state.get("reply") or f"已完成 {stock.name}（{stock.code}）的行情、估值、盘口和技术面观察。",
    }


def _screen_node(state: AgentState) -> AgentState:
    criteria = state.get("criteria") or ScreenCriteria()
    universe, notes = screening_universe(state["provider"])
    result = screen_stocks(universe, criteria, notes=notes)
    return {
        "criteria": criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已完成基础选股。",
    }


def _sector_screen_node(state: AgentState) -> AgentState:
    request = state.get("sector_screen") or SectorScreenRequest(
        criteria=state.get("criteria") or ScreenCriteria()
    )
    universe, notes = screening_universe(state["provider"])
    result = screen_stocks_by_sector(universe, request, notes=notes)
    return {
        "sector_screen": request,
        "criteria": request.criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已按板块分组选股。",
    }


def _graph_screen_node(state: AgentState) -> AgentState:
    request = state.get("graph_screen") or GraphScreenRequest(
        criteria=state.get("criteria") or ScreenCriteria()
    )
    result = graph_screen_stocks(state["provider"], request)
    return {
        "graph_screen": request,
        "criteria": request.criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已按知识图谱和GNN式关系评分完成选股。",
    }


def _trend_screen_node(state: AgentState) -> AgentState:
    request = state.get("trend_screen") or TrendScreenRequest(
        criteria=state.get("criteria") or ScreenCriteria()
    )
    result = trend_screen_stocks(state["provider"], request)
    return {
        "trend_screen": request,
        "criteria": request.criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已按趋势指标完成选股。",
    }


def _backtest_node(state: AgentState) -> AgentState:
    request = state.get("backtest") or BacktestRequest(
        criteria=state.get("criteria") or ScreenCriteria(),
        start_date="20200101",
        end_date=current_system_date_yyyymmdd(),
    )
    result = backtest_hold(state["provider"], request)
    return {
        "backtest": request,
        "criteria": request.criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已完成回测。",
    }


def _news_rag_node(state: AgentState) -> AgentState:
    request = state.get("news_rag") or NewsRagRequest(
        criteria=state.get("criteria") or ScreenCriteria()
    )
    if request.llm is None and state.get("llm_config") is not None:
        request = request.model_copy(update={"llm": state.get("llm_config")})
    result = analyze_supply_chain_news(state["provider"], request)
    return {
        "news_rag": request,
        "criteria": request.criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已完成上下游消息分析，结果包含影响判断、证据、置信度和待验证点。",
    }


def _data_status_node(state: AgentState) -> AgentState:
    source = getattr(state["provider"], "name", state["provider"].__class__.__name__)
    result = data_source_status(source)
    return {
        "data": result.model_dump(),
        "reply": state.get("reply")
        or f"当前数据源是 {source}，股票池 {result.universe_count} 只，缓存占用 {result.cache_bytes} 字节。",
    }


def _refresh_data_node(state: AgentState) -> AgentState:
    source = getattr(state["provider"], "name", state["provider"].__class__.__name__)
    result = refresh_universe(source)
    return {
        "data": result.model_dump(),
        "reply": state.get("reply")
        or (
            f"已刷新 {result.source} 股票池，当前 {result.status.universe_count} 只。"
            if result.refreshed
            else "刷新股票池失败，请查看返回详情。"
        ),
    }


def _prune_cache_node(state: AgentState) -> AgentState:
    source = getattr(state["provider"], "name", state["provider"].__class__.__name__)
    result = prune_cache(source)
    return {
        "data": result.model_dump(),
        "reply": state.get("reply")
        or f"已清理缓存，删除 {result.removed_files} 个文件，释放 {result.removed_bytes} 字节。",
    }


def _clarify_node(state: AgentState) -> AgentState:
    return {
        "data": None,
        "reply": state.get("reply")
        or "你想做普通选股、关系图选股、趋势指标选股，还是回测？请补充条件。",
    }


def _state_to_response(state: AgentState) -> AgentResponse:
    action = state.get("action") or "clarify"
    return AgentResponse(
        reply=_research_reply(state.get("reply") or "已处理。", action),
        action=action,
        criteria=state.get("criteria"),
        observe=state.get("observe"),
        sector_screen=state.get("sector_screen"),
        graph_screen=state.get("graph_screen"),
        trend_screen=state.get("trend_screen"),
        backtest=state.get("backtest"),
        news_rag=state.get("news_rag"),
        data=state.get("data"),
    )


def _call_llm(message: str, override: Optional[LlmClientConfig] = None) -> Dict[str, Any]:
    config = _resolve_llm_config(override)
    if not config.api_key:
        result = _heuristic_parse(message)
        result["reply"] = (
            f"{result.get('reply', '已处理。')}（未配置 OPENAI_API_KEY，已使用本地规则解析。）"
        )
        return result

    from openai import OpenAI

    try:
        client_kwargs: Dict[str, Any] = {
            "api_key": config.api_key,
            "timeout": config.timeout_seconds,
        }
        if config.base_url:
            client_kwargs["base_url"] = config.base_url
        if config.organization:
            client_kwargs["organization"] = config.organization
        if config.project:
            client_kwargs["project"] = config.project
        client = OpenAI(**client_kwargs)

        request: Dict[str, Any] = {
            "model": config.model,
            "messages": [
                {"role": "system", "content": _system_prompt()},
                {"role": "user", "content": message},
            ],
            "temperature": config.temperature,
        }
        if config.json_mode:
            request["response_format"] = {"type": "json_object"}
        completion = _create_chat_completion(client, request)
        content = completion.choices[0].message.content or "{}"
        return _parse_json_response(content)
    except Exception as exc:
        result = _heuristic_parse(message)
        result["reply"] = (
            f"{result.get('reply', '已处理。')}（LLM 调用失败，已使用本地规则解析：{_safe_error(exc)}）"
        )
        return result


def _create_chat_completion(client: Any, request: Dict[str, Any]) -> Any:
    try:
        return client.chat.completions.create(**request)
    except Exception:
        fallback = dict(request)
        if "response_format" not in fallback:
            raise
        fallback.pop("response_format", None)
        return client.chat.completions.create(**fallback)


def _parse_json_response(content: str) -> Dict[str, Any]:
    try:
        return json.loads(content)
    except json.JSONDecodeError:
        match = re.search(r"\{.*\}", content, flags=re.DOTALL)
        if not match:
            raise
        return json.loads(match.group(0))


def _system_prompt() -> str:
    base_prompt = SYSTEM_PROMPT.replace("__CURRENT_DATE_YYYYMMDD__", current_system_date_yyyymmdd())
    return f"{_stock_soul()}\n\n{base_prompt}"


@lru_cache(maxsize=1)
def _stock_soul() -> str:
    candidates = [
        Path(__file__).resolve().parents[1] / "prompts" / "stock_soul.md",
        Path(getattr(sys, "_MEIPASS", Path.cwd())) / "app" / "prompts" / "stock_soul.md",
    ]
    for path in candidates:
        try:
            if path.exists():
                return path.read_text(encoding="utf-8").strip()
        except OSError:
            continue
    return (
        "你是 A 股选股研究助手，不是投资顾问。不得承诺收益，不得给出直接交易指令。"
        f"回复必须提示：{RESEARCH_RISK_NOTE}"
    )


def _research_reply(reply: str, action: str) -> str:
    text = str(reply or "").strip() or DEFAULT_RESEARCH_REPLIES.get(action, "已按选股研究口径处理。")
    if _contains_forbidden_advice(text):
        text = DEFAULT_RESEARCH_REPLIES.get(action, "已按选股研究口径处理。")
    if "不构成投资建议" not in text:
        text = f"{text} {RESEARCH_RISK_NOTE}"
    return text


def _contains_forbidden_advice(text: str) -> bool:
    return any(re.search(pattern, text, flags=re.IGNORECASE) for pattern in FORBIDDEN_ADVICE_PATTERNS)


def _resolve_llm_config(override: Optional[LlmClientConfig]) -> ResolvedLlmConfig:
    api_key = _coalesce_str(
        override.api_key if override else None,
        os.getenv("OPENAI_API_KEY"),
    )
    base_url = _normalize_base_url(
        _coalesce_str(override.base_url if override else None, os.getenv("OPENAI_BASE_URL"))
    )
    model = _coalesce_str(
        override.model if override else None,
        os.getenv("OPENAI_MODEL"),
        "gpt-4o-mini",
    )
    temperature = _coalesce_float(
        override.temperature if override else None,
        os.getenv("OPENAI_TEMPERATURE"),
        0.2,
    )
    timeout_seconds = _coalesce_float(
        override.timeout_seconds if override else None,
        os.getenv("OPENAI_TIMEOUT_SECONDS"),
        30.0,
    )
    json_mode = _coalesce_bool(
        override.json_mode if override else None,
        os.getenv("OPENAI_JSON_MODE"),
        True,
    )
    organization = _coalesce_str(
        override.organization if override else None,
        os.getenv("OPENAI_ORG_ID"),
    )
    project = _coalesce_str(
        override.project if override else None,
        os.getenv("OPENAI_PROJECT_ID"),
    )
    return ResolvedLlmConfig(
        api_key=api_key,
        base_url=base_url,
        model=model,
        temperature=min(max(temperature, 0.0), 2.0),
        timeout_seconds=min(max(timeout_seconds, 1.0), 180.0),
        json_mode=json_mode,
        organization=organization,
        project=project,
    )


def _coalesce_str(*values: Optional[str]) -> Optional[str]:
    for value in values:
        if value is None:
            continue
        stripped = str(value).strip()
        if stripped:
            return stripped
    return None


def _coalesce_float(*values: Any) -> float:
    fallback = float(values[-1])
    for value in values[:-1]:
        if value is None or value == "":
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return fallback


def _coalesce_bool(*values: Any) -> bool:
    fallback = bool(values[-1])
    for value in values[:-1]:
        if value is None or value == "":
            continue
        if isinstance(value, bool):
            return value
        lowered = str(value).strip().lower()
        if lowered in {"1", "true", "yes", "y", "on"}:
            return True
        if lowered in {"0", "false", "no", "n", "off"}:
            return False
    return fallback


def _normalize_base_url(value: Optional[str]) -> Optional[str]:
    if not value:
        return None
    return value.rstrip("/")


def _safe_error(exc: Exception) -> str:
    text = str(exc).replace(os.getenv("OPENAI_API_KEY", "") or "\0", "[redacted]")
    text = re.sub(r"sk-[A-Za-z0-9_-]{8,}", "sk-[redacted]", text)
    return text[:180]


def _heuristic_parse(message: str) -> Dict[str, Any]:
    lower = message.lower()
    criteria = _heuristic_criteria(message)
    codes = _extract_codes(message)

    if _contains_any(lower, ["清理缓存", "清缓存", "释放空间", "缓存瘦身", "prune cache", "clear cache"]):
        return {
            "action": "prune_cache",
            "reply": "已按轻量缓存策略清理可丢弃缓存，股票池基础缓存会保留。",
        }

    if _contains_any(
        lower,
        ["刷新数据", "刷新股票池", "更新股票池", "同步数据", "更新数据源", "refresh data", "refresh universe"],
    ):
        return {
            "action": "refresh_data",
            "reply": "已触发股票池刷新；刷新结果会返回股票池数量、缓存占用和数据源状态。",
        }

    if _contains_any(
        lower,
        ["数据状态", "数据源状态", "缓存状态", "缓存占用", "股票池数量", "股票池状态", "data status"],
    ):
        return {
            "action": "data_status",
            "reply": "已读取当前数据源、股票池数量、更新时间和缓存占用。",
        }

    if _contains_any(
        lower,
        [
            "上下游消息",
            "利好消息",
            "利空消息",
            "消息分析",
            "新闻分析",
            "产业链消息",
            "供应链消息",
            "催化",
            "rag",
        ],
    ):
        return {
            "action": "news_rag",
            "reply": "已按已有上下游关系图检索本地消息缓存，并生成影响判断、证据和待验证点。",
            "news_rag": {
                "criteria": criteria,
                "code": codes[0] if codes else None,
                "seed_codes": codes,
                "days": _extract_limited_int(
                    message,
                    ["最近", "近", "days"],
                    default=30,
                    minimum=1,
                    maximum=365,
                ),
                "max_items": 24,
            },
        }

    if codes and _is_observe_intent(message):
        return {
            "action": "observe_stock",
            "reply": f"已为 {codes[0]} 拉取行情、估值、盘口和技术面观察。",
            "observe": _observe_request_dict(message, codes[0]),
        }

    if _contains_any(
        lower,
        ["板块分组", "按板块", "每个板块", "分板块", "行业分组", "每个行业", "分行业", "sector"],
    ):
        return {
            "action": "sector_screen",
            "reply": "已按板块分组选股，每个板块返回排名靠前的候选股票。",
            "sector_screen": {
                "criteria": criteria,
                "max_sectors": _extract_limited_int(
                    message,
                    ["最多板块", "板块数量", "行业数量", "max sectors"],
                    default=12,
                    minimum=1,
                    maximum=50,
                ),
                "per_sector_limit": _extract_limited_int(
                    message,
                    ["每板块", "每个板块", "每行业", "每个行业", "per sector"],
                    default=3,
                    minimum=1,
                    maximum=50,
                ),
                "min_sector_candidates": 3,
            },
        }

    if _contains_any(
        lower,
        [
            "趋势",
            "上升趋势",
            "趋势指标",
            "短买",
            "主力吸筹",
            "红色持股",
            "青色观望",
            "swl",
            "sws",
            "量化评分",
            "支撑",
            "阻力",
        ],
    ):
        return {
            "action": "trend_screen",
            "reply": "已按趋势指标做选股排序，结果包含 SWL/SWS、持股/观望状态、短买/离场信号和量化评分。",
            "trend_screen": {
                "criteria": criteria,
                "start_date": _extract_date(message, default="20200101", first=True),
                "end_date": _extract_date(message, default=current_system_date_yyyymmdd(), first=False),
                "limit": 10,
            },
        }

    if _contains_any(
        lower,
        [
            "关系",
            "产业链",
            "上下游",
            "供应链",
            "关联",
            "联动",
            "图学习",
            "知识图谱",
            "graph",
            "gnn",
            "langgraph",
        ],
    ):
        return {
            "action": "graph_screen",
            "reply": "已按股票知识图谱和GNN式关系评分做选股，结果包含基础分、关系分和建议权重。",
            "graph_screen": {
                "criteria": criteria,
                "seed_codes": _extract_codes(message),
                "relation_depth": 2
                if _contains_any(lower, ["二级", "2层", "2-hop", "two hop"])
                else 1,
                "relation_weight": 0.4,
                "limit": 10,
            },
        }

    if "回测" in message or "backtest" in lower:
        return {
            "action": "backtest",
            "reply": "已按你的描述尝试回测；如果没有给日期，将使用默认区间。",
            "backtest": {
                "criteria": criteria,
                "start_date": _extract_date(message, default="20200101", first=True),
                "end_date": _extract_date(message, default=current_system_date_yyyymmdd(), first=False),
                "top_n": _extract_limited_int(message, ["持仓", "top_n", "top n"], default=10, minimum=1, maximum=100),
                "rebalance_frequency": _extract_rebalance_frequency(message),
                "transaction_cost_bps": _extract_cost_bps(message),
                "benchmark": _extract_benchmark(message),
            },
        }

    if _contains_any(lower, ["选股", "筛选", "screen", "挑股票"]):
        return {
            "action": "screen",
            "reply": "已按你的描述筛选股票。",
            "criteria": criteria,
        }

    return {
        "action": "clarify",
        "reply": "你想做普通选股、关系图选股、趋势指标选股，还是回测？请补充条件。",
    }


def _heuristic_criteria(message: str) -> Dict[str, Any]:
    criteria: Dict[str, Any] = {}
    pe = _extract_number_after(message, ["PE", "pe", "市盈率"])
    pb = _extract_number_after(message, ["PB", "pb", "市净率"])
    roe = _extract_percent_after(message, ["ROE", "roe", "净资产收益率"])

    if pe is not None:
        criteria["max_pe"] = pe
    if pb is not None:
        criteria["max_pb"] = pb
    if roe is not None:
        criteria["min_roe"] = roe

    industry = _extract_industry(message)
    if industry:
        criteria["industry"] = industry
    return criteria


def _extract_number_after(message: str, labels: list[str]) -> Optional[float]:
    for label in labels:
        pattern = rf"{re.escape(label)}\s*(?:低于|小于|<=|<|不高于|少于|在)?\s*(\d+(?:\.\d+)?)"
        match = re.search(pattern, message, flags=re.IGNORECASE)
        if match:
            return float(match.group(1))
    return None


def _extract_percent_after(message: str, labels: list[str]) -> Optional[float]:
    for label in labels:
        pattern = rf"{re.escape(label)}\s*(?:高于|大于|>=|>|不低于|超过|在)?\s*(\d+(?:\.\d+)?)\s*(%)?"
        match = re.search(pattern, message, flags=re.IGNORECASE)
        if match:
            value = float(match.group(1))
            return value / 100 if match.group(2) or value > 1 else value
    return None


def _extract_limited_int(
    message: str,
    labels: list[str],
    default: int,
    minimum: int,
    maximum: int,
) -> int:
    for label in labels:
        pattern = rf"{re.escape(label)}\s*(?:各)?\s*(?:数量|取|选|返回|不超过|最多|为|=|:|：)?\s*(\d+)"
        match = re.search(pattern, message, flags=re.IGNORECASE)
        if match:
            value = int(match.group(1))
            return min(max(value, minimum), maximum)
    return default


def _extract_industry(message: str) -> Optional[str]:
    known = {
        "银行": "银行",
        "白酒": "白酒",
        "饮料": "白酒",
        "电池": "动力电池",
        "动力电池": "动力电池",
        "新能源": "动力电池",
        "汽车": "汽车",
        "电子": "电子制造",
        "电子制造": "电子制造",
        "光伏": "光伏",
        "化工": "化工",
    }
    for keyword, industry in known.items():
        if keyword in message:
            return industry
    return None


def _extract_codes(message: str) -> list[str]:
    codes = []
    for code in re.findall(r"\b\d{6}(?:\.(?:SH|SZ|BJ))?\b", message.upper()):
        if "." in code:
            codes.append(code)
        elif code.startswith("6"):
            codes.append(f"{code}.SH")
        elif code.startswith(("4", "8")):
            codes.append(f"{code}.BJ")
        else:
            codes.append(f"{code}.SZ")
    return codes


def _extract_date(message: str, default: Optional[str], first: bool) -> Optional[str]:
    compact_dates = re.findall(r"\b(20\d{2})(\d{2})(\d{2})\b", message)
    separated_dates = re.findall(
        r"\b(20\d{2})(?:[-/.年])(\d{1,2})(?:[-/.月])(\d{1,2})",
        message,
    )
    dates = compact_dates or separated_dates
    if not dates:
        years = re.findall(r"\b(20\d{2})\b", message)
        if years:
            year = years[0] if first else years[-1]
            return f"{year}0101" if first else f"{year}1231"
        return default
    year, month, day = dates[0 if first else -1]
    return f"{year}{int(month):02d}{int(day):02d}"


def _extract_rebalance_frequency(message: str) -> str:
    lower = message.lower()
    if _contains_any(lower, ["不再平衡", "不调仓", "买入持有", "持有到期", "buy and hold", "no rebalance"]):
        return "none"
    if _contains_any(lower, ["季度", "每季", "quarterly", "quarter"]):
        return "quarterly"
    if _contains_any(lower, ["月度", "每月", "monthly", "month"]):
        return "monthly"
    return "monthly"


def _extract_cost_bps(message: str) -> float:
    lower = message.lower()
    if _contains_any(lower, ["无成本", "不计成本", "零成本", "0成本", "no cost"]):
        return 0.0
    match = re.search(r"(\d+(?:\.\d+)?)\s*(?:bps|bp|基点)", message, flags=re.IGNORECASE)
    if not match:
        match = re.search(r"(?:交易成本|成本|手续费)\D{0,8}(\d+(?:\.\d+)?)", message, flags=re.IGNORECASE)
    if not match:
        return 10.0
    value = float(match.group(1))
    return min(max(value, 0.0), 500.0)


def _extract_benchmark(message: str) -> str:
    lower = message.lower()
    if _contains_any(lower, ["无基准", "不对比", "不要基准", "no benchmark"]):
        return "none"
    return "candidate_equal_weight"


def _contains_any(text: str, needles: list[str]) -> bool:
    return any(needle.lower() in text for needle in needles)


def _is_observe_intent(message: str) -> bool:
    return bool(_extract_codes(message)) and _contains_any(message.lower(), OBSERVE_INTENT_KEYWORDS)


def _observe_request_dict(message: str, code: str) -> Dict[str, Any]:
    return {
        "code": code,
        "start_date": _extract_date(message, default=None, first=True),
        "end_date": _extract_date(message, default=None, first=False),
        "series_limit": _extract_limited_int(
            message,
            ["日线数量", "趋势点数", "series limit"],
            default=120,
            minimum=20,
            maximum=500,
        ),
        "minute_period": _extract_minute_period(message),
        "minute_limit": _extract_limited_int(
            message,
            ["分钟线数量", "分钟数量", "minute limit"],
            default=160,
            minimum=1,
            maximum=500,
        ),
        "include_order_book": not _contains_any(message.lower(), ["不要盘口", "不看盘口", "no order book"]),
    }


def _observe_request_from_message(message: str) -> Optional[StockObserveRequest]:
    codes = _extract_codes(message)
    if not codes:
        return None
    return _parse_observe(_observe_request_dict(message, codes[0]))


def _extract_minute_period(message: str) -> str:
    match = re.search(r"(?<!\d)(60|30|15|5|1)\s*(?:m|min|分钟|分)(?!\d)", message, flags=re.IGNORECASE)
    if match:
        return match.group(1)
    return "1"


def _parse_criteria(data: Optional[Dict[str, Any]]) -> Optional[ScreenCriteria]:
    if not data:
        return None
    try:
        return ScreenCriteria(**data)
    except Exception:
        return None


def _parse_observe(data: Optional[Dict[str, Any]]) -> Optional[StockObserveRequest]:
    if not data:
        return None
    try:
        return StockObserveRequest(**data)
    except Exception:
        return None


def _parse_sector_screen(data: Optional[Dict[str, Any]]) -> Optional[SectorScreenRequest]:
    if not data:
        return None
    try:
        return SectorScreenRequest(**data)
    except Exception:
        return None


def _parse_graph_screen(data: Optional[Dict[str, Any]]) -> Optional[GraphScreenRequest]:
    if not data:
        return None
    try:
        return GraphScreenRequest(**data)
    except Exception:
        return None


def _parse_trend_screen(data: Optional[Dict[str, Any]]) -> Optional[TrendScreenRequest]:
    if not data:
        return None
    try:
        return TrendScreenRequest(**data)
    except Exception:
        return None


def _parse_backtest(data: Optional[Dict[str, Any]]) -> Optional[BacktestRequest]:
    if not data:
        return None
    try:
        return BacktestRequest(**data)
    except Exception:
        return None


def _parse_news_rag(data: Optional[Dict[str, Any]]) -> Optional[NewsRagRequest]:
    if not data:
        return None
    try:
        return NewsRagRequest(**data)
    except Exception:
        return None
