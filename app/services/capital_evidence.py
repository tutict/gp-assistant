from __future__ import annotations

import concurrent.futures
import json
import math
import os
from datetime import date, datetime, time, timedelta
from io import StringIO
from numbers import Real
from pathlib import Path
from typing import Any, Iterable, Sequence

import pandas as pd

from app.providers.base import proxy_environment
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
from app.services.text_utils import format_amount


CACHE_PATH = Path(os.getenv("GP_CAPITAL_CACHE", "data/cache/capital_evidence.sqlite"))
FUND_FLOW_WEIGHT = 0.35
INSTITUTION_WEIGHT = 0.25
NEWS_WEIGHT = 0.15
TECHNICAL_WEIGHT = 0.25
FRESHNESS_CACHE = "fresh-cache"
FRESHNESS_REFRESHED = "refreshed"
FRESHNESS_STALE = "stale-cache"
INSTITUTION_BUY_LABEL = "\u673a\u6784\u4e70\u5165\u989d"
INSTITUTION_SELL_LABEL = "\u673a\u6784\u5356\u51fa\u989d"
INSTITUTION_NET_LABEL = "\u673a\u6784\u51c0\u4e70\u989d"
LHB_REASON_LABEL = "\u4e0a\u699c\u539f\u56e0"
INSTITUTION_STATUS_CATEGORY = "institution_lhb_status"
INSTITUTION_SOURCE_NAMES = (
    "东方财富龙虎榜机构统计",
    "新浪龙虎榜机构席位明细",
    "新浪龙虎榜机构席位追踪",
)


def fetch_capital_evidence(
    stock: StockItem,
    start_date: str,
    end_date: str,
    *,
    trend: Any | None = None,
    llm: LlmClientConfig | None = None,
    refresh: bool = False,
    proxy_mode: str | None = None,
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

    result.items.extend(_fetch_external_capital_items(stock, start_date, end_date, proxy_mode=proxy_mode))
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


def _fetch_external_capital_items(
    stock: StockItem,
    start_date: str,
    end_date: str,
    *,
    proxy_mode: str | None = None,
) -> list[CapitalEvidenceItem]:
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
        ("fund_flow", _fetch_ths_individual_fund_flow, env_float("GP_CAPITAL_FUND_FLOW_TIMEOUT", 8.0, minimum=2.0, maximum=45.0), True),
        ("institution_lhb", _fetch_institution_lhb, env_float("GP_CAPITAL_LHB_TIMEOUT", 12.0, minimum=2.0, maximum=60.0), env_bool("GP_CAPITAL_LHB_FAST_TIMEOUT", False)),
    )
    for category, fetcher, timeout_seconds, use_timeout in fetchers:
        try:
            if use_timeout:
                item = _call_fetcher_with_timeout(
                    fetcher,
                    ak,
                    stock,
                    start_date,
                    end_date,
                    proxy_mode=proxy_mode,
                    timeout_seconds=timeout_seconds,
                )
            else:
                with proxy_environment(proxy_mode):
                    item = fetcher(ak, stock, start_date, end_date)
        except Exception as exc:
            notes.append(f"{_category_label(category)}不可用：{_safe_external_error(exc)}")
            if category == "institution_lhb":
                items.append(
                    _institution_lhb_unavailable_item(
                        start_date,
                        end_date,
                        INSTITUTION_SOURCE_NAMES,
                        [f"机构席位抓取：{_safe_external_error(exc)}"],
                    )
                )
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
        items[-1].note = (
            "\u672a\u53d6\u5f97\u4e1c\u65b9\u8d22\u5bcc/\u65b0\u6d6a\u9f99\u864e\u699c\u673a\u6784\u5e2d\u4f4d\u8bc1\u636e\uff0c"
            "\u8d44\u91d1\u4fa7\u7ed3\u8bba\u4fdd\u6301\u4f4e\u7f6e\u4fe1\u3002"
        )
    return items


def _call_fetcher_with_timeout(
    fetcher: Any,
    ak: Any,
    stock: StockItem,
    start_date: str,
    end_date: str,
    *,
    proxy_mode: str | None = None,
    timeout_seconds: float | None = None,
):
    if timeout_seconds is None:
        timeout_seconds = env_float("GP_CAPITAL_FETCH_TIMEOUT", 5.0, minimum=0.5, maximum=30.0)
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)

    def run_fetcher():
        with proxy_environment(proxy_mode):
            return fetcher(ak, stock, start_date, end_date)

    future = executor.submit(run_fetcher)
    try:
        return future.result(timeout=timeout_seconds)
    except concurrent.futures.TimeoutError as exc:
        future.cancel()
        raise TimeoutError(f"超过 {timeout_seconds:.1f}s") from exc
    finally:
        executor.shutdown(wait=False, cancel_futures=True)


