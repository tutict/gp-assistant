from typing import Any, List, Optional, Tuple

from app.providers.base import StockProvider
from app.schemas import BacktestMetrics, BacktestRequest, BacktestResult, EquityPoint, ScreenCriteria, ScreenedStock
from app.services.screener import screen_stocks, screening_universe


REBALANCE_STEPS = {
    "none": None,
    "monthly": 20,
    "quarterly": 60,
}
REBALANCE_LABELS = {
    "none": "不再平衡",
    "monthly": "月度再平衡",
    "quarterly": "季度再平衡",
}
BENCHMARK_CANDIDATE_LIMIT = 30


def backtest_hold(provider: StockProvider, request: BacktestRequest) -> BacktestResult:
    import pandas as pd

    universe, universe_notes = screening_universe(provider)
    frequency = _normalize_rebalance_frequency(request.rebalance_frequency)
    screened = screen_stocks(universe, request.criteria)
    selected = screened.items[: request.top_n]
    symbols = [item.stock.code for item in selected]

    price_frame, missing_symbols = _price_frame(provider, selected, request.start_date, request.end_date)
    if price_frame.empty:
        benchmark_curve, benchmark_symbols, benchmark_notes = _benchmark_curve(
            provider=provider,
            universe=universe,
            request=request,
            portfolio_dates=None,
        )
        return BacktestResult(
            metrics=BacktestMetrics(
                total_return=0.0,
                annualized_return=None,
                max_drawdown=None,
                num_stocks=len(symbols),
                benchmark_total_return=_metric_or_none(benchmark_curve, "total_return"),
                benchmark_annualized_return=_metric_or_none(benchmark_curve, "annualized_return"),
                benchmark_max_drawdown=_metric_or_none(benchmark_curve, "max_drawdown"),
                excess_return=None,
                total_transaction_cost=0.0,
                total_turnover=0.0,
                rebalance_count=0,
            ),
            equity_curve=[],
            benchmark_curve=_equity_points(benchmark_curve),
            symbols=symbols,
            benchmark_symbols=benchmark_symbols,
            rebalance_dates=[],
            notes=_quality_notes(
                selected_count=len(selected),
                used_count=0,
                requested_top_n=request.top_n,
                missing_symbols=missing_symbols,
                start_date=request.start_date,
                end_date=request.end_date,
                curve_points=0,
                rebalance_frequency=frequency,
                transaction_cost_bps=request.transaction_cost_bps,
                benchmark_symbols=benchmark_symbols,
                benchmark_notes=benchmark_notes,
                universe_notes=universe_notes,
            ),
        )

    equity, rebalance_dates, total_cost, total_turnover = _simulate_rebalanced_portfolio(
        price_frame,
        initial_cash=request.initial_cash,
        rebalance_frequency=frequency,
        transaction_cost_bps=request.transaction_cost_bps,
    )
    total_return, annualized_return, max_drawdown = _metrics(equity)

    benchmark_curve, benchmark_symbols, benchmark_notes = _benchmark_curve(
        provider=provider,
        universe=universe,
        request=request,
        portfolio_dates=equity.index,
    )
    benchmark_total_return, benchmark_annualized_return, benchmark_max_drawdown = _metrics_or_empty(benchmark_curve)
    excess_return = None
    if benchmark_total_return is not None:
        excess_return = total_return - benchmark_total_return

    return BacktestResult(
        metrics=BacktestMetrics(
            total_return=total_return,
            annualized_return=annualized_return,
            max_drawdown=max_drawdown,
            num_stocks=len(symbols),
            benchmark_total_return=benchmark_total_return,
            benchmark_annualized_return=benchmark_annualized_return,
            benchmark_max_drawdown=benchmark_max_drawdown,
            excess_return=excess_return,
            total_transaction_cost=round(float(total_cost), 2),
            total_turnover=round(float(total_turnover), 6),
            rebalance_count=max(len(rebalance_dates) - 1, 0),
        ),
        equity_curve=_equity_points(equity),
        benchmark_curve=_equity_points(benchmark_curve),
        symbols=symbols,
        benchmark_symbols=benchmark_symbols,
        rebalance_dates=rebalance_dates,
        notes=_quality_notes(
            selected_count=len(selected),
            used_count=len(price_frame.columns),
            requested_top_n=request.top_n,
            missing_symbols=missing_symbols,
            start_date=request.start_date,
            end_date=request.end_date,
            curve_points=len(equity),
            rebalance_frequency=frequency,
            transaction_cost_bps=request.transaction_cost_bps,
            benchmark_symbols=benchmark_symbols,
            benchmark_notes=benchmark_notes,
            universe_notes=universe_notes,
        ),
    )


