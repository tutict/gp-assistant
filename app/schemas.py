from datetime import date
from typing import List, Optional

from pydantic import BaseModel, Field


def current_system_date_yyyymmdd() -> str:
    return date.today().strftime("%Y%m%d")


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


class FinancialIndicatorItem(BaseModel):
    label: str
    value: str
    raw_value: Optional[float] = None
    unit: Optional[str] = None
    tone: str = "neutral"


class FinancialIndicatorSection(BaseModel):
    title: str = "\u6700\u65b0\u6307\u6807"
    period: Optional[str] = None
    source: Optional[str] = None
    items: List[FinancialIndicatorItem] = Field(default_factory=list)
    notes: List[str] = Field(default_factory=list)


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
    notes: List[str] = Field(default_factory=list)


class SectorScreenRequest(BaseModel):
    criteria: ScreenCriteria = Field(default_factory=ScreenCriteria)
    per_sector_limit: int = Field(default=3, ge=1, le=50)
    max_sectors: int = Field(default=12, ge=1, le=50)
    min_sector_candidates: int = Field(default=3, ge=1, le=500)


class SectorScreenGroup(BaseModel):
    sector: str
    total: int
    returned: int
    average_score: float
    items: List[ScreenedStock]


class SectorScreenResult(BaseModel):
    total: int
    returned: int
    sector_count: int
    groups: List[SectorScreenGroup]
    notes: List[str] = Field(default_factory=list)


class StockRelation(BaseModel):
    source_code: str
    target_code: str
    relation_type: str
    weight: float = Field(default=1.0, ge=0, le=1)
    description: Optional[str] = None


class LlmClientConfig(BaseModel):
    api_key: Optional[str] = Field(default=None, max_length=4096)
    base_url: Optional[str] = Field(default=None, max_length=512)
    model: Optional[str] = Field(default=None, max_length=128)
    temperature: Optional[float] = Field(default=None, ge=0, le=2)
    timeout_seconds: Optional[float] = Field(default=None, gt=0, le=180)
    json_mode: Optional[bool] = None
    organization: Optional[str] = Field(default=None, max_length=256)
    project: Optional[str] = Field(default=None, max_length=256)


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
    end_date: str = Field(default_factory=current_system_date_yyyymmdd, description="YYYYMMDD")
    series_limit: int = Field(default=120, ge=20, le=500)


class StockObserveRequest(BaseModel):
    code: str
    start_date: Optional[str] = None
    end_date: Optional[str] = None
    series_limit: int = Field(default=120, ge=20, le=500)
    minute_period: str = Field(default="1")
    minute_start: Optional[str] = None
    minute_end: Optional[str] = None
    minute_limit: int = Field(default=160, ge=1, le=500)
    include_order_book: bool = True
    llm: Optional[LlmClientConfig] = None


class TrendIndicatorPoint(BaseModel):
    date: str
    close: float
    swl: Optional[float] = None
    sws: Optional[float] = None
    accumulation_index: Optional[float] = None
    accumulation_strength: Optional[float] = None
    swing_opportunity: Optional[float] = None
    rebound_signal: Optional[float] = None
    trend_heat: Optional[float] = None
    volume_price_heat: Optional[float] = None
    anomaly_heat: Optional[float] = None
    popularity_heat: Optional[float] = None
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
    pattern_score: int = 0
    pattern_score_max: int = 100
    pattern_signals: List[str] = Field(default_factory=list)
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
    end_date: str = Field(default_factory=current_system_date_yyyymmdd, description="YYYYMMDD")
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


class CapitalEvidenceItem(BaseModel):
    category: str
    source: str
    title: str
    date: Optional[str] = None
    metrics: dict = Field(default_factory=dict)
    sentiment: str = "uncertain"
    weight: float = Field(default=0.0, ge=0, le=1)
    confidence: str = "低"
    url: Optional[str] = None
    score: Optional[float] = None
    note: Optional[str] = None


class CapitalEvidenceResult(BaseModel):
    stock_code: str
    generated_at: str
    composite_score: Optional[float] = None
    confidence: str = "低"
    model_used: bool = False
    as_of_trade_date: Optional[str] = None
    freshness: str = "unknown"
    contributions: dict = Field(default_factory=dict)
    summary: Optional[str] = None
    items: List[CapitalEvidenceItem] = Field(default_factory=list)
    notes: List[str] = Field(default_factory=list)