def _fetch_ths_individual_fund_flow(
    ak: Any,
    stock: StockItem,
    _start_date: str,
    end_date: str,
) -> CapitalEvidenceItem | None:
    digits = stock_digits(stock.code)
    if not digits:
        return None
    frame = None
    if getattr(ak, "__name__", "") == "akshare" and env_bool("GP_CAPITAL_USE_THS_DIRECT", True):
        frame = _fetch_ths_fund_flow_direct(stock)
    if frame is None or frame.empty:
        frame = ak.stock_fund_flow_individual(symbol="\u5373\u65f6")
    row = _matching_last_row(frame, stock)
    if row is None:
        return None
    latest_price = _row_text(row, ("\u6700\u65b0\u4ef7",))
    change_pct = _row_text(row, ("\u6da8\u8dcc\u5e45",))
    turnover = _row_text(row, ("\u6362\u624b\u7387",))
    inflow = _row_text(row, ("\u6d41\u5165\u8d44\u91d1",))
    outflow = _row_text(row, ("\u6d41\u51fa\u8d44\u91d1",))
    net = _row_text(row, ("\u51c0\u989d",))
    amount = _row_text(row, ("\u6210\u4ea4\u989d",))
    metrics = {
        label: value
        for label, value in {
            "\u6700\u65b0\u4ef7": latest_price,
            "\u6da8\u8dcc\u5e45": change_pct,
            "\u6362\u624b\u7387": turnover,
            "\u6d41\u5165\u8d44\u91d1": inflow,
            "\u6d41\u51fa\u8d44\u91d1": outflow,
            "\u51c0\u989d": net,
            "\u6210\u4ea4\u989d": amount,
        }.items()
        if value
    }
    raw = {
        "\u6d41\u5165\u8d44\u91d1": _numeric_value(inflow),
        "\u6d41\u51fa\u8d44\u91d1": _numeric_value(outflow),
        "\u51c0\u989d": _numeric_value(net),
        "\u6210\u4ea4\u989d": _numeric_value(amount),
    }
    raw = {key: value for key, value in raw.items() if value is not None}
    score = _ths_fund_flow_score(raw)
    metrics["\u8bc1\u636e\u5206"] = f"{score:.1f}"
    return CapitalEvidenceItem(
        category="fund_flow",
        source="AkShare/\u540c\u82b1\u987a\u4e2a\u80a1\u8d44\u91d1\u6d41",
        title="\u540c\u82b1\u987a\u4e2a\u80a1\u8d44\u91d1\u6d41",
        date=effective_trade_date(end_date),
        metrics=metrics,
        sentiment=_score_sentiment(score),
        weight=FUND_FLOW_WEIGHT,
        confidence="\u4e2d" if "\u51c0\u989d" in raw else "\u4f4e",
        score=score,
        note="\u540c\u82b1\u987a\u4e2a\u80a1\u8d44\u91d1\u6d41\u5f53\u65e5\u5feb\u7167\uff0c\u7528\u4e8e\u8865\u9f50\u89c2\u5bdf\u9875\u8d44\u91d1\u6d41\u8bc1\u636e\uff0c\u4e0d\u7b49\u540c\u4e8e\u6301\u4ed3\u7ed3\u8bba\u3002",
    )


