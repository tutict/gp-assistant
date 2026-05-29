from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Dict, Iterable, List, Sequence, Tuple

import numpy as np

from app.schemas import ScreenedStock, StockItem, StockRelation


RELATION_TYPE_PRIORS = {
    "industry_peer": 1.0,
    "valuation_peer": 0.84,
    "size_peer": 0.72,
    "supply_chain": 1.0,
    "thematic": 0.78,
    "manufacturing_chain": 0.74,
    "upstream_material": 0.76,
}

GRAPH_NOTES = [
    "知识图谱会合并数据源关系，并补充行业、估值和市值相近关系。",
    "关系分融合个性化传播、两层图消息传递和局部邻域相关性。",
]


@dataclass
class KnowledgeGraph:
    nodes: Dict[str, StockItem]
    relations: List[StockRelation]
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]]
    node_index: Dict[str, int]
    features: np.ndarray
    embeddings: np.ndarray
    stats: Dict[str, object]


def build_knowledge_graph(
    universe: Sequence[StockItem],
    provider_relations: Sequence[StockRelation],
) -> KnowledgeGraph:
    nodes = {stock.code: stock for stock in universe}
    relations = _merge_relations(
        [
            *provider_relations,
            *_infer_industry_relations(universe),
            *_infer_valuation_relations(universe),
            *_infer_size_relations(universe),
        ]
    )
    adjacency = _build_adjacency(relations)
    node_index = {code: index for index, code in enumerate(nodes)}
    features = _build_feature_matrix(list(nodes.values()))
    embeddings = _message_passing_embeddings(node_index, adjacency, features)
    relation_types = sorted({relation.relation_type for relation in relations})
    stats = {
        "node_count": len(nodes),
        "relation_count": len(relations),
        "relation_types": relation_types,
    }
    return KnowledgeGraph(
        nodes=nodes,
        relations=relations,
        adjacency=adjacency,
        node_index=node_index,
        features=features,
        embeddings=embeddings,
        stats=stats,
    )


def score_candidates(
    graph: KnowledgeGraph,
    screened: Dict[str, ScreenedStock],
    seed_codes: Sequence[str],
    max_depth: int,
) -> Dict[str, float]:
    if not screened:
        return {}

    candidate_codes = list(screened.keys())
    base_scores = _normalize({code: screened[code].score for code in candidate_codes})
    prior = _personalized_prior(graph, base_scores, seed_codes)
    pagerank_scores = _personalized_pagerank(graph, prior)
    prototype = _prototype_embedding(graph, base_scores, seed_codes)
    similarity_scores = {
        code: _cosine_similarity(graph.embeddings[graph.node_index[code]], prototype)
        for code in candidate_codes
        if code in graph.node_index
    }
    local_scores = {
        code: _local_relevance(graph, code, base_scores, set(seed_codes), max_depth)
        for code in candidate_codes
        if code in graph.node_index
    }

    pagerank_norm = _normalize(pagerank_scores)
    similarity_norm = _normalize(similarity_scores)
    local_norm = _normalize(local_scores)

    relation_scores: Dict[str, float] = {}
    for code in candidate_codes:
        score = (
            0.45 * pagerank_norm.get(code, 0.0)
            + 0.30 * similarity_norm.get(code, 0.0)
            + 0.25 * local_norm.get(code, 0.0)
        )
        relation_scores[code] = round(min(max(score, 0.0), 1.0), 6)
    return relation_scores


def graph_notes(graph: KnowledgeGraph) -> List[str]:
    relation_types = ", ".join(graph.stats.get("relation_types", []))
    return [
        *GRAPH_NOTES,
        f"关系图包含 {graph.stats.get('node_count', 0)} 个节点、{graph.stats.get('relation_count', 0)} 条关系边。",
        f"关系类型：{relation_types or '暂无'}。",
    ]


