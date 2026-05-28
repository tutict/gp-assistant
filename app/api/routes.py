from fastapi import APIRouter, HTTPException

from app.providers.base import get_provider
from app.schemas import (
    AgentRequest,
    AgentResponse,
    BacktestRequest,
    BacktestResult,
    GraphScreenRequest,
    GraphScreenResult,
    ScreenCriteria,
    ScreenResult,
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


@router.get("/stocks/{code}", response_model=StockItem)
def get_stock(code: str):
    provider = get_provider()
    try:
        stock = provider.get_stock(code)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")
    return stock


@router.post("/screen", response_model=ScreenResult)
def screen(criteria: ScreenCriteria):
    provider = get_provider()
    universe = provider.list_stocks()
    return screen_stocks(universe, criteria)


@router.post("/graph-screen", response_model=GraphScreenResult)
def graph_screen(request: GraphScreenRequest):
    provider = get_provider()
    return graph_screen_stocks(provider, request)


@router.post("/trend", response_model=TrendIndicatorResult)
def trend(request: TrendIndicatorRequest):
    provider = get_provider()
    try:
        return analyze_trend(provider, request)
    except KeyError:
        raise HTTPException(status_code=404, detail="Stock not found")
    except ValueError as exc:
        raise HTTPException(status_code=404, detail=str(exc))


@router.post("/trend-screen", response_model=TrendScreenResult)
def trend_screen(request: TrendScreenRequest):
    provider = get_provider()
    return trend_screen_stocks(provider, request)


@router.post("/backtest", response_model=BacktestResult)
def backtest(request: BacktestRequest):
    provider = get_provider()
    return backtest_hold(provider, request)


@router.post("/agent", response_model=AgentResponse)
def agent(request: AgentRequest):
    provider = get_provider()
    return run_agent(provider, request.message, request.llm)
