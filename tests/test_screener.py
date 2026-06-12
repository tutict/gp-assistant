import unittest

from app.schemas import ScreenCriteria, SectorScreenRequest, StockItem
from app.services.screener import screen_stocks, screen_stocks_by_sector


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

    def test_score_sort_biases_toward_hot_energy_and_tech_sectors(self):
        universe = [
            StockItem(code="000001.SZ", name="平安银行", industry="银行", price=10.0, pe=10.0, pb=1.0, roe=0.1),
            StockItem(code="688001.SH", name="芯片公司", industry="半导体", price=10.0, pe=10.0, pb=1.0, roe=0.1),
            StockItem(code="601012.SH", name="光伏公司", industry="光伏", price=10.0, pe=10.0, pb=1.0, roe=0.1),
        ]

        result = screen_stocks(universe, ScreenCriteria(sort_by="score", sort_dir="desc"))

        self.assertEqual([item.stock.code for item in result.items[:2]], ["688001.SH", "601012.SH"])

    def test_score_sort_promotes_hot_tech_and_energy_candidates_into_limited_results(self):
        universe = [
            StockItem(code="000001.SZ", name="高分银行", industry="银行", price=10.0, pe=2.0, pb=0.2, roe=0.3),
            StockItem(code="600000.SH", name="普通银行", industry="银行", price=10.0, pe=3.0, pb=0.3, roe=0.2),
            StockItem(code="688001.SH", name="芯片公司", industry="半导体", price=10.0, pe=60.0, pb=8.0, roe=0.03),
            StockItem(code="601012.SH", name="光伏公司", industry="光伏", price=10.0, pe=50.0, pb=7.0, roe=0.03),
        ]

        result = screen_stocks(universe, ScreenCriteria(sort_by="score", sort_dir="desc", limit=2))

        self.assertEqual([item.stock.code for item in result.items], ["688001.SH", "601012.SH"])
        self.assertTrue(any("科技与能源" in note for note in result.notes))

    def test_sector_screen_defaults_return_more_groups_with_three_stocks_each(self):
        universe = []
        for sector_index in range(13):
            for stock_index in range(3):
                universe.append(
                    StockItem(
                        code=f"{sector_index:03d}{stock_index:03d}.SZ",
                        name=f"stock-{sector_index}-{stock_index}",
                        industry=f"sector-{sector_index:02d}",
                        price=10.0 + stock_index,
                    )
                )
        universe.extend(
            [
                StockItem(code="900001.SZ", name="small-1", industry="small-sector", price=10.0),
                StockItem(code="900002.SZ", name="small-2", industry="small-sector", price=11.0),
            ]
        )

        result = screen_stocks_by_sector(universe, SectorScreenRequest())

        self.assertEqual(result.sector_count, 12)
        self.assertTrue(all(group.returned == 3 for group in result.groups))
        self.assertNotIn("small-sector", {group.sector for group in result.groups})


if __name__ == "__main__":
    unittest.main()