def _price_frame(
    provider: StockProvider,
    items: List[ScreenedStock],
    start_date: str,
    end_date: str,
) -> Tuple[Any, List[str]]:
    import pandas as pd

    series_list: List[pd.Series] = []
    missing_symbols: List[str] = []
    for item in items:
        code = item.stock.code
        try:
            history = provider.get_history(code, start_date, end_date)
        except (KeyError, ValueError, RuntimeError):
            history = None
        series = _close_series(history)
        if series is None or series.empty:
            missing_symbols.append(code)
            continue
        series.name = code
        series_list.append(series)

    if not series_list:
        return pd.DataFrame(), missing_symbols

    frame = pd.concat(series_list, axis=1).sort_index()
    frame = frame.apply(pd.to_numeric, errors="coerce")
    frame = frame.dropna(how="all").ffill().dropna(axis=1, how="all")
    frame = frame.dropna(how="any")
    return frame, missing_symbols


def _close_series(history: Any):
    import pandas as pd

    if history is None:
        return None
    frame = pd.DataFrame(history).copy()
    if frame.empty:
        return None
    frame = _normalize_columns(frame)
    if "date" not in frame or "close" not in frame:
        return None
    frame["date"] = pd.to_datetime(frame["date"], errors="coerce")
    frame["close"] = pd.to_numeric(frame["close"], errors="coerce")
    frame = frame.dropna(subset=["date", "close"]).sort_values("date")
    if frame.empty:
        return None
    return frame.set_index("date")["close"].astype(float)


def _normalize_columns(frame: Any) -> Any:
    aliases = {
        "日期": "date",
        "date": "date",
        "时间": "date",
        "开盘": "open",
        "open": "open",
        "最高": "high",
        "high": "high",
        "最低": "low",
        "low": "low",
        "收盘": "close",
        "close": "close",
        "成交量": "volume",
        "volume": "volume",
        "vol": "volume",
    }
    rename = {}
    for column in frame.columns:
        key = str(column).strip()
        lower_key = key.lower()
        if key in aliases:
            rename[column] = aliases[key]
        elif lower_key in aliases:
            rename[column] = aliases[lower_key]
    return frame.rename(columns=rename)


def _simulate_rebalanced_portfolio(
    prices: Any,
    *,
    initial_cash: float,
    rebalance_frequency: str,
    transaction_cost_bps: float,
):
    import pandas as pd

    step = REBALANCE_STEPS.get(rebalance_frequency)
    returns = prices.pct_change().fillna(0.0)
    target_weights = pd.Series(1.0 / len(prices.columns), index=prices.columns)
    weights = pd.Series(0.0, index=prices.columns)
    value = float(initial_cash)
    total_cost = 0.0
    total_turnover = 0.0
    last_rebalance_index: Optional[int] = None
    equity_values: List[float] = []
    rebalance_dates: List[str] = []

    for index, (date, daily_returns) in enumerate(returns.iterrows()):
        if index > 0 and weights.sum() > 0:
            portfolio_return = float((weights * daily_returns).sum())
            value *= 1 + portfolio_return
            weights = weights * (1 + daily_returns)
            weight_sum = float(weights.sum())
            if weight_sum > 0:
                weights = weights / weight_sum

        should_rebalance = index == 0
        if step is not None and last_rebalance_index is not None and index - last_rebalance_index >= step:
            should_rebalance = True

        if should_rebalance:
            turnover = float((target_weights - weights).abs().sum())
            cost = value * turnover * max(transaction_cost_bps, 0.0) / 10_000
            value = max(value - cost, 0.0)
            total_cost += cost
            total_turnover += turnover
            weights = target_weights.copy()
            last_rebalance_index = index
            rebalance_dates.append(_format_date(date))

        equity_values.append(value)

    equity = pd.Series(equity_values, index=prices.index, name="portfolio", dtype=float)
    return equity, rebalance_dates, total_cost, total_turnover


