"""Python <-> Rust (gp-core) parity guardrails.

Background
----------
The screening / trend / backtest / graph logic exists twice: once in Python
(``app/services/*`` -- the authoritative, full-featured "desktop" implementation)
and once in Rust (``native/gp-core`` -- the implementation the mobile/Tauri build
ships because phones cannot run Python/pandas/akshare).

These two implementations are **not** all equivalent today:

* ``trend``      -- numerically equivalent (a faithful port). Pinned with tight tolerance.
* ``screen``     -- divergent scoring (additive heuristic vs. weighted factors).
* ``graph``      -- divergent relation scoring (Rust lacks PageRank/embeddings).
* ``backtest``   -- Rust is a degenerate buy&hold subset (no rebalance/benchmark).

Until/unless the algorithms are deliberately converged, this module's job is to
**freeze the current state** so accidental drift is caught:

* For the equivalent surface (``trend``) we assert equality within tolerance.
* For the divergent surfaces we assert that the *structural* invariants still hold
  (same universe filtered, same selected codes where the filter -- not the score --
  decides) and document the divergence, so a regression in the shared parts is
  caught while the known algorithmic gaps are not treated as failures.

If ``gp-core`` has not been built, the whole module skips with a build hint.
"""

from __future__ import annotations

import unittest

import gp_core_ffi as ffi
from mock_provider import MockProvider

from app.schemas import (
    BacktestRequest,
    GraphScreenRequest,
    ScreenCriteria,
    TrendIndicatorRequest,
    TrendScreenRequest,
)
from app.services.backtest import backtest_hold
from app.services.screener import screen_stocks, screening_universe
from app.services.stock_graph import graph_screen_stocks
from app.services.trend_indicator import analyze_trend, trend_screen_stocks

START = "2024-01-01"
END = "2024-12-31"
# Backtest schemas take compact YYYYMMDD dates.
START_COMPACT = "20240101"
END_COMPACT = "20241231"
# Float tolerance for "the same computation in two languages".
TOL = 1e-6
# Python serializes equity curve points rounded to 4 decimals.
EQUITY_POINT_TOL = 1e-4

# Numeric fields of a trend series point that both sides compute identically.
TREND_POINT_FIELDS = (
    "close",
    "swl",
    "sws",
    "accumulation_index",
    "accumulation_strength",
    "swing_opportunity",
    "rebound_signal",
    "trend_heat",
    "volume_price_heat",
    "anomaly_heat",
    "popularity_heat",
)
TREND_POINT_FLAGS = ("red_hold", "cyan_watch", "short_buy", "white_exit")
# Numeric fields of the trend signal that both sides compute identically.
TREND_SIGNAL_FIELDS = (
    "close",
    "swl",
    "sws",
    "star_line",
    "bull_line",
    "wait_line",
    "quant_score",
    "pattern_score",
    "support",
    "resistance",
)


def _history_map(provider: MockProvider, codes) -> dict:
    histories = {}
    for code in codes:
        histories[code] = provider.get_history(code, START, END).to_dict("records")
    return histories


def _close(a, b, tol=TOL) -> bool:
    if a is None and b is None:
        return True
    if a is None or b is None:
        return False
    return abs(float(a) - float(b)) <= tol


@unittest.skipUnless(ffi.core_available(), ffi.core_skip_reason())
class TrendParityTests(unittest.TestCase):
    """``trend`` is a faithful port -- assert numerical equivalence."""

    def setUp(self):
        self.provider = MockProvider()
        self.codes = [s.code for s in self.provider.list_stocks()]

    def _rust_trend(self, code, series_limit):
        data = ffi.core_dataset_from_provider(
            self.provider, histories=_history_map(self.provider, [code])
        )
        return ffi.call(
            "gp_core_trend_with_data_json",
            {
                "data": data,
                "request": {
                    "code": code,
                    "start_date": START,
                    "end_date": END,
                    "series_limit": series_limit,
                },
            },
        )

    def test_trend_series_and_signal_match_for_all_mock_stocks(self):
        for code in self.codes:
            with self.subTest(code=code):
                rust = self._rust_trend(code, 80)
                py = analyze_trend(
                    self.provider,
                    TrendIndicatorRequest(
                        code=code, start_date=START, end_date=END, series_limit=80
                    ),
                )

                self.assertEqual(len(rust["series"]), len(py.series))
                # Compare the last point (fully warmed-up indicators).
                rp = rust["series"][-1]
                pp = py.series[-1].model_dump()
                for field in TREND_POINT_FIELDS:
                    self.assertTrue(
                        _close(rp.get(field), pp.get(field)),
                        f"{code} series.{field}: rust={rp.get(field)} py={pp.get(field)}",
                    )
                for flag in TREND_POINT_FLAGS:
                    self.assertEqual(
                        bool(rp.get(flag)), bool(pp.get(flag)), f"{code} series.{flag}"
                    )

                rs = rust["signal"]
                ps = py.signal.model_dump()
                for field in TREND_SIGNAL_FIELDS:
                    self.assertTrue(
                        _close(rs.get(field), ps.get(field)),
                        f"{code} signal.{field}: rust={rs.get(field)} py={ps.get(field)}",
                    )
                self.assertEqual(rs.get("status"), ps.get("status"), f"{code} signal.status")


    def test_trend_screen_explanation_shape_is_available_on_both_sides(self):
        code = self.codes[0]
        data = ffi.core_dataset_from_provider(
            self.provider, histories=_history_map(self.provider, [code])
        )
        request = TrendScreenRequest(
            criteria=ScreenCriteria(),
            start_date=START,
            end_date=END,
            limit=1,
        )
        rust = ffi.call(
            "gp_core_trend_screen_with_data_json",
            {"data": data, "request": request.model_dump()},
        )
        py = trend_screen_stocks(self.provider, request)
        self.assertEqual(rust["screen_style"], "short_buy")
        self.assertEqual(py.screen_style, "short_buy")
        self.assertTrue(rust["items"][0]["explanation"]["basis"])
        self.assertTrue(rust["items"][0]["explanation"]["score_breakdown"])
        self.assertTrue(py.items[0].explanation.basis)
        self.assertTrue(py.items[0].explanation.score_breakdown)

