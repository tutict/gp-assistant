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
    limit: int = Field(default=50, ge=1, le=200)
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
    limit: int = Field(default=20, ge=1, le=100)


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


class AgentRequest(BaseModel):
    message: str


class AgentResponse(BaseModel):
    reply: str
    action: str
    criteria: Optional[ScreenCriteria] = None
    backtest: Optional[BacktestRequest] = None
    graph_screen: Optional[GraphScreenRequest] = None
    data: Optional[dict] = None
