import unittest

from app.schemas import ScreenCriteria, StockItem
from app.services.screener import screen_stocks


class ScreenerIndustryTests(unittest.TestCase):
    def test_selected_industry_can_match_more_specific_stock_industry(self):
        universe = [
            StockItem(
                code="300750.SZ",
                name="宁德时代",
                industry="动力电池",
                price=195.0,
            )
        ]

        result = screen_stocks(universe, ScreenCriteria(industry="电池"))

        self.assertEqual(result.returned, 1)
        self.assertEqual(result.items[0].stock.code, "300750.SZ")

    def test_selected_industry_does_not_match_missing_stock_industry(self):
        universe = [
            StockItem(
                code="300750.SZ",
                name="宁德时代",
                industry="",
                price=195.0,
            )
        ]

        result = screen_stocks(universe, ScreenCriteria(industry="电池"))

        self.assertEqual(result.returned, 0)

    def test_optional_metric_sort_keeps_missing_values_last(self):
        universe = [
            StockItem(code="000001.SZ", name="平安银行", industry="银行", price=11.0, pe=None, pb=None),
            StockItem(code="600000.SH", name="浦发银行", industry="银行", price=9.0, pe=5.0, pb=0.6),
            StockItem(code="600036.SH", name="招商银行", industry="银行", price=31.0, pe=7.0, pb=0.9),
        ]

        result = screen_stocks(universe, ScreenCriteria(sort_by="pe", sort_dir="asc"))

        self.assertEqual([item.stock.code for item in result.items], ["600000.SH", "600036.SH", "000001.SZ"])


if __name__ == "__main__":
    unittest.main()
