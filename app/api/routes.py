import json
from dataclasses import asdict
from datetime import datetime, timedelta
from typing import Optional

from fastapi import APIRouter, Depends, Header, HTTPException
from fastapi.responses import StreamingResponse

from app.providers.base import get_provider
from app.schemas import (
    AgentRequest,
    AgentResponse,
    AutoRefreshResult,
    BacktestRequest,
    BacktestResult,
    CachePolicy,
    CachePruneResult,
    DataCacheStatus,
    DataRefreshResult,
    GraphScreenRequest,
    GraphScreenResult,
    MinuteBar,
    NewsRagRequest,
    NewsRagResult,
    OrderBookSnapshot,
    RagPackBuildFromNewsCacheRequest,
    RagPackBuildRequest,
    RagPackBuildResult,
    RagPackQueryRequest,
    RagPackQueryResult,
    RagPackStatusResult,
    ScreenCriteria,
    ScreenResult,
    SectorScreenRequest,
    SectorScreenResult,
    StockObservation,
    StockObserveRequest,
    StockItem,
    TrendIndicatorRequest,
    TrendIndicatorResult,
    TrendScreenRequest,
    TrendScreenResult,
    UpstreamRagPackBuildRequest,
    UpstreamRagPackBuildResult,
    UpstreamRagPackStatusResult,
    UpstreamRagTransferResult,
    UpstreamRagTransferStartRequest,
)
from app.services.agent import run_agent, run_agent_stream
from app.services.backtest import backtest_hold
from app.services.data_maintenance import auto_refresh_universe_after_close, data_source_status, prune_cache, refresh_universe
from app.services.news_rag import analyze_supply_chain_news
from app.services.observation import observe_stock as build_stock_observation
from app.services.rag_pack import build_rag_pack, build_rag_pack_from_news_cache, query_rag_pack, rag_pack_status
from app.services.screener import screen_stocks, screen_stocks_by_sector, screening_universe
from app.services.stock_graph import graph_screen_stocks
from app.services.stock_search import search_stock_items
from app.services.trend_indicator import analyze_trend, trend_screen_stocks
from app.services.upstream_rag_pack import build_upstream_rag_pack, latest_upstream_pack
from app.services.upstream_rag_transfer import active_upstream_rag_transfer, start_upstream_rag_transfer

router = APIRouter()


def _parse_bool(value: Optional[str]) -> Optional[bool]:
    if value is None or value == "":
        return None
    return value.strip().lower() in {"1", "true", "yes", "y", "on"}


def _provider_from_headers(
    x_stock_provider: Optional[str] = Header(default=None),
    x_stock_refresh: Optional[str] = Header(default=None),
    x_akshare_refresh: Optional[str] = Header(default=None),
    x_stock_proxy: Optional[str] = Header(default=None),
):
    try:
        refresh = _parse_bool(x_stock_refresh)
        if refresh is None:
            refresh = _parse_bool(x_akshare_refresh)
        return get_provider(x_stock_provider, refresh=refresh, proxy_mode=x_stock_proxy)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


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
    start_dt = _parse_date_or_datetime(minute_start) or end_dt.replace(hour=9, minute=30, second=0)
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
                "name": "质量价值",
                "description": "低估值且净资产收益率为正。",
            },
            {
                "id": "defensive_dividend",
                "name": "防御分红",
                "description": "盈利稳定、估值适中、分红较好。",
            },
        ]
    }


@router.get("/data-sources")
def list_data_sources(provider=Depends(_provider_from_headers)):
    return {
        "current": getattr(provider, "name", provider.__class__.__name__),
        "available": [
            {
                "id": "tdx",
                "name": "通达信",
                "description": "通过通达信获取 A 股股票池、日线和分钟线；筛选与盘口行情优先使用腾讯股票。",
            },
        ],
    }


@router.get("/data-sources/status", response_model=DataCacheStatus)
def get_data_source_status(provider=Depends(_provider_from_headers)):
    return data_source_status(getattr(provider, "name", provider.__class__.__name__))


