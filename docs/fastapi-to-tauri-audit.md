# FastAPI to Tauri Migration Audit

Last checked: 2026-06-26.

This document tracks whether the retired FastAPI surface, RAG pack workflow, and data-maintenance scripts have Rust/Tauri replacements. The default desktop runtime is Tauri + Rust; anything marked legacy must not be treated as available in the default desktop path.

## Summary

| Area | Status | Evidence |
| --- | --- | --- |
| Core stock workflows | Replaced | `app/static/app.js` bridges screen, sector screen, graph screen, trend, trend screen, observe, backtest, news RAG, stock search, and agent stream to Tauri commands. Commands are registered in `desktop/src-tauri/src/lib.rs`. |
| Data maintenance UI | Replaced | Data status, refresh, auto-refresh, and prune are bridged to `api_market_status`, `api_market_refresh`, and `api_market_clear_cache`. |
| Standalone data maintenance script | Replaced | `scripts/maintain-data-tauri.ps1` inspects and prunes Tauri AppData caches without Python/FastAPI. Online refresh remains inside the Tauri app because it uses AppHandle progress events. |
| Local `/api/rag-pack/*` vector pack | Replaced | `app/static/app.js` routes status/build/build-from-news-cache/query to `api_rag_pack_*`; `desktop/src-tauri/src/rag_pack.rs` stores a lightweight Tauri/Rust lexical pack in AppData. |
| Desktop upstream RAG build and inline transfer | Replaced | `api_upstream_rag_build`, `api_upstream_rag_status`, and `api_upstream_rag_transfer_start` build a Tauri/Rust upstream evidence pack and expose an inline descriptor JSON for mobile import. The old Python LAN HTTP transfer remains legacy only. |
| Mobile upstream RAG package management | Replaced | `core_upstream_rag_import`, `core_upstream_rag_list`, `core_upstream_rag_detail`, and `core_upstream_rag_rollback` are Tauri commands. |

## FastAPI Route Coverage

| Old route | Default Tauri/Rust replacement | Status |
| --- | --- | --- |
| `GET /` | Static Tauri frontend | Replaced |
| `GET /health` | `api_health` | Replaced |
| `GET /api/strategies` | `api_strategies` | Replaced |
| `GET /api/data-sources` | `api_data_sources` | Replaced |
| `GET /api/data-sources/status` | `api_market_status` | Replaced |
| `POST /api/data-sources/refresh-universe` | `tauriRefreshUniverse` -> `api_market_refresh` | Replaced |
| `POST /api/data-sources/auto-refresh-universe` | `tauriAutoRefreshUniverse` -> `api_market_status` / `api_market_refresh` | Replaced for app usage; Rust path uses a lightweight trading-day heuristic instead of the old Python AkShare calendar |
| `POST /api/data-sources/prune-cache` | `api_market_clear_cache` | Replaced |
| `GET /api/stock-search` | `api_stock_search` | Replaced |
| `GET /api/stocks/{code}` | `api_stock_get` | Replaced |
| `GET /api/observe/{code}` | `api_observe` plus WebView EPS/history enrichment | Replaced |
| `GET /api/minutes/{code}` | `api_minutes` | Replaced |
| `GET /api/order-book/{code}` | `api_order_book` | Replaced with explicit empty local book response when no level-2 cache exists |
| `POST /api/screen` | `api_screen` / `gp_core::screen_with_data_value` | Replaced |
| `POST /api/sector-screen` | `api_sector_screen` | Replaced |
| `POST /api/graph-screen` | `api_graph_screen` | Replaced |
| `POST /api/trend` | `api_trend_analyze` | Replaced |
| `POST /api/trend-screen` | `api_trend_screen` | Replaced |
| `POST /api/backtest` | `api_backtest` | Replaced |
| `POST /api/news-rag` | `api_news_rag` / `desktop/src-tauri/src/news_rag.rs` | Replaced |
| `GET /api/rag-pack/status` | `api_rag_pack_status` | Replaced |
| `POST /api/rag-pack/build` | `api_rag_pack_build` | Replaced |
| `POST /api/rag-pack/build-from-news-cache` | `api_rag_pack_build_from_news_cache` | Replaced |
| `POST /api/rag-pack/query` | `api_rag_pack_query` | Replaced |
| `GET /api/upstream-rag/status` | `api_upstream_rag_status` | Replaced for desktop build/transfer status; mobile list remains `core_upstream_rag_list` |
| `POST /api/upstream-rag/build` | `api_upstream_rag_build` | Replaced |
| `POST /api/upstream-rag/transfer/start` | `api_upstream_rag_transfer_start` | Replaced with inline descriptor JSON transfer |
| `POST /api/agent/stream` | `api_agent_stream` | Replaced |

## Legacy Python Boundary

The Python/FastAPI service layer is retained only as the parity reference implementation for the Rust port (see `tests/test_core_parity.py`) and is exercised by the active `pytest` suite. It is not part of the default Tauri desktop runtime:

- Script-level Tauri cache status/prune uses `scripts/maintain-data-tauri.ps1`; the retired Python-only `scripts/maintain_data.py` and the `tests/legacy_python` comparison suite have been removed.
- The Tauri RAG replacement is intentionally lightweight and lexical. It removes the default Python/FastAPI dependency; it does not claim parity with the old Python ONNX/sqlite-vec vector implementation.
