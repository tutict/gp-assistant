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
    args = parser.parse_args()

    provider = TdxProvider(refresh=True)
    stocks, notes = provider.list_stocks_for_screen()
    if args.limit > 0:
        stocks = stocks[: args.limit]
    if not stocks:
        raise RuntimeError("TDX returned an empty stock universe; refusing to build a mobile data set.")

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


if __name__ == "__main__":
    main()