def _fetch_ths_fund_flow_direct(stock: StockItem) -> pd.DataFrame | None:
    digits = stock_digits(stock.code)
    if not digits:
        return None
    try:
        import requests
        from bs4 import BeautifulSoup
        from akshare.stock_feature import stock_fund_flow as ths_mod
    except Exception:
        return None

    headers = _ths_headers(ths_mod)
    first_frame, first_html = _read_ths_fund_page(1, headers)
    page_count = _ths_page_count(first_html) or 110
    target = int(digits)
    low, high = 1, page_count
    for _ in range(10):
        if low > high:
            break
        page = (low + high) // 2
        frame = first_frame if page == 1 else _read_ths_fund_page(page, headers)[0]
        if frame is None or frame.empty:
            refreshed_headers = _ths_headers(ths_mod)
            frame = _read_ths_fund_page(page, refreshed_headers)[0]
        code_column = _first_matching_column(frame, ("\u80a1\u7968\u4ee3\u7801", "\u4ee3\u7801"))
        if frame is None or frame.empty or code_column is None:
            return None
        codes = frame[code_column].astype(str).str.extract(r"(\d{1,6})", expand=False).fillna("").str.zfill(6)
        numbers = pd.to_numeric(codes, errors="coerce").dropna()
        if numbers.empty:
            return None
        page_min = int(numbers.min())
        page_max = int(numbers.max())
        matched = frame[codes == digits]
        if not matched.empty:
            return matched
        if target < page_min:
            high = page - 1
        elif target > page_max:
            low = page + 1
        else:
            return None
    return None


def _ths_headers(ths_mod: Any) -> dict[str, str]:
    js_code = ths_mod.py_mini_racer.MiniRacer()
    js_code.eval(ths_mod._get_file_content_ths("ths.js"))
    v_code = js_code.call("v")
    return {
        "Accept": "text/html, */*; q=0.01",
        "Accept-Encoding": "gzip, deflate",
        "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
        "Cache-Control": "no-cache",
        "Connection": "keep-alive",
        "hexin-v": v_code,
        "Host": "data.10jqka.com.cn",
        "Pragma": "no-cache",
        "Referer": "http://data.10jqka.com.cn/funds/hyzjl/",
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Safari/537.36",
        "X-Requested-With": "XMLHttpRequest",
    }


def _read_ths_fund_page(page: int, headers: dict[str, str]) -> tuple[pd.DataFrame | None, str]:
    import requests

    url = (
        "http://data.10jqka.com.cn/funds/ggzjl/"
        f"field/code/order/asc/page/{max(1, page)}/ajax/1/free/1/"
    )
    response = requests.get(url, headers=headers, timeout=env_float("GP_CAPITAL_THS_PAGE_TIMEOUT", 8.0, minimum=1.0, maximum=20.0))
    html = response.text
    if response.status_code >= 400 or "<table" not in html:
        return None, html
    try:
        tables = pd.read_html(StringIO(html))
    except Exception:
        return None, html
    if not tables:
        return None, html
    return tables[0], html


def _ths_page_count(html: str) -> int | None:
    try:
        from bs4 import BeautifulSoup

        soup = BeautifulSoup(html or "", "lxml")
        raw = soup.find(name="span", attrs={"class": "page_info"})
        if raw is None:
            return None
        parts = raw.text.strip().split("/")
        return int(parts[-1]) if parts and parts[-1].isdigit() else None
    except Exception:
        return None


def _first_matching_column(frame: pd.DataFrame | None, keywords: tuple[str, ...]) -> Any | None:
    if frame is None:
        return None
    for column in frame.columns:
        if any(keyword in str(column) for keyword in keywords):
            return column
    return None


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
    attempted_sources: list[str] = []
    successful_sources: list[str] = []
    failures: list[str] = []

    source = INSTITUTION_SOURCE_NAMES[0]
    attempted_sources.append(source)
    try:
        frame = ak.stock_lhb_jgmmtj_em(start_date=compact_date(start_date), end_date=compact_date(end_date))
    except Exception as exc:
        failures.append(f"{source}：{_safe_external_error(exc)}")
    else:
        successful_sources.append(source)
        try:
            row = _matching_last_row(frame, stock)
        except Exception as exc:
            row = None
            failures.append(f"{source}解析：{_safe_external_error(exc)}")
        if row is not None:
            modern_item = _institution_lhb_item_from_row(
                row,
                source="AkShare/\u4e1c\u65b9\u8d22\u5bcc\u9f99\u864e\u699c\u673a\u6784\u7edf\u8ba1",
                title="\u4e1c\u65b9\u8d22\u5bcc\u9f99\u864e\u699c\u673a\u6784\u5e2d\u4f4d",
                fallback_note="\u4e1c\u65b9\u8d22\u5bcc\u9f99\u864e\u699c\u673a\u6784\u5e2d\u4f4d\u7edf\u8ba1\uff0c\u4e0d\u7b49\u540c\u4e8e\u5168\u90e8\u673a\u6784\u6301\u4ed3\u53d8\u5316\u3002",
            )
            if modern_item is not None:
                return modern_item
            failures.append(f"{source}：命中股票但缺少可读席位指标")

    sina_fetchers = (
        (INSTITUTION_SOURCE_NAMES[1], _fetch_sina_institution_lhb),
        (INSTITUTION_SOURCE_NAMES[2], _fetch_sina_institution_lhb_tracking),
    )
    for source, fetcher in sina_fetchers:
        attempted_sources.append(source)
        try:
            item = fetcher(ak, stock, start_date, end_date)
        except Exception as exc:
            failures.append(f"{source}：{_safe_external_error(exc)}")
            continue
        successful_sources.append(source)
        if item is not None:
            return item

    if successful_sources:
        return _institution_lhb_no_hit_item(stock, start_date, end_date, attempted_sources)
    return _institution_lhb_unavailable_item(start_date, end_date, attempted_sources, failures)


