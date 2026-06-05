import unittest

from app.providers.mock import MockProvider
from app.schemas import BacktestRequest, ScreenCriteria
from app.services.backtest import backtest_hold
from app.services.agent import run_agent


class BacktestQualityNotesTests(unittest.TestCase):
    def test_backtest_returns_cost_rebalance_and_benchmark_metrics(self):
        result = backtest_hold(
            MockProvider(),
            BacktestRequest(
                criteria=ScreenCriteria(industry="银行", limit=10),
                start_date="20200101",
                end_date="20200301",
                top_n=5,
                rebalance_frequency="monthly",
                transaction_cost_bps=10,
            ),
        )

        self.assertGreater(len(result.equity_curve), 0)
        self.assertGreater(len(result.benchmark_curve), 0)
        self.assertGreater(result.metrics.total_transaction_cost, 0)
        self.assertGreater(result.metrics.total_turnover, 0)
        self.assertGreater(result.metrics.rebalance_count, 0)
        self.assertIsNotNone(result.metrics.benchmark_total_return)
        self.assertIsNotNone(result.metrics.excess_return)
        self.assertTrue(any("交易成本" in note for note in result.notes))
        self.assertTrue(any("少于请求持仓数" in note for note in result.notes))

    def test_backtest_can_disable_periodic_rebalance(self):
        result = backtest_hold(
            MockProvider(),
            BacktestRequest(
                criteria=ScreenCriteria(industry="银行", limit=10),
                start_date="20200101",
                end_date="20200301",
                top_n=3,
                rebalance_frequency="none",
                transaction_cost_bps=0,
            ),
        )

        self.assertEqual(result.metrics.rebalance_count, 0)
        self.assertEqual(result.metrics.total_transaction_cost, 0)
        self.assertEqual(len(result.rebalance_dates), 1)

    def test_agent_backtest_parses_new_controls(self):
        result = run_agent(MockProvider(), "回测银行股，持仓 3 只，季度再平衡，交易成本 20 bps，不对比基准")

        self.assertEqual(result.action, "backtest")
        self.assertIsNotNone(result.backtest)
        self.assertEqual(result.backtest.top_n, 3)
        self.assertEqual(result.backtest.rebalance_frequency, "quarterly")
        self.assertEqual(result.backtest.transaction_cost_bps, 20)
        self.assertEqual(result.backtest.benchmark, "none")
        self.assertIsNotNone(result.data)
        self.assertIn("metrics", result.data)


if __name__ == "__main__":
    unittest.main()