@router.post("/data-sources/refresh-universe", response_model=DataRefreshResult)
def refresh_data_source_universe(
    policy: Optional[CachePolicy] = None,
    provider=Depends(_provider_from_headers),
):
    return refresh_universe(getattr(provider, "name", provider.__class__.__name__), policy)


@router.post("/data-sources/auto-refresh-universe", response_model=AutoRefreshResult)
def auto_refresh_data_source_universe(
    policy: Optional[CachePolicy] = None,
    provider=Depends(_provider_from_headers),
):
    return auto_refresh_universe_after_close(getattr(provider, "name", provider.__class__.__name__), policy)


@router.post("/data-sources/prune-cache", response_model=CachePruneResult)
def prune_data_source_cache(
    policy: Optional[CachePolicy] = None,
    provider=Depends(_provider_from_headers),
):
    return prune_cache(getattr(provider, "name", provider.__class__.__name__), policy)


@router.get("/stock-search", response_model=list[StockItem])
def search_stocks(q: str = "", limit: int = 3, provider=Depends(_provider_from_headers)):
    try:
        return search_stock_items(provider.list_stocks(), q, limit)
    except Exception as exc:
        raise HTTPException(status_code=503, detail=f"股票搜索不可用：{exc}") from exc


@router.get("/stocks/{code}", response_model=StockItem)
def get_stock(code: str, provider=Depends(_provider_from_headers)):
    try:
        stock = provider.get_stock(code)
    except KeyError:
        raise HTTPException(status_code=404, detail="未找到股票")
    return stock


@router.get("/observe/{code}", response_model=StockObservation)
def observe_stock(
    code: str,
    start_date: Optional[str] = None,
    end_date: Optional[str] = None,
    series_limit: int = 120,
    include_order_book: bool = True,
    include_chip_distribution: bool = True,
    provider=Depends(_provider_from_headers),
):
    try:
        return build_stock_observation(
            provider,
            StockObserveRequest(
                code=code,
                start_date=start_date,
                end_date=end_date,
                series_limit=series_limit,
                include_order_book=include_order_book,
                include_chip_distribution=include_chip_distribution,
            ),
        )
    except KeyError:
        raise HTTPException(status_code=404, detail="未找到股票")


@router.post("/observe", response_model=StockObservation)
def observe_stock_post(
    request: StockObserveRequest,
    provider=Depends(_provider_from_headers),
):
    try:
        return build_stock_observation(provider, request)
    except KeyError:
        raise HTTPException(status_code=404, detail="未找到股票")


@router.get("/minutes/{code}", response_model=list[MinuteBar])
def stock_minutes(
    code: str,
    start: Optional[str] = None,
    end: Optional[str] = None,
    period: str = "1",
    limit: int = 500,
    provider=Depends(_provider_from_headers),
):
    try:
        stock = provider.get_stock(code)
    except KeyError:
        raise HTTPException(status_code=404, detail="未找到股票")
    start_value, end_value = _default_minute_range(start, end, None)
    period = period if period in {"1", "5", "15", "30", "60"} else "1"
    return provider.get_minutes(stock.code, start_value, end_value, period)[-max(1, min(limit, 500)) :]


@router.get("/order-book/{code}", response_model=OrderBookSnapshot)
def stock_order_book(code: str, provider=Depends(_provider_from_headers)):
    try:
        stock = provider.get_stock(code)
        snapshot = provider.get_order_book(stock.code)
    except KeyError:
        raise HTTPException(status_code=404, detail="未找到股票")
    if snapshot is None:
        raise HTTPException(status_code=404, detail="盘口不可用")
    return snapshot


@router.post("/screen", response_model=ScreenResult)
def screen(criteria: ScreenCriteria, provider=Depends(_provider_from_headers)):
    universe, notes = screening_universe(provider)
    return screen_stocks(universe, criteria, notes=notes)


@router.post("/sector-screen", response_model=SectorScreenResult)
def sector_screen(request: SectorScreenRequest, provider=Depends(_provider_from_headers)):
    universe, notes = screening_universe(provider)
    return screen_stocks_by_sector(universe, request, notes=notes)


