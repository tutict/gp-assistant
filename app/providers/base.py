import os
from contextlib import contextmanager
from threading import RLock
from typing import List, Optional

from app.schemas import FinancialIndicatorSection, MinuteBar, OrderBookSnapshot, StockItem, StockRelation


PROXY_MODE_SYSTEM = "system"
PROXY_MODE_NONE = "none"
PROXY_ENV_KEYS = (
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
)
_PROXY_ENV_LOCK = RLock()


def normalize_proxy_mode(proxy_mode: Optional[str] = None) -> str:
    raw = (proxy_mode or os.getenv("STOCK_PROXY_MODE", PROXY_MODE_SYSTEM)).strip().lower()
    if raw in {"none", "off", "direct", "disable", "disabled", "no", "noproxy", "no_proxy"}:
        return PROXY_MODE_NONE
    return PROXY_MODE_SYSTEM


@contextmanager
def proxy_environment(proxy_mode: Optional[str] = None):
    if normalize_proxy_mode(proxy_mode) != PROXY_MODE_NONE:
        yield
        return

    with _PROXY_ENV_LOCK:
        saved = {key: os.environ.get(key) for key in PROXY_ENV_KEYS}
        for key in PROXY_ENV_KEYS:
            os.environ.pop(key, None)
        try:
            yield
        finally:
            for key, value in saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value


class StockProvider:
    name = "base"

    def list_stocks(self) -> List[StockItem]:
        raise NotImplementedError

    def list_stocks_for_screen(self) -> tuple[List[StockItem], List[str]]:
        return self.list_stocks(), []

    def get_stock(self, code: str) -> StockItem:
        raise NotImplementedError

    def get_history(self, code: str, start_date: str, end_date: str):
        raise NotImplementedError

    def get_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> List[MinuteBar]:
        return []

    def get_order_book(self, code: str) -> OrderBookSnapshot | None:
        return None

    def get_financial_indicators(self, stock: StockItem) -> FinancialIndicatorSection | None:
        return None

    def list_relations(self) -> List[StockRelation]:
        return []


def get_provider(
    provider_name: Optional[str] = None,
    refresh: Optional[bool] = None,
    proxy_mode: Optional[str] = None,
) -> StockProvider:
    provider_name = (provider_name or os.getenv("STOCK_PROVIDER", "astock")).strip().lower()
    proxy_mode = normalize_proxy_mode(proxy_mode)
    if provider_name == "akshare":
        from app.providers.akshare import AkShareProvider

        return AkShareProvider(refresh=bool(refresh) if refresh is not None else False, proxy_mode=proxy_mode)
    if provider_name == "eastmoney":
        from app.providers.eastmoney import EastmoneyProvider

        return EastmoneyProvider(refresh=bool(refresh) if refresh is not None else False, proxy_mode=proxy_mode)
    if provider_name == "astock":
        from app.providers.astock import AStockDataProvider

        return AStockDataProvider(refresh=bool(refresh) if refresh is not None else False, proxy_mode=proxy_mode)
    raise ValueError(f"不支持的数据源：{provider_name}")
