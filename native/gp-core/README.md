# GP Core

Embeddable Rust core for mobile/native builds. It mirrors the current Python
service logic for:

- criteria-based stock screening
- relation-aware graph screening
- deterministic mock-data backtests or native supplied history backtests
- local heuristic agent routing
- pluggable Rust/native data sources

Build for the host:

```bash
cargo test --manifest-path native/gp-core/Cargo.toml
cargo build --manifest-path native/gp-core/Cargo.toml --release
```

The library exposes both Rust functions and a small C ABI. The C ABI functions
accept JSON strings and return a JSON envelope:

```json
{"ok":true,"data":{...}}
```

Call `gp_core_free_string` after reading the returned string.

## Native Data Source
Mobile shells should collect data in the native app layer and pass it to the
Rust core as a `CoreDataSet`:

```json
{
  "stocks": [
    {
      "code": "111111.SZ",
      "name": "Alpha Bank",
      "industry": "Banking",
      "price": 10.0,
      "pe": 5.0,
      "pb": 0.8,
      "roe": 0.15,
      "market_cap_billion": 100.0,
      "dividend_yield": 0.04
    }
  ],
  "relations": [
    {
      "source_code": "111111.SZ",
      "target_code": "222222.SZ",
      "relation_type": "custom_peer",
      "weight": 0.5,
      "description": "native supplied relation"
    }
  ],
  "histories": {
    "111111.SZ": [
      { "date": "2020-01-01", "close": 10.0 },
      { "date": "2020-01-02", "close": 11.0 }
    ]
  }
}
```

The JSON ABI exposes data-backed calls:

- `gp_core_validate_data_source_json(data_json)`
- `gp_core_screen_with_data_json({"data": {...}, "criteria": {...}})`
- `gp_core_graph_screen_with_data_json({"data": {...}, "request": {...}})`
- `gp_core_backtest_with_data_json({"data": {...}, "request": {...}})`
- `gp_core_agent_with_data_json({"data": {...}, "message": "..."})`

The desktop Tauri shell also registers these command names for a future mobile
UI:

- `core_screen`
- `core_screen_with_data`
- `core_graph_screen`
- `core_graph_screen_with_data`
- `core_backtest`
- `core_backtest_with_data`
- `core_agent`
- `core_agent_with_data`
- `core_validate_data_source`
