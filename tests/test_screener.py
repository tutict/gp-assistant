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


if __name__ == "__main__":
    unittest.main()