def _benchmark_curve(
    *,
    provider: StockProvider,
    universe: List[Any],
    request: BacktestRequest,
    portfolio_dates: Optional[Any],
):
    import pandas as pd

    benchmark = (request.benchmark or "candidate_equal_weight").strip().lower()
    if benchmark in {"none", "off", "disabled"}:
        return pd.Series(dtype=float), [], ["未启用基准对比。"]

    if request.benchmark_code:
        code = request.benchmark_code.strip().upper()
        try:
            series = _close_series(provider.get_history(code, request.start_date, request.end_date))
        except (KeyError, ValueError, RuntimeError):
            series = None
        if series is None or series.empty:
            return pd.Series(dtype=float), [code], [f"基准代码 {code} 没有可用历史行情，已跳过基准对比。"]
        equity = _normalized_equity(series, request.initial_cash)
        return _align_series(equity, portfolio_dates), [code], [f"基准：{code} 收盘价归一化净值。"]

    criteria = _benchmark_criteria(request.criteria)
    candidates = screen_stocks(universe, criteria).items[:BENCHMARK_CANDIDATE_LIMIT]
    frame, missing = _price_frame(provider, candidates, request.start_date, request.end_date)
    if frame.empty:
        note = "候选池等权基准没有可用历史行情，已跳过基准对比。"
        return pd.Series(dtype=float), [item.stock.code for item in candidates], [note]

    normalized = frame / frame.iloc[0]
    equity = normalized.mean(axis=1) * request.initial_cash
    notes = [f"基准：候选池前 {len(frame.columns)} 只股票等权净值，不扣交易成本。"]
    if len(candidates) >= BENCHMARK_CANDIDATE_LIMIT:
        notes.append(f"候选池基准最多取前 {BENCHMARK_CANDIDATE_LIMIT} 只，避免一次回测抓取过多历史行情。")
    if missing:
        notes.append(f"基准中有 {len(missing)} 只股票缺少历史行情。")
    return _align_series(equity, portfolio_dates), list(frame.columns), notes


def _benchmark_criteria(criteria: ScreenCriteria) -> ScreenCriteria:
    return criteria.model_copy(update={"limit": max(criteria.limit, BENCHMARK_CANDIDATE_LIMIT)})


def _normalized_equity(series: Any, initial_cash: float):
    if series.empty:
        return series
    base = float(series.iloc[0])
    if base <= 0:
        base = 1.0
    return series / base * initial_cash


def _align_series(series: Any, dates: Optional[Any]):
    if dates is None or series.empty:
        return series
    aligned = series.reindex(dates).ffill().dropna()
    aligned.name = series.name
    return aligned


def _metrics_or_empty(equity: Any) -> Tuple[Optional[float], Optional[float], Optional[float]]:
    if equity is None or equity.empty:
        return None, None, None
    return _metrics(equity)


def _metric_or_none(equity: Any, metric: str) -> Optional[float]:
    total_return, annualized_return, max_drawdown = _metrics_or_empty(equity)
    values = {
        "total_return": total_return,
        "annualized_return": annualized_return,
        "max_drawdown": max_drawdown,
    }
    return values.get(metric)


