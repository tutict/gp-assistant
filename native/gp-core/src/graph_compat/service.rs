use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::demo_data::MockDataSource;
use crate::error::CoreResult;
use crate::graph_compat::types::{
    GraphCenterContext, GraphScreenRequest, GraphScreenResult, GraphStockSignal,
};
use crate::market_data::{StockItem, StockRelation};
use crate::numeric::round6;
use crate::screening::service::screen_stocks;
use crate::screening::types::SelectionExplanation;

pub fn graph_screen_with_mock(request: &GraphScreenRequest) -> GraphScreenResult {
    graph_screen_with_source(&MockDataSource, request).expect("mock data source should be valid")
}

pub fn graph_screen_with_data(
    data: &CoreDataSet,
    request: &GraphScreenRequest,
) -> CoreResult<GraphScreenResult> {
    let source = StaticDataSource::new(data);
    graph_screen_with_source(&source, request)
}

pub fn graph_screen_with_source(
    source: &impl MarketDataSource,
    request: &GraphScreenRequest,
) -> CoreResult<GraphScreenResult> {
    let universe = source.stocks()?;
    let relations = source.relations()?;
    Ok(graph_screen_stocks(&universe, &relations, request))
}

pub fn graph_screen_stocks(
    universe: &[StockItem],
    _provider_relations: &[StockRelation],
    request: &GraphScreenRequest,
) -> GraphScreenResult {
    let mut criteria = request.criteria.clone();
    criteria.limit = request.limit.clamp(1, 100);
    let screen_result = screen_stocks(universe, &criteria);
    let center_context = GraphCenterContext {
        mode: "custom_criteria".to_string(),
        label: "自定义筛选".to_string(),
        codes: Vec::new(),
    };

    let mut signals: Vec<GraphStockSignal> = screen_result
        .items
        .into_iter()
        .map(|screened| {
            let score = round6(screened.score);
            GraphStockSignal {
                stock: screened.stock,
                base_score: score,
                relation_score: 0.0,
                final_score: score,
                suggested_weight: 0.0,
                reasons: screened.reasons,
                related: Vec::new(),
                explanation: SelectionExplanation {
                    basis: vec![screened.score_explanation],
                    ..SelectionExplanation::default()
                },
            }
        })
        .collect();
    signals.sort_by(|left, right| right.final_score.total_cmp(&left.final_score));
    signals.truncate(request.limit.clamp(1, 100));
    assign_weights(&mut signals);

    let mut notes = vec![
        "自定义筛选使用页面筛选条件，不再依赖中心股票或关系传播。".to_string(),
        "旧关系图入口已降级为自定义筛选兼容结果，relation_count 固定为 0。".to_string(),
    ];
    notes.extend(screen_result.notes);

    GraphScreenResult {
        total: screen_result.total,
        returned: signals.len(),
        relation_count: 0,
        items: signals,
        center_context,
        notes,
    }
}

fn assign_weights(signals: &mut [GraphStockSignal]) {
    if signals.is_empty() {
        return;
    }

    let positive_sum: f64 = signals.iter().map(|item| item.final_score.max(0.0)).sum();
    if positive_sum <= 0.0 {
        let equal_weight = round6(1.0 / signals.len() as f64);
        for item in signals {
            item.suggested_weight = equal_weight;
        }
        return;
    }

    for item in signals {
        item.suggested_weight = round6(item.final_score.max(0.0) / positive_sum);
    }
}
