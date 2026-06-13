from __future__ import annotations

import concurrent.futures
import json
import math
import os
from datetime import date, datetime, time, timedelta
from numbers import Real
from pathlib import Path
from typing import Any, Iterable, Sequence

import pandas as pd

from app.schemas import CapitalEvidenceItem, CapitalEvidenceResult, CapitalEvidenceSection, LlmClientConfig, StockItem
from app.services.llm_support import (
    create_chat_completion,
    create_openai_client,
    parse_json_response,
    resolve_llm_config,
    safe_llm_error,
)
from app.services.runtime_config import env_bool, env_float, env_int, redact_error, safe_string_list
from app.services.sqlite_json_cache import SQLiteJsonCache
from app.services.stock_code import compact_date, market_prefix, normalize_stock_code, stock_digits


CACHE_PATH = Path(os.getenv("GP_CAPITAL_CACHE", "data/cache/capital_evidence.sqlite"))
FUND_FLOW_WEIGHT = 0.35
INSTITUTION_WEIGHT = 0.25
NEWS_WEIGHT = 0.15
TECHNICAL_WEIGHT = 0.25
FRESHNESS_CACHE = "fresh-cache"
FRESHNESS_REFRESHED = "refreshed"
FRESHNESS_STALE = "stale-cache"


def fetch_capital_evidence(
    stock: StockItem,
    start_date: str,
    end_date: str,
    *,
    trend: Any | None = None,
    llm: LlmClientConfig | None = None,
    refresh: bool = False,
) -> CapitalEvidenceResult:
    as_of_trade_date = effective_trade_date(end_date)
    cached = None if refresh else _load_cached_result(stock.code, as_of_trade_date)
    if cached is not None:
        cached.freshness = FRESHNESS_CACHE
        cached.notes = [f"已读取资金证据缓存：{CACHE_PATH}。", *cached.notes]
        return _ensure_sections(_sanitize_result(cached))

    result = CapitalEvidenceResult(
        stock_code=normalize_stock_code(stock.code),
        generated_at=datetime.now().isoformat(timespec="seconds"),
        as_of_trade_date=as_of_trade_date,
        freshness=FRESHNESS_REFRESHED,
        notes=[f"资金证据按交易日 {as_of_trade_date} 汇总；缓存：{CACHE_PATH}。"],
    )

    result.items.extend(_fetch_external_capital_items(stock, start_date, end_date))
    news_items, news_notes = _load_news_cache_items(stock)
    result.items.extend(news_items)
    result.notes.extend(news_notes)
    technical_item = _technical_evidence_item(trend)
    if technical_item is not None:
        result.items.append(technical_item)

    _apply_rule_score(result)
    _apply_llm_enhancement(stock, result, llm)

    if not refresh and not _has_primary_capital_evidence(result.items):
        stale = _load_latest_cached_result(stock.code, exclude_trade_date=as_of_trade_date)
        if stale is not None:
            stale.freshness = FRESHNESS_STALE
            stale.notes = [
                f"当前交易日资金接口未取得有效资金/机构证据，已回退最近缓存（{stale.as_of_trade_date}）。",
                *result.notes,
                *stale.notes,
            ]
            return _ensure_sections(_sanitize_result(stale))

    if not result.items:
        result.notes.append("未取得资金、消息或技术证据，综合资金证据分保持低置信。")

    _ensure_sections(_sanitize_result(result))
    _store_cached_result(result)
    return result


def effective_trade_date(value: str | None, *, now: datetime | None = None) -> str:
    current = now or datetime.now()
    requested = _parse_date(value) or current.date()
    today = current.date()
    if requested >= today:
        if today.weekday() >= 5:
            requested = _previous_weekday(today)
        elif current.time() < time(15, 0):
            requested = _prior_weekday(today)
        else:
            requested = today
    return _previous_weekday(requested).isoformat()


def _safe_external_error(error: Exception | object) -> str:
    return redact_error(error, max_chars=120)


def _sanitize_result(result: CapitalEvidenceResult) -> CapitalEvidenceResult:
    result.notes = [_sanitize_evidence_text(note, max_chars=220) for note in result.notes if str(note or "").strip()]
    for item in result.items:
        _sanitize_evidence_item(item)
    for section in result.sections:
        if section.summary:
            section.summary = _sanitize_evidence_text(section.summary, max_chars=180)
        for item in section.items:
            _sanitize_evidence_item(item)
    return result


