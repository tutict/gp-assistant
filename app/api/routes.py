from datetime import date, datetime, timedelta
from typing import Optional

from fastapi import APIRouter, Depends, Header, HTTPException

from app.providers.base import get_provider
from app.schemas import (
    AgentRequest,
    AgentResponse,
    BacktestRequest,
    BacktestResult,
    GraphScreenRequest,
    GraphScreenResult,
    MinuteBar,
    OrderBookSnapshot,
    ScreenCriteria,
    ScreenResult,
    StockObservation,
    StockItem,
    TrendIndicatorRequest,
    TrendIndicatorResult,
    TrendScreenRequest,
    TrendScreenResult,
)
from app.services.agent import run_agent
from app.services.backtest import backtest_hold
from app.services.screener import screen_stocks
from app.services.stock_graph import graph_screen_stocks
from app.services.trend_indicator import analyze_trend, trend_screen_stocks

router = APIRouter()


def _parse_bool(value: Optional[str]) -> Optional[bool]:
    if value is None or value == "":
        return None
    return value.strip().lower() in {"1", "true", "yes", "y", "on"}


def _provider_from_headers(
    x_stock_provider: Optional[str] = Header(default=None),
    x_akshare_refresh: Optional[str] = Header(default=None),
):
    try:
        return get_provider(x_stock_provider, refresh=_parse_bool(x_akshare_refresh))
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


def _default_dates(start_date: Optional[str], end_date: Optional[str]) -> tuple[str, str]:
    today = date.today()
    end_value = end_date or today.strftime("%Y%m%d")
    start_value = start_date or (today - timedelta(days=420)).strftime("%Y%m%d")
    return start_value, end_value


def _default_minute_range(
    minute_start: Optional[str],
    minute_end: Optional[str],
    end_date: Optional[str],
) -> tuple[str, str]:
    if minute_start and minute_end:
        return _normalize_minute_datetime(minute_start, is_end=False), _normalize_minute_datetime(minute_end, is_end=True)

    end_dt = _parse_date_or_datetime(minute_end or end_date) or datetime.now()
    if end_dt.hour == 0 and end_dt.minute == 0:
        end_dt = end_dt.replace(hour=15, minute=0, second=0)
    start_dt = _parse_date_or_datetime(minute_start) or (end_dt - timedelta(days=5)).replace(hour=9, minute=30, second=0)
    return start_dt.strftime("%Y-%m-%d %H:%M:%S"), end_dt.strftime("%Y-%m-%d %H:%M:%S")


def _normalize_minute_datetime(value: str, is_end: bool) -> str:
    parsed = _parse_date_or_datetime(value)
    if parsed is None:
        return value
    if len(value.strip()) <= 10:
        parsed = parsed.replace(hour=15 if is_end else 9, minute=0 if is_end else 30, second=0)
    return parsed.strftime("%Y-%m-%d %H:%M:%S")


def _parse_date_or_datetime(value: Optional[str]) -> Optional[datetime]:
    if not value:
        return None
    raw = value.strip()
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y%m%d %H:%M:%S", "%Y-%m-%d", "%Y%m%d"):
        try:
            return datetime.strptime(raw, fmt)
        except ValueError:
            continue
    return None


@router.get("/strategies")
def list_strategies():
    return {
        "strategies": [
            {
                "id": "quality_value",
                "name": "Quality + Value",
                "description": "Low PB/PE with positive ROE.",
            },
            {
                "id": "defensive_dividend",
                "name": "Defensive Dividend",
                "description": "Stable earnings with moderate valuation.",
            },
        ]
    }


@router.get("/data-sources")
def list_data_sources(provider=Depends(_provider_from_headers)):
    return {
        "current": getattr(provider, "name", provider.__class__.__name__),
        "available": [
            {
                "id": "mock",
                "name": "Mock",
                "description": "Local deterministic demo data.",
            },
            {
                "id": "akshare",
                "name": "AkShare",
                "description": "A-share public market data via AkShare.",
            },
        ],
    }


@router.get("/stocks/{code}", response_model=StockItem)
def get_stock(code: str, provider=Depends(_provider_from_headers)):
    try:
        stock = provider.get_stock(code)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")
    return stock


