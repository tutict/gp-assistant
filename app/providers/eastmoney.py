from __future__ import annotations

import math
import os
import time
from typing import Dict, List, Optional

import pandas as pd
import requests

from app.providers.akshare import AkShareProvider
from app.providers.base import PROXY_MODE_NONE, StockProvider, normalize_proxy_mode
from app.schemas import MinuteBar, OrderBookSnapshot, StockItem, StockRelation


class EastmoneyProvider(StockProvider):
    name = "eastmoney"

    def __init__(
        self,
        cache_path: Optional[str] = None,
        refresh: bool = False,
        proxy_mode: Optional[str] = None,
    ):
        self.cache_path = cache_path or os.getenv("EASTMONEY_CACHE", "data/cache/eastmoney_stocks.csv")
        self.refresh = refresh or os.getenv("EASTMONEY_REFRESH", "false").lower() == "true"
        self.max_pages = int(os.getenv("EASTMONEY_MAX_PAGES", "70"))
        self.page_size = int(os.getenv("EASTMONEY_PAGE_SIZE", "100"))
        self.proxy_mode = normalize_proxy_mode(proxy_mode)
        self._session = requests.Session()
        self._session.trust_env = self.proxy_mode != PROXY_MODE_NONE
        self._akshare = AkShareProvider(refresh=False, proxy_mode=self.proxy_mode)

    def list_stocks(self) -> List[StockItem]:
        if not self.refresh and os.path.exists(self.cache_path):
            df = pd.read_csv(self.cache_path)
        else:
            df = self._fetch_spot()
            os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
            df.to_csv(self.cache_path, index=False)
        return [stock for _, row in df.iterrows() if (stock := self._row_to_stock(row)) is not None]

    def get_stock(self, code: str) -> StockItem:
        normalized_code = self._normalize_code(code)
        for item in self.list_stocks():
            if item.code == normalized_code:
                return item
        raise KeyError(f"未找到股票 {code}")

    def get_history(self, code: str, start_date: str, end_date: str):
        return self._akshare.get_history(code, start_date, end_date)

    def get_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> List[MinuteBar]:
        return self._akshare.get_minutes(code, start_datetime, end_datetime, period)

    def get_order_book(self, code: str) -> OrderBookSnapshot | None:
        return self._akshare.get_order_book(code)

    def list_relations(self) -> List[StockRelation]:
        items = self.list_stocks()
        by_industry: Dict[str, List[StockItem]] = {}
        for item in items:
            if item.industry and item.industry != "未知行业":
                by_industry.setdefault(item.industry, []).append(item)

        relations: List[StockRelation] = []
        for industry, members in by_industry.items():
            top_members = sorted(
                members,
                key=lambda stock: stock.market_cap_billion or 0,
                reverse=True,
            )[:20]
            for index, source in enumerate(top_members):
                for target in top_members[index + 1 : index + 4]:
                    relations.append(
                        StockRelation(
                            source_code=source.code,
                            target_code=target.code,
                            relation_type="industry_peer",
                            weight=0.5,
                            description=f"同行业：{industry}",
                        )
                    )
        return relations

    def _fetch_spot(self) -> pd.DataFrame:
        rows: list[dict] = []
        missed_pages = 0
        hosts = [
            "https://push2.eastmoney.com",
            "https://82.push2.eastmoney.com",
            "https://18.push2.eastmoney.com",
            "https://19.push2.eastmoney.com",
        ]

        for page in range(1, self.max_pages + 1):
            page_rows = self._fetch_page(page, hosts)
            if page_rows is None:
                missed_pages += 1
                if missed_pages >= 8 and rows:
                    break
                continue
            missed_pages = 0
            rows.extend(page_rows)
            if len(page_rows) < self.page_size:
                break

        if not rows:
            raise RuntimeError("东方财富 A 股实时行情不可用")
        return pd.DataFrame(rows).drop_duplicates(subset=["f12"])

    def _fetch_page(self, page: int, hosts: list[str]) -> Optional[list[dict]]:
        params = {
            "pn": page,
            "pz": self.page_size,
            "po": 1,
            "np": 1,
            "ut": "bd1d9ddb04089700cf9c27f6f7426281",
            "fltt": 2,
            "invt": 2,
            "fid": "f12",
            "fs": "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048",
            "fields": "f2,f9,f12,f14,f20,f23,f100",
        }
        for host in hosts:
            try:
                response = self._session.get(
                    f"{host}/api/qt/clist/get",
                    params=params,
                    headers={"User-Agent": "Mozilla/5.0", "Referer": "https://quote.eastmoney.com/"},
                    timeout=12,
                )
                response.raise_for_status()
                data = response.json().get("data") or {}
                return data.get("diff") or []
            except Exception:
                time.sleep(0.15)
        return None

    def _row_to_stock(self, row: pd.Series) -> StockItem | None:
        code = str(row.get("f12", "")).strip()
        if not code or not code.startswith(("0", "3", "6")):
            return None

        name = str(row.get("f14", "")).strip() or code
        price = self._to_float(row.get("f2"))
        if price is None or price <= 0:
            return None

        pe = self._non_negative(row.get("f9"))
        pb = self._non_negative(row.get("f23"))
        market_cap = self._to_float(row.get("f20"))
        market_cap_billion = None
        if market_cap is not None:
            market_cap_billion = market_cap / 1e8 if market_cap > 1e6 else market_cap

        industry = str(row.get("f100", "未知行业")).strip() or "未知行业"
        return StockItem(
            code=self._normalize_code(code),
            name=name,
            industry=industry,
            is_st="ST" in name.upper(),
            price=price,
            pe=pe,
            pb=pb,
            roe=None,
            market_cap_billion=market_cap_billion,
            dividend_yield=None,
        )

    @staticmethod
    def _non_negative(value) -> Optional[float]:
        result = EastmoneyProvider._to_float(value)
        return result if result is not None and result >= 0 else None

    @staticmethod
    def _to_float(value) -> Optional[float]:
        try:
            if value is None:
                return None
            if isinstance(value, str) and value.strip() in {"", "-", "None", "nan"}:
                return None
            result = float(value)
            return result if math.isfinite(result) else None
        except (TypeError, ValueError):
            return None

    @staticmethod
    def _normalize_code(code: str) -> str:
        normalized = str(code or "").strip().upper()
        if "." in normalized:
            return normalized
        if normalized.startswith("6"):
            return f"{normalized}.SH"
        if normalized:
            return f"{normalized}.SZ"
        return normalized
