import unittest
from types import SimpleNamespace
from unittest.mock import patch

import pandas as pd

from app.schemas import ScreenCriteria, SectorScreenRequest, StockItem
from app.services.screener import screen_stocks, screen_stocks_by_sector
from app.services.screening_rules import (
    concept_group_for_stock,
    is_cold_sector,
    load_screening_rules,
    theme_category_for_stock,
)


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

    def test_deducted_profit_rule_requires_positive_profit_and_growth_above_ten_percent(self):
        universe = [
            StockItem(
                code="300001.SZ",
                name="达标公司",
                industry="半导体",
                price=10.0,
                deducted_net_profit_billion=1.2,
                deducted_net_profit_margin=12.0,
                deducted_net_profit_growth_rate=12.0,
            ),
            StockItem(
                code="300002.SZ",
                name="低净利率公司",
                industry="半导体",
                price=10.0,
                deducted_net_profit_billion=1.1,
                deducted_net_profit_margin=30.0,
                deducted_net_profit_growth_rate=9.9,
            ),
            StockItem(
                code="300003.SZ",
                name="亏损公司",
                industry="半导体",
                price=10.0,
                deducted_net_profit_billion=-0.1,
                deducted_net_profit_margin=15.0,
                deducted_net_profit_growth_rate=15.0,
            ),
            StockItem(code="300004.SZ", name="缺财务公司", industry="半导体", price=10.0),
        ]

        result = screen_stocks(
            universe,
            ScreenCriteria(
                min_deducted_net_profit_billion=0,
                min_deducted_net_profit_growth_rate=10,
                limit=10,
            ),
        )

        self.assertEqual([item.stock.code for item in result.items], ["300001.SZ"])
        self.assertIn("deducted_net_profit_ok", result.items[0].reasons)
        self.assertIn("deducted_net_profit_growth_rate_ok", result.items[0].reasons)

    def test_deducted_profit_growth_accepts_ratio_values(self):
        universe = [
            StockItem(
                code="300001.SZ",
                name="达标公司",
                industry="半导体",
                price=10.0,
                deducted_net_profit_billion=1.2,
                deducted_net_profit_growth_rate=0.12,
            )
        ]

        result = screen_stocks(
            universe,
            ScreenCriteria(min_deducted_net_profit_billion=0, min_deducted_net_profit_growth_rate=10),
        )

        self.assertEqual(result.returned, 1)

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
        self.assertTrue(any("共享筛选规则" in note and "热门主题" in note for note in result.notes))

    def test_score_sort_promotes_duofuduo_like_hot_themes_and_deprioritizes_bank_infra(self):
        universe = [
            StockItem(code="000001.SZ", name="高分银行", industry="银行", price=10.0, pe=2.0, pb=0.2, roe=0.3),
            StockItem(code="601668.SH", name="中国建筑", industry="建筑装饰", price=10.0, pe=3.0, pb=0.4, roe=0.2),
            StockItem(code="002407.SZ", name="多氟多", industry="化工", price=10.0, pe=70.0, pb=8.0, roe=0.03),
            StockItem(code="002408.SZ", name="氟材料公司", industry="锂电材料", price=10.0, pe=70.0, pb=8.0, roe=0.03),
            StockItem(code="688001.SH", name="芯片公司", industry="半导体", price=10.0, pe=60.0, pb=8.0, roe=0.03),
            StockItem(code="601012.SH", name="光伏公司", industry="光伏", price=10.0, pe=50.0, pb=7.0, roe=0.03),
        ]

        result = screen_stocks(universe, ScreenCriteria(sort_by="score", sort_dir="desc", limit=3))

        self.assertEqual([item.stock.code for item in result.items], ["002408.SZ", "688001.SH", "601012.SH"])
        self.assertNotIn("002407.SZ", {item.stock.code for item in result.items})
        self.assertNotIn("000001.SZ", {item.stock.code for item in result.items})
        self.assertNotIn("601668.SH", {item.stock.code for item in result.items})

    def test_score_sort_promotes_game_candidates_with_other_hot_sectors(self):
        universe = [
            StockItem(code="000001.SZ", name="高分银行", industry="银行", price=10.0, pe=2.0, pb=0.2, roe=0.3),
            StockItem(code="601668.SH", name="中国建筑", industry="建筑装饰", price=10.0, pe=3.0, pb=0.4, roe=0.2),
            StockItem(code="002408.SZ", name="氟材料公司", industry="锂电材料", price=10.0, pe=70.0, pb=8.0, roe=0.03),
            StockItem(code="688001.SH", name="芯片公司", industry="半导体", price=10.0, pe=60.0, pb=8.0, roe=0.03),
            StockItem(code="601012.SH", name="光伏公司", industry="光伏", price=10.0, pe=50.0, pb=7.0, roe=0.03),
            StockItem(code="002555.SZ", name="游戏公司", industry="网络游戏", price=10.0, pe=80.0, pb=9.0, roe=0.02),
        ]

        result = screen_stocks(universe, ScreenCriteria(sort_by="score", sort_dir="desc", limit=4))

        self.assertEqual([item.stock.code for item in result.items], ["002408.SZ", "688001.SH", "601012.SH", "002555.SZ"])
        self.assertTrue(any("共享筛选规则" in note for note in result.notes))

    def test_score_sort_promotes_ai_chain_candidates(self):
        universe = [
            StockItem(code="000001.SZ", name="high-score-bank", industry="银行", price=10.0, pe=2.0, pb=0.2, roe=0.3),
            StockItem(code="300308.SZ", name="cpo-leader", industry="光模块", price=10.0, pe=80.0, pb=12.0, roe=0.02),
            StockItem(code="002463.SZ", name="server-pcb", industry="PCB", price=10.0, pe=70.0, pb=9.0, roe=0.03),
            StockItem(code="688256.SH", name="ai-chip", industry="半导体", price=10.0, pe=90.0, pb=15.0, roe=0.01),
            StockItem(code="600845.SH", name="cloud-compute", industry="云计算", price=10.0, pe=50.0, pb=6.0, roe=0.05),
        ]

        result = screen_stocks(universe, ScreenCriteria(sort_by="score", sort_dir="desc", limit=4))

        codes = [item.stock.code for item in result.items]
        self.assertIn("300308.SZ", codes)
        self.assertIn("002463.SZ", codes)
        self.assertIn("688256.SH", codes)
        self.assertNotIn("000001.SZ", codes)
        hot_group = next(group for group in result.groups if group.key == "hot")
        self.assertTrue(any(item.stock.industry in {"光模块", "PCB", "半导体", "云计算"} for item in hot_group.items))

    def test_shared_rules_classify_concepts_and_cold_sectors(self):
        rules = load_screening_rules()
        chip = StockItem(code="688001.SH", name="AI芯片公司", industry="半导体", price=10.0)
        wafer = StockItem(code="688002.SH", name="晶圆制造公司", industry="硅片", price=10.0)

        self.assertGreaterEqual(rules.group_limit, 10)
        self.assertEqual(concept_group_for_stock(chip, rules), "AI算力与芯片")
        self.assertEqual(concept_group_for_stock(wafer, rules), "半导体晶圆")
        self.assertEqual(theme_category_for_stock(wafer, rules), "semiconductor_wafer")
        self.assertTrue(is_cold_sector("银行", rules))

    def test_screened_stock_includes_factor_breakdown_and_explanation(self):
        universe = [
            StockItem(code="688001.SH", name="AI芯片公司", industry="半导体", price=10.0, pe=60.0, pb=8.0, roe=0.03),
        ]

        result = screen_stocks(universe, ScreenCriteria(limit=1))

        item = result.items[0]
        self.assertIn("theme", item.factor_scores)
        self.assertIn("valuation", item.factor_scores)
        self.assertEqual(item.concept, "AI算力与芯片")
        self.assertEqual(item.theme_category, "ai_chain")
        self.assertIn("主题命中", item.score_explanation)

    def test_missing_optional_metrics_use_neutral_factor_defaults(self):
        universe = [
            StockItem(code="300001.SZ", name="未知公司", industry="未知行业", price=10.0),
        ]

        result = screen_stocks(universe, ScreenCriteria(limit=1))

        self.assertEqual(result.returned, 1)
        self.assertGreater(result.items[0].score, 0)
        self.assertTrue(result.items[0].score_explanation)

    def test_cold_sector_is_deprioritized_but_not_deleted(self):
        universe = [
            StockItem(code="000001.SZ", name="高分银行", industry="银行", price=10.0, pe=2.0, pb=0.2, roe=0.3),
        ]

        result = screen_stocks(universe, ScreenCriteria(limit=1))

        self.assertEqual(result.returned, 1)
        self.assertEqual(result.items[0].stock.code, "000001.SZ")
        self.assertIn("低热度降权", result.items[0].reasons)

    def test_institution_buy_ratio_filter_keeps_only_net_buy_candidates(self):
        universe = [
            StockItem(code="300750.SZ", name="宁德时代", industry="电池", price=10.0),
            StockItem(code="002594.SZ", name="比亚迪", industry="新能源车", price=10.0),
            StockItem(code="000001.SZ", name="平安银行", industry="银行", price=10.0),
        ]
        fake_ak = SimpleNamespace(
            stock_lhb_jgmmtj_em=lambda start_date, end_date: pd.DataFrame(
                [
                    {
                        "代码": "300750",
                        "日期": "2026-06-12",
                        "机构买入总额": 200_000_000,
                        "机构卖出总额": 80_000_000,
                        "成交额": 1_000_000_000,
                    },
                    {
                        "代码": "002594",
                        "日期": "2026-06-12",
                        "机构买入总额": 50_000_000,
                        "机构卖出总额": 90_000_000,
                        "成交额": 1_000_000_000,
                    },
                ]
            )
        )

        with patch.dict("sys.modules", {"akshare": fake_ak}):
            result = screen_stocks(
                universe,
                ScreenCriteria(require_institution_buy_ratio_gt_sell_ratio=True, limit=10),
            )

        self.assertEqual([item.stock.code for item in result.items], ["300750.SZ"])
        self.assertIn("机构买入占比", result.items[0].reasons[-1])
        self.assertTrue(any("机构买入占比规则" in note for note in result.notes))

    def test_institution_buy_ratio_filter_fails_closed_when_source_unavailable(self):
        universe = [
            StockItem(code="300750.SZ", name="宁德时代", industry="电池", price=10.0),
        ]
        fake_ak = SimpleNamespace(
            stock_lhb_jgmmtj_em=lambda start_date, end_date: (_ for _ in ()).throw(RuntimeError("blocked")),
            stock_lhb_jgmx_sina=lambda: pd.DataFrame(),
        )

        with patch.dict("sys.modules", {"akshare": fake_ak}):
            result = screen_stocks(
                universe,
                ScreenCriteria(require_institution_buy_ratio_gt_sell_ratio=True, limit=10),
            )

        self.assertEqual(result.returned, 0)
        self.assertTrue(any("候选股未放行" in note for note in result.notes))

    def test_basic_screen_returns_hot_and_potential_groups(self):
        universe = [
            *[
                StockItem(
                    code=f"688{index:03d}.SH",
                    name=f"AI-hot-{index}",
                    industry="AI software",
                    price=10.0,
                    pe=60.0,
                    pb=8.0,
                    roe=0.03,
                )
                for index in range(12)
            ],
            *[
                StockItem(
                    code=f"300{index:03d}.SZ",
                    name=f"potential-{index}",
                    industry="consumer",
                    price=10.0,
                    pe=1.0,
                    pb=0.2,
                    roe=6.0 - index * 0.1,
                )
                for index in range(12)
            ],
        ]

        result = screen_stocks(universe, ScreenCriteria(limit=10))

        groups = {group.key: group for group in result.groups}
        self.assertEqual(len(groups["hot"].items), 10)
        self.assertEqual(len(groups["potential"].items), 10)
        self.assertTrue(all("AI" in item.stock.industry for item in groups["hot"].items))
        self.assertTrue(all(item.score > 10 for item in groups["potential"].items))
        self.assertFalse(
            {item.stock.code for item in groups["hot"].items}
            & {item.stock.code for item in groups["potential"].items}
        )

    def test_concept_screen_merges_unknown_industries_into_other_concept(self):
        universe = []
        for sector_index in range(13):
            for stock_index in range(5):
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

        self.assertEqual(result.sector_count, 1)
        self.assertEqual(result.groups[0].sector, "其他概念")
        self.assertEqual(result.groups[0].returned, 5)
        self.assertEqual(result.groups[0].total, len(universe))

    def test_sector_screen_groups_by_concept_before_industry(self):
        universe = [
            StockItem(code="300001.SZ", name="chip-a", industry="半导体", price=10.0),
            StockItem(code="300002.SZ", name="optical-cpo", industry="光模块", price=11.0),
            StockItem(code="300003.SZ", name="server-pcb", industry="PCB", price=12.0),
            StockItem(code="300004.SZ", name="ai-server", industry="服务器", price=13.0),
            StockItem(code="300005.SZ", name="gpu-cooling", industry="液冷", price=14.0),
            StockItem(code="300006.SZ", name="battery-a", industry="储能", price=15.0),
            StockItem(code="300007.SZ", name="battery-b", industry="电池", price=16.0),
            StockItem(code="300008.SZ", name="solar-a", industry="光伏", price=17.0),
            StockItem(code="300009.SZ", name="wind-a", industry="风电", price=18.0),
            StockItem(code="300010.SZ", name="new-energy", industry="新能源", price=19.0),
        ]

        result = screen_stocks_by_sector(universe, SectorScreenRequest())

        groups = {group.sector: group for group in result.groups}
        self.assertIn("AI算力与芯片", groups)
        self.assertIn("新能源与储能", groups)
        self.assertEqual(groups["AI算力与芯片"].returned, 5)
        self.assertEqual(
            {item.stock.code for item in groups["AI算力与芯片"].items},
            {"300001.SZ", "300002.SZ", "300003.SZ", "300004.SZ", "300005.SZ"},
        )


if __name__ == "__main__":
    unittest.main()