def _sanitize_evidence_item(item: CapitalEvidenceItem) -> CapitalEvidenceItem:
    item.source = _sanitize_evidence_text(item.source, max_chars=80)
    item.title = _sanitize_evidence_text(item.title, max_chars=80)
    if item.note:
        item.note = _sanitize_evidence_text(item.note, max_chars=220)
    item.metrics = {
        str(label): _sanitize_evidence_text(value, max_chars=220) if isinstance(value, str) else value
        for label, value in (item.metrics or {}).items()
    }
    return item


def _sanitize_evidence_text(value: object, *, max_chars: int) -> str:
    text = str(value or "").strip()
    if not text:
        return ""
    parts = [part.strip() for part in text.split("；") if part.strip()]
    if len(parts) > 1:
        return "；".join(_sanitize_evidence_text(part, max_chars=max_chars) for part in parts[:3])
    prefix, body = _split_status_prefix(text)
    if _looks_like_runtime_error(body):
        return f"{prefix}{redact_error(body, max_chars=max_chars)}"
    return redact_error(text, max_chars=max_chars)


def _split_status_prefix(text: str) -> tuple[str, str]:
    prefixes = (
        "个股资金流不可用：",
        "龙虎榜机构席位不可用：",
        "资金证据模型配置不可用，已保留本地规则分：",
        "资金证据模型分析失败，已保留本地规则分：",
        "消息缓存证据不可用：",
        "未抓取资金证据：",
    )
    for prefix in prefixes:
        if text.startswith(prefix):
            return prefix, text[len(prefix) :].strip()
    return "", text


def _looks_like_runtime_error(text: str) -> bool:
    lowered = text.lower()
    return any(
        token in lowered
        for token in (
            "httpconnectionpool",
            "httpsconnectionpool",
            "proxyerror",
            "remote disconnected",
            "max retries exceeded",
            "unable to connect to proxy",
            "socksio",
            "/api/",
            "push2his",
            "eastmoney",
        )
    ) or "超过 " in text


def _fetch_external_capital_items(stock: StockItem, start_date: str, end_date: str) -> list[CapitalEvidenceItem]:
    notes: list[str] = []
    items: list[CapitalEvidenceItem] = []
    if not env_bool("GP_CAPITAL_ENABLE_EXTERNAL", True):
        return [
            CapitalEvidenceItem(
                category="external_status",
                source="本地配置",
                title="外部资金证据抓取已关闭",
                sentiment="uncertain",
                confidence="低",
                note="仅展示消息缓存和日线量价推断指标。",
            )
        ]

    try:
        import akshare as ak
    except Exception as exc:
        return [
            CapitalEvidenceItem(
                category="external_status",
                source="AkShare",
                title="AkShare 不可用",
                sentiment="uncertain",
                confidence="低",
                note=f"未抓取资金证据：{_safe_external_error(exc)}",
            )
        ]

    fetchers = (
        ("fund_flow", _fetch_individual_fund_flow),
        ("institution_lhb", _fetch_institution_lhb),
    )
    for category, fetcher in fetchers:
        try:
            item = _call_fetcher_with_timeout(fetcher, ak, stock, start_date, end_date)
        except Exception as exc:
            notes.append(f"{_category_label(category)}不可用：{_safe_external_error(exc)}")
            continue
        if item is not None:
            items.append(item)

    if notes:
        items.append(
            CapitalEvidenceItem(
                category="external_status",
                source="外部资金接口",
                title="部分资金证据不可用",
                metrics={"说明": "；".join(notes[:3])},
                sentiment="uncertain",
                confidence="低",
            )
        )
    if not any(item.category in {"fund_flow", "institution_lhb"} for item in items):
        items.append(
            CapitalEvidenceItem(
                category="external_status",
                source="外部资金接口",
                title="未命中资金证据",
                sentiment="uncertain",
                confidence="低",
                note="未取得龙虎榜机构席位或个股资金流证据，资金侧结论保持低置信。",
            )
        )
    return items


