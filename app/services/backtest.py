from typing import Any, List, Tuple

from app.providers.base import StockProvider
from app.schemas import BacktestMetrics, BacktestRequest, BacktestResult, EquityPoint, ScreenedStock
from app.services.screener import screen_stocks


def backtest_hold(provider: StockProvider, request: BacktestRequest) -> BacktestResult:
    import pandas as pd

    universe = provider.list_stocks()
    screened = screen_stocks(universe, request.criteria)
    selected = screened.items[: request.top_n]
    symbols = [item.stock.code for item in selected]

    curves: List[pd.Series] = []
    missing_symbols: List[str] = []
    for item in selected:
        history = provider.get_history(item.stock.code, request.start_date, request.end_date)
        if history is None or history.empty:
            missing_symbols.append(item.stock.code)
            continue
        series = _normalize_series(history)
        curves.append(series)

    if not curves:
        return BacktestResult(
            metrics=BacktestMetrics(
                total_return=0.0,
                annualized_return=None,
                max_drawdown=None,
                num_stocks=len(symbols),
            ),
            equity_curve=[],
            symbols=symbols,
            notes=_quality_notes(
                selected_count=len(selected),
                used_count=0,
                requested_top_n=request.top_n,
                missing_symbols=missing_symbols,
                start_date=request.start_date,
                end_date=request.end_date,
                curve_points=0,
            ),
        )

    df = pd.concat(curves, axis=1).dropna(how="all")
    df["portfolio"] = df.mean(axis=1)
    equity = df["portfolio"] * request.initial_cash

    equity_curve = [EquityPoint(date=str(idx.date()), equity=float(val)) for idx, val in equity.items()]
    total_return, annualized_return, max_drawdown = _metrics(equity)

    return BacktestResult(
        metrics=BacktestMetrics(
            total_return=total_return,
            annualized_return=annualized_return,
            max_drawdown=max_drawdown,
            num_stocks=len(symbols),
        ),
        equity_curve=equity_curve,
        symbols=symbols,
        notes=_quality_notes(
            selected_count=len(selected),
            used_count=len(curves),
            requested_top_n=request.top_n,
            missing_symbols=missing_symbols,
            start_date=request.start_date,
            end_date=request.end_date,
            curve_points=len(equity_curve),
        ),
    )


def _normalize_series(history: Any) -> Any:
    import pandas as pd

    history = history.copy()
    history["date"] = pd.to_datetime(history["date"])
    history = history.sort_values("date")
    history = history.set_index("date")
    base = history["close"].iloc[0]
    if base == 0:
        base = 1.0
    return history["close"] / base


def _metrics(equity: Any) -> Tuple[float, float, float]:
    total_return = float(equity.iloc[-1] / equity.iloc[0] - 1)
    days = max((equity.index[-1] - equity.index[0]).days, 1)
    years = days / 365.0
    annualized = float((1 + total_return) ** (1 / years) - 1) if years > 0 else None
    rolling_max = equity.cummax()
    drawdown = (equity / rolling_max - 1).min()
    return total_return, annualized, float(drawdown)


def _quality_notes(
    *,
    selected_count: int,
    used_count: int,
    requested_top_n: int,
    missing_symbols: List[str],
    start_date: str,
    end_date: str,
    curve_points: int,
) -> List[str]:
    notes: List[str] = [
        "当前回测为筛选候选等权买入并持有到结束日期，未计入交易成本、定期再平衡和基准对比。"
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
    notes.append(f"验证区间：{_format_compact_date(start_date)} 至 {_format_compact_date(end_date)}。")
    return notes


def _format_compact_date(value: str) -> str:
    if len(value) == 8 and value.isdigit():
        return f"{value[:4]}-{value[4:6]}-{value[6:]}"
    return value