def _metrics(equity: Any) -> Tuple[float, Optional[float], Optional[float]]:
    import numpy as np

    if equity is None or equity.empty:
        return 0.0, None, None
    start = float(equity.iloc[0])
    end = float(equity.iloc[-1])
    if start <= 0:
        return 0.0, None, None
    total_return = float(end / start - 1)
    days = max((equity.index[-1] - equity.index[0]).days, 1)
    years = days / 365.0
    annualized = float((1 + total_return) ** (1 / years) - 1) if years > 0 and total_return > -1 else None
    rolling_max = equity.cummax().replace(0, np.nan)
    drawdown = (equity / rolling_max - 1).replace([np.inf, -np.inf], np.nan).min()
    max_drawdown = None if np.isnan(drawdown) else float(drawdown)
    return total_return, annualized, max_drawdown


def _equity_points(equity: Any) -> List[EquityPoint]:
    if equity is None or equity.empty:
        return []
    return [EquityPoint(date=_format_date(idx), equity=round(float(val), 4)) for idx, val in equity.items()]


def _quality_notes(
    *,
    selected_count: int,
    used_count: int,
    requested_top_n: int,
    missing_symbols: List[str],
    start_date: str,
    end_date: str,
    curve_points: int,
    rebalance_frequency: str,
    transaction_cost_bps: float,
    benchmark_symbols: List[str],
    benchmark_notes: List[str],
    universe_notes: List[str],
) -> List[str]:
    label = REBALANCE_LABELS.get(rebalance_frequency, REBALANCE_LABELS["monthly"])
    notes: List[str] = [
        f"回测模型：筛选候选等权建仓，{label}，交易成本按换手额的 {transaction_cost_bps:g} bps 扣减。",
        "当前筛选条件使用最新截面指标，尚未使用历史基本面快照；再平衡会按同一套条件恢复目标权重。",
    ]
    if selected_count == 0:
        notes.append("当前研究条件没有筛出候选股票，回测结果不可用于判断策略表现。")
    elif selected_count < requested_top_n:
        notes.append(f"候选股票只有 {selected_count} 只，少于请求持仓数 {requested_top_n}。")
    if used_count == 0:
        notes.append("没有股票取得可用历史行情，净值曲线为空。")
    elif used_count < selected_count:
        notes.append(f"实际使用 {used_count}/{selected_count} 只股票生成净值曲线。")
    if missing_symbols:
        sample = ", ".join(missing_symbols[:5])
        suffix = " 等" if len(missing_symbols) > 5 else ""
        notes.append(f"以下股票缺少历史行情：{sample}{suffix}。")
    if curve_points and curve_points < 30:
        notes.append(f"净值曲线只有 {curve_points} 个交易日点，样本偏短。")
    if benchmark_symbols:
        notes.extend(benchmark_notes)
    else:
        notes.append("未生成可比较的基准净值曲线。")
    notes.extend(universe_notes)
    notes.append(f"验证区间：{_format_compact_date(start_date)} 至 {_format_compact_date(end_date)}。")
    return notes


def _normalize_rebalance_frequency(value: str) -> str:
    normalized = (value or "monthly").strip().lower()
    aliases = {
        "hold": "none",
        "buy_and_hold": "none",
        "monthly": "monthly",
        "month": "monthly",
        "quarterly": "quarterly",
        "quarter": "quarterly",
    }
    return aliases.get(normalized, normalized if normalized in REBALANCE_STEPS else "monthly")


def _format_date(value: Any) -> str:
    if hasattr(value, "strftime"):
        return value.strftime("%Y-%m-%d")
    return str(value)


def _format_compact_date(value: str) -> str:
    if len(value) == 8 and value.isdigit():
        return f"{value[:4]}-{value[4:6]}-{value[6:]}"
    return value