def _infer_industry_relations(universe: Sequence[StockItem]) -> List[StockRelation]:
    by_industry: Dict[str, List[StockItem]] = {}
    for stock in universe:
        if stock.industry and stock.industry != "未知行业":
            by_industry.setdefault(stock.industry, []).append(stock)

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
                        description=f"同行业：{industry}",
                    )
                )
    return relations


def _infer_valuation_relations(universe: Sequence[StockItem]) -> List[StockRelation]:
    sample = sorted(
        [stock for stock in universe if stock.price > 0],
        key=lambda item: item.market_cap_billion or 0,
        reverse=True,
    )[:80]
    relations: List[StockRelation] = []
    for index, source in enumerate(sample):
        scored_targets: List[Tuple[float, StockItem]] = []
        for target in sample[index + 1 :]:
            score = _valuation_similarity(source, target)
            if score >= 0.52:
                scored_targets.append((score, target))
        scored_targets.sort(key=lambda item: item[0], reverse=True)
        for score, target in scored_targets[:3]:
            relations.append(
                StockRelation(
                    source_code=source.code,
                    target_code=target.code,
                    relation_type="valuation_peer",
                    weight=round(0.28 + 0.58 * score, 6),
                    description="Similar valuation/style profile.",
                )
            )
    return relations


def _infer_size_relations(universe: Sequence[StockItem]) -> List[StockRelation]:
    sample = sorted(
        [stock for stock in universe if stock.market_cap_billion is not None],
        key=lambda item: item.market_cap_billion or 0,
        reverse=True,
    )[:60]
    relations: List[StockRelation] = []
    for index, source in enumerate(sample):
        scored_targets: List[Tuple[float, StockItem]] = []
        for target in sample[index + 1 :]:
            size_score = _size_similarity(source, target)
            if size_score >= 0.58:
                scored_targets.append((size_score, target))
        scored_targets.sort(key=lambda item: item[0], reverse=True)
        for score, target in scored_targets[:2]:
            if source.industry == target.industry:
                continue
            relations.append(
                StockRelation(
                    source_code=source.code,
                    target_code=target.code,
                    relation_type="size_peer",
                    weight=round(0.24 + 0.42 * score, 6),
                    description="Comparable market-cap scale.",
                )
            )
    return relations


def _valuation_similarity(left: StockItem, right: StockItem) -> float:
    parts = []
    if left.pe and right.pe and left.pe > 0 and right.pe > 0:
        parts.append(abs(math.log1p(left.pe) - math.log1p(right.pe)) * 0.85)
    if left.pb and right.pb and left.pb > 0 and right.pb > 0:
        parts.append(abs(math.log1p(left.pb) - math.log1p(right.pb)) * 0.75)
    if left.roe is not None and right.roe is not None:
        parts.append(abs(left.roe - right.roe) * 3.2)
    if left.price > 0 and right.price > 0:
        parts.append(abs(math.log1p(left.price) - math.log1p(right.price)) * 0.25)
    if left.market_cap_billion and right.market_cap_billion and left.market_cap_billion > 0 and right.market_cap_billion > 0:
        parts.append(
            abs(
                math.log1p(left.market_cap_billion)
                - math.log1p(right.market_cap_billion)
            )
            * 0.35
        )
    if not parts:
        return 0.0
    distance = sum(parts) / len(parts)
    return 1 / (1 + distance)


def _size_similarity(left: StockItem, right: StockItem) -> float:
    if not left.market_cap_billion or not right.market_cap_billion:
        return 0.0
    distance = abs(
        math.log1p(left.market_cap_billion) - math.log1p(right.market_cap_billion)
    )
    return 1 / (1 + distance * 0.9)


