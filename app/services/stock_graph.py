from collections import defaultdict
from typing import Dict, List, Sequence

from app.providers.base import StockProvider
from app.schemas import (
    GraphCenterContext,
    GraphScreenRequest,
    GraphScreenResult,
    GraphStockSignal,
    ScoreContribution,
    ScreenCriteria,
    ScreenedStock,
    SelectionExplanation,
    StockItem,
    StockRelation,
)
from app.services.knowledge_graph import build_knowledge_graph, graph_notes, score_candidates
from app.services.screener import screen_stocks, screening_universe


def graph_screen_stocks(provider: StockProvider, request: GraphScreenRequest) -> GraphScreenResult:
    universe, screen_notes = screening_universe(provider)
    candidate_pool, criteria_notes = _screen_candidate_pool(universe, request.criteria, request.limit)
    candidate_by_code = {item.stock.code: item for item in candidate_pool}
    all_stocks = {stock.code: stock for stock in universe}

    center_context = _resolve_center_context(candidate_pool, request.seed_codes)
    scoring_seed_codes = center_context.codes

    graph = build_knowledge_graph(universe, provider.list_relations())
    base_scores = _normalize_scores({code: item.score for code, item in candidate_by_code.items()})
    relation_scores = score_candidates(
        graph=graph,
        screened=candidate_by_code,
        seed_codes=scoring_seed_codes,
        max_depth=request.relation_depth,
    )

    signals: List[GraphStockSignal] = []
    for code, screened in candidate_by_code.items():
        relation_score = relation_scores.get(code, 0.0)
        base_score = base_scores.get(code, 0.0)
        final_score = (1 - request.relation_weight) * base_score + request.relation_weight * relation_score
        related = _top_related(code, graph.relations, all_stocks)
        signals.append(
            GraphStockSignal(
                stock=screened.stock,
                base_score=round(base_score, 6),
                relation_score=round(relation_score, 6),
                final_score=round(final_score, 6),
                suggested_weight=0.0,
                reasons=_reasons(screened, relation_score),
                related=related,
                explanation=_graph_explanation(
                    screened=screened,
                    base_score=base_score,
                    relation_score=relation_score,
                    final_score=final_score,
                    relation_weight=request.relation_weight,
                    center_context=center_context,
                    related=related,
                ),
            )
        )

    signals.sort(key=lambda item: item.final_score, reverse=True)
    signals = _assign_weights(signals[: request.limit])

    notes = [
        *screen_notes,
        *criteria_notes,
        *graph_notes(graph),
        "LangGraph 负责编排智能体状态；股票关系由知识图谱评分层建模。",
    ]
    if center_context.mode == "theme_center":
        notes.append(f"未提供种子股，图谱评分使用 {center_context.label}。")
    if not graph.relations:
        notes.append("暂无股票关系数据，结果退回基础筛选分。")

    return GraphScreenResult(
        total=len(candidate_pool),
        returned=len(signals),
        relation_count=len(graph.relations),
        items=signals,
        center_context=center_context,
        notes=notes,
    )


def _screen_candidate_pool(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    requested_limit: int,
) -> tuple[List[ScreenedStock], List[str]]:
    pool_size = min(200, max(requested_limit * 5, criteria.limit, 50))
    expanded_criteria = criteria.model_copy(update={"limit": pool_size})
    result = screen_stocks(universe, expanded_criteria)
    return result.items, result.notes


def _resolve_center_context(candidate_pool: Sequence[ScreenedStock], seed_codes: Sequence[str]) -> GraphCenterContext:
    normalized_seed_codes = [code.strip().upper() for code in seed_codes if code and code.strip()]
    if normalized_seed_codes:
        return GraphCenterContext(
            mode="seed_codes",
            label="种子股中心",
            codes=normalized_seed_codes[:50],
        )

    if not candidate_pool:
        return GraphCenterContext(mode="theme_center", label="空主题中心", codes=[])

    grouped: dict[str, list[ScreenedStock]] = defaultdict(list)
    labels: dict[str, str] = {}
    for item in candidate_pool:
        key, label = _center_group_key(item)
        grouped[key].append(item)
        labels[key] = label

    def group_rank(entry: tuple[str, list[ScreenedStock]]) -> tuple[int, float, float, str]:
        key, items = entry
        avg_score = sum(item.score for item in items) / max(len(items), 1)
        top_score = max((item.score for item in items), default=0.0)
        return (len(items), avg_score, top_score, labels.get(key, key))

    selected_key, selected_items = max(grouped.items(), key=group_rank)
    center_items = sorted(selected_items, key=lambda item: item.score, reverse=True)[:5]
    return GraphCenterContext(
        mode="theme_center",
        label=labels.get(selected_key, selected_key),
        codes=[item.stock.code for item in center_items],
    )