@router.get("/observe/{code}", response_model=StockObservation)
def observe_stock(
    code: str,
    start_date: Optional[str] = None,
    end_date: Optional[str] = None,
    series_limit: int = 120,
    minute_period: str = "1",
    minute_start: Optional[str] = None,
    minute_end: Optional[str] = None,
    minute_limit: int = 160,
    include_order_book: bool = True,
    provider=Depends(_provider_from_headers),
):
    start_value, end_value = _default_dates(start_date, end_date)
    minute_start_value, minute_end_value = _default_minute_range(minute_start, minute_end, end_date)
    try:
        stock = provider.get_stock(code)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")

    notes = [f"Data source: {getattr(provider, 'name', provider.__class__.__name__)}."]
    try:
        trend = analyze_trend(
            provider,
            TrendIndicatorRequest(
                code=stock.code,
                start_date=start_value,
                end_date=end_value,
                series_limit=max(20, min(series_limit, 500)),
            ),
        )
    except ValueError as exc:
        trend = None
        notes.append(str(exc))

    minute_period = minute_period if minute_period in {"1", "5", "15", "30", "60"} else "1"
    try:
        minute_bars = provider.get_minutes(
            stock.code,
            minute_start_value,
            minute_end_value,
            minute_period,
        )[-max(20, min(minute_limit, 500)) :]
    except Exception as exc:
        minute_bars = []
        notes.append(f"Minute data unavailable: {exc}")

    order_book = None
    if include_order_book:
        try:
            order_book = provider.get_order_book(stock.code)
        except Exception as exc:
            notes.append(f"Order book unavailable: {exc}")

    return StockObservation(
        source=getattr(provider, "name", provider.__class__.__name__),
        stock=stock,
        trend=trend,
        minute_period=minute_period,
        minute_bars=minute_bars,
        order_book=order_book,
        notes=notes,
    )


@router.get("/minutes/{code}", response_model=list[MinuteBar])
def stock_minutes(
    code: str,
    start: Optional[str] = None,
    end: Optional[str] = None,
    period: str = "1",
    limit: int = 240,
    provider=Depends(_provider_from_headers),
):
    try:
        stock = provider.get_stock(code)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")
    start_value, end_value = _default_minute_range(start, end, None)
    period = period if period in {"1", "5", "15", "30", "60"} else "1"
    return provider.get_minutes(stock.code, start_value, end_value, period)[-max(1, min(limit, 500)) :]


@router.get("/order-book/{code}", response_model=OrderBookSnapshot)
def stock_order_book(code: str, provider=Depends(_provider_from_headers)):
    try:
        stock = provider.get_stock(code)
        snapshot = provider.get_order_book(stock.code)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")
    if snapshot is None:
        raise HTTPException(status_code=404, detail="Order book unavailable")
    return snapshot


@router.post("/screen", response_model=ScreenResult)
def screen(criteria: ScreenCriteria, provider=Depends(_provider_from_headers)):
    universe = provider.list_stocks()
    return screen_stocks(universe, criteria)


@router.post("/graph-screen", response_model=GraphScreenResult)
def graph_screen(request: GraphScreenRequest, provider=Depends(_provider_from_headers)):
    return graph_screen_stocks(provider, request)


@router.post("/trend", response_model=TrendIndicatorResult)
def trend(request: TrendIndicatorRequest, provider=Depends(_provider_from_headers)):
    try:
        return analyze_trend(provider, request)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")
    except ValueError as exc:
        raise HTTPException(status_code=404, detail=str(exc))


@router.post("/trend-screen", response_model=TrendScreenResult)
def trend_screen(request: TrendScreenRequest, provider=Depends(_provider_from_headers)):
    return trend_screen_stocks(provider, request)


@router.post("/backtest", response_model=BacktestResult)
def backtest(request: BacktestRequest, provider=Depends(_provider_from_headers)):
    return backtest_hold(provider, request)


@router.post("/agent", response_model=AgentResponse)
def agent(request: AgentRequest, provider=Depends(_provider_from_headers)):
    return run_agent(provider, request.message, request.llm)
