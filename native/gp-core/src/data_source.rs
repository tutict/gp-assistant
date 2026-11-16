use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::capital_evidence::CapitalEvidenceResult;
use crate::dates::parse_date;
use crate::error::{CoreError, CoreResult};
use crate::market_data::{
    normalize_stock_code, HistoryBar, StockFactorSnapshot, StockFinancialSnapshot, StockItem,
    StockRelation,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoreDataSet {
    #[serde(default)]
    pub stocks: Vec<StockItem>,
    #[serde(default)]
    pub relations: Vec<StockRelation>,
    #[serde(default)]
    pub histories: HashMap<String, Vec<HistoryBar>>,
    #[serde(default)]
    pub financials: HashMap<String, StockFinancialSnapshot>,
    #[serde(default)]
    pub factor_snapshots: HashMap<String, Vec<StockFactorSnapshot>>,
    #[serde(default)]
    pub capital_evidence: HashMap<String, CapitalEvidenceResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceSummary {
    pub stock_count: usize,
    pub relation_count: usize,
    pub history_symbol_count: usize,
    pub history_bar_count: usize,
    #[serde(default)]
    pub factor_snapshot_symbol_count: usize,
    #[serde(default)]
    pub factor_snapshot_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryPoint {
    pub(crate) date: NaiveDate,
    pub(crate) close: f64,
}

pub trait MarketDataSource {
    fn stocks(&self) -> CoreResult<&[StockItem]>;

    fn relations(&self) -> CoreResult<&[StockRelation]> {
        Ok(&[])
    }

    fn get_history(
        &self,
        code: &str,
        start_date: &str,
        end_date: &str,
    ) -> CoreResult<Vec<HistoryBar>>;

    fn get_financial_snapshot(&self, _code: &str) -> Option<StockFinancialSnapshot> {
        None
    }

    fn get_factor_snapshots(&self, _code: &str) -> CoreResult<Vec<StockFactorSnapshot>> {
        Ok(Vec::new())
    }

    fn factor_snapshot_codes(&self) -> CoreResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn get_capital_evidence(&self, _code: &str) -> Option<CapitalEvidenceResult> {
        None
    }
}

pub struct StaticDataSource<'a> {
    data: &'a CoreDataSet,
    stock_override: Option<&'a [StockItem]>,
    history_override: Option<&'a HashMap<String, Vec<HistoryBar>>>,
}

impl<'a> StaticDataSource<'a> {
    pub fn new(data: &'a CoreDataSet) -> Self {
        Self {
            data,
            stock_override: None,
            history_override: None,
        }
    }

    pub fn with_stocks(data: &'a CoreDataSet, stocks: &'a [StockItem]) -> Self {
        Self {
            data,
            stock_override: Some(stocks),
            history_override: None,
        }
    }

    pub fn with_overrides(
        data: &'a CoreDataSet,
        stocks: Option<&'a [StockItem]>,
        histories: Option<&'a HashMap<String, Vec<HistoryBar>>>,
    ) -> Self {
        Self {
            data,
            stock_override: stocks,
            history_override: histories,
        }
    }
}

impl MarketDataSource for StaticDataSource<'_> {
    fn stocks(&self) -> CoreResult<&[StockItem]> {
        Ok(self.stock_override.unwrap_or(&self.data.stocks))
    }

    fn relations(&self) -> CoreResult<&[StockRelation]> {
        Ok(&self.data.relations)
    }

    fn get_history(
        &self,
        code: &str,
        start_date: &str,
        end_date: &str,
    ) -> CoreResult<Vec<HistoryBar>> {
        let normalized = normalize_stock_code(code);
        if let Some(history) = self
            .history_override
            .and_then(|histories| histories.get(code).or_else(|| histories.get(&normalized)))
        {
            return history_bars_in_range(history, start_date, end_date);
        }
        let Some(history) = self
            .data
            .histories
            .get(code)
            .or_else(|| self.data.histories.get(&normalized))
        else {
            return Ok(Vec::new());
        };
        history_bars_in_range(history, start_date, end_date)
    }

    fn get_financial_snapshot(&self, code: &str) -> Option<StockFinancialSnapshot> {
        let normalized = normalize_stock_code(code);
        self.data
            .financials
            .get(code)
            .cloned()
            .or_else(|| self.data.financials.get(&normalized).cloned())
    }

    fn get_factor_snapshots(&self, code: &str) -> CoreResult<Vec<StockFactorSnapshot>> {
        let normalized = normalize_stock_code(code);
        Ok(self
            .data
            .factor_snapshots
            .get(code)
            .cloned()
            .or_else(|| self.data.factor_snapshots.get(&normalized).cloned())
            .unwrap_or_default())
    }

    fn factor_snapshot_codes(&self) -> CoreResult<Vec<String>> {
        let mut codes = self
            .data
            .factor_snapshots
            .keys()
            .map(|code| normalize_stock_code(code))
            .collect::<Vec<_>>();
        codes.sort();
        codes.dedup();
        Ok(codes)
    }

    fn get_capital_evidence(&self, code: &str) -> Option<CapitalEvidenceResult> {
        let normalized = normalize_stock_code(code);
        self.data
            .capital_evidence
            .get(code)
            .cloned()
            .or_else(|| self.data.capital_evidence.get(&normalized).cloned())
    }
}

pub fn validate_data_set(data: &CoreDataSet) -> CoreResult<DataSourceSummary> {
    let mut warnings = Vec::new();
    let mut stock_codes = HashSet::new();
    for stock in &data.stocks {
        if stock.code.trim().is_empty() {
            return Err(CoreError::new("Stock code cannot be empty"));
        }
        if !stock_codes.insert(stock.code.clone()) {
            return Err(CoreError::new(format!(
                "Duplicate stock code: {}",
                stock.code
            )));
        }
    }

    for relation in &data.relations {
        if !stock_codes.contains(&relation.source_code) {
            warnings.push(format!(
                "Relation source code is not in stock universe: {}",
                relation.source_code
            ));
        }
        if !stock_codes.contains(&relation.target_code) {
            warnings.push(format!(
                "Relation target code is not in stock universe: {}",
                relation.target_code
            ));
        }
        if !(0.0..=1.0).contains(&relation.weight) {
            return Err(CoreError::new(format!(
                "Relation weight must be within [0, 1]: {} -> {}",
                relation.source_code, relation.target_code
            )));
        }
    }

    let mut history_bar_count = 0;
    for (code, bars) in &data.histories {
        if !stock_codes.contains(code) {
            warnings.push(format!("History code is not in stock universe: {code}"));
        }
        let mut previous_date = None;
        for bar in bars {
            let date = parse_date(&bar.date)?;
            if bar.close < 0.0 {
                return Err(CoreError::new(format!(
                    "History close cannot be negative: {code} {}",
                    bar.date
                )));
            }
            for (label, value) in [
                ("open", bar.open),
                ("high", bar.high),
                ("low", bar.low),
                ("volume", bar.volume),
                ("capital", bar.capital),
            ] {
                if value.map(|value| value < 0.0).unwrap_or(false) {
                    return Err(CoreError::new(format!(
                        "History {label} cannot be negative: {code} {}",
                        bar.date
                    )));
                }
            }
            if previous_date
                .map(|previous| date < previous)
                .unwrap_or(false)
            {
                warnings.push(format!("History is not sorted by date: {code}"));
            }
            previous_date = Some(date);
            history_bar_count += 1;
        }
    }

    Ok(DataSourceSummary {
        stock_count: data.stocks.len(),
        relation_count: data.relations.len(),
        history_symbol_count: data.histories.len(),
        history_bar_count,
        factor_snapshot_symbol_count: data.factor_snapshots.len(),
        factor_snapshot_count: data.factor_snapshots.values().map(Vec::len).sum(),
        warnings,
    })
}

fn history_bars_in_range(
    history: &[HistoryBar],
    start_date: &str,
    end_date: &str,
) -> CoreResult<Vec<HistoryBar>> {
    let start = parse_date(start_date)?;
    let end = parse_date(end_date)?;
    if start > end {
        return Ok(Vec::new());
    }

    let mut filtered = Vec::new();
    for bar in history {
        let date = parse_date(&bar.date)?;
        if date >= start && date <= end {
            filtered.push(bar.clone());
        }
    }
    filtered.sort_by(|left, right| {
        parse_date(&left.date)
            .unwrap_or(NaiveDate::MIN)
            .cmp(&parse_date(&right.date).unwrap_or(NaiveDate::MIN))
    });
    Ok(filtered)
}

pub(crate) fn history_points_from_bars(history: &[HistoryBar]) -> CoreResult<Vec<HistoryPoint>> {
    let mut points = Vec::with_capacity(history.len());
    for bar in history {
        points.push(HistoryPoint {
            date: parse_date(&bar.date)?,
            close: bar.close,
        });
    }
    points.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(points)
}
