from datetime import datetime, timedelta
from typing import Dict, List

from app.providers.base import StockProvider
from app.schemas import MinuteBar, OrderBookLevel, OrderBookSnapshot, StockItem, StockRelation


class MockProvider(StockProvider):
    name = "mock"

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

    def get_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> List[MinuteBar]:
        import math

        if code not in self._data:
            raise KeyError(f"Stock {code} not found")

        stock = self._data[code]
        step = max(int(period or "1"), 1)
        end = _parse_dt(end_datetime) or datetime.now().replace(second=0, microsecond=0)
        start = _parse_dt(start_datetime) or (end - timedelta(days=3))
        base = stock.price or 10.0
        seed = sum(ord(char) for char in code)
        phase = (seed % 19) / 4

        bars: List[MinuteBar] = []
        previous = base
        cursor = start
        index = 0
        while cursor <= end and len(bars) < 520:
            if cursor.weekday() < 5 and (
                cursor.replace(hour=9, minute=30) <= cursor <= cursor.replace(hour=11, minute=30)
                or cursor.replace(hour=13, minute=0) <= cursor <= cursor.replace(hour=15, minute=0)
            ):
                drift = index * 0.00004
                wave = math.sin(index / 8 + phase) * 0.006
                close = max(base * (1 + drift + wave), 0.01)
                open_price = previous
                high = max(open_price, close) * 1.0018
                low = min(open_price, close) * 0.9982
                volume = 12_000 + (seed % 17) * 900 + abs(math.sin(index / 5 + phase)) * 6_000
                bars.append(
                    MinuteBar(
                        datetime=cursor.strftime("%Y-%m-%d %H:%M:%S"),
                        open=open_price,
                        high=high,
                        low=low,
                        close=close,
                        volume=round(volume, 2),
                        amount=round(volume * close, 2),
                    )
                )
                previous = close
                index += 1
            cursor += timedelta(minutes=step)
        return bars[-240:]

    def get_order_book(self, code: str) -> OrderBookSnapshot | None:
        if code not in self._data:
            raise KeyError(f"Stock {code} not found")
        stock = self._data[code]
        base = stock.price or 10.0
        seed = sum(ord(char) for char in code)
        bids = [
            OrderBookLevel(level=level, price=round(base - level * 0.02, 3), volume=10_000 + seed % 997 + level * 1800)
            for level in range(1, 6)
        ]
        asks = [
            OrderBookLevel(level=level, price=round(base + level * 0.02, 3), volume=9_500 + seed % 863 + level * 1700)
            for level in range(1, 6)
        ]
        return OrderBookSnapshot(
            code=code,
            timestamp=datetime.now().isoformat(timespec="seconds"),
            bids=bids,
            asks=asks,
            metrics={
                "最新": base,
                "今开": round(base * 0.996, 3),
                "最高": round(base * 1.018, 3),
                "最低": round(base * 0.982, 3),
                "昨收": round(base * 0.994, 3),
                "涨跌": round(base * 0.006, 3),
                "涨幅": 0.6,
                "量比": 1.15,
                "换手": 1.8,
            },
        )

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


def _parse_dt(value: str) -> datetime | None:
    if not value:
        return None
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y%m%d %H:%M:%S", "%Y-%m-%d", "%Y%m%d"):
        try:
            parsed = datetime.strptime(value, fmt)
            if fmt in {"%Y-%m-%d", "%Y%m%d"}:
                return parsed.replace(hour=15, minute=0)
            return parsed
        except ValueError:
            continue
    return None
