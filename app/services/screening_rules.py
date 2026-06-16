from __future__ import annotations

import json
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Any, Mapping, Sequence

from app.schemas import StockItem


RULES_PATH = Path(__file__).resolve().parents[1] / "static" / "screening_rules.json"
OTHER_CONCEPT = "其他概念"

DEFAULT_RULES: dict[str, Any] = {
    "version": 0,
    "score_scale": 20.0,
    "group_limit": 10,
    "potential_score_threshold": 10.0,
    "factor_weights": {
        "theme": 0.24,
        "fundamental": 0.24,
        "valuation": 0.24,
        "size": 0.14,
        "risk": 0.14,
    },
    "theme_promotion_order": ["materials", "ai_chain", "tech", "energy", "game"],
    "theme_fill_order": ["ai_chain", "materials", "tech", "energy", "game"],
    "theme_categories": [
        {
            "key": "materials",
            "label": "新材料",
            "score": 0.96,
            "keywords": ["氟化工", "氟材料", "锂电材料", "电解液", "六氟磷酸锂", "新能材", "新材料", "固态电池", "磁材"],
        },
        {
            "key": "ai_chain",
            "label": "AI算力与芯片",
            "score": 0.95,
            "keywords": ["半导体", "芯片", "算力", "人工智能", "ai", "光模块", "cpo", "服务器", "液冷", "gpu", "hbm", "存储", "数据中心", "云计算", "大模型", "aigc", "边缘计算", "pcb", "封装", "封测", "eda", "soc"],
        },
        {"key": "tech", "label": "科技制造", "score": 0.84, "keywords": ["机器人", "软件", "通信", "科技", "电子", "自动化", "高端制造", "智能制造"]},
        {"key": "energy", "label": "新能源", "score": 0.82, "keywords": ["新能源", "电池", "储能", "光伏", "电力", "能源", "油气", "煤炭", "风电", "充电桩"]},
        {"key": "game", "label": "游戏传媒", "score": 0.78, "keywords": ["游戏", "网络游戏", "手游", "电竞", "云游戏", "互动娱乐", "文化传媒", "传媒"]},
    ],
    "concept_groups": [
        {"label": "AI算力与芯片", "keywords": ["半导体", "芯片", "算力", "人工智能", "ai", "光模块", "cpo", "服务器", "液冷", "gpu", "hbm", "存储", "数据中心", "云计算", "大模型", "aigc", "边缘计算", "pcb", "封装", "封测", "eda", "soc"]},
        {"label": "新材料", "keywords": ["氟化工", "氟材料", "锂电材料", "电解液", "六氟磷酸锂", "新能材", "新材料", "固态电池", "磁材"]},
        {"label": "新能源与储能", "keywords": ["新能源", "电池", "储能", "光伏", "电力", "能源", "风电", "充电桩"]},
        {"label": "游戏传媒", "keywords": ["游戏", "网络游戏", "手游", "电竞", "云游戏", "互动娱乐", "传媒", "广告营销"]},
        {"label": "机器人与高端制造", "keywords": ["机器人", "工业母机", "自动化", "高端制造", "智能制造", "机械设备"]},
        {"label": "消费零售", "keywords": ["食品", "饮料", "白酒", "休闲食品", "一般零售", "商贸零售", "家电", "旅游", "酒店", "餐饮"]},
        {"label": "医药医疗", "keywords": ["医药", "医疗", "生物制品", "创新药", "中药", "化学制药", "医疗器械", "cro"]},
        {"label": "金融地产", "keywords": ["银行", "证券", "保险", "房地产", "地产", "物业"]},
        {"label": "基建建筑", "keywords": ["建筑", "房屋建设", "工程建设", "基础建设", "水泥", "铁路", "公路", "装修装饰"]},
        {"label": "周期资源", "keywords": ["煤炭", "钢铁", "普钢", "有色", "金属", "化工", "石油", "油气", "矿业"]},
        {"label": "汽车产业链", "keywords": ["汽车", "整车", "零部件", "轮胎", "智能驾驶", "无人驾驶", "汽车服务"]},
        {"label": "军工航天", "keywords": ["军工", "航天", "航空", "卫星", "船舶", "无人机", "国防"]},
        {"label": "交运物流", "keywords": ["物流", "航运", "港口", "机场", "航空运输", "铁路运输", "快递"]},
        {"label": "公用环保", "keywords": ["环保", "水务", "燃气", "供热", "公用事业"]},
    ],
    "cold_keywords": ["银行", "基建", "建筑", "建筑装饰", "工程建设", "基础建设", "水泥", "铁路", "公路"],
}


@dataclass(frozen=True)
class KeywordGroup:
    key: str
    label: str
    keywords: tuple[str, ...]
    score: float = 0.0


