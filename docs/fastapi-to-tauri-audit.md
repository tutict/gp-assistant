# FastAPI to Tauri Migration Audit

Last checked: 2026-06-26.

This document tracks whether the retired FastAPI surface, RAG pack workflow, and data-maintenance scripts have Rust/Tauri replacements. The default desktop runtime is Tauri + Rust; anything marked legacy must not be treated as available in the default desktop path.

## Summary

| Area | Status | Evidence |
| --- | --- | --- |
| Core stock workflows | Replaced | `app/static/app.js` bridges screen, sector screen, graph screen, trend, trend screen, observe, backtest, news RAG, stock search, and agent stream to Tauri commands. Commands are registered in `desktop/src-tauri/src/lib.rs`. |
| Data maintenance UI | Replaced for app usage | Data status, refresh, auto-refresh, and prune are bridged to `api_market_status`, `api_market_refresh`, and `api_market_clear_cache`. |
| Standalone `scripts/maintain_data.py` | Legacy Python-only | The script still imports `app.services.data_maintenance`; it has no standalone Rust CLI replacement. The Tauri app covers the same user-facing maintenance actions. |
| Local `/api/rag-pack/*` vector pack | Not migrated | Build/query/status live in `app/services/rag_pack.py` and FastAPI routes. No Tauri command or `gp-core` implementation exists. |
| Desktop upstream RAG build and LAN transfer | Not migrated | Build and transfer live in `app/services/upstream_rag_pack.py` and `app/services/upstream_rag_transfer.py`. Tauri currently only imports, lists, reads, and rolls back mobile packs. |
| Mobile upstream RAG package management | Replaced | `core_upstream_rag_import`, `core_upstream_rag_list`, `core_upstream_rag_detail`, and `core_upstream_rag_rollback` are Tauri commands. |

## FastAPI Route Coverage

| Old route | Default Tauri/Rust replacement | Status |
| --- | --- | --- |
| `GET /` | Static Tauri frontend | Replaced |
| `GET /health` | Tauri process/preflight scripts | Retired, not needed by desktop app |
| `GET /api/strategies` | None | Retired; no current frontend caller |
| `GET /api/data-sources` | Static data-source UI state | Retired; no current frontend caller |
| `GET /api/data-sources/status` | `api_market_status` | Replaced |
| `POST /api/data-sources/refresh-universe` | `tauriRefreshUniverse` -> `api_market_refresh` | Replaced |
| `POST /api/data-sources/auto-refresh-universe` | `tauriAutoRefreshUniverse` -> `api_market_status` / `api_market_refresh` | Replaced for app usage; Rust path uses a lightweight trading-day heuristic instead of the old Python AkShare calendar |
| `POST /api/data-sources/prune-cache` | `api_market_clear_cache` | Replaced |
| `GET /api/stock-search` | `api_stock_search` | Replaced |
| `GET /api/stocks/{code}` | None | Retired; no current frontend caller |
| `GET /api/observe/{code}` | `api_observe` plus WebView EPS/history enrichment | Replaced |
| `GET /api/minutes/{code}` | None | Retired; no current frontend caller |
| `GET /api/order-book/{code}` | Included opportunistically inside observe when available | Partially replaced; standalone endpoint is retired |
| `POST /api/screen` | `api_screen` / `gp_core::screen_with_data_value` | Replaced |
| `POST /api/sector-screen` | `api_sector_screen` | Replaced |
| `POST /api/graph-screen` | `api_graph_screen` | Replaced |
| `POST /api/trend` | `api_trend_analyze` | Replaced |
| `POST /api/trend-screen` | `api_trend_screen` | Replaced |
| `POST /api/backtest` | `api_backtest` | Replaced |
| `POST /api/news-rag` | `api_news_rag` / `desktop/src-tauri/src/news_rag.rs` | Replaced |
| `GET /api/rag-pack/status` | None | Legacy Python-only |
| `POST /api/rag-pack/build` | None | Legacy Python-only |
| `POST /api/rag-pack/build-from-news-cache` | None | Legacy Python-only |
| `POST /api/rag-pack/query` | None | Legacy Python-only |
| `GET /api/upstream-rag/status` | `core_upstream_rag_list` | Partially replaced; lists imported local packs, but does not expose desktop build/transfer status |
| `POST /api/upstream-rag/build` | None | Legacy Python-only |
| `POST /api/upstream-rag/transfer/start` | None | Legacy Python-only |
| `POST /api/agent/stream` | `api_agent_stream` | Replaced |

## Required Follow-Up

To fully remove Python instead of only retiring it from the default Tauri path:

1. Port `app/services/rag_pack.py` to Rust or delete the old local vector-pack feature and tests.
2. Port `app/services/upstream_rag_pack.py` and `app/services/upstream_rag_transfer.py` to Tauri/Rust if desktop still needs to build QR-transferable RAG packs.
3. Replace or remove `scripts/maintain_data.py`; the app already covers status, refresh, and prune, but there is no Rust CLI equivalent.
4. Move or mark Python-only tests under a legacy test target so default checks do not imply those APIs are part of the Tauri runtime.
