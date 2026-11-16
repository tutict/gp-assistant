use crate::data_source::{CoreDataSet, MarketDataSource, StaticDataSource};
use crate::demo_data::{mock_stocks, MockDataSource};
use crate::error::CoreResult;
use crate::market_data::StockItem;
use crate::screening::classification::{
    board_group_for_stock, board_rank, concept_rank, concept_sector_group_for_stock,
};
use crate::screening::criteria::{deducted_profit_rule_notes, matches_stock};
use crate::screening::grouping::{
    primary_screen_items, screen_result_groups, sort_screened, POTENTIAL_SCORE_THRESHOLD,
};
use crate::screening::scoring::{apply_industry_relative_scores, score_stock};
use crate::screening::types::{
    ScreenCriteria, ScreenResult, ScreenedStock, SectorScreenGroup, SectorScreenRequest,
    SectorScreenResult,
};
use std::collections::HashMap;

pub fn screen_with_mock(criteria: &ScreenCriteria) -> ScreenResult {
    screen_with_source(&MockDataSource, criteria).expect("mock data source should be valid")
}

pub fn sector_screen_with_mock(request: &SectorScreenRequest) -> SectorScreenResult {
    let universe = mock_stocks();
    sector_screen_stocks(&universe, request)
}

pub fn screen_with_data(data: &CoreDataSet, criteria: &ScreenCriteria) -> CoreResult<ScreenResult> {
    let source = StaticDataSource::new(data);
    screen_with_source(&source, criteria)
}

pub fn screen_with_source(
    source: &impl MarketDataSource,
    criteria: &ScreenCriteria,
) -> CoreResult<ScreenResult> {
    let universe = source.stocks()?;
    Ok(screen_stocks(&universe, criteria))
}

pub fn sector_screen_with_data(
    data: &CoreDataSet,
    request: &SectorScreenRequest,
) -> CoreResult<SectorScreenResult> {
    let source = StaticDataSource::new(data);
    let universe = source.stocks()?;
    Ok(sector_screen_stocks(&universe, request))
}

pub fn sector_screen_stocks(
    universe: &[StockItem],
    request: &SectorScreenRequest,
) -> SectorScreenResult {
    let max_sectors = request.max_sectors.clamp(1, 50);
    let per_sector_limit = request.per_sector_limit.clamp(1, 50);
    let min_sector_candidates = request.min_sector_candidates.clamp(1, 500);
    let group_by = request.group_by.trim().to_ascii_lowercase();
    let board_mode = matches!(group_by.as_str(), "board" | "market" | "market_board");
    let mut screened = Vec::new();
    let mut notes = deducted_profit_rule_notes(universe, &request.criteria);

    for stock in universe {
        let Some(reasons) = matches_stock(stock, &request.criteria) else {
            continue;
        };
        screened.push(score_stock(
            stock,
            &reasons,
            &request.criteria.score_profile,
        ));
    }
    sort_screened(&mut screened, &request.criteria);

    let mut by_sector: HashMap<String, Vec<ScreenedStock>> = HashMap::new();
    for item in &screened {
        let sector = if board_mode {
            board_group_for_stock(&item.stock).to_string()
        } else {
            concept_sector_group_for_stock(&item.stock)
        };
        by_sector.entry(sector).or_default().push(item.clone());
    }

    let omitted_small_sector_count = by_sector
        .values()
        .filter(|items| items.len() < min_sector_candidates)
        .count();
    let omitted_small_stock_count = by_sector
        .values()
        .filter(|items| items.len() < min_sector_candidates)
        .map(Vec::len)
        .sum::<usize>();

    let mut groups: Vec<SectorScreenGroup> = by_sector
        .into_iter()
        .filter(|(_, items)| items.len() >= min_sector_candidates)
        .map(|(sector, mut items)| {
            let total = items.len();
            items.truncate(per_sector_limit);
            let returned = items.len();
            let average_score = if returned == 0 {
                0.0
            } else {
                items.iter().map(|item| item.score).sum::<f64>() / returned as f64
            };
            SectorScreenGroup {
                sector,
                total,
                returned,
                average_score: (average_score * 1_000_000.0).round() / 1_000_000.0,
                items,
            }
        })
        .collect();
    groups.sort_by(|left, right| {
        let left_rank = if board_mode {
            board_rank(&left.sector)
        } else {
            concept_rank(&left.sector)
        };
        let right_rank = if board_mode {
            board_rank(&right.sector)
        } else {
            concept_rank(&right.sector)
        };
        left_rank
            .cmp(&right_rank)
            .then_with(|| right.average_score.total_cmp(&left.average_score))
            .then_with(|| right.total.cmp(&left.total))
            .then_with(|| left.sector.cmp(&right.sector))
    });
    groups.truncate(max_sectors);
    let returned = groups.iter().map(|group| group.returned).sum();
    if request
        .criteria
        .industry
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        notes.push(format!(
            "{}{}",
            "\u{5df2}\u{9650}\u{5b9a}\u{884c}\u{4e1a}\u{ff1a}",
            request.criteria.industry.as_deref().unwrap_or_default()
        ));
    }
    if omitted_small_sector_count > 0 {
        notes.push(format!(
            "已隐藏 {omitted_small_sector_count} 个样本不足的行业分组（共 {omitted_small_stock_count} 只股票）；分组门槛为 {min_sector_candidates} 只。"
        ));
    }
    if board_mode {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    } else {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }
    SectorScreenResult {
        total: screened.len(),
        returned,
        sector_count: groups.len(),
        groups,
        notes,
    }
}

pub fn screen_stocks(universe: &[StockItem], criteria: &ScreenCriteria) -> ScreenResult {
    let mut screened = Vec::new();
    let notes = deducted_profit_rule_notes(universe, criteria);

    for stock in universe {
        let Some(reasons) = matches_stock(stock, criteria) else {
            continue;
        };
        screened.push(score_stock(stock, &reasons, &criteria.score_profile));
    }

    apply_industry_relative_scores(&mut screened, &criteria.score_profile);
    sort_screened(&mut screened, criteria);

    let limit = criteria.limit.clamp(1, 200);
    let (items, promoted) = primary_screen_items(&screened, criteria, limit);
    let groups = screen_result_groups(&screened);
    let mut notes = notes;
    if promoted {
        notes.push("基准为候选池买入持有等权曲线，用于衡量调仓与成本后的超额收益。".to_string());
    }
    notes.push(format!(
        "普通筛选已拆成热门股和综合分大于 {} 的潜力股，每类最多 10 只。",
        POTENTIAL_SCORE_THRESHOLD
    ));
    ScreenResult {
        total: screened.len(),
        returned: items.len(),
        items,
        groups,
        notes,
    }
}

pub(crate) fn screen_candidate_pool(
    universe: &[StockItem],
    criteria: &ScreenCriteria,
    requested_limit: usize,
) -> Vec<ScreenedStock> {
    let pool_size = 200.min((requested_limit * 5).max(criteria.limit).max(50));
    let mut expanded_criteria = criteria.clone();
    expanded_criteria.limit = pool_size;
    screen_stocks(universe, &expanded_criteria).items
}