@dataclass(frozen=True)
class ScreeningRules:
    version: int
    score_scale: float
    group_limit: int
    potential_score_threshold: float
    factor_weights: Mapping[str, float]
    theme_promotion_order: tuple[str, ...]
    theme_fill_order: tuple[str, ...]
    theme_categories: tuple[KeywordGroup, ...]
    concept_groups: tuple[KeywordGroup, ...]
    cold_keywords: tuple[str, ...]
    source: str

    @classmethod
    def from_dict(cls, payload: Mapping[str, Any], *, source: str) -> "ScreeningRules":
        return cls(
            version=_int_value(payload.get("version"), 0),
            score_scale=_float_value(payload.get("score_scale"), 20.0),
            group_limit=max(1, _int_value(payload.get("group_limit"), 10)),
            potential_score_threshold=_float_value(payload.get("potential_score_threshold"), 10.0),
            factor_weights=_normalized_weights(payload.get("factor_weights")),
            theme_promotion_order=tuple(str(value) for value in payload.get("theme_promotion_order", []) if str(value).strip()),
            theme_fill_order=tuple(str(value) for value in payload.get("theme_fill_order", []) if str(value).strip()),
            theme_categories=tuple(_keyword_group(item, with_key=True) for item in _sequence(payload.get("theme_categories"))),
            concept_groups=tuple(_keyword_group(item, with_key=False) for item in _sequence(payload.get("concept_groups"))),
            cold_keywords=tuple(_normalize_keyword(value) for value in _sequence(payload.get("cold_keywords")) if _normalize_keyword(value)),
            source=source,
        )

    def concept_rank(self, concept: str) -> int:
        for index, group in enumerate(self.concept_groups):
            if group.label == concept:
                return index
        return len(self.concept_groups)

    def theme_rank(self, theme_key: str | None) -> int:
        if not theme_key:
            return len(self.theme_promotion_order)
        try:
            return self.theme_promotion_order.index(theme_key)
        except ValueError:
            return len(self.theme_promotion_order)


@lru_cache(maxsize=1)
def load_screening_rules() -> ScreeningRules:
    try:
        payload = json.loads(RULES_PATH.read_text(encoding="utf-8"))
        rules = ScreeningRules.from_dict(payload, source=str(RULES_PATH))
    except Exception:
        rules = ScreeningRules.from_dict(DEFAULT_RULES, source="fallback")
    if not rules.concept_groups or not rules.theme_categories:
        return ScreeningRules.from_dict(DEFAULT_RULES, source="fallback")
    return rules


def concept_group_for_stock(stock: StockItem, rules: ScreeningRules | None = None) -> str:
    text = _stock_text(stock)
    for group in (rules or load_screening_rules()).concept_groups:
        if _matches_keywords(text, group.keywords):
            return group.label
    return OTHER_CONCEPT


def concept_rank(concept: str, rules: ScreeningRules | None = None) -> int:
    return (rules or load_screening_rules()).concept_rank(concept)


def theme_match_for_stock(stock: StockItem, rules: ScreeningRules | None = None) -> KeywordGroup | None:
    text = _stock_text(stock)
    for group in (rules or load_screening_rules()).theme_categories:
        if _matches_keywords(text, group.keywords):
            return group
    return None


def theme_category_for_stock(stock: StockItem, rules: ScreeningRules | None = None) -> str | None:
    match = theme_match_for_stock(stock, rules)
    return match.key if match else None


def is_cold_sector(industry: str, rules: ScreeningRules | None = None) -> bool:
    text = str(industry or "").strip().lower()
    return bool(text) and _matches_keywords(text, (rules or load_screening_rules()).cold_keywords)


def _stock_text(stock: StockItem) -> str:
    return f"{stock.name or ''} {stock.industry or ''}".strip().lower()


def _matches_keywords(text: str, keywords: Sequence[str]) -> bool:
    return any(keyword and keyword in text for keyword in keywords)


def _keyword_group(item: object, *, with_key: bool) -> KeywordGroup:
    data = item if isinstance(item, Mapping) else {}
    label = str(data.get("label") or data.get("key") or "").strip()
    key = str(data.get("key") or label).strip()
    keywords = tuple(_normalize_keyword(value) for value in _sequence(data.get("keywords")) if _normalize_keyword(value))
    return KeywordGroup(
        key=key if with_key else label,
        label=label or key,
        keywords=keywords,
        score=_float_value(data.get("score"), 0.0),
    )


def _normalized_weights(value: object) -> dict[str, float]:
    defaults = DEFAULT_RULES["factor_weights"]
    source = value if isinstance(value, Mapping) else {}
    weights = {key: max(0.0, _float_value(source.get(key), default)) for key, default in defaults.items()}
    total = sum(weights.values())
    if total <= 0:
        return dict(defaults)
    return {key: weight / total for key, weight in weights.items()}


def _sequence(value: object) -> list[Any]:
    if isinstance(value, list):
        return value
    if isinstance(value, tuple):
        return list(value)
    return []


def _normalize_keyword(value: object) -> str:
    return str(value or "").strip().lower()


def _float_value(value: object, default: float) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def _int_value(value: object, default: int) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default