def _call_fetcher_with_timeout(fetcher: Any, ak: Any, stock: StockItem, start_date: str, end_date: str):
    timeout_seconds = env_float("GP_CAPITAL_FETCH_TIMEOUT", 5.0, minimum=0.5, maximum=30.0)
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
    future = executor.submit(fetcher, ak, stock, start_date, end_date)
    try:
        return future.result(timeout=timeout_seconds)
    except concurrent.futures.TimeoutError as exc:
        future.cancel()
        raise TimeoutError(f"超过 {timeout_seconds:.1f}s") from exc
    finally:
        executor.shutdown(wait=False, cancel_futures=True)


def _fetch_individual_fund_flow(ak: Any, stock: StockItem, _start_date: str, _end_date: str) -> CapitalEvidenceItem | None:
    digits = stock_digits(stock.code)
    if not digits:
        return None
    frame = ak.stock_individual_fund_flow(stock=digits, market=market_prefix(stock.code))
    row = _last_valid_row(frame)
    if row is None:
        return None
    specs = (
        ("主力净流入", ("主力净流入-净额", "主力净流入净额", "主力净流入")),
        ("超大单净流入", ("超大单净流入-净额", "超大单净流入")),
        ("大单净流入", ("大单净流入-净额", "大单净流入")),
        ("中单净流入", ("中单净流入-净额", "中单净流入")),
        ("小单净流入", ("小单净流入-净额", "小单净流入")),
    )
    metrics, raw = _pick_metrics(row, specs)
    score = _fund_flow_score(raw)
    metrics["证据分"] = f"{score:.1f}"
    return CapitalEvidenceItem(
        category="fund_flow",
        source="AkShare/东方财富资金流",
        title="个股资金流",
        date=_row_text(row, ("日期", "交易日期")),
        metrics=metrics,
        sentiment=_score_sentiment(score),
        weight=FUND_FLOW_WEIGHT,
        confidence="中" if raw else "低",
        score=score,
        note="资金流为公开接口返回值，可能受数据源限流或延迟影响。",
    )


def _fetch_institution_lhb(ak: Any, stock: StockItem, start_date: str, end_date: str) -> CapitalEvidenceItem | None:
    frame = ak.stock_lhb_jgmmtj_em(start_date=compact_date(start_date), end_date=compact_date(end_date))
    row = _matching_last_row(frame, stock)
    if row is None:
        return None
    specs = (
        ("机构买入额", ("机构买入额", "机构买入金额", "买入额")),
        ("机构卖出额", ("机构卖出额", "机构卖出金额", "卖出额")),
        ("机构净买额", ("机构净买额", "净买额", "净额")),
        ("上榜原因", ("上榜原因", "解读")),
    )
    metrics, raw = _pick_metrics(row, specs)
    score = _institution_score(raw)
    metrics["证据分"] = f"{score:.1f}"
    return CapitalEvidenceItem(
        category="institution_lhb",
        source="AkShare/龙虎榜机构统计",
        title="龙虎榜机构席位",
        date=_row_text(row, ("日期", "交易日期", "上榜日")),
        metrics=metrics,
        sentiment=_score_sentiment(score),
        weight=INSTITUTION_WEIGHT,
        confidence="高" if raw.get("机构净买额") is not None else "中",
        score=score,
        note="仅代表龙虎榜公开席位统计，不等同于全部机构持仓变化。",
    )


def _load_news_cache_items(stock: StockItem) -> tuple[list[CapitalEvidenceItem], list[str]]:
    if not env_bool("GP_CAPITAL_ENABLE_NEWS_EVIDENCE", True):
        return [], ["消息/股吧证据融合已关闭。"]
    try:
        from app.services import news_rag

        if not news_rag.CACHE_PATH.exists():
            return [], [f"消息缓存不存在，未纳入股吧/新闻证据：{news_rag.CACHE_PATH}。"]
        evidence = news_rag.query_cached_evidence(
            [normalize_stock_code(stock.code)],
            env_int("GP_CAPITAL_NEWS_DAYS", 30, minimum=1, maximum=365),
            env_int("GP_CAPITAL_NEWS_LIMIT", 8, minimum=1, maximum=50),
        )
    except Exception as exc:
        return [], [f"消息缓存证据不可用：{exc}"]

    items = [_news_evidence_item(item) for item in evidence]
    if not items:
        return [], ["当前股票未命中可用股吧/新闻缓存证据。"]
    return items, [f"已纳入股吧/新闻缓存证据 {len(items)} 条；消息只作辅助确认或风险提示。"]