class StockObservation(BaseModel):
    source: str
    stock: StockItem
    financial_indicators: Optional[FinancialIndicatorSection] = None
    trend: Optional[TrendIndicatorResult] = None
    capital_evidence: Optional[CapitalEvidenceResult] = None
    minute_period: str = "1"
    minute_bars: List[MinuteBar] = Field(default_factory=list)
    order_book: Optional[OrderBookSnapshot] = None
    notes: List[str] = Field(default_factory=list)


class BacktestRequest(BaseModel):
    criteria: ScreenCriteria = Field(default_factory=ScreenCriteria)
    source: str = Field(default="criteria", max_length=20)
    stock_codes: List[str] = Field(default_factory=list, max_length=100)
    start_date: str = Field(description="YYYYMMDD")
    end_date: str = Field(default_factory=current_system_date_yyyymmdd, description="YYYYMMDD")
    top_n: int = Field(default=10, ge=1, le=100)
    initial_cash: float = Field(default=1_000_000, ge=0)
    rebalance_frequency: str = Field(default="monthly", max_length=20)
    transaction_cost_bps: float = Field(default=10.0, ge=0, le=500)
    benchmark: str = Field(default="candidate_equal_weight", max_length=40)
    benchmark_code: Optional[str] = Field(default=None, max_length=16)


class EquityPoint(BaseModel):
    date: str
    equity: float


class BacktestMetrics(BaseModel):
    total_return: float
    annualized_return: Optional[float] = None
    max_drawdown: Optional[float] = None
    num_stocks: int
    benchmark_total_return: Optional[float] = None
    benchmark_annualized_return: Optional[float] = None
    benchmark_max_drawdown: Optional[float] = None
    excess_return: Optional[float] = None
    total_transaction_cost: float = 0.0
    total_turnover: float = 0.0
    rebalance_count: int = 0


class BacktestResult(BaseModel):
    metrics: BacktestMetrics
    equity_curve: List[EquityPoint]
    benchmark_curve: List[EquityPoint] = Field(default_factory=list)
    symbols: List[str]
    benchmark_symbols: List[str] = Field(default_factory=list)
    rebalance_dates: List[str] = Field(default_factory=list)
    notes: List[str] = Field(default_factory=list)


class NewsRagRequest(BaseModel):
    code: Optional[str] = Field(default=None, max_length=16)
    criteria: ScreenCriteria = Field(default_factory=ScreenCriteria)
    seed_codes: List[str] = Field(default_factory=list, max_length=20)
    days: int = Field(default=30, ge=1, le=365)
    max_items: int = Field(default=24, ge=1, le=100)
    llm: Optional[LlmClientConfig] = None


class NewsEvidence(BaseModel):
    title: str
    summary: Optional[str] = None
    source: str
    source_tier: str = "news"
    published_at: Optional[str] = None
    url: Optional[str] = None
    stock_codes: List[str] = Field(default_factory=list)
    relation_types: List[str] = Field(default_factory=list)
    sentiment: str = "uncertain"


class NewsImpactFinding(BaseModel):
    target: str
    direction: str
    confidence: str
    impact_chain: str
    evidence: List[NewsEvidence] = Field(default_factory=list)
    pending_checks: List[str] = Field(default_factory=list)


class NewsRagResult(BaseModel):
    scope_codes: List[str] = Field(default_factory=list)
    relation_count: int = 0
    message_count: int = 0
    findings: List[NewsImpactFinding] = Field(default_factory=list)
    notes: List[str] = Field(default_factory=list)


class RagPackDocument(BaseModel):
    source: str
    source_tier: str = "news"
    title: str
    text: str
    summary: Optional[str] = None
    url: Optional[str] = None
    published_at: Optional[str] = None
    fetched_at: Optional[str] = None
    stock_codes: List[str] = Field(default_factory=list)
    relation_types: List[str] = Field(default_factory=list)
    sentiment: str = "uncertain"


class RagPackBuildRequest(BaseModel):
    documents: List[RagPackDocument] = Field(default_factory=list, max_length=1000)
    pack_version: str = Field(default="local", max_length=64)
    target_chars: int = Field(default=500, ge=120, le=1200)
    overlap_chars: int = Field(default=80, ge=0, le=300)