@router.post("/graph-screen", response_model=GraphScreenResult)
def graph_screen(request: GraphScreenRequest, provider=Depends(_provider_from_headers)):
    return graph_screen_stocks(provider, request)


@router.post("/trend", response_model=TrendIndicatorResult)
def trend(request: TrendIndicatorRequest, provider=Depends(_provider_from_headers)):
    try:
        return analyze_trend(provider, request)
    except KeyError:
        raise HTTPException(status_code=404, detail="未找到股票")
    except ValueError as exc:
        raise HTTPException(status_code=404, detail=str(exc))


@router.post("/trend-screen", response_model=TrendScreenResult)
def trend_screen(request: TrendScreenRequest, provider=Depends(_provider_from_headers)):
    return trend_screen_stocks(provider, request)


@router.post("/backtest", response_model=BacktestResult)
def backtest(request: BacktestRequest, provider=Depends(_provider_from_headers)):
    return backtest_hold(provider, request)


@router.post("/news-rag", response_model=NewsRagResult)
def news_rag(request: NewsRagRequest, provider=Depends(_provider_from_headers)):
    return analyze_supply_chain_news(provider, request)


@router.get("/rag-pack/status", response_model=RagPackStatusResult)
def rag_pack_get_status():
    return rag_pack_status()


@router.post("/rag-pack/build", response_model=RagPackBuildResult)
def rag_pack_build(request: RagPackBuildRequest):
    try:
        return build_rag_pack(request)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.post("/rag-pack/build-from-news-cache", response_model=RagPackBuildResult)
def rag_pack_build_from_news_cache(request: RagPackBuildFromNewsCacheRequest):
    try:
        return build_rag_pack_from_news_cache(request)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.post("/rag-pack/query", response_model=RagPackQueryResult)
def rag_pack_query(request: RagPackQueryRequest):
    try:
        return query_rag_pack(request)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.get("/upstream-rag/status", response_model=UpstreamRagPackStatusResult)
def upstream_rag_status():
    manifest = latest_upstream_pack()
    return UpstreamRagPackStatusResult(
        exists=manifest is not None,
        manifest=manifest or {},
        transfer=active_upstream_rag_transfer(),
        notes=[] if manifest else ["尚未构建上下游 RAG 包。"],
    )


@router.post("/upstream-rag/build", response_model=UpstreamRagPackBuildResult)
def upstream_rag_build(request: UpstreamRagPackBuildRequest, provider=Depends(_provider_from_headers)):
    try:
        result = build_upstream_rag_pack(
            provider=provider,
            code=request.code,
            data_until=request.data_until,
            filing_days=request.filing_days,
            news_days=request.news_days,
            manual_urls=request.manual_urls,
        )
    except KeyError as exc:
        raise HTTPException(status_code=404, detail="未找到股票") from exc
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return UpstreamRagPackBuildResult(
        pack_path=result.pack_path,
        manifest_path=result.manifest_path,
        manifest=result.manifest,
        quality=asdict(result.quality),
        notes=result.notes,
    )


@router.post("/upstream-rag/transfer/start", response_model=UpstreamRagTransferResult)
def upstream_rag_transfer_start(request: UpstreamRagTransferStartRequest):
    try:
        return start_upstream_rag_transfer(request.ttl_minutes)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.post("/agent", response_model=AgentResponse)
def agent(request: AgentRequest, provider=Depends(_provider_from_headers)):
    return run_agent(provider, request.message, request.llm)


@router.post("/agent/stream")
def agent_stream(request: AgentRequest, provider=Depends(_provider_from_headers)):
    def events():
        for event in run_agent_stream(provider, request.message, request.llm, request.run_id):
            event_type = str(event.get("type") or "message")
            data = json.dumps(event, ensure_ascii=False)
            yield f"event: {event_type}\ndata: {data}\n\n"

    return StreamingResponse(events(), media_type="text/event-stream")