def _news_evidence_item(item: Any) -> CapitalEvidenceItem:
    tier = getattr(item, "source_tier", "") or "news"
    sentiment = getattr(item, "sentiment", "uncertain") or "uncertain"
    score = {"positive": 58.0, "negative": 42.0, "neutral": 50.0}.get(sentiment, 50.0)
    category = "community_sentiment" if tier == "community" else "news_rag"
    weight = 0.04 if tier == "community" else 0.08
    return CapitalEvidenceItem(
        category=category,
        source=getattr(item, "source", "") or "消息缓存",
        title=getattr(item, "title", "") or "消息证据",
        date=getattr(item, "published_at", None),
        metrics={
            "情绪": _sentiment_label(sentiment),
            "来源层": "社区讨论" if tier == "community" else "公开消息",
        },
        sentiment=sentiment,
        weight=weight,
        confidence="低" if tier == "community" else "中",
        url=getattr(item, "url", None),
        score=score,
        note=(getattr(item, "summary", None) or "")[:180],
    )


def _technical_evidence_item(trend: Any | None) -> CapitalEvidenceItem | None:
    if trend is None or not getattr(trend, "series", None):
        return None
    latest = trend.series[-1]
    signal = getattr(trend, "signal", None)
    strength = _finite_float(getattr(latest, "accumulation_strength", None))
    swing = _finite_float(getattr(latest, "swing_opportunity", None))
    rebound = _finite_float(getattr(latest, "rebound_signal", None))
    pattern = _finite_float(getattr(signal, "pattern_score", None))
    score = 50.0
    if strength is not None:
        score += max(-35.0, min(35.0, strength)) * 0.45
    if swing is not None:
        score += max(-30.0, min(30.0, swing)) * 0.25
    if rebound is not None:
        score += max(-30.0, min(30.0, rebound)) * 0.15
    if pattern is not None:
        score += max(-10.0, min(10.0, pattern))
    score = _clamp_score(score)
    return CapitalEvidenceItem(
        category="technical_behavior",
        source="日线量价技术推断",
        title="吸筹/四维擒龙技术线",
        date=getattr(latest, "date", None),
        metrics={
            "吸筹强度": _metric_or_dash(strength),
            "波段机会": _metric_or_dash(swing),
            "绝地反击": _metric_or_dash(rebound),
            "形态分": _metric_or_dash(pattern),
            "证据分": f"{score:.1f}",
        },
        sentiment=_score_sentiment(score),
        weight=TECHNICAL_WEIGHT,
        confidence="中",
        score=score,
        note="技术线只作为辅助解释，不等同于真实筹码分布或主力持仓变化。",
    )


def _apply_rule_score(result: CapitalEvidenceResult) -> None:
    buckets = {
        "资金流": (FUND_FLOW_WEIGHT, _scores_for(result.items, {"fund_flow"})),
        "机构席位": (INSTITUTION_WEIGHT, _scores_for(result.items, {"institution_lhb"})),
        "消息情绪": (NEWS_WEIGHT, _scores_for(result.items, {"news_rag", "community_sentiment"})),
        "技术推断": (TECHNICAL_WEIGHT, _scores_for(result.items, {"technical_behavior"})),
    }
    weighted_sum = 0.0
    total_weight = 0.0
    contributions: dict[str, dict[str, Any]] = {}
    for label, (weight, scores) in buckets.items():
        available = bool(scores)
        score = sum(scores) / len(scores) if scores else 50.0
        contributions[label] = {
            "score": round(score, 2) if available else None,
            "weight": weight,
            "available": available,
        }
        weighted_sum += score * weight
        total_weight += weight
    result.composite_score = round(weighted_sum / max(total_weight, 0.01), 2)
    result.contributions = contributions
    result.confidence = _overall_confidence(result.items)
    result.summary = _rule_summary(result)
    result.model_used = False
    result.notes.append("未调用模型，综合资金证据分由本地规则生成。")