def _institution_lhb_item_from_row(
    row: pd.Series,
    *,
    source: str,
    title: str,
    fallback_note: str,
) -> CapitalEvidenceItem | None:
    specs = (
        (
            INSTITUTION_BUY_LABEL,
            (
                "\u673a\u6784\u4e70\u5165\u603b\u989d",
                "\u673a\u6784\u5e2d\u4f4d\u4e70\u5165\u989d",
                "\u673a\u6784\u4e70\u5165\u989d",
                "\u7d2f\u79ef\u4e70\u5165\u989d",
                "\u7d2f\u8ba1\u4e70\u5165\u989d",
                "\u4e70\u5165\u989d",
            ),
        ),
        (
            INSTITUTION_SELL_LABEL,
            (
                "\u673a\u6784\u5356\u51fa\u603b\u989d",
                "\u673a\u6784\u5e2d\u4f4d\u5356\u51fa\u989d",
                "\u673a\u6784\u5356\u51fa\u989d",
                "\u7d2f\u79ef\u5356\u51fa\u989d",
                "\u7d2f\u8ba1\u5356\u51fa\u989d",
                "\u5356\u51fa\u989d",
            ),
        ),
        (INSTITUTION_NET_LABEL, ("\u673a\u6784\u51c0\u4e70\u989d", "\u673a\u6784\u4e70\u5165\u51c0\u989d", "\u51c0\u4e70\u989d", "\u51c0\u989d")),
        (LHB_REASON_LABEL, ("\u4e0a\u699c\u539f\u56e0", "\u89e3\u8bfb", "\u7c7b\u578b")),
    )
    metrics, raw = _pick_metrics(row, specs)
    if raw.get(INSTITUTION_NET_LABEL) is None:
        buy = raw.get(INSTITUTION_BUY_LABEL)
        sell = raw.get(INSTITUTION_SELL_LABEL)
        if buy is not None and sell is not None:
            net = buy - sell
            raw[INSTITUTION_NET_LABEL] = net
            metrics[INSTITUTION_NET_LABEL] = _format_metric(net)
    if not metrics and not raw:
        return None
    score = _institution_score(raw)
    metrics["\u8bc1\u636e\u5206"] = f"{score:.1f}"
    return CapitalEvidenceItem(
        category="institution_lhb",
        source=source,
        title=title,
        date=_row_text(row, ("\u4ea4\u6613\u65e5\u671f", "\u65e5\u671f", "\u4e0a\u699c\u65e5")),
        metrics=metrics,
        sentiment=_score_sentiment(score),
        weight=INSTITUTION_WEIGHT,
        confidence="\u9ad8" if raw.get(INSTITUTION_NET_LABEL) is not None else "\u4e2d",
        score=score,
        note=fallback_note,
    )


def _safe_fetch_sina_institution_lhb(
    ak: Any,
    stock: StockItem,
    start_date: str,
    end_date: str,
) -> CapitalEvidenceItem | None:
    try:
        return _fetch_sina_institution_lhb(ak, stock, start_date, end_date)
    except Exception:
        return None


