import os
from typing import List

from app.schemas import StockItem, StockRelation


class StockProvider:
    def list_stocks(self) -> List[StockItem]:
        raise NotImplementedError

    def get_stock(self, code: str) -> StockItem:
        raise NotImplementedError

    def get_history(self, code: str, start_date: str, end_date: str):
        raise NotImplementedError

    def list_relations(self) -> List[StockRelation]:
        return []


def get_provider() -> StockProvider:
    provider_name = os.getenv("STOCK_PROVIDER", "mock").lower()
    if provider_name == "mock":
        from app.providers.mock import MockProvider

        return MockProvider()
    if provider_name == "akshare":
        from app.providers.akshare import AkShareProvider

        return AkShareProvider()
    return MockProvider()
