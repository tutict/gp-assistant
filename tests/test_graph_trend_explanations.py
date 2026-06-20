import unittest

from mock_provider import MockProvider

from app.schemas import GraphScreenRequest, ScreenCriteria, TrendScreenRequest
from app.services.stock_graph import graph_screen_stocks
from app.services.trend_indicator import trend_screen_stocks


class GraphTrendExplanationTests(unittest.TestCase):
    def setUp(self):
        self.provider = MockProvider()

    def test_graph_without_seed_uses_theme_center_and_explains_items(self):
        result = graph_screen_stocks(
            self.provider,
            GraphScreenRequest(criteria=ScreenCriteria(min_roe=0.10), seed_codes=[], limit=5),
        )

        self.assertEqual(result.center_context.mode, "theme_center")
        self.assertTrue(result.center_context.label)
        self.assertTrue(result.center_context.codes)
        self.assertTrue(result.items)
        first = result.items[0]
        self.assertTrue(first.explanation.basis)
        self.assertTrue(first.explanation.score_breakdown)
        self.assertTrue(first.explanation.risk_checks)
        self.assertTrue(first.explanation.verification)

    def test_graph_with_seed_reports_seed_center(self):
        result = graph_screen_stocks(
            self.provider,
            GraphScreenRequest(
                criteria=ScreenCriteria(min_roe=0.10),
                seed_codes=["600036.SH"],
                relation_depth=2,
                limit=5,
            ),
        )

        self.assertEqual(result.center_context.mode, "seed_codes")
        self.assertEqual(result.center_context.codes, ["600036.SH"])

    def test_trend_screen_uses_short_buy_style_and_explains_risks(self):
        result = trend_screen_stocks(
            self.provider,
            TrendScreenRequest(
                criteria=ScreenCriteria(min_roe=0.10),
                start_date="20240101",
                end_date="20241231",
                limit=5,
            ),
        )

        self.assertEqual(result.screen_style, "short_buy")
        self.assertTrue(result.items)
        first = result.items[0]
        self.assertTrue(first.explanation.basis)
        self.assertTrue(first.explanation.score_breakdown)
        self.assertTrue(first.explanation.risk_checks)
        self.assertTrue(first.explanation.verification)
        self.assertTrue(any(item.key == "short_buy_score" for item in first.explanation.score_breakdown))


if __name__ == "__main__":
    unittest.main()