def _fetch_sina_institution_lhb(
    ak: Any,
    stock: StockItem,
    _start_date: str,
    _end_date: str,
) -> CapitalEvidenceItem | None:
    frame = ak.stock_lhb_jgmx_sina()
    row = _matching_last_row(frame, stock)
    if row is None:
        return None
    specs = (
        (INSTITUTION_BUY_LABEL, ("\u673a\u6784\u5e2d\u4f4d\u4e70\u5165\u989d", "\u7d2f\u79ef\u4e70\u5165\u989d", "\u7d2f\u8ba1\u4e70\u5165\u989d", INSTITUTION_BUY_LABEL)),
        (INSTITUTION_SELL_LABEL, ("\u673a\u6784\u5e2d\u4f4d\u5356\u51fa\u989d", "\u7d2f\u79ef\u5356\u51fa\u989d", "\u7d2f\u8ba1\u5356\u51fa\u989d", INSTITUTION_SELL_LABEL)),
        (INSTITUTION_NET_LABEL, (INSTITUTION_NET_LABEL, "\u673a\u6784\u4e70\u5165\u51c0\u989d", "\u51c0\u989d")),
        (LHB_REASON_LABEL, ("\u7c7b\u578b", "\u89e3\u8bfb", LHB_REASON_LABEL)),
    )
    metrics, raw = _pick_metrics(row, specs)
    if raw.get(INSTITUTION_NET_LABEL) is None:
        buy = raw.get(INSTITUTION_BUY_LABEL)
        sell = raw.get(INSTITUTION_SELL_LABEL)
        if buy is not None and sell is not None:
            net = buy - sell
            raw[INSTITUTION_NET_LABEL] = net
            metrics[INSTITUTION_NET_LABEL] = _format_metric(net)
    score = _institution_score(raw)
    metrics["\u8bc1\u636e\u5206"] = f"{score:.1f}"
    return CapitalEvidenceItem(
        category="institution_lhb",
        source="AkShare/\u65b0\u6d6a\u9f99\u864e\u699c\u673a\u6784\u5e2d\u4f4d",
        title="\u65b0\u6d6a\u9f99\u864e\u699c\u673a\u6784\u5e2d\u4f4d",
        date=_row_text(row, ("\u4ea4\u6613\u65e5\u671f", "\u65e5\u671f", "\u4e0a\u699c\u65e5")),
        metrics=metrics,
        sentiment=_score_sentiment(score),
        weight=INSTITUTION_WEIGHT,
        confidence="\u4e2d" if raw else "\u4f4e",
        score=score,
        note="\u4e1c\u65b9\u8d22\u5bcc\u9f99\u864e\u699c\u672a\u547d\u4e2d\u6216\u4e0d\u53ef\u7528\u65f6\uff0c\u4f7f\u7528\u65b0\u6d6a\u673a\u6784\u5e2d\u4f4d\u660e\u7ec6\u4f5c\u5907\u7528\u8bc1\u636e\u3002",
    )


def _fetch_sina_institution_lhb_tracking(
    ak: Any,
    stock: StockItem,
    start_date: str,
    end_date: str,
) -> CapitalEvidenceItem | None:
    days = _window_days(start_date, end_date)
    if days <= 30:
        symbol = "30"
    elif days <= 60:
        symbol = "60"
    else:
        symbol = "120"
    frame = ak.stock_lhb_jgzz_sina(symbol=symbol)
    row = _matching_last_row(frame, stock)
    if row is None:
        return None
    item = _institution_lhb_item_from_row(
        row,
        source="AkShare/新浪龙虎榜机构席位追踪",
        title="新浪龙虎榜机构席位追踪",
        fallback_note=f"新浪龙虎榜机构席位追踪按近 {symbol} 日统计，作为东方财富龙虎榜机构统计的备用证据。",
    )
    if item is not None:
        item.date = item.date or _window_date_label(start_date, end_date)
        item.metrics.setdefault(LHB_REASON_LABEL, f"近 {symbol} 日机构席位追踪")
    return item


def _institution_lhb_no_hit_item(
    stock: StockItem,
    start_date: str,
    end_date: str,
    attempted_sources: Sequence[str],
) -> CapitalEvidenceItem:
    days = _window_days(start_date, end_date)
    window = _window_date_label(start_date, end_date)
    return CapitalEvidenceItem(
        category=INSTITUTION_STATUS_CATEGORY,
        source="东方财富龙虎榜 / 新浪龙虎榜",
        title=f"近 {days} 日未上龙虎榜机构席位",
        date=effective_trade_date(end_date),
        metrics={
            "状态": f"近 {days} 日未上榜",
            "查询窗口": window,
            "已尝试信源": "、".join(attempted_sources),
            "股票": f"{stock.name or stock.code} {normalize_stock_code(stock.code)}",
        },
        sentiment="uncertain",
        weight=INSTITUTION_WEIGHT,
        confidence="中",
        note="没有龙虎榜机构专用席位记录，不代表机构没有买卖；它只说明查询窗口内未公开上榜。",
    )


