from __future__ import annotations

from datetime import date, datetime, timedelta

from app.providers.base import StockProvider
from app.schemas import StockObservation, StockObserveRequest, TrendIndicatorRequest
from app.services.capital_evidence import fetch_capital_evidence
from app.services.trend_indicator import analyze_trend


DEFAULT_OBSERVE_TRADING_DAYS = 10


def observe_stock(provider: StockProvider, request: StockObserveRequest) -> StockObservation:
    start_value, end_value = _default_dates(request.start_date, request.end_date)
    minute_start_value, minute_end_value = _default_minute_range(
        request.minute_start,
        request.minute_end,
        request.end_date,
    )
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
            ),
        )
    except ValueError as exc:
        trend = None
        notes.append(str(exc))

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

    minute_period = request.minute_period if request.minute_period in {"1", "5", "15", "30", "60"} else "1"
    try:
        minute_bars = provider.get_minutes(
            stock.code,
            minute_start_value,
            minute_end_value,
            minute_period,
        )[-max(1, min(request.minute_limit, 500)) :]
    except Exception as exc:
        minute_bars = []
        notes.append(f"\u5206\u949f\u7ebf\u4e0d\u53ef\u7528\uff1a{exc}")

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
        minute_period=minute_period,
        minute_bars=minute_bars,
        order_book=order_book,
        notes=notes,
    )


def _provider_display_name(source: str) -> str:
    labels = {
        "tdx": "\u901a\u8fbe\u4fe1",
        "astock": "\u901a\u8fbe\u4fe1",
        "akshare": "\u901a\u8fbe\u4fe1",
        "eastmoney": "\u901a\u8fbe\u4fe1",
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


def _default_minute_range(
    minute_start: str | None,
    minute_end: str | None,
    end_date: str | None,
) -> tuple[str, str]:
    if minute_start and minute_end:
        return _normalize_minute_datetime(minute_start, is_end=False), _normalize_minute_datetime(
            minute_end,
            is_end=True,
        )

    end_dt = _parse_date_or_datetime(minute_end or end_date) or datetime.now()
    if end_dt.hour == 0 and end_dt.minute == 0:
        end_dt = end_dt.replace(hour=15, minute=0, second=0)
    start_dt = _parse_date_or_datetime(minute_start) or (end_dt - timedelta(days=5)).replace(
        hour=9,
        minute=30,
        second=0,
    )
    return start_dt.strftime("%Y-%m-%d %H:%M:%S"), end_dt.strftime("%Y-%m-%d %H:%M:%S")


def _normalize_minute_datetime(value: str, is_end: bool) -> str:
    parsed = _parse_date_or_datetime(value)
    if parsed is None:
        return value
    if len(value.strip()) <= 10:
        parsed = parsed.replace(hour=15 if is_end else 9, minute=0 if is_end else 30, second=0)
    return parsed.strftime("%Y-%m-%d %H:%M:%S")


def _parse_date_or_datetime(value: str | None) -> datetime | None:
    if not value:
        return None
    raw = value.strip()
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y%m%d %H:%M:%S", "%Y-%m-%d", "%Y%m%d"):
        try:
            return datetime.strptime(raw, fmt)
        except ValueError:
            continue
    return None