class RagPackBuildFromNewsCacheRequest(BaseModel):
    pack_version: str = Field(default="local-news-cache", max_length=64)
    days: int = Field(default=30, ge=1, le=3650)
    stock_codes: List[str] = Field(default_factory=list, max_length=100)
    relation_types: List[str] = Field(default_factory=list, max_length=50)
    source_tiers: List[str] = Field(default_factory=lambda: ["filing", "news", "community"], max_length=10)
    limit: int = Field(default=1000, ge=1, le=5000)
    target_chars: int = Field(default=500, ge=120, le=1200)
    overlap_chars: int = Field(default=80, ge=0, le=300)


class RagPackBuildResult(BaseModel):
    path: str
    document_count: int
    chunk_count: int
    content_hash: str
    embedding_model: str
    embedding_backend: str = "onnx"
    embedding_quantization: str = "int8"
    embedding_dim: int
    notes: List[str] = Field(default_factory=list)


class RagPackQueryRequest(BaseModel):
    query: str = Field(min_length=1, max_length=1000)
    stock_codes: List[str] = Field(default_factory=list, max_length=50)
    relation_types: List[str] = Field(default_factory=list, max_length=50)
    source_tiers: List[str] = Field(default_factory=list, max_length=10)
    published_after: Optional[str] = None
    top_k: int = Field(default=8, ge=1, le=50)


class RagPackChunkHit(BaseModel):
    chunk_id: str
    document_id: str
    score: float
    title: str
    text: str
    source: str
    source_tier: str
    published_at: Optional[str] = None
    url: Optional[str] = None
    stock_codes: List[str] = Field(default_factory=list)
    relation_types: List[str] = Field(default_factory=list)
    sentiment: str = "uncertain"


class RagPackQueryResult(BaseModel):
    hits: List[RagPackChunkHit] = Field(default_factory=list)
    manifest: dict = Field(default_factory=dict)
    notes: List[str] = Field(default_factory=list)


class RagPackStatusResult(BaseModel):
    exists: bool
    path: str
    valid: bool = False
    manifest: dict = Field(default_factory=dict)
    notes: List[str] = Field(default_factory=list)


class UpstreamRagPackBuildRequest(BaseModel):
    code: str = Field(min_length=1, max_length=16)
    data_until: Optional[str] = None
    filing_days: int = Field(default=1095, ge=30, le=3650)
    news_days: int = Field(default=180, ge=1, le=3650)
    manual_urls: List[str] = Field(default_factory=list, max_length=12)


class UpstreamRagPackBuildResult(BaseModel):
    pack_path: str
    manifest_path: str
    manifest: dict = Field(default_factory=dict)
    quality: dict = Field(default_factory=dict)
    notes: List[str] = Field(default_factory=list)


class UpstreamRagTransferStartRequest(BaseModel):
    ttl_minutes: int = Field(default=15, ge=1, le=60)


class UpstreamRagTransferResult(BaseModel):
    manifest_url: str
    pack_url: str
    token: str
    expires_at: str
    qr_svg: Optional[str] = None
    notes: List[str] = Field(default_factory=list)


class UpstreamRagPackStatusResult(BaseModel):
    exists: bool
    manifest: dict = Field(default_factory=dict)
    transfer: dict = Field(default_factory=dict)
    notes: List[str] = Field(default_factory=list)


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


class AutoRefreshResult(BaseModel):
    source: str
    checked_at: str
    trading_day: bool
    after_close: bool
    due: bool
    refreshed: bool
    status: DataCacheStatus
    notes: List[str] = Field(default_factory=list)


class CachePruneResult(BaseModel):
    removed_files: int
    removed_bytes: int
    status: DataCacheStatus
    notes: List[str] = Field(default_factory=list)


class AgentRequest(BaseModel):
    message: str
    llm: Optional[LlmClientConfig] = None


class AgentResponse(BaseModel):
    reply: str
    action: str
    criteria: Optional[ScreenCriteria] = None
    backtest: Optional[BacktestRequest] = None
    news_rag: Optional[NewsRagRequest] = None
    observe: Optional[StockObserveRequest] = None
    sector_screen: Optional[SectorScreenRequest] = None
    graph_screen: Optional[GraphScreenRequest] = None
    trend_screen: Optional[TrendScreenRequest] = None
    data: Optional[dict] = None