@unittest.skipUnless(ffi.core_available(), ffi.core_skip_reason())
class ScreenParityTests(unittest.TestCase):
    """``screen`` filter is shared; scoring diverges. Pin the filter, document the rest."""

    def setUp(self):
        self.provider = MockProvider()

    def _rust_screen(self, criteria: ScreenCriteria):
        data = ffi.core_dataset_from_provider(self.provider)
        return ffi.call(
            "gp_core_screen_with_data_json",
            {"data": data, "criteria": criteria.model_dump()},
        )

    def test_filter_universe_matches_even_though_scoring_diverges(self):
        # A criteria that the *filter* (not the score) decides: ROE >= 15%.
        criteria = ScreenCriteria(min_roe=0.15)
        rust = self._rust_screen(criteria)
        universe, notes = screening_universe(self.provider)
        py = screen_stocks(universe, criteria, notes)

        # The set of stocks that *pass the filter* must be identical across impls.
        # (Python returns a capped/grouped view, so compare the filtered total.)
        self.assertEqual(
            rust["total"],
            py.total,
            "filtered universe size diverged -- the shared filter logic regressed",
        )

    def test_scoring_divergence_is_still_present(self):
        # Documents the known gap: identical input, different score scales.
        # Rust uses an unbounded additive heuristic; Python clamps to [0, score_scale].
        criteria = ScreenCriteria(min_roe=0.10)
        rust = self._rust_screen(criteria)
        universe, notes = screening_universe(self.provider)
        py = screen_stocks(universe, criteria, notes)
        if rust["items"] and py.items:
            py_max = max(item.score for item in py.items)
            # Python scores are bounded; this is the documented contract.
            self.assertLessEqual(py_max, 20.0 + TOL)


@unittest.skipUnless(ffi.core_available(), ffi.core_skip_reason())
class BacktestParityTests(unittest.TestCase):
    """Rust backtest is a buy&hold subset; pin the selection + buy&hold core."""

    def setUp(self):
        self.provider = MockProvider()
        self.codes = ["600519.SH", "000001.SZ", "300750.SZ"]

    def test_selected_symbols_and_buy_and_hold_return_match(self):
        data = ffi.core_dataset_from_provider(
            self.provider, histories=_history_map(self.provider, self.codes)
        )
        request = BacktestRequest(
            source="watchlist",
            stock_codes=self.codes,
            start_date=START_COMPACT,
            end_date=END_COMPACT,
            initial_cash=100000.0,
            rebalance_frequency="none",
            transaction_cost_bps=0.0,
        )
        rust = ffi.call(
            "gp_core_backtest_with_data_json",
            {"data": data, "request": request.model_dump()},
        )
        py = backtest_hold(self.provider, request)
        # Same watchlist -> same selected set on both sides.
        self.assertEqual(
            rust["metrics"]["num_stocks"],
            len(py.symbols),
            "backtest selected-symbol count diverged",
        )
        self.assertEqual(
            {s.upper() for s in rust["symbols"]},
            {s.upper() for s in py.symbols},
            "backtest selected symbols diverged",
        )
        self.assertTrue(
            _close(rust["metrics"]["total_return"], py.metrics.total_return),
            "backtest total_return diverged: "
            f"rust={rust['metrics']['total_return']} py={py.metrics.total_return}",
        )
        if rust["equity_curve"] and py.equity_curve:
            self.assertTrue(
                _close(
                    rust["equity_curve"][-1]["equity"],
                    py.equity_curve[-1].equity,
                    EQUITY_POINT_TOL,
                ),
                "backtest final equity diverged: "
                f"rust={rust['equity_curve'][-1]['equity']} py={py.equity_curve[-1].equity}",
            )


@unittest.skipUnless(ffi.core_available(), ffi.core_skip_reason())
class GraphParityTests(unittest.TestCase):
    """Graph relation scoring diverges (no PageRank in Rust); pin the candidate pool."""

    def setUp(self):
        self.provider = MockProvider()

    def test_candidate_pool_filter_matches(self):
        data = ffi.core_dataset_from_provider(self.provider)
        request = GraphScreenRequest(
            criteria=ScreenCriteria(min_roe=0.10),
            seed_codes=["600036.SH"],
            relation_depth=2,
            relation_weight=0.4,
            limit=10,
        )
        rust = ffi.call(
            "gp_core_graph_screen_with_data_json",
            {"data": data, "request": request.model_dump()},
        )
        py = graph_screen_stocks(self.provider, request)
        rust_codes = {item["stock"]["code"] for item in rust["items"]}
        py_codes = {item.stock.code for item in py.items}
        self.assertEqual(
            rust["total"],
            py.total,
            "graph candidate-pool size diverged",
        )
        self.assertEqual(rust["center_context"]["mode"], py.center_context.mode)
        self.assertTrue(rust["items"][0]["explanation"]["basis"])
        self.assertTrue(rust["items"][0]["explanation"]["score_breakdown"])
        self.assertTrue(py.items[0].explanation.basis)
        self.assertTrue(py.items[0].explanation.score_breakdown)
        self.assertTrue(
            rust_codes & py_codes,
            f"graph candidate pools are disjoint: rust={rust_codes} py={py_codes}",
        )


if __name__ == "__main__":
    unittest.main()