def _build_feature_matrix(stocks: Sequence[StockItem]) -> np.ndarray:
    numeric_rows = []
    industries = sorted({stock.industry or "未知行业" for stock in stocks})
    industry_index = {industry: index for index, industry in enumerate(industries)}

    for stock in stocks:
        numeric_rows.append(
            [
                math.log1p(max(stock.price, 0.0)),
                math.log1p(max(stock.pe or 0.0, 0.0)),
                math.log1p(max(stock.pb or 0.0, 0.0)),
                stock.roe or 0.0,
                math.log1p(max(stock.market_cap_billion or 0.0, 0.0)),
                stock.dividend_yield or 0.0,
                1.0 if stock.is_st else 0.0,
            ]
        )

    numeric = np.asarray(numeric_rows, dtype=float)
    if numeric.size == 0:
        return numeric

    means = numeric.mean(axis=0)
    stds = numeric.std(axis=0)
    stds[stds == 0] = 1.0
    numeric = (numeric - means) / stds

    categorical = np.zeros((len(stocks), len(industries)), dtype=float)
    for row, stock in enumerate(stocks):
        categorical[row, industry_index[stock.industry or "未知行业"]] = 1.0

    return np.concatenate([numeric, categorical], axis=1)


def _message_passing_embeddings(
    node_index: Dict[str, int],
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]],
    features: np.ndarray,
) -> np.ndarray:
    if features.size == 0:
        return features

    embeddings = features.copy()
    for _ in range(2):
        aggregated = embeddings.copy()
        for code, neighbors in adjacency.items():
            index = node_index.get(code)
            if index is None or not neighbors:
                continue
            total = 0.0
            weighted = np.zeros(embeddings.shape[1], dtype=float)
            for neighbor, weight, relation in neighbors:
                neighbor_index = node_index.get(neighbor)
                if neighbor_index is None:
                    continue
                scaled_weight = weight * _relation_type_multiplier(relation.relation_type)
                weighted += embeddings[neighbor_index] * scaled_weight
                total += scaled_weight
            if total > 0:
                aggregated[index] = 0.62 * embeddings[index] + 0.38 * (weighted / total)
        embeddings = np.tanh(aggregated)
    return embeddings


def _personalized_prior(
    graph: KnowledgeGraph,
    base_scores: Dict[str, float],
    seed_codes: Sequence[str],
) -> Dict[str, float]:
    prior = {code: base_scores.get(code, 0.0) for code in graph.nodes}
    if seed_codes:
        for seed in seed_codes:
            if seed in prior:
                prior[seed] += 0.5
    total = sum(prior.values())
    if total <= 0:
        count = max(len(prior), 1)
        return {code: 1 / count for code in prior}
    return {code: value / total for code, value in prior.items()}


def _personalized_pagerank(
    graph: KnowledgeGraph,
    prior: Dict[str, float],
    iterations: int = 18,
    damping: float = 0.82,
) -> Dict[str, float]:
    rank = prior.copy()
    for _ in range(iterations):
        updated = {code: damping * prior.get(code, 0.0) for code in graph.nodes}
        for code, neighbors in graph.adjacency.items():
            total_weight = sum(weight * _relation_type_multiplier(relation.relation_type) for _, weight, relation in neighbors)
            if total_weight <= 0:
                continue
            source_rank = rank.get(code, 0.0)
            if source_rank <= 0:
                continue
            contribution = (1 - damping) * source_rank
            for neighbor, weight, relation in neighbors:
                scaled_weight = weight * _relation_type_multiplier(relation.relation_type)
                updated[neighbor] = updated.get(neighbor, 0.0) + contribution * scaled_weight / total_weight
        rank = updated
    return rank


