from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from functools import lru_cache
from typing import Any, Dict, Optional, TypedDict

from app.providers.base import StockProvider
from app.schemas import (
    AgentResponse,
    BacktestRequest,
    GraphScreenRequest,
    LlmClientConfig,
    ScreenCriteria,
    TrendScreenRequest,
)
from app.services.backtest import backtest_hold
from app.services.data_maintenance import data_source_status, prune_cache, refresh_universe
from app.services.screener import screen_stocks
from app.services.stock_graph import graph_screen_stocks
from app.services.trend_indicator import trend_screen_stocks


SYSTEM_PROMPT = """You are an A-share stock assistant. You must respond in JSON.
Decide whether the user wants a basic stock screen, relation-aware graph screen, trend screen, backtest, or clarification.
LangGraph is only the workflow/state orchestration layer. Stock-to-stock relationships must be handled by the graph_screen tool, which uses a stock knowledge graph and GNN-style relation scoring.
Return this shape:
{
  "action": "screen" | "graph_screen" | "trend_screen" | "backtest" | "data_status" | "refresh_data" | "prune_cache" | "clarify",
  "criteria": { ...ScreenCriteria fields... } | null,
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
    "end_date": "20240101",
    "limit": 10
  } | null,
  "backtest": { ...BacktestRequest fields... } | null,
  "reply": "short Chinese reply to the user"
}
Use action "graph_screen" when the user mentions stock relations, industry chain, upstream/downstream,
supply chain, peer linkage, GNN, knowledge graph, graph learning, or LangGraph-related stock relation analysis.
Use action "trend_screen" when the user asks for uptrend, trend indicator, SWL/SWS, short-buy,
main-force accumulation, red hold, cyan watch, support/resistance, or quantitative score screening.
Use action "data_status" for data source status, stock universe count, cache usage, or freshness questions.
Use action "refresh_data" when the user asks to refresh, sync, or update the stock universe/data source.
Use action "prune_cache" when the user asks to clean, shrink, or free local cache/storage.
If the user request is unclear, use action "clarify" and ask a brief question in reply.
"""

VALID_ACTIONS = {
    "screen",
    "graph_screen",
    "trend_screen",
    "backtest",
    "data_status",
    "refresh_data",
    "prune_cache",
    "clarify",
}


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
    graph_screen: Optional[GraphScreenRequest]
    trend_screen: Optional[TrendScreenRequest]
    backtest: Optional[BacktestRequest]
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
    builder.add_node("screen", _screen_node)
    builder.add_node("graph_screen", _graph_screen_node)
    builder.add_node("trend_screen", _trend_screen_node)
    builder.add_node("backtest", _backtest_node)
    builder.add_node("data_status", _data_status_node)
    builder.add_node("refresh_data", _refresh_data_node)
    builder.add_node("prune_cache", _prune_cache_node)
    builder.add_node("clarify", _clarify_node)

    builder.add_edge(START, "parse_intent")
    builder.add_conditional_edges(
        "parse_intent",
        _route_action,
        {
            "screen": "screen",
            "graph_screen": "graph_screen",
            "trend_screen": "trend_screen",
            "backtest": "backtest",
            "data_status": "data_status",
            "refresh_data": "refresh_data",
            "prune_cache": "prune_cache",
            "clarify": "clarify",
        },
    )
    for node in (
        "screen",
        "graph_screen",
        "trend_screen",
        "backtest",
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
    if action == "screen":
        state.update(_screen_node(state))
    elif action == "graph_screen":
        state.update(_graph_screen_node(state))
    elif action == "trend_screen":
        state.update(_trend_screen_node(state))
    elif action == "backtest":
        state.update(_backtest_node(state))
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
    response = _call_llm(state.get("message", ""), state.get("llm_config"))
    action = str(response.get("action") or "clarify")
    if action not in VALID_ACTIONS:
        action = "clarify"

    criteria = _parse_criteria(response.get("criteria"))
    graph_request = _parse_graph_screen(response.get("graph_screen"))
    trend_request = _parse_trend_screen(response.get("trend_screen"))
    backtest = _parse_backtest(response.get("backtest"))

    if graph_request is not None and criteria is None:
        criteria = graph_request.criteria
    if trend_request is not None and criteria is None:
        criteria = trend_request.criteria
    if backtest is not None and criteria is None:
        criteria = backtest.criteria

    if action == "screen" and criteria is None:
        criteria = ScreenCriteria()
    elif action == "graph_screen" and graph_request is None:
        graph_request = GraphScreenRequest(criteria=criteria or ScreenCriteria())
    elif action == "trend_screen" and trend_request is None:
        trend_request = TrendScreenRequest(criteria=criteria or ScreenCriteria())
    elif action == "backtest" and backtest is None:
        backtest = BacktestRequest(
            criteria=criteria or ScreenCriteria(),
            start_date="20200101",
            end_date="20240101",
        )

    return {
        "action": action,
        "reply": str(response.get("reply") or ""),
        "criteria": criteria,
        "graph_screen": graph_request,
        "trend_screen": trend_request,
        "backtest": backtest,
        "data": None,
    }


def _route_action(state: AgentState) -> str:
    action = state.get("action") or "clarify"
    return action if action in VALID_ACTIONS else "clarify"


def _screen_node(state: AgentState) -> AgentState:
    criteria = state.get("criteria") or ScreenCriteria()
    result = screen_stocks(state["provider"].list_stocks(), criteria)
    return {
        "criteria": criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已完成基础选股。",
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
        end_date="20240101",
    )
    result = backtest_hold(state["provider"], request)
    return {
        "backtest": request,
        "criteria": request.criteria,
        "data": result.model_dump(),
        "reply": state.get("reply") or "已完成回测。",
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
    return AgentResponse(
        reply=state.get("reply") or "已处理。",
        action=state.get("action") or "clarify",
        criteria=state.get("criteria"),
        graph_screen=state.get("graph_screen"),
        trend_screen=state.get("trend_screen"),
        backtest=state.get("backtest"),
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
                {"role": "system", "content": SYSTEM_PROMPT},
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
                "end_date": _extract_date(message, default="20240101", first=False),
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
                "end_date": _extract_date(message, default="20240101", first=False),
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


def _extract_date(message: str, default: str, first: bool) -> str:
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


def _contains_any(text: str, needles: list[str]) -> bool:
    return any(needle.lower() in text for needle in needles)


def _parse_criteria(data: Optional[Dict[str, Any]]) -> Optional[ScreenCriteria]:
    if not data:
        return None
    try:
        return ScreenCriteria(**data)
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
