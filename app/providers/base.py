import os
from typing import List, Optional

from app.schemas import MinuteBar, OrderBookSnapshot, StockItem, StockRelation


class StockProvider:
    name = "base"

    def list_stocks(self) -> List[StockItem]:
        raise NotImplementedError

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

    def list_relations(self) -> List[StockRelation]:
        return []


def get_provider(provider_name: Optional[str] = None, refresh: Optional[bool] = None) -> StockProvider:
    provider_name = (provider_name or os.getenv("STOCK_PROVIDER", "mock")).strip().lower()
    if provider_name == "mock":
        from app.providers.mock import MockProvider

        return MockProvider()
    if provider_name == "akshare":
        from app.providers.akshare import AkShareProvider

        return AkShareProvider(refresh=bool(refresh) if refresh is not None else False)
    if provider_name == "eastmoney":
        from app.providers.eastmoney import EastmoneyProvider

        return EastmoneyProvider(refresh=bool(refresh) if refresh is not None else False)
    raise ValueError(f"不支持的数据源：{provider_name}")