def _ensure_sections(result: CapitalEvidenceResult) -> CapitalEvidenceResult:
    if result.sections:
        return result
    result.sections = [
        _build_section(result, "fund_flow", "资金流", "资金流", {"fund_flow"}),
        _build_section(result, "institution_lhb", "机构席位", "机构席位", {"institution_lhb"}),
        _build_section(result, "message_sentiment", "消息情绪", "消息情绪", {"news_rag", "community_sentiment"}),
        _build_section(result, "technical_behavior", "技术推断", "技术推断", {"technical_behavior"}),
        _build_section(result, "external_status", "接口状态", None, {"external_status"}),
    ]
    return result


def _build_section(
    result: CapitalEvidenceResult,
    key: str,
    title: str,
    contribution_key: str | None,
    categories: set[str],
) -> CapitalEvidenceSection:
    items = [item for item in result.items if item.category in categories]
    contribution = result.contributions.get(contribution_key or "", {}) if result.contributions else {}
    score = contribution.get("score") if isinstance(contribution, dict) else None
    weight = contribution.get("weight", 0.0) if isinstance(contribution, dict) else 0.0
    available = (bool(items) or bool(contribution.get("available"))) if isinstance(contribution, dict) else bool(items)
    return CapitalEvidenceSection(
        key=key,
        title=title,
        score=score,
        weight=weight,
        available=available,
        summary=_section_summary(title, score, available, len(items)),
        items=items,
    )


def _section_summary(title: str, score: Any, available: bool, item_count: int) -> str:
    if available and score is not None:
        return f"{title}证据分 {float(score):.1f}，命中 {item_count} 条证据。"
    if available:
        return f"{title}有 {item_count} 条状态或辅助证据。"
    return f"{title}暂无可用证据。"


def _apply_llm_enhancement(stock: StockItem, result: CapitalEvidenceResult, llm: LlmClientConfig | None) -> None:
    try:
        config = resolve_llm_config(llm)
    except Exception as exc:
        result.notes.append(f"资金证据模型配置不可用，已保留本地规则分：{exc}")
        return
    if not getattr(config, "api_key", None):
        return
    try:
        llm_result = _call_capital_llm(config, stock, result)
    except Exception as exc:
        result.notes.append(f"资金证据模型分析失败，已保留本地规则分：{safe_llm_error(exc)}")
        return

    llm_score = _finite_float(llm_result.get("composite_score"))
    if llm_score is not None and result.composite_score is not None:
        result.composite_score = round(result.composite_score * 0.7 + _clamp_score(llm_score) * 0.3, 2)
    confidence = str(llm_result.get("confidence") or "").strip()
    if confidence in {"低", "中", "高"}:
        result.confidence = confidence
    summary = str(llm_result.get("summary") or "").strip()
    if summary:
        result.summary = summary[:240]
    for note in safe_string_list(llm_result.get("notes"), 3):
        result.notes.append(note)
    result.model_used = True
    result.notes.append(f"已调用模型 {config.model} 基于资金、消息和技术证据参与综合判断。")


def _call_capital_llm(config: Any, stock: StockItem, result: CapitalEvidenceResult) -> dict[str, Any]:
    client = create_openai_client(config)
    request: dict[str, Any] = {
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": (
                    "你是A股资金证据分析器，只能基于输入的资金流、龙虎榜、消息缓存和技术指标判断。"
                    "不要编造主力、机构、公告或交易建议；社区内容只能作为情绪线索。"
                    "输出严格JSON：{\"summary\":\"一句中文结论\",\"composite_score\":0-100,"
                    "\"confidence\":\"低|中|高\",\"notes\":[\"说明\"]}。"
                ),
            },
            {
                "role": "user",
                "content": json.dumps(
                    {
                        "stock": {
                            "code": stock.code,
                            "name": stock.name,
                            "industry": stock.industry,
                        },
                        "local_score": result.composite_score,
                        "local_confidence": result.confidence,
                        "contributions": result.contributions,
                        "items": [
                            {
                                "category": item.category,
                                "source": item.source,
                                "title": item.title,
                                "date": item.date,
                                "metrics": item.metrics,
                                "sentiment": item.sentiment,
                                "confidence": item.confidence,
                                "score": item.score,
                                "note": item.note,
                            }
                            for item in result.items[:16]
                        ],
                    },
                    ensure_ascii=False,
                ),
            },
        ],
        "temperature": config.temperature,
    }
    if config.json_mode:
        request["response_format"] = {"type": "json_object"}
    completion = create_chat_completion(client, request)
    content = completion.choices[0].message.content or "{}"
    parsed = parse_json_response(content)
    return parsed if isinstance(parsed, dict) else {}


