from typing import Dict, List

from app.providers.base import StockProvider
from app.schemas import (
    GraphScreenRequest,
    GraphScreenResult,
    GraphStockSignal,
    ScreenCriteria,
    ScreenedStock,
    StockItem,
    StockRelation,
)
from app.services.knowledge_graph import build_knowledge_graph, graph_notes, score_candidates
from app.services.screener import screen_stocks


def graph_screen_stocks(provider: StockProvider, request: GraphScreenRequest) -> GraphScreenResult:
    universe = provider.list_stocks()
    candidate_pool = _screen_candidate_pool(universe, request.criteria, request.limit)
    candidate_by_code = {item.stock.code: item for item in candidate_pool}
    all_stocks = {stock.code: stock for stock in universe}

    graph = build_knowledge_graph(universe, provider.list_relations())
    base_scores = _normalize_scores({code: item.score for code, item in candidate_by_code.items()})
    relation_scores = score_candidates(
        graph=graph,
        screened=candidate_by_code,
        seed_codes=request.seed_codes,
        max_depth=request.relation_depth,
    )

    signals: List[GraphStockSignal] = []
    for code, screened in candidate_by_code.items():
        relation_score = relation_scores.get(code, 0.0)
        base_score = base_scores.get(code, 0.0)
        final_score = (1 - request.relation_weight) * base_score + request.relation_weight * relation_score
        signals.append(
            GraphStockSignal(
                stock=screened.stock,
                base_score=round(base_score, 6),
                relation_score=round(relation_score, 6),
                final_score=round(final_score, 6),
                suggested_weight=0.0,
                reasons=_reasons(screened, relation_score),
                related=_top_related(code, graph.relations, all_stocks),
            )
        )

    signals.sort(key=lambda item: item.final_score, reverse=True)
    signals = _assign_weights(signals[: request.limit])

    notes = [
        *graph_notes(graph),
        "LangGraph orchestrates agent state/tool flow; this service models stock relationships as a knowledge graph.",
    ]
    if not graph.relations:
        notes.append("No stock relations were available; result falls back to base screening scores.")

    return GraphScreenResult(
        total=len(candidate_pool),
        returned=len(signals),
        relation_count=len(graph.relations),
        items=signals,
        notes=notes,
    )


def _screen_candidate_pool(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    requested_limit: int,
) -> List[ScreenedStock]:
    pool_size = min(200, max(requested_limit * 5, criteria.limit, 50))
    expanded_criteria = criteria.model_copy(update={"limit": pool_size})
    return screen_stocks(universe, expanded_criteria).items


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
