import os
from typing import Dict, List, Optional

import pandas as pd

from app.providers.base import StockProvider
from app.schemas import StockItem, StockRelation


class AkShareProvider(StockProvider):
    def __init__(self, cache_path: Optional[str] = None, refresh: bool = False):
        self.cache_path = cache_path or os.getenv("AKSHARE_CACHE", "data/cache/stocks.csv")
        self.refresh = refresh or os.getenv("AKSHARE_REFRESH", "false").lower() == "true"

    def list_stocks(self) -> List[StockItem]:
        if not self.refresh and os.path.exists(self.cache_path):
            df = pd.read_csv(self.cache_path)
            return [self._row_to_stock(row) for _, row in df.iterrows()]

        df = self._fetch_spot()
        os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
        df.to_csv(self.cache_path, index=False)
        return [self._row_to_stock(row) for _, row in df.iterrows()]

    def get_stock(self, code: str) -> StockItem:
        for item in self.list_stocks():
            if item.code == code:
                return item
        raise KeyError(f"Stock {code} not found")

    def get_history(self, code: str, start_date: str, end_date: str):
        ak = self._import_akshare()
        history_fn = getattr(ak, "stock_zh_a_hist", None)
        if history_fn is None:
            raise RuntimeError("AkShare missing stock_zh_a_hist")

        symbol = code.split(".")[0]
        df = history_fn(symbol=symbol, period="daily", start_date=start_date, end_date=end_date, adjust="")
        if df is None or df.empty:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume"])

        columns = {
            "date": self._pick_col(df, ["日期", "date"]),
            "open": self._pick_col(df, ["开盘", "open"], required=False),
            "high": self._pick_col(df, ["最高", "high"], required=False),
            "low": self._pick_col(df, ["最低", "low"], required=False),
            "close": self._pick_col(df, ["收盘", "close"]),
            "volume": self._pick_col(df, ["成交量", "volume", "vol"], required=False),
        }
        selected = [source for source in columns.values() if source]
        renamed = {source: target for target, source in columns.items() if source}
        df = df[selected].rename(columns=renamed)
        df["date"] = df["date"].astype(str)
        return df

    def list_relations(self) -> List[StockRelation]:
        items = self.list_stocks()
        by_industry: Dict[str, List[StockItem]] = {}
        for item in items:
            if item.industry and item.industry != "Unknown":
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
                            description=f"Same industry: {industry}",
                        )
                    )
        return relations

    def _fetch_spot(self) -> pd.DataFrame:
        ak = self._import_akshare()
        candidates = ["stock_zh_a_spot_em", "stock_zh_a_spot"]
        for name in candidates:
            fn = getattr(ak, name, None)
            if fn:
                df = fn()
                if df is not None and not df.empty:
                    return df
        raise RuntimeError("AkShare spot data unavailable")

    @staticmethod
    def _pick_col(df: pd.DataFrame, options: List[str], required: bool = True) -> Optional[str]:
        for col in options:
            if col in df.columns:
                return col
        if not required:
            return None
        raise KeyError(f"Column not found in AkShare data: {options}")

    def _row_to_stock(self, row: pd.Series) -> StockItem:
        code_col = self._first_present(row, ["代码", "code", "symbol", "股票代码"])
        name_col = self._first_present(row, ["名称", "name", "股票简称"])
        price_col = self._first_present(row, ["最新价", "price", "现价"])
        pe_col = self._first_present(row, ["市盈率-动态", "市盈率", "pe", "PE"])
        pb_col = self._first_present(row, ["市净率", "pb", "PB"])
        mcap_col = self._first_present(row, ["总市值", "总市值-元", "market_cap"])
        industry_col = self._first_present(row, ["行业", "industry", "板块"])

        code = str(row.get(code_col, "")).strip()
        name = str(row.get(name_col, "")).strip()
        price = self._to_float(row.get(price_col))
        pe = self._to_float(row.get(pe_col))
        pb = self._to_float(row.get(pb_col))
        mcap = self._to_float(row.get(mcap_col))
        industry = str(row.get(industry_col, "Unknown")).strip() or "Unknown"

        market_cap_billion = None
        if mcap is not None:
            market_cap_billion = mcap / 1e8 if mcap > 1e6 else mcap

        is_st = "ST" in name.upper()
        return StockItem(
            code=code,
            name=name or code,
            industry=industry,
            is_st=is_st,
            price=price or 0.0,
            pe=pe,
            pb=pb,
            roe=None,
            market_cap_billion=market_cap_billion,
            dividend_yield=None,
        )

    @staticmethod
    def _to_float(value) -> Optional[float]:
        try:
            if value is None:
                return None
            if isinstance(value, str) and value.strip() in {"", "-", "None"}:
                return None
            return float(value)
        except (TypeError, ValueError):
            return None

    @staticmethod
    def _first_present(row: pd.Series, options: List[str]) -> str:
        for col in options:
            if col in row:
                return col
        return options[0]

    @staticmethod
    def _import_akshare():
        try:
            import akshare as ak
        except ImportError as exc:
            raise RuntimeError("AkShare not installed. Run: pip install akshare") from exc
        return ak
