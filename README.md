# A-Share Screener Agent (FastAPI)

Minimal A-share stock screener scaffold with AkShare data, lightweight backtest,
agent endpoint, relation-aware graph screening, and SWL/SWS trend signals.

## Run
```bash
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -r requirements.txt
set STOCK_PROVIDER=akshare
set OPENAI_API_KEY=your_key_here
uvicorn app.main:app --reload
```

Open `http://127.0.0.1:8000` for the responsive web UI.

## Desktop App (Tauri)
The Tauri shell lives in `desktop/` and starts the local FastAPI service before
opening the desktop window.

```bash
cd desktop
cmd /c npm install
cmd /c npm run dev
```

Dev mode uses `GP_ASSISTANT_PYTHON` when set, then falls back to
`.venv-cpython\Scripts\python.exe`, `.venv\Scripts\python.exe`, and finally
`python` on `PATH`. The backend defaults to `http://127.0.0.1:8010` and
`STOCK_PROVIDER=mock`.

Build a Windows desktop package with an embedded FastAPI sidecar:

```bash
cd desktop
cmd /c npm run build:windows
```

The sidecar build uses PyInstaller and writes the Tauri external binary to
`desktop/src-tauri/binaries/`.

## Mobile / Native Core
The mobile path should not embed the Python/FastAPI sidecar. The shared stock
logic now lives in `native/gp-core` as a Rust library that can be used by Tauri
mobile commands or wrapped from Swift/Kotlin through a small C ABI.

```bash
cargo test --manifest-path native/gp-core/Cargo.toml
powershell -ExecutionPolicy Bypass -File scripts/build-mobile-core.ps1
```

Set `GP_CORE_TARGET` to build a specific mobile target, for example
`aarch64-linux-android` after installing the Rust target and Android NDK
toolchain.

The current Rust core includes mock-data stock screening, relation-aware graph
screening, SWL/SWS trend screening, deterministic mock backtests, and the local
heuristic agent router.
Native/mobile shells can pass their own `CoreDataSet` into the Rust core for
stocks, relations, and historical bars, so Android/iOS can source data from
SQLite, bundled JSON, or a remote API without embedding Python.

## Notes
- AkShare spot data is cached to `data/cache/stocks.csv`. Set `AKSHARE_REFRESH=true` to refresh.
- Backtest dates use `YYYYMMDD` format.
- Trend screening ports the provided TongDaXin-style formula into calculable
  signals: SWL/SWS, red-hold/watch states, short-buy/exit flags, oversold,
  support/resistance, and a 90-point quant score. `WINNER(C)` is omitted because
  it needs chip-distribution data.
- Relation-aware screening is implemented as a lightweight graph propagation layer:
  single-stock scores are computed first, then peer/supply-chain/thematic relations
  spread signals across connected stocks.
- LangGraph is useful for orchestrating a multi-step agent workflow. It is not the
  stock relationship model itself; stock relationships should come from a knowledge
  graph, graph learning model, or structured relation dataset.

## Build Local Training Data (RQData HTTP API)
```bash
set RQDATA_USERNAME=your_username
set RQDATA_PASSWORD=your_password
python scripts\build_dataset.py --start-date 20190101 --end-date 20240101 --intent-samples 500 --report-samples 100
```

Outputs:
- `data/datasets/intent.jsonl` (instruction -> JSON params)
- `data/datasets/report.jsonl` (metrics -> report text)

## Example
```bash
curl -X POST http://127.0.0.1:8000/api/screen ^
  -H "Content-Type: application/json" ^
  -d "{\"max_pe\":10,\"min_roe\":0.1,\"limit\":10}"
```

```bash
curl -X POST http://127.0.0.1:8000/api/graph-screen ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":25,\"min_roe\":0.1},\"seed_codes\":[\"300750.SZ\"],\"relation_depth\":1,\"relation_weight\":0.4,\"limit\":10}"
```

```bash
curl -X POST http://127.0.0.1:8000/api/trend-screen ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":30},\"start_date\":\"20200101\",\"end_date\":\"20240101\",\"limit\":10}"
```

```bash
curl -X POST http://127.0.0.1:8000/api/backtest ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":10},\"start_date\":\"20200101\",\"end_date\":\"20240101\",\"top_n\":10}"
```
