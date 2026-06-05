from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from app.schemas import CachePolicy
from app.services.data_maintenance import data_source_status, prune_cache, refresh_universe


def main() -> None:
    parser = argparse.ArgumentParser(description="Maintain GP Assistant market-data cache.")
    parser.add_argument("--source", default="astock", choices=["akshare", "eastmoney", "astock"])
    parser.add_argument("--refresh", action="store_true", help="Refresh the stock universe cache.")
    parser.add_argument("--prune", action="store_true", help="Prune disposable cache files.")
    parser.add_argument("--max-mb", type=int, default=200, help="Maximum cache size in MB.")
    parser.add_argument("--mode", default="light", choices=["light", "balanced", "full"])
    args = parser.parse_args()

    policy = CachePolicy(mode=args.mode, max_bytes=args.max_mb * 1024 * 1024)
    outputs = {"before": data_source_status(args.source, policy).model_dump()}

    if args.refresh:
        outputs["refresh"] = refresh_universe(args.source, policy).model_dump()
    if args.prune:
        outputs["prune"] = prune_cache(args.source, policy).model_dump()

    outputs["after"] = data_source_status(args.source, policy).model_dump()
    print(json.dumps(outputs, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
