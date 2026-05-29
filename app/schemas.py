from typing import List, Optional

from pydantic import BaseModel, Field


class StockItem(BaseModel):
    code: str
    name: str
    industry: str
    is_st: bool = False
    price: float = Field(ge=0)
    pe: Optional[float] = Field(default=None, ge=0)
    pb: Optional[float] = Field(default=None, ge=0)
    roe: Optional[float] = None
    market_cap_billion: Optional[float] = None
    dividend_yield: Optional[float] = None


class ScreenCriteria(BaseModel):
    min_roe: Optional[float] = None
    max_pe: Optional[float] = None
    max_pb: Optional[float] = None
    min_market_cap_billion: Optional[float] = None
    industry: Optional[str] = None
    include_st: bool = False
    limit: int = Field(default=10, ge=1, le=200)
    sort_by: str = Field(default="score")
    sort_dir: str = Field(default="desc")


class ScreenedStock(BaseModel):
    stock: StockItem
    score: float
    reasons: List[str]


class ScreenResult(BaseModel):
    total: int
    returned: int
    items: List[ScreenedStock]


class StockRelation(BaseModel):
    source_code: str
    target_code: str
    relation_type: str
    weight: float = Field(default=1.0, ge=0, le=1)
    description: Optional[str] = None


class GraphScreenRequest(BaseModel):
    criteria: ScreenCriteria = Field(default_factory=ScreenCriteria)
    seed_codes: List[str] = Field(default_factory=list, max_length=50)
    relation_depth: int = Field(default=1, ge=1, le=3)
    relation_weight: float = Field(default=0.35, ge=0, le=1)
    limit: int = Field(default=10, ge=1, le=100)


class GraphStockSignal(BaseModel):
    stock: StockItem
    base_score: float
    relation_score: float
    final_score: float
    suggested_weight: float
    reasons: List[str]
    related: List[StockRelation] = Field(default_factory=list)


class GraphScreenResult(BaseModel):
    total: int
    returned: int
    relation_count: int
    items: List[GraphStockSignal]
    notes: List[str] = Field(default_factory=list)


class TrendIndicatorRequest(BaseModel):
    code: str
    start_date: str = Field(default="20200101", description="YYYYMMDD")
    end_date: str = Field(default="20240101", description="YYYYMMDD")
    series_limit: int = Field(default=120, ge=20, le=500)


class TrendIndicatorPoint(BaseModel):
    date: str
    close: float
    swl: Optional[float] = None
    sws: Optional[float] = None
    red_hold: bool = False
    cyan_watch: bool = False
    short_buy: bool = False
    white_exit: bool = False


class TrendIndicatorSignal(BaseModel):
    code: str
    date: str
    close: float
    swl: Optional[float] = None
    sws: Optional[float] = None
    star_line: Optional[float] = None
    bull_line: Optional[float] = None
    wait_line: Optional[float] = None
    support: Optional[float] = None
    resistance: Optional[float] = None
    breakout: Optional[float] = None
    reversal: Optional[float] = None
    swl_above_sws: bool = False
    red_hold: bool = False
    cyan_watch: bool = False
    short_buy: bool = False
    white_exit: bool = False
    oversold: bool = False
    quant_score: int = 0
    quant_score_max: int = 90
    status: str = "neutral"
    reasons: List[str] = Field(default_factory=list)
    notes: List[str] = Field(default_factory=list)


class TrendIndicatorResult(BaseModel):
    stock: StockItem
    signal: TrendIndicatorSignal
    series: List[TrendIndicatorPoint] = Field(default_factory=list)


class TrendScreenRequest(BaseModel):
    criteria: ScreenCriteria = Field(default_factory=ScreenCriteria)
    start_date: str = Field(default="20200101", description="YYYYMMDD")
    end_date: str = Field(default="20240101", description="YYYYMMDD")
    limit: int = Field(default=10, ge=1, le=100)


