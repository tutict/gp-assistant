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
    for item in selected:
        history = provider.get_history(item.stock.code, request.start_date, request.end_date)
        if history is None or history.empty:
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