def _prototype_embedding(
    graph: KnowledgeGraph,
    base_scores: Dict[str, float],
    seed_codes: Sequence[str],
) -> np.ndarray:
    vectors: List[np.ndarray] = []
    weights: List[float] = []

    if seed_codes:
        for seed in seed_codes:
            index = graph.node_index.get(seed)
            if index is None:
                continue
            vectors.append(graph.embeddings[index])
            weights.append(1.0)

    if not vectors:
        ranked = sorted(base_scores.items(), key=lambda item: item[1], reverse=True)[:5]
        for code, score in ranked:
            index = graph.node_index.get(code)
            if index is None:
                continue
            vectors.append(graph.embeddings[index])
            weights.append(max(score, 0.05))

    if not vectors:
        return np.zeros(graph.embeddings.shape[1], dtype=float)

    stacked = np.stack(vectors, axis=0)
    weights_array = np.asarray(weights, dtype=float)
    total = weights_array.sum()
    if total <= 0:
        return stacked.mean(axis=0)
    return (stacked * (weights_array / total)[:, None]).sum(axis=0)


def _cosine_similarity(left: np.ndarray, right: np.ndarray) -> float:
    if left.size == 0 or right.size == 0:
        return 0.0
    left_norm = np.linalg.norm(left)
    right_norm = np.linalg.norm(right)
    if left_norm <= 0 or right_norm <= 0:
        return 0.0
    return float(np.dot(left, right) / (left_norm * right_norm))


def _local_relevance(
    graph: KnowledgeGraph,
    code: str,
    base_scores: Dict[str, float],
    seed_codes: set[str],
    max_depth: int,
) -> float:
    if code not in graph.adjacency:
        return 0.0

    queue: List[Tuple[str, int, float]] = [(code, 0, 1.0)]
    visited = {code}
    weighted_sum = 0.0
    total_weight = 0.0

    while queue:
        current, depth, path_weight = queue.pop(0)
        if depth >= max_depth:
            continue
        for neighbor, edge_weight, relation in graph.adjacency.get(current, []):
            if neighbor in visited:
                continue
            visited.add(neighbor)
            relation_weight = edge_weight * _relation_type_multiplier(relation.relation_type)
            propagated = path_weight * relation_weight
            if neighbor in base_scores:
                distance_discount = 1 / (depth + 1)
                weighted_sum += base_scores[neighbor] * propagated * distance_discount
                total_weight += propagated * distance_discount
            queue.append((neighbor, depth + 1, propagated))

    score = weighted_sum / total_weight if total_weight else 0.0
    if seed_codes:
        score += _seed_proximity(code, seed_codes, graph.adjacency, max_depth) * 0.25
    return min(max(score, 0.0), 1.0)


def _seed_proximity(
    code: str,
    seed_codes: set[str],
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]],
    max_depth: int,
) -> float:
    if code in seed_codes:
        return 1.0

    queue: List[Tuple[str, int]] = [(code, 0)]
    visited = {code}
    while queue:
        current, depth = queue.pop(0)
        if depth >= max_depth:
            continue
        for neighbor, _weight, _relation in adjacency.get(current, []):
            if neighbor in visited:
                continue
            if neighbor in seed_codes:
                return max(0.0, (max_depth - depth) / max_depth)
            visited.add(neighbor)
            queue.append((neighbor, depth + 1))
    return 0.0


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


def _build_adjacency(
    relations: Sequence[StockRelation],
) -> Dict[str, List[Tuple[str, float, StockRelation]]]:
    adjacency: Dict[str, List[Tuple[str, float, StockRelation]]] = {}
    for relation in relations:
        adjacency.setdefault(relation.source_code, []).append(
            (relation.target_code, relation.weight, relation)
        )
        adjacency.setdefault(relation.target_code, []).append(
            (relation.source_code, relation.weight, relation)
        )
    return adjacency


def _relation_type_multiplier(relation_type: str) -> float:
    return RELATION_TYPE_PRIORS.get(relation_type, 0.62)


def _normalize(values: Dict[str, float]) -> Dict[str, float]:
    if not values:
        return {}
    min_value = min(values.values())
    max_value = max(values.values())
    if max_value == min_value:
        value = 1.0 if max_value > 0 else 0.0
        return {key: value for key in values}
    return {key: (value - min_value) / (max_value - min_value) for key, value in values.items()}