class TrendStockSignal(BaseModel):
    stock: StockItem
    base_score: float
    trend_score: float
    final_score: float
    signal: TrendIndicatorSignal
    reasons: List[str] = Field(default_factory=list)


class TrendScreenResult(BaseModel):
    total: int
    returned: int
    items: List[TrendStockSignal]
    notes: List[str] = Field(default_factory=list)


class MinuteBar(BaseModel):
    datetime: str
    open: float
    high: float
    low: float
    close: float
    volume: Optional[float] = None
    amount: Optional[float] = None


class OrderBookLevel(BaseModel):
    level: int
    price: Optional[float] = None
    volume: Optional[float] = None


class OrderBookSnapshot(BaseModel):
    code: str
    timestamp: Optional[str] = None
    bids: List[OrderBookLevel] = Field(default_factory=list)
    asks: List[OrderBookLevel] = Field(default_factory=list)
    metrics: dict = Field(default_factory=dict)


class StockObservation(BaseModel):
    source: str
    stock: StockItem
    trend: Optional[TrendIndicatorResult] = None
    minute_period: str = "1"
    minute_bars: List[MinuteBar] = Field(default_factory=list)
    order_book: Optional[OrderBookSnapshot] = None
    notes: List[str] = Field(default_factory=list)


class BacktestRequest(BaseModel):
    criteria: ScreenCriteria = Field(default_factory=ScreenCriteria)
    start_date: str = Field(description="YYYYMMDD")
    end_date: str = Field(description="YYYYMMDD")
    top_n: int = Field(default=10, ge=1, le=100)
    initial_cash: float = Field(default=1_000_000, ge=0)


class EquityPoint(BaseModel):
    date: str
    equity: float


class BacktestMetrics(BaseModel):
    total_return: float
    annualized_return: Optional[float] = None
    max_drawdown: Optional[float] = None
    num_stocks: int


class BacktestResult(BaseModel):
    metrics: BacktestMetrics
    equity_curve: List[EquityPoint]
    symbols: List[str]


class CachePolicy(BaseModel):
    mode: str = Field(default="light")
    max_bytes: int = Field(default=200 * 1024 * 1024, ge=1)
    daily_days: int = Field(default=500, ge=1, le=5000)
    minute_days: int = Field(default=3, ge=0, le=90)
    keep_watchlist_forever: bool = True
    auto_prune: bool = True


class DataCacheStatus(BaseModel):
    source: str
    cache_dir: str
    cache_bytes: int
    cache_limit_bytes: int
    cache_usage: float
    universe_count: int
    universe_cache_path: Optional[str] = None
    universe_updated_at: Optional[str] = None
    universe_age_hours: Optional[float] = None
    stale: bool = False
    policy: CachePolicy
    notes: List[str] = Field(default_factory=list)


class DataRefreshResult(BaseModel):
    source: str
    refreshed: bool
    status: DataCacheStatus
    notes: List[str] = Field(default_factory=list)


class CachePruneResult(BaseModel):
    removed_files: int
    removed_bytes: int
    status: DataCacheStatus
    notes: List[str] = Field(default_factory=list)


class LlmClientConfig(BaseModel):
    api_key: Optional[str] = Field(default=None, max_length=4096)
    base_url: Optional[str] = Field(default=None, max_length=512)
    model: Optional[str] = Field(default=None, max_length=128)
    temperature: Optional[float] = Field(default=None, ge=0, le=2)
    timeout_seconds: Optional[float] = Field(default=None, gt=0, le=180)
    json_mode: Optional[bool] = None
    organization: Optional[str] = Field(default=None, max_length=256)
    project: Optional[str] = Field(default=None, max_length=256)


class AgentRequest(BaseModel):
    message: str
    llm: Optional[LlmClientConfig] = None


class AgentResponse(BaseModel):
    reply: str
    action: str
    criteria: Optional[ScreenCriteria] = None
    backtest: Optional[BacktestRequest] = None
    graph_screen: Optional[GraphScreenRequest] = None
    trend_screen: Optional[TrendScreenRequest] = None
    data: Optional[dict] = None
