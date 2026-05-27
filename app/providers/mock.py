from typing import Dict, List

from app.providers.base import StockProvider
from app.schemas import StockItem, StockRelation


class MockProvider(StockProvider):
    def __init__(self):
        self._data: Dict[str, StockItem] = {
            "600519.SH": StockItem(
                code="600519.SH",
                name="Kweichow Moutai",
                industry="Beverages",
                price=1700.0,
                pe=32.1,
                pb=10.5,
                roe=0.32,
                market_cap_billion=2200.0,
                dividend_yield=0.02,
            ),
            "000001.SZ": StockItem(
                code="000001.SZ",
                name="Ping An Bank",
                industry="Banking",
                price=12.3,
                pe=5.8,
                pb=0.6,
                roe=0.12,
                market_cap_billion=240.0,
                dividend_yield=0.05,
            ),
            "300750.SZ": StockItem(
                code="300750.SZ",
                name="CATL",
                industry="Batteries",
                price=195.0,
                pe=25.5,
                pb=6.5,
                roe=0.18,
                market_cap_billion=900.0,
                dividend_yield=0.01,
            ),
            "002594.SZ": StockItem(
                code="002594.SZ",
                name="BYD",
                industry="Auto",
                price=246.0,
                pe=22.4,
                pb=4.8,
                roe=0.22,
                market_cap_billion=720.0,
                dividend_yield=0.006,
            ),
            "002475.SZ": StockItem(
                code="002475.SZ",
                name="Luxshare Precision",
                industry="Electronics",
                price=36.8,
                pe=24.0,
                pb=4.1,
                roe=0.17,
                market_cap_billion=260.0,
                dividend_yield=0.008,
            ),
            "600036.SH": StockItem(
                code="600036.SH",
                name="China Merchants Bank",
                industry="Banking",
                price=31.2,
                pe=6.9,
                pb=0.9,
                roe=0.14,
                market_cap_billion=900.0,
                dividend_yield=0.04,
            ),
            "600000.SH": StockItem(
                code="600000.SH",
                name="Shanghai Pudong Dev Bank",
                industry="Banking",
                price=9.1,
                pe=4.8,
                pb=0.5,
                roe=0.11,
                market_cap_billion=170.0,
                dividend_yield=0.06,
            ),
            "601012.SH": StockItem(
                code="601012.SH",
                name="LONGi Green Energy",
                industry="Solar",
                price=18.6,
                pe=14.2,
                pb=1.8,
                roe=0.13,
                market_cap_billion=140.0,
                dividend_yield=0.012,
            ),
            "600309.SH": StockItem(
                code="600309.SH",
                name="Wanhua Chemical",
                industry="Chemicals",
                price=78.4,
                pe=15.6,
                pb=2.6,
                roe=0.19,
                market_cap_billion=245.0,
                dividend_yield=0.025,
            ),
        }

    def list_stocks(self) -> List[StockItem]:
        return list(self._data.values())

    def get_stock(self, code: str) -> StockItem:
        if code not in self._data:
            raise KeyError(f"Stock {code} not found")
        return self._data[code]

    def get_history(self, code: str, start_date: str, end_date: str):
        import math

        import pandas as pd

        if code not in self._data:
            raise KeyError(f"Stock {code} not found")
        dates = pd.date_range(start=pd.to_datetime(start_date), end=pd.to_datetime(end_date), freq="B")
        stock = self._data[code]
        base = stock.price or 10.0
        seed = sum(ord(char) for char in code)
        drift = 0.00028 + (seed % 7) * 0.00006
        phase = (seed % 17) / 3

        rows = []
        previous_close = base
        for index, date in enumerate(dates):
            wave = math.sin(index / 8 + phase) * 0.018
            pullback = math.sin(index / 23 + phase) * 0.008
            close = max(base * (1 + drift * index + wave + pullback), 0.01)
            open_price = previous_close * (1 + math.cos(index / 9 + phase) * 0.004)
            high = max(open_price, close) * (1.006 + abs(math.sin(index / 11 + phase)) * 0.006)
            low = min(open_price, close) * (0.994 - abs(math.cos(index / 13 + phase)) * 0.004)
            volume = 2_000_000 + (seed % 31) * 55_000 + index * 2_500
            volume *= 1 + abs(math.sin(index / 10 + phase)) * 0.35
            capital = None
            if stock.market_cap_billion:
                capital = stock.market_cap_billion * 1_000_000_000 / max(base, 0.01)
            rows.append(
                {
                    "date": date.strftime("%Y-%m-%d"),
                    "open": open_price,
                    "high": high,
                    "low": low,
                    "close": close,
                    "volume": volume,
                    "capital": capital,
                }
            )
            previous_close = close
        return pd.DataFrame(rows)

    def list_relations(self) -> List[StockRelation]:
        return [
            StockRelation(
                source_code="600036.SH",
                target_code="000001.SZ",
                relation_type="industry_peer",
                weight=0.75,
                description="Joint exposure to banking valuation and credit cycle.",
            ),
            StockRelation(
                source_code="600036.SH",
                target_code="600000.SH",
                relation_type="industry_peer",
                weight=0.8,
                description="Large A-share commercial bank peer group.",
            ),
            StockRelation(
                source_code="000001.SZ",
                target_code="600000.SH",
                relation_type="industry_peer",
                weight=0.7,
                description="Regional and credit-cycle peer signal.",
            ),
            StockRelation(
                source_code="300750.SZ",
                target_code="002594.SZ",
                relation_type="supply_chain",
                weight=0.65,
                description="Power battery and EV demand-chain linkage.",
            ),
            StockRelation(
                source_code="300750.SZ",
                target_code="601012.SH",
                relation_type="thematic",
                weight=0.45,
                description="Clean-energy capital-cycle exposure.",
            ),
            StockRelation(
                source_code="002475.SZ",
                target_code="002594.SZ",
                relation_type="manufacturing_chain",
                weight=0.35,
                description="Advanced manufacturing and export-demand linkage.",
            ),
            StockRelation(
                source_code="600309.SH",
                target_code="300750.SZ",
                relation_type="upstream_material",
                weight=0.3,
                description="Chemical materials can feed new-energy supply chains.",
            ),
        ]
