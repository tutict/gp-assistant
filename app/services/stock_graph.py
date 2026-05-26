from collections import defaultdict, deque
from typing import Dict, Iterable, List, Set, Tuple

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
from app.services.screener import screen_stocks


def graph_screen_stocks(provider: StockProvider, request: GraphScreenRequest) -> GraphScreenResult:
    universe = provider.list_stocks()
    candidate_pool = _screen_candidate_pool(universe, request.criteria, request.limit)
    candidate_by_code = {item.stock.code: item for item in candidate_pool}
    all_stocks = {stock.code: stock for stock in universe}

    relations = _merge_relations(
        [
            *provider.list_relations(),
            *_infer_industry_relations(universe),
        ]
    )
    adjacency = _build_adjacency(relations)
    base_scores = _normalize_scores({code: item.score for code, item in candidate_by_code.items()})

    signals: List[GraphStockSignal] = []
    for code, screened in candidate_by_code.items():
        relation_score = _relation_score(
            code=code,
            base_scores=base_scores,
            adjacency=adjacency,
            seed_codes=set(request.seed_codes),
            max_depth=request.relation_depth,
        )
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
                related=_top_related(code, relations, all_stocks),
            )
        )

    signals.sort(key=lambda item: item.final_score, reverse=True)
    signals = _assign_weights(signals[: request.limit])

    notes = [
        "Relation graph is a lightweight propagation layer, not LangGraph workflow orchestration.",
        "Use LangGraph for agent state flow; use graph learning or knowledge graph data for stock relations.",
    ]
    if not relations:
        notes.append("No stock relations were available; result falls back to base screening scores.")

    return GraphScreenResult(
        total=len(candidate_pool),
        returned=len(signals),
        relation_count=len(relations),
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


def _merge_relations(relations: Iterable[StockRelation]) -> List[StockRelation]:
    merged: Dict[Tuple[str, str, str], StockRelation] = {}
    for relation in relations:
        if relation.source_code == relation.target_code:
            continue
        source, target = sorted([relation.source_code, relation.target_code])
        key = (source, target, relation.relation_type)
        existing = merged.get(key)
        if existing is None or relation.weight > existing.weight:
            merged[key] = relation.model_copy(update={"source_code": source, "target_code": target})
    return list(merged.values())


def _infer_industry_relations(universe: List[StockItem]) -> List[StockRelation]:
    by_industry: Dict[str, List[StockItem]] = defaultdict(list)
    for stock in universe:
        if stock.industry and stock.industry != "Unknown":
            by_industry[stock.industry].append(stock)

    relations: List[StockRelation] = []
    for industry, members in by_industry.items():
        ranked = sorted(members, key=lambda item: item.market_cap_billion or 0, reverse=True)[:12]
        for index, source in enumerate(ranked):
            for target in ranked[index + 1 : index + 4]:
                relations.append(
                    StockRelation(
                        source_code=source.code,
                        target_code=target.code,
                        relation_type="industry_peer",
                        weight=0.45,
                        description=f"Same industry: {industry}",
                    )
                )
    return relations


def _build_adjacency(relations: List[StockRelation]) -> Dict[str, List[Tuple[str, float, StockRelation]]]:
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]] = defaultdict(list)
    for relation in relations:
        adjacency[relation.source_code].append((relation.target_code, relation.weight, relation))
        adjacency[relation.target_code].append((relation.source_code, relation.weight, relation))
    return adjacency


def _normalize_scores(scores: Dict[str, float]) -> Dict[str, float]:
    if not scores:
        return {}
    min_score = min(scores.values())
    max_score = max(scores.values())
    if max_score == min_score:
        return {code: 1.0 for code in scores}
    return {code: (score - min_score) / (max_score - min_score) for code, score in scores.items()}


def _relation_score(
    code: str,
    base_scores: Dict[str, float],
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]],
    seed_codes: Set[str],
    max_depth: int,
) -> float:
    if code not in adjacency:
        return 0.0

    weighted_sum = 0.0
    total_weight = 0.0
    queue = deque([(code, 0, 1.0)])
    visited = {code}

    while queue:
        current, depth, path_weight = queue.popleft()
        if depth >= max_depth:
            continue
        for neighbor, edge_weight, _relation in adjacency.get(current, []):
            if neighbor in visited:
                continue
            visited.add(neighbor)
            propagated_weight = path_weight * edge_weight
            if neighbor in base_scores:
                distance_discount = 1 / (depth + 1)
                weighted_sum += base_scores[neighbor] * propagated_weight * distance_discount
                total_weight += propagated_weight * distance_discount
            queue.append((neighbor, depth + 1, propagated_weight))

    score = weighted_sum / total_weight if total_weight else 0.0

    if seed_codes:
        proximity = _seed_proximity(code, seed_codes, adjacency, max_depth)
        score = min(1.0, score + proximity * 0.25)
    return score


def _seed_proximity(
    code: str,
    seed_codes: Set[str],
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]],
    max_depth: int,
) -> float:
    if code in seed_codes:
        return 1.0

    queue = deque([(code, 0)])
    visited = {code}
    while queue:
        current, depth = queue.popleft()
        if depth >= max_depth:
            continue
        for neighbor, _edge_weight, _relation in adjacency.get(current, []):
            if neighbor in visited:
                continue
            if neighbor in seed_codes:
                return max(0.0, (max_depth - depth) / max_depth)
            visited.add(neighbor)
            queue.append((neighbor, depth + 1))
    return 0.0


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