def _institution_lhb_unavailable_item(
    start_date: str,
    end_date: str,
    attempted_sources: Sequence[str],
    failures: Sequence[str],
) -> CapitalEvidenceItem:
    metrics = {
        "状态": "接口不可用",
        "查询窗口": _window_date_label(start_date, end_date),
        "已尝试信源": "、".join(attempted_sources),
    }
    if failures:
        metrics["失败源"] = "；".join(failures[:3])
    return CapitalEvidenceItem(
        category=INSTITUTION_STATUS_CATEGORY,
        source="东方财富龙虎榜 / 新浪龙虎榜",
        title="机构席位接口不可用",
        date=effective_trade_date(end_date),
        metrics=metrics,
        sentiment="uncertain",
        weight=INSTITUTION_WEIGHT,
        confidence="低",
        note="已跳过本次机构席位外部请求，资金侧结论不使用机构席位加分。",
    )


def _load_news_cache_items(stock: StockItem) -> tuple[list[CapitalEvidenceItem], list[str]]:
    if not env_bool("GP_CAPITAL_ENABLE_NEWS_EVIDENCE", True):
        return [], ["消息/股吧证据融合已关闭。"]
    try:
        from app.services import news_rag

        days = env_int("GP_CAPITAL_NEWS_DAYS", 30, minimum=1, maximum=365)
        limit = env_int("GP_CAPITAL_NEWS_LIMIT", 8, minimum=1, maximum=50)
        notes: list[str] = []
        evidence = []
        if news_rag.CACHE_PATH.exists():
            evidence = news_rag.query_cached_evidence([normalize_stock_code(stock.code)], days, limit)
        else:
            notes.append(f"消息缓存不存在，观察页将按需抓取新闻/股吧证据：{news_rag.CACHE_PATH}。")
        if not evidence and env_bool("GP_CAPITAL_NEWS_ON_DEMAND", True):
            evidence, fetch_notes = news_rag.fetch_and_cache_stock_evidence([stock], days, limit)
            notes.extend(fetch_notes)
    except Exception as exc:
        return [], [f"消息缓存证据不可用：{exc}"]

    items = [_news_evidence_item(item) for item in evidence]
    if not items:
        return [], [*notes, "当前股票未命中可用股吧/新闻证据。"]
    return items, [*notes, f"已纳入股吧/新闻证据 {len(items)} 条；消息只作辅助确认或风险提示。"]


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
        note="技术线只作为辅助解释，不等同于成本分布估算或主力持仓变化。",
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
    _backfill_institution_status(result)
    result.sections = [
        _build_section(result, "fund_flow", "资金流", "资金流", {"fund_flow"}),
        _build_section(
            result,
            "institution_lhb",
            "机构席位",
            "机构席位",
            {"institution_lhb", INSTITUTION_STATUS_CATEGORY},
        ),
        _build_section(result, "message_sentiment", "消息情绪", "消息情绪", {"news_rag", "community_sentiment"}),
        _build_section(result, "technical_behavior", "技术推断", "技术推断", {"technical_behavior"}),
    ]
    return result


