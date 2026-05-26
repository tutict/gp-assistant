# A-Share Screener Agent (FastAPI)

Minimal A-share stock screener scaffold with AkShare data, lightweight backtest,
agent endpoint, and relation-aware graph screening.

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

## Notes
- AkShare spot data is cached to `data/cache/stocks.csv`. Set `AKSHARE_REFRESH=true` to refresh.
- Backtest dates use `YYYYMMDD` format.
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
curl -X POST http://127.0.0.1:8000/api/backtest ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":10},\"start_date\":\"20200101\",\"end_date\":\"20240101\",\"top_n\":10}"
```