def _scores_for(items: Sequence[CapitalEvidenceItem], categories: set[str]) -> list[float]:
    return [_clamp_score(item.score) for item in items if item.category in categories and item.score is not None]


def _has_primary_capital_evidence(items: Sequence[CapitalEvidenceItem]) -> bool:
    return any(item.category in {"fund_flow", "institution_lhb"} for item in items)


def _overall_confidence(items: Sequence[CapitalEvidenceItem]) -> str:
    has_fund = any(item.category == "fund_flow" for item in items)
    has_institution = any(item.category == "institution_lhb" for item in items)
    evidence_count = sum(1 for item in items if item.score is not None and item.category != "technical_behavior")
    if has_fund and has_institution and evidence_count >= 2:
        return "高"
    if has_fund or has_institution or evidence_count >= 2:
        return "中"
    return "低"


def _rule_summary(result: CapitalEvidenceResult) -> str:
    score = result.composite_score
    if score is None:
        return "缺少可用资金证据，暂不形成综合判断。"
    if score >= 62:
        tone = "偏积极"
    elif score <= 42:
        tone = "偏谨慎"
    else:
        tone = "中性"
    return f"综合资金证据分 {score:.1f}，结论{tone}；资金流和机构席位优先，消息与技术线仅作辅助。"


def _load_cached_result(stock_code: str, as_of_trade_date: str) -> CapitalEvidenceResult | None:
    try:
        return _cache_adapter().load(
            CapitalEvidenceResult,
            {"stock_code": normalize_stock_code(stock_code), "as_of_trade_date": as_of_trade_date},
        )
    except Exception:
        return None


def _load_latest_cached_result(stock_code: str, *, exclude_trade_date: str | None = None) -> CapitalEvidenceResult | None:
    exclude = {"as_of_trade_date": exclude_trade_date} if exclude_trade_date else None
    try:
        return _cache_adapter().load_latest(
            CapitalEvidenceResult,
            {"stock_code": normalize_stock_code(stock_code)},
            exclude=exclude,
        )
    except Exception:
        return None


def _store_cached_result(result: CapitalEvidenceResult) -> None:
    if not result.as_of_trade_date:
        return
    try:
        _cache_adapter().store(
            {
                "stock_code": normalize_stock_code(result.stock_code),
                "as_of_trade_date": result.as_of_trade_date,
            },
            result.generated_at,
            result,
        )
    except Exception:
        return


def _cache_adapter() -> SQLiteJsonCache:
    return SQLiteJsonCache(
        CACHE_PATH,
        "capital_evidence_cache",
        ("stock_code", "as_of_trade_date"),
        order_columns=("as_of_trade_date", "generated_at"),
    )


def _pick_metrics(row: pd.Series, specs: Iterable[tuple[str, tuple[str, ...]]]) -> tuple[dict[str, str], dict[str, float]]:
    metrics: dict[str, str] = {}
    raw: dict[str, float] = {}
    for label, keywords in specs:
        value = _row_value(row, keywords)
        if value is not None:
            metrics[label] = _format_metric(value)
            numeric = _numeric_value(value)
            if numeric is not None:
                raw[label] = numeric
    return metrics, raw


def _matching_last_row(frame: Any, stock: StockItem) -> pd.Series | None:
    if not isinstance(frame, pd.DataFrame) or frame.empty:
        return None
    digits = stock_digits(stock.code)
    name = (stock.name or "").strip()
    matched = frame
    if digits:
        code_columns = [column for column in frame.columns if "代码" in str(column)]
        if code_columns:
            mask = pd.Series(False, index=frame.index)
            for column in code_columns:
                mask = mask | frame[column].astype(str).str.contains(digits, na=False)
            matched = frame[mask]
    if matched.empty and name:
        name_columns = [column for column in frame.columns if "名称" in str(column) or "股票" in str(column)]
        if name_columns:
            mask = pd.Series(False, index=frame.index)
            for column in name_columns:
                mask = mask | frame[column].astype(str).str.contains(name, na=False)
            matched = frame[mask]
    return _last_valid_row(matched)