def _backfill_institution_status(result: CapitalEvidenceResult) -> None:
    if any(item.category in {"institution_lhb", INSTITUTION_STATUS_CATEGORY} for item in result.items):
        return
    context = " ".join(
        str(part)
        for part in [
            result.summary or "",
            *result.notes,
            *[item.title for item in result.items if item.category == "external_status"],
            *[str(item.metrics) for item in result.items if item.category == "external_status"],
        ]
        if str(part or "").strip()
    )
    if any(
        keyword in context
        for keyword in ("机构席位不可用", "龙虎榜机构席位不可用", "机构席位抓取", "抓取已关闭", "AkShare 不可用", "超时", "timeout")
    ):
        failure_parts = [part for part in safe_string_list(result.notes, 4) if "机构席位" in part]
        for item in result.items:
            if item.category != "external_status":
                continue
            if item.metrics:
                for value in item.metrics.values():
                    if "机构席位" in str(value) or "龙虎榜" in str(value):
                        failure_parts.append(str(value))
            if item.note and ("机构席位" in item.note or "龙虎榜" in item.note):
                failure_parts.append(item.note)
        if not failure_parts:
            failure_parts.append("机构席位状态来自旧缓存或本地配置，缺少可复原的失败明细。")
        result.items.append(
            _institution_lhb_unavailable_item(
                result.as_of_trade_date or effective_trade_date(None),
                result.as_of_trade_date or effective_trade_date(None),
                INSTITUTION_SOURCE_NAMES,
                failure_parts[:3],
            )
        )
        return
    result.items.append(
        _institution_lhb_no_hit_item(
            StockItem(code=result.stock_code, name="", industry="", price=0.0),
            result.as_of_trade_date or effective_trade_date(None),
            result.as_of_trade_date or effective_trade_date(None),
            INSTITUTION_SOURCE_NAMES,
        )
    )


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
        exact_code_mask = pd.Series(False, index=frame.index)
        for column in frame.columns:
            extracted = frame[column].astype(str).str.extract(r"(\d{6})", expand=False)
            exact_code_mask = exact_code_mask | (extracted == digits)
        if exact_code_mask.any():
            return _last_valid_row(frame[exact_code_mask])
    if digits:
        code_columns = [column for column in frame.columns if "代码" in str(column)]
        if code_columns:
            mask = pd.Series(False, index=frame.index)
            for column in code_columns:
                extracted = frame[column].astype(str).str.extract(r"(\d{1,6})", expand=False).fillna("")
                mask = mask | (extracted.str.zfill(6) == digits)
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
        return _format_amount(number)
    return str(value).strip()


def _format_amount(number: float) -> str:
    return format_amount(number, decimals=2)


def _numeric_value(value: Any) -> float | None:
    if isinstance(value, Real) and pd.notna(value):
        number = float(value)
        return number if math.isfinite(number) else None
    text = str(value or "").strip().replace(",", "")
    if not text or text in {"-", "None", "nan"}:
        return None
    multiplier = 1.0
    if text.endswith("\u4ebf"):
        multiplier = 100_000_000.0
        text = text[:-1]
    elif text.endswith("\u4e07"):
        multiplier = 10_000.0
        text = text[:-1]
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


def _ths_fund_flow_score(raw: dict[str, float]) -> float:
    net = raw.get("\u51c0\u989d")
    amount = raw.get("\u6210\u4ea4\u989d")
    inflow = raw.get("\u6d41\u5165\u8d44\u91d1")
    outflow = raw.get("\u6d41\u51fa\u8d44\u91d1")
    value = 0.0
    if net is not None:
        value += _amount_signal(net, 300_000_000.0) * 0.62
    if net is not None and amount:
        ratio = max(-0.08, min(0.08, net / max(abs(amount), 1.0))) / 0.08
        value += ratio * 0.28
    if inflow is not None and outflow is not None:
        denominator = max(inflow + outflow, 1.0)
        balance = max(-0.12, min(0.12, (inflow - outflow) / denominator)) / 0.12
        value += balance * 0.10
    return _clamp_score(50.0 + value * 38.0)


def _institution_score(raw: dict[str, float]) -> float:
    canonical_net = raw.get(INSTITUTION_NET_LABEL)
    canonical_buy = raw.get(INSTITUTION_BUY_LABEL)
    canonical_sell = raw.get(INSTITUTION_SELL_LABEL)
    if canonical_net is None and canonical_buy is not None and canonical_sell is not None:
        canonical_net = canonical_buy - canonical_sell
    if canonical_net is not None:
        return _clamp_score(50.0 + _amount_signal(canonical_net, 150_000_000.0) * 38.0)
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


def _window_days(start_date: str | None, end_date: str | None) -> int:
    start = _parse_date(start_date)
    end = _parse_date(end_date)
    if start is None or end is None:
        return env_int("GP_CAPITAL_LHB_LOOKBACK_DAYS", 30, minimum=1, maximum=365)
    if end < start:
        start, end = end, start
    return max(1, min(365, (end - start).days + 1))


def _window_date_label(start_date: str | None, end_date: str | None) -> str:
    start = _parse_date(start_date)
    end = _parse_date(end_date)
    if start is None or end is None:
        return "最近可用交易窗口"
    if end < start:
        start, end = end, start
    return f"{start.isoformat()} 至 {end.isoformat()}"


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
        INSTITUTION_STATUS_CATEGORY: "机构席位状态",
    }
    return labels.get(category, category)
