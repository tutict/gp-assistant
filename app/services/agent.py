import json
import os
import re
from typing import Any, Dict, Optional

from app.providers.base import StockProvider
from app.schemas import AgentResponse, BacktestRequest, GraphScreenRequest, ScreenCriteria
from app.services.backtest import backtest_hold
from app.services.screener import screen_stocks
from app.services.stock_graph import graph_screen_stocks


SYSTEM_PROMPT = """You are an A-share stock assistant. You must respond in JSON.
Decide whether the user wants a basic stock screen, relation-aware graph screen, backtest, or clarification.
Return this shape:
{
  "action": "screen" | "graph_screen" | "backtest" | "clarify",
  "criteria": { ...ScreenCriteria fields... } | null,
  "graph_screen": {
    "criteria": { ...ScreenCriteria fields... },
    "seed_codes": ["optional stock codes"],
    "relation_depth": 1,
    "relation_weight": 0.35,
    "limit": 20
  } | null,
  "backtest": { ...BacktestRequest fields... } | null,
  "reply": "short Chinese reply to the user"
}
Use action "graph_screen" when the user mentions stock relations, industry chain, upstream/downstream,
supply chain, peer linkage, GNN, knowledge graph, graph learning, or LangGraph-related stock relation analysis.
If the user request is unclear, use action "clarify" and ask a brief question in reply.
"""


def run_agent(provider: StockProvider, message: str) -> AgentResponse:
    response = _call_llm(message)
    action = response.get("action", "clarify")
    reply = response.get("reply", "")
    criteria = _parse_criteria(response.get("criteria"))
    graph_request = _parse_graph_screen(response.get("graph_screen"))
    backtest = _parse_backtest(response.get("backtest"))

    data = None
    if action == "screen" and criteria:
        result = screen_stocks(provider.list_stocks(), criteria)
        data = result.model_dump()
    elif action == "graph_screen" and graph_request:
        result = graph_screen_stocks(provider, graph_request)
        data = result.model_dump()
    elif action == "backtest" and backtest:
        result = backtest_hold(provider, backtest)
        data = result.model_dump()

    return AgentResponse(
        reply=reply or "已处理。",
        action=action,
        criteria=criteria,
        graph_screen=graph_request,
        backtest=backtest,
        data=data,
    )


def _call_llm(message: str) -> Dict[str, Any]:
    api_key = os.getenv("OPENAI_API_KEY")
    if not api_key:
        result = _heuristic_parse(message)
        result["reply"] = f"{result.get('reply', '已处理。')}（未配置 OPENAI_API_KEY，已使用本地规则解析。）"
        return result

    from openai import OpenAI

    client = OpenAI(api_key=api_key)
    model = os.getenv("OPENAI_MODEL", "gpt-4o-mini")

    try:
        completion = client.chat.completions.create(
            model=model,
            messages=[
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": message},
            ],
            response_format={"type": "json_object"},
            temperature=0.2,
        )
        content = completion.choices[0].message.content or "{}"
        return json.loads(content)
    except Exception:
        return _heuristic_parse(message)


def _heuristic_parse(message: str) -> Dict[str, Any]:
    lower = message.lower()
    criteria = _heuristic_criteria(message)

    if _contains_any(lower, ["关系", "产业链", "上下游", "供应链", "关联", "联动", "图学习", "知识图谱", "graph", "gnn", "langgraph"]):
        return {
            "action": "graph_screen",
            "reply": "已按股票关系图做关系传播选股，结果包含基础分、关系分和建议权重。",
            "graph_screen": {
                "criteria": criteria,
                "seed_codes": _extract_codes(message),
                "relation_depth": 2 if _contains_any(lower, ["二级", "2层", "2-hop", "two hop"]) else 1,
                "relation_weight": 0.4,
                "limit": 20,
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

    return {"action": "clarify", "reply": "你是要普通选股、关系图选股，还是回测？请补充条件。"}


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
        match = re.search(pattern, message)
        if match:
            return float(match.group(1))
    return None


def _extract_percent_after(message: str, labels: list[str]) -> Optional[float]:
    for label in labels:
        pattern = rf"{re.escape(label)}\s*(?:高于|大于|>=|>|不低于|超过|在)?\s*(\d+(?:\.\d+)?)\s*(%)?"
        match = re.search(pattern, message)
        if match:
            value = float(match.group(1))
            return value / 100 if match.group(2) or value > 1 else value
    return None


def _extract_industry(message: str) -> Optional[str]:
    known = {
        "银行": "Banking",
        "白酒": "Beverages",
        "饮料": "Beverages",
        "电池": "Batteries",
        "新能源": "Batteries",
        "汽车": "Auto",
        "电子": "Electronics",
        "光伏": "Solar",
        "化工": "Chemicals",
    }
    for keyword, industry in known.items():
        if keyword in message:
            return industry
    return None


def _extract_codes(message: str) -> list[str]:
    return re.findall(r"\b\d{6}\.(?:SH|SZ|BJ)\b", message.upper())


def _extract_date(message: str, default: str, first: bool) -> str:
    dates = re.findall(r"\b(20\d{2})(?:[-/.年]?)(\d{2})(?:[-/.月]?)(\d{2})", message)
    if not dates:
        years = re.findall(r"\b(20\d{2})\b", message)
        if years:
            year = years[0] if first else years[-1]
            return f"{year}0101" if first else f"{year}1231"
        return default
    index = 0 if first else -1
    year, month, day = dates[index]
    return f"{year}{month}{day}"


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


def _parse_backtest(data: Optional[Dict[str, Any]]) -> Optional[BacktestRequest]:
    if not data:
        return None
    try:
        return BacktestRequest(**data)
    except Exception:
        return None