def _center_group_key(item: ScreenedStock) -> tuple[str, str]:
    if item.theme_category:
        label = item.concept or item.theme_category
        return f"theme:{item.theme_category}", f"主题中心：{label}"
    if item.concept:
        return f"concept:{item.concept}", f"概念中心：{item.concept}"
    industry = item.stock.industry or "未知行业"
    return f"industry:{industry}", f"行业中心：{industry}"


def _normalize_scores(scores: Dict[str, float]) -> Dict[str, float]:
    if not scores:
        return {}
    min_score = min(scores.values())
    max_score = max(scores.values())
    if max_score == min_score:
        value = 1.0 if max_score > 0 else 0.0
        return {code: value for code in scores}
    return {code: (score - min_score) / (max_score - min_score) for code, score in scores.items()}


def _top_related(
    code: str,
    relations: List[StockRelation],
    all_stocks: Dict[str, StockItem],
    limit: int = 5,
) -> List[StockRelation]:
    connected = [
        relation
        for relation in relations
        if relation.source_code == code or relation.target_code == code
    ]
    connected.sort(key=lambda relation: relation.weight, reverse=True)
    return [
        _relation_with_names(relation, all_stocks)
        for relation in connected[:limit]
    ]


def _relation_with_names(relation: StockRelation, all_stocks: Dict[str, StockItem]) -> StockRelation:
    source = all_stocks.get(relation.source_code)
    target = all_stocks.get(relation.target_code)
    if source is None or target is None:
        return relation
    description = relation.description or ""
    label = f"{source.name} <-> {target.name}"
    return relation.model_copy(update={"description": f"{label}. {description}".strip()})


def _reasons(screened: ScreenedStock, relation_score: float) -> List[str]:
    reasons = list(screened.reasons)
    if relation_score >= 0.65:
        reasons.append("strong_relation_signal")
    elif relation_score >= 0.35:
        reasons.append("moderate_relation_signal")
    return reasons


def _graph_explanation(
    *,
    screened: ScreenedStock,
    base_score: float,
    relation_score: float,
    final_score: float,
    relation_weight: float,
    center_context: GraphCenterContext,
    related: Sequence[StockRelation],
) -> SelectionExplanation:
    center_label = center_context.label or "种子股中心"
    basis = [
        f"通过当前基础筛选，原始基础分 {screened.score:.2f}。",
        f"关系传播中心：{center_label}；中心代码：{', '.join(center_context.codes) if center_context.codes else '无'}。",
    ]
    if related:
        strongest = related[0]
        basis.append(f"展示的最强关系边为 {strongest.relation_type}，权重 {strongest.weight:.2f}。")
    else:
        basis.append("暂无直接展示的关系边，排序主要依赖基础分和图谱传播分。")

    base_component = (1 - relation_weight) * base_score
    relation_component = relation_weight * relation_score
    risk_checks: List[str] = []
    if relation_score >= 0.65:
        risk_checks.append("关系信号较强，需要用近期业务证据确认关系仍然成立。")
    elif relation_score >= 0.35:
        risk_checks.append("关系信号中等，更适合作为候选扩展线索，而不是独立买入理由。")
    else:
        risk_checks.append("关系信号偏弱，当前入选更多由基础筛选分驱动。")
    if not related:
        risk_checks.append("缺少直接关系边，建议刷新或补充关系数据后再依赖图谱证据。")

    return SelectionExplanation(
        basis=basis,
        score_breakdown=[
            ScoreContribution(
                key="base_score",
                label="基础分",
                value=round(base_score, 6),
                contribution=round(base_component, 6),
                tone="strong" if base_score >= 0.7 else "watch" if base_score >= 0.35 else "weak",
            ),
            ScoreContribution(
                key="relation_score",
                label="关系分",
                value=round(relation_score, 6),
                contribution=round(relation_component, 6),
                tone="strong" if relation_score >= 0.65 else "watch" if relation_score >= 0.35 else "weak",
            ),
            ScoreContribution(
                key="final_score",
                label="最终分",
                value=round(final_score, 6),
                contribution=round(final_score, 6),
                tone="strong" if final_score >= 0.65 else "watch" if final_score >= 0.35 else "weak",
            ),
        ],
        risk_checks=risk_checks,
        verification=[
            "交叉查看趋势择时，确认是否存在短买或退出信号。",
            "打开关系边，核验同业、供应链或主题证据。",
        ],
    )


def _assign_weights(signals: List[GraphStockSignal]) -> List[GraphStockSignal]:
    if not signals:
        return signals

    positive_sum = sum(max(item.final_score, 0.0) for item in signals)
    if positive_sum <= 0:
        equal_weight = 1 / len(signals)
        return [item.model_copy(update={"suggested_weight": round(equal_weight, 6)}) for item in signals]

    return [
        item.model_copy(
            update={"suggested_weight": round(max(item.final_score, 0.0) / positive_sum, 6)}
        )
        for item in signals
    ]