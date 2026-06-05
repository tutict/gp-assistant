import unittest

from app.providers.mock import MockProvider
from app.schemas import BacktestRequest, ScreenCriteria
from app.services.backtest import backtest_hold


class BacktestQualityNotesTests(unittest.TestCase):
    def test_backtest_returns_quality_notes(self):
        result = backtest_hold(
            MockProvider(),
            BacktestRequest(
                criteria=ScreenCriteria(industry="银行", limit=10),
                start_date="20200101",
                end_date="20200301",
                top_n=5,
            ),
        )

        self.assertGreater(len(result.equity_curve), 0)
        self.assertTrue(result.notes)
        self.assertTrue(any("未计入交易成本" in note for note in result.notes))
        self.assertTrue(any("少于请求持仓数" in note for note in result.notes))


if __name__ == "__main__":
    unittest.main()
