from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from app.providers.tdx import TdxProvider


def main() -> None:
    parser = argparse.ArgumentParser(description="Build bundled TDX market data for the Android app.")
    parser.add_argument("--output", required=True, help="Output JSON path.")
    parser.add_argument("--limit", type=int, default=0, help="Optional stock limit for diagnostics.")
    parser.add_argument("--refresh", action="store_true", help="Refresh the TDX universe before building the bundle.")
    args = parser.parse_args()

    provider = TdxProvider(refresh=args.refresh)
    stocks, notes = provider.list_stocks_for_screen()
    if args.limit > 0:
        stocks = stocks[: args.limit]
    if not stocks:
        raise RuntimeError("TDX returned an empty stock universe; refusing to build a mobile data set.")
    validate_mobile_data_quality(stocks)

    payload = {
        "source": "tdx",
        "generated_at": datetime.now().astimezone().isoformat(timespec="seconds"),
        "notes": notes,
        "stocks": [stock.model_dump() for stock in stocks],
        "relations": [],
        "histories": {},
    }

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    print(f"Wrote {len(stocks)} TDX stocks to {output}")


def validate_mobile_data_quality(stocks) -> None:
    generic_industries = {"", "通达信股票池", "深市A股", "沪市A股", "科创板", "创业板", "未知行业"}
    metric_count = sum(
        1
        for stock in stocks
        if stock.pe is not None or stock.pb is not None or stock.market_cap_billion is not None
    )
    industry_count = sum(1 for stock in stocks if (stock.industry or "").strip() not in generic_industries)
    minimum = max(10, int(len(stocks) * 0.1))

    if metric_count < minimum:
        raise RuntimeError(
            "Mobile data set has too few valuation fields. "
            "Refresh or provide data/cache/astock_stocks.csv or data/cache/eastmoney_stocks.csv before Android packaging."
        )
    if industry_count < minimum:
        raise RuntimeError(
            "Mobile data set has too few real industry fields. "
            "Refresh or provide data/cache/astock_stocks.csv or data/cache/eastmoney_stocks.csv before Android packaging."
        )


if __name__ == "__main__":
    main()
