import os
import sys
from pathlib import Path

import uvicorn


def _runtime_base_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parents[1]


def _configure_runtime_paths() -> None:
    data_root = Path(os.getenv("GP_ASSISTANT_DATA_ROOT") or (_runtime_base_dir() / "data")).resolve()
    cache_root = data_root / "cache"
    os.environ.setdefault("GP_ASSISTANT_DATA_ROOT", str(data_root))
    os.environ.setdefault("GP_CACHE_DIR", str(cache_root))
    os.environ.setdefault("TDX_CACHE", str(cache_root / "tdx_stocks.csv"))
    os.environ.setdefault("TDX_FUNDAMENTAL_CACHE", str(cache_root / "tdx_fundamentals.csv"))
    os.environ.setdefault("AKSHARE_CACHE", str(cache_root / "stocks.csv"))
    os.environ.setdefault("GP_NEWS_CACHE", str(cache_root / "news.sqlite"))
    os.environ.setdefault("GP_CAPITAL_CACHE", str(cache_root / "capital_evidence.sqlite"))
    os.environ.setdefault("GP_RAG_PACK_PATH", str(cache_root / "rag_pack.sqlite"))
    os.environ.setdefault("GP_UPSTREAM_RAG_ROOT", str(cache_root / "upstream_rag"))


_configure_runtime_paths()

from app.main import app  # noqa: E402


def main() -> None:
    os.environ.setdefault("STOCK_PROVIDER", "tdx")
    host = os.getenv("GP_ASSISTANT_HOST", "127.0.0.1")
    port = int(os.getenv("GP_ASSISTANT_PORT", "8010"))
    log_level = os.getenv("GP_ASSISTANT_LOG_LEVEL", "warning")
    uvicorn.run(app, host=host, port=port, log_level=log_level)


if __name__ == "__main__":
    main()
