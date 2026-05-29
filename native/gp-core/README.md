# GP Core

`native/gp-core` 是移动端和原生端可嵌入的 Rust 核心库。它复用当前 Python 服务中的核心选股逻辑，覆盖：

- 条件选股
- 关系图选股
- 本地演示数据回测或原生侧传入历史行情后的回测
- 本地启发式智能体路由
- 可插拔 Rust/原生数据源

在当前主机上构建和测试：

```bash
cargo test --manifest-path native/gp-core/Cargo.toml
cargo build --manifest-path native/gp-core/Cargo.toml --release
```

该库同时暴露 Rust 函数和一组小型 C ABI。C ABI 接收 JSON 字符串，并返回统一 JSON 包装：

```json
{"ok":true,"data":{...}}
```

读取返回字符串后，需要调用 `gp_core_free_string` 释放内存。

## 原生数据源

移动端壳层应在原生应用层采集数据，再把它封装为 `CoreDataSet` 传入 Rust 核心：

```json
{
  "stocks": [
    {
      "code": "111111.SZ",
      "name": "样本银行",
      "industry": "银行",
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
      "description": "原生侧传入的自定义关系"
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

JSON ABI 支持以下带数据集的调用：

- `gp_core_validate_data_source_json(data_json)`
- `gp_core_screen_with_data_json({"data": {...}, "criteria": {...}})`
- `gp_core_graph_screen_with_data_json({"data": {...}, "request": {...}})`
- `gp_core_backtest_with_data_json({"data": {...}, "request": {...}})`
- `gp_core_agent_with_data_json({"data": {...}, "message": "..."})`

Tauri 桌面壳也已经预留这些命令名，后续移动端界面可以复用：

- `core_screen`
- `core_screen_with_data`
- `core_graph_screen`
- `core_graph_screen_with_data`
- `core_backtest`
- `core_backtest_with_data`
- `core_agent`
- `core_agent_with_data`
- `core_validate_data_source`
