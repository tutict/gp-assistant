from __future__ import annotations

from datetime import date, datetime, timedelta

from app.providers.base import StockProvider
from app.schemas import StockObservation, StockObserveRequest, TrendIndicatorRequest
from app.services.capital_evidence import fetch_capital_evidence
from app.services.trend_indicator import analyze_trend

DEFAULT_OBSERVE_TRADING_DAYS = 10

def observe_stock(provider: StockProvider, request: StockObserveRequest) -> StockObservation:
    start_value, end_value = _default_dates(request.start_date, request.end_date)

    stock = provider.get_stock(request.code)
    source = getattr(provider, "name", provider.__class__.__name__)
    notes = [f"\u6570\u636e\u6e90\uff1a{_provider_display_name(source)}\u3002"]

    try:
        financial_indicators = provider.get_financial_indicators(stock)
    except Exception as exc:
        financial_indicators = None
        notes.append(f"\u8d22\u52a1\u6307\u6807\u4e0d\u53ef\u7528\uff1a{exc}")

    try:
        trend = analyze_trend(
            provider,
            TrendIndicatorRequest(
                code=stock.code,
                start_date=start_value,
                end_date=end_value,
                series_limit=max(20, min(request.series_limit, 500)),
                include_chip_distribution=True,
            ),
        )
    except Exception as exc:
        trend = None
        notes.append(f"\u8d8b\u52bf\u6307\u6807\u4e0d\u53ef\u7528\uff1a{exc}")

    try:
        capital_evidence = fetch_capital_evidence(
            stock,
            start_value,
            end_value,
            trend=trend,
            llm=request.llm,
            proxy_mode=getattr(provider, "proxy_mode", None),
        )
    except Exception as exc:
        capital_evidence = None
        notes.append(f"资金证据不可用：{exc}")

    order_book = None
    if request.include_order_book:
        try:
            order_book = provider.get_order_book(stock.code)
        except Exception as exc:
            notes.append(f"\u76d8\u53e3\u4e0d\u53ef\u7528\uff1a{exc}")

    return StockObservation(
        source=source,
        stock=stock,
        financial_indicators=financial_indicators,
        trend=trend,
        capital_evidence=capital_evidence,

        order_book=order_book,
        notes=notes,
    )

def _provider_display_name(source: str) -> str:
    labels = {
        "tdx": "\u901a\u8fbe\u4fe1",
    }
    return labels.get((source or "").lower(), source or "\u672a\u77e5\u6570\u636e\u6e90")

def _default_dates(start_date: str | None, end_date: str | None) -> tuple[str, str]:
    end_base = _parse_compact_date(end_date) or date.today()
    end_value = end_date or end_base.strftime("%Y%m%d")
    start_value = start_date or _trading_window_start(end_base, DEFAULT_OBSERVE_TRADING_DAYS).strftime("%Y%m%d")
    return start_value, end_value

def _trading_window_start(end_date: date, trading_days: int) -> date:
    target_count = max(1, trading_days)
    cursor = end_date
    counted = 0
    while counted < target_count:
        if cursor.weekday() < 5:
            counted += 1
        if counted >= target_count:
            return cursor
        cursor -= timedelta(days=1)
    return cursor

def _parse_compact_date(value: str | None) -> date | None:
    raw = str(value or "").replace("-", "").strip()
    if len(raw) != 8 or not raw.isdigit():
        return None
    try:
        return datetime.strptime(raw, "%Y%m%d").date()
    except ValueError:
        return None