def _last_valid_row(frame: Any) -> pd.Series | None:
    if not isinstance(frame, pd.DataFrame) or frame.empty:
        return None
    cleaned = frame.dropna(how="all")
    return cleaned.tail(1).iloc[0] if not cleaned.empty else None


def _row_text(row: pd.Series, keywords: tuple[str, ...]) -> str | None:
    value = _row_value(row, keywords)
    return None if value is None else str(value).strip()


def _row_value(row: pd.Series, keywords: tuple[str, ...]) -> Any | None:
    for keyword in keywords:
        for column in row.index:
            if keyword in str(column):
                value = row[column]
                if _has_value(value):
                    return value
    return None


def _has_value(value: Any) -> bool:
    if value is None:
        return False
    try:
        return bool(pd.notna(value))
    except Exception:
        return True


def _format_metric(value: Any) -> str:
    number = _numeric_value(value)
    if number is not None:
        if abs(number) >= 100_000_000:
            return f"{number / 100_000_000:.2f} 亿"
        if abs(number) >= 10_000:
            return f"{number / 10_000:.2f} 万"
        return f"{number:.2f}"
    return str(value).strip()


def _numeric_value(value: Any) -> float | None:
    if isinstance(value, Real) and pd.notna(value):
        number = float(value)
        return number if math.isfinite(number) else None
    text = str(value or "").strip().replace(",", "")
    if not text or text in {"-", "None", "nan"}:
        return None
    multiplier = 1.0
    if text.endswith("亿"):
        multiplier = 100_000_000.0
        text = text[:-1]
    elif text.endswith("万"):
        multiplier = 10_000.0
        text = text[:-1]
    try:
        number = float(text)
    except ValueError:
        return None
    return number * multiplier if math.isfinite(number) else None


def _fund_flow_score(raw: dict[str, float]) -> float:
    main = raw.get("主力净流入")
    super_order = raw.get("超大单净流入")
    large_order = raw.get("大单净流入")
    value = 0.0
    if main is not None:
        value += _amount_signal(main, 120_000_000.0) * 0.55
    if super_order is not None:
        value += _amount_signal(super_order, 80_000_000.0) * 0.25
    if large_order is not None:
        value += _amount_signal(large_order, 60_000_000.0) * 0.20
    return _clamp_score(50.0 + value * 36.0)


def _institution_score(raw: dict[str, float]) -> float:
    net = raw.get("机构净买额")
    if net is None and raw.get("机构买入额") is not None and raw.get("机构卖出额") is not None:
        net = raw["机构买入额"] - raw["机构卖出额"]
    if net is None:
        return 50.0
    return _clamp_score(50.0 + _amount_signal(net, 150_000_000.0) * 38.0)


def _amount_signal(value: float, strong: float) -> float:
    return max(-1.0, min(1.0, value / max(strong, 1.0)))


def _score_sentiment(score: float) -> str:
    if score >= 58:
        return "positive"
    if score <= 42:
        return "negative"
    return "neutral"


def _sentiment_label(sentiment: str) -> str:
    return {"positive": "偏积极", "negative": "偏谨慎", "neutral": "中性"}.get(sentiment, "不确定")


def _clamp_score(value: float | None) -> float:
    if value is None or not math.isfinite(float(value)):
        return 50.0
    return max(0.0, min(100.0, float(value)))


def _finite_float(value: Any) -> float | None:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    return number if math.isfinite(number) else None


def _metric_or_dash(value: float | None) -> str:
    return "-" if value is None else f"{value:.2f}"


def _parse_date(value: str | None) -> date | None:
    raw = compact_date(value or "")
    if len(raw) != 8:
        return None
    try:
        return datetime.strptime(raw, "%Y%m%d").date()
    except ValueError:
        return None


def _previous_weekday(day: date) -> date:
    current = day
    while current.weekday() >= 5:
        current -= timedelta(days=1)
    return current


def _prior_weekday(day: date) -> date:
    return _previous_weekday(day - timedelta(days=1))


def _category_label(category: str) -> str:
    labels = {
        "fund_flow": "个股资金流",
        "institution_lhb": "龙虎榜机构席位",
    }
    return labels.get(category, category)
