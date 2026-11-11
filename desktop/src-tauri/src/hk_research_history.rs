use super::{collect_adaptive_history_results, local_data, DataPolicy};
use crate::hk_market::{
    merge_hk_history_record as merge_record, HkFetchedHistory, HkHistoryPriceAdjustment,
    HkHistoryRecord, HkHistoryRequestRange, HkMarketCache,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub(super) struct PreparedHistory {
    pub cache: HkMarketCache,
    pub data: stock_optimizer_core::CoreDataSet,
    pub summary: Value,
    pub coverage: Vec<Value>,
    fetched_records: HashMap<String, HkHistoryRecord>,
    fetch_errors: HashMap<String, String>,
    attempted_at: u128,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct DatedHistoryFetch {
    fetched: HkFetchedHistory,
    started_at: String,
}

fn date_key(raw: &str) -> Result<String, String> {
    let key =
        crate::market::compact_date_key(raw).ok_or_else(|| format!("无效港股历史日期：{raw}"))?;
    chrono::NaiveDate::parse_from_str(&key, "%Y%m%d").map_err(|error| error.to_string())?;
    Ok(key)
}

fn expected_end(end: &str) -> Result<String, String> {
    let key = date_key(end)?;
    let day =
        chrono::NaiveDate::parse_from_str(&key, "%Y%m%d").map_err(|error| error.to_string())?;
    let epoch = day
        .and_hms_opt(12, 0, 0)
        .ok_or_else(|| "无效历史日期".to_owned())?
        .and_utc()
        .timestamp_millis();
    let trading = crate::market::expected_market_quote_date_from_epoch_ms(epoch.max(0) as u128)
        .unwrap_or(key);
    let current =
        crate::market::expected_market_quote_date_from_epoch_ms(crate::market::epoch_millis())
            .unwrap_or_else(|| trading.clone());
    Ok(trading.min(current))
}

fn range_covers(history: &HkHistoryRecord, start: &str, end: &str) -> bool {
    if history.request_range.is_some() || !history.request_ranges.is_empty() {
        return history
            .request_ranges
            .iter()
            .chain(history.request_range.iter())
            .any(|range| range.start_date.as_str() <= start && range.end_date.as_str() >= end);
    }
    let first = history
        .bars
        .iter()
        .filter_map(|bar| crate::market::compact_date_key(&bar.date))
        .min();
    let last = history
        .bars
        .iter()
        .filter_map(|bar| crate::market::compact_date_key(&bar.date))
        .max();
    first.is_some_and(|first| first.as_str() <= start)
        && last.is_some_and(|last| last.as_str() >= end)
}

fn coverage(
    cache: &HkMarketCache,
    codes: &[String],
    start: &str,
    end: &str,
    minimum: usize,
) -> Result<Vec<Value>, String> {
    let start = date_key(start)?;
    let end = date_key(end)?;
    if start > end {
        return Err("历史开始日期不能晚于结束日期".to_owned());
    }
    let target_end = expected_end(&end)?;
    Ok(codes
        .iter()
        .map(|code| {
            let history = cache.histories.get(code);
            let mut row = local_data::history_coverage(
                history
                    .into_iter()
                    .flat_map(|history| {
                        history.bars.iter().filter(|bar| {
                            crate::hk_market::hk_history_bar_is_prepared(history, bar)
                        })
                    })
                    .filter(|bar| bar.close.is_finite() && bar.close > 0.0)
                    .map(|bar| bar.date.as_str()),
                &start,
                &end,
                minimum,
            );
            row["code"] = json!(format!("{code}.HK"));
            let ready = history.is_some_and(|history| {
                (history.request_range.is_some() || !history.request_ranges.is_empty())
                    && range_covers(history, &start, &target_end)
            });
            if !ready {
                row["sufficient"] = json!(false);
                row["reason"] = json!("所选港股历史区间尚未补齐");
            }
            row["price_adjustment"] = history
                .map(|history| json!(history.price_adjustment))
                .unwrap_or(Value::Null);
            let quote_date = cache
                .quotes
                .get(code)
                .and_then(|quote| quote.quote_time.as_deref())
                .and_then(crate::market::compact_date_key)
                .filter(|date| date <= &end);
            if let Some(quote_date) = quote_date {
                let suspended =
                    history
                        .zip(cache.quotes.get(code))
                        .is_some_and(|(history, quote)| {
                            crate::hk_market::hk_history_suspension_matches_quote(history, quote)
                        });
                row["suspended"] = json!(suspended);
                if !suspended
                    && row["as_of"]
                        .as_str()
                        .is_none_or(|last| last < quote_date.as_str())
                {
                    row["sufficient"] = json!(false);
                    row["reason"] = json!("日线落后于本机港股行情");
                }
            }
            if row["sufficient"] != true {
                if let Some(reason) = history.and_then(|history| history.revision_reason.as_ref()) {
                    row["reason"] = json!(format!(
                        "{}：{reason}",
                        row["reason"].as_str().unwrap_or("历史区间未补齐")
                    ));
                }
                if let Some(error) = cache.history_errors.get(code) {
                    row["provider_error"] = json!(error);
                    row["reason"] = json!(format!(
                        "{}：{error}",
                        row["reason"].as_str().unwrap_or("历史区间未补齐")
                    ));
                }
            }
            row
        })
        .collect())
}

fn effective_fetch_range(
    cache: &HkMarketCache,
    code: &str,
    start: &str,
    end: &str,
) -> Result<HkHistoryRequestRange, String> {
    let mut start_date = date_key(start)?;
    let mut end_date = date_key(end)?;
    if let Some(history) = cache.histories.get(code) {
        for range in history
            .request_ranges
            .iter()
            .chain(history.request_range.iter())
        {
            if let Ok(first) = date_key(&range.start_date) {
                start_date = start_date.min(first);
            }
            if let Ok(last) = date_key(&range.end_date) {
                end_date = end_date.max(last);
            }
        }
        for bar in &history.bars {
            if let Ok(date) = date_key(&bar.date) {
                start_date = start_date.min(date.clone());
                end_date = end_date.max(date);
            }
        }
    }
    Ok(HkHistoryRequestRange {
        start_date,
        end_date,
    })
}

fn filter_calculation_histories(data: &mut stock_optimizer_core::CoreDataSet, coverage: &[Value]) {
    let usable = coverage
        .iter()
        .filter(|row| row["sufficient"] == true)
        .filter_map(|row| row["code"].as_str())
        .collect::<HashSet<_>>();
    data.histories
        .retain(|code, _| usable.contains(code.as_str()));
}

fn persist_prepared_updates(latest: &mut HkMarketCache, prepared: &PreparedHistory) {
    for (code, record) in &prepared.fetched_records {
        let merged = merge_record(latest.histories.get(code), record.clone());
        if merged.last_error.is_none() {
            latest.history_errors.remove(code);
        }
        latest.histories.insert(code.clone(), merged);
    }
    for (code, error) in &prepared.fetch_errors {
        let newer_success = latest.histories.get(code).is_some_and(|history| {
            history.fetched_at.parse::<u128>().unwrap_or(0) > prepared.attempted_at
                && history.last_error.is_none()
        });
        if !newer_success {
            latest.history_errors.insert(code.clone(), error.clone());
            if let Some(history) = latest.histories.get_mut(code) {
                history.last_error = Some(error.clone());
            }
        }
    }
}

async fn prepare_with_fetch<F, Fut>(
    mut cache: HkMarketCache,
    codes: Vec<String>,
    start: &str,
    end: &str,
    minimum: usize,
    policy: DataPolicy,
    fetcher: F,
) -> Result<PreparedHistory, String>
where
    F: Fn(&str, &str, &str) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<HkFetchedHistory, String>> + Send + 'static,
{
    let mut seen = HashSet::new();
    let codes = codes
        .into_iter()
        .map(|code| {
            crate::hk_market::canonical_hk_code(&code).ok_or_else(|| "无效港股历史代码".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|code| seen.insert(code.clone()))
        .collect::<Vec<_>>();
    for code in &codes {
        if !cache
            .securities
            .iter()
            .any(|security| security.code == *code && security.supports_hkd_research())
        {
            return Err(format!("{code} 不在可用港币股票范围内"));
        }
    }
    let before = coverage(&cache, &codes, start, end, minimum)?;
    let stock_snapshot = crate::hk_research::project_hk_market_data(&cache)?.stocks;
    if policy == DataPolicy::CacheOnly {
        local_data::require_coverage("hk_history", &before)?;
    }
    let now = crate::market::epoch_millis();
    let mut requested = Vec::new();
    if policy == DataPolicy::Refresh {
        for (code, row) in codes.iter().zip(&before) {
            let history = cache.histories.get(code);
            let fetched = history.and_then(|history| history.fetched_at.parse::<u128>().ok());
            let fresh =
                fetched.is_some_and(|fetched| now.saturating_sub(fetched) < 12 * 60 * 60 * 1000);
            let attempted_range = history.is_some_and(|history| {
                (history.request_range.is_some() || !history.request_ranges.is_empty())
                    && range_covers(
                        history,
                        &date_key(start).unwrap_or_default(),
                        &expected_end(end).unwrap_or_default(),
                    )
            });
            if row["sufficient"] != true
                && !(fresh
                    && attempted_range
                    && row["available_bars"].as_u64().unwrap_or(0) < minimum as u64
                    && row["as_of"]
                        .as_str()
                        .is_some_and(|last| last >= expected_end(end).unwrap_or_default().as_str())
                    && history.is_some_and(|history| history.last_error.is_none()))
            {
                requested.push(code.clone());
            }
        }
    }
    let fetch_ranges = requested
        .iter()
        .map(|code| {
            Ok((
                code.clone(),
                effective_fetch_range(&cache, code, start, end)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let network_ranges = fetch_ranges.clone();
    let outcome = collect_adaptive_history_results(
        requested.clone(),
        start.to_owned(),
        Duration::from_secs(45),
        12,
        move |code, _| {
            let range = network_ranges.get(code).unwrap();
            let started_at = crate::hk_market::current_epoch_millis();
            let fetched = fetcher(code, &range.start_date, &range.end_date);
            async move {
                let fetched = fetched.await?;
                Ok(vec![serde_json::to_value(DatedHistoryFetch {
                    fetched,
                    started_at,
                })
                .map_err(|error| error.to_string())?])
            }
        },
        |_, _| {},
    )
    .await;
    let mut updated = 0;
    let mut failures = HashMap::new();
    let mut completed = HashSet::new();
    let mut fetched_records = HashMap::new();
    for (code, result) in outcome.results {
        completed.insert(code.clone());
        match result {
            Ok(mut rows) => {
                if rows.len() != 1 {
                    return Err("港股历史准备返回格式无效".to_owned());
                }
                let dated = serde_json::from_value::<DatedHistoryFetch>(rows.pop().unwrap())
                    .map_err(|error| error.to_string())?;
                let fetched = dated.fetched;
                let bars = fetched.bars;
                if bars.is_empty() {
                    failures.insert(code, "数据源未返回港股日线".to_owned());
                    continue;
                }
                let range = fetch_ranges.get(&code).unwrap().clone();
                let record = HkHistoryRecord {
                    bars,
                    source: "Tencent Finance".to_owned(),
                    fetched_at: dated.started_at,
                    last_error: None,
                    request_range: Some(range.clone()),
                    request_ranges: vec![range.clone()],
                    price_adjustment: fetched.price_adjustment,
                    bar_fetched_at: HashMap::new(),
                    revision_reason: None,
                    suspended_as_of: fetched.suspended_as_of,
                };
                fetched_records.insert(code.clone(), record.clone());
                let record = merge_record(cache.histories.get(&code), record);
                cache.histories.insert(code.clone(), record);
                cache.history_errors.remove(&code);
                updated += 1;
            }
            Err(error) => {
                failures.insert(code, error);
            }
        }
    }
    for code in &requested {
        if !completed.contains(code) {
            failures.insert(code.clone(), "港股日线准备超时，请重试".to_owned());
        }
    }
    for (code, error) in &failures {
        cache.history_errors.insert(code.clone(), error.clone());
        if let Some(history) = cache.histories.get_mut(code) {
            history.last_error = Some(error.clone());
        }
    }
    let coverage = coverage(&cache, &codes, start, end, minimum)?;
    let available = coverage
        .iter()
        .filter(|row| row["sufficient"] == true)
        .count();
    let summary = json!({"requested_count":requested.len(),"requested_codes":requested,"updated_count":updated,"failed_count":failures.len(),"available_count":available,"candidate_count":codes.len(),"suspended_count":coverage.iter().filter(|row|row["suspended"]==true).count(),"qfq_count":coverage.iter().filter(|row|row["sufficient"]==true && row["price_adjustment"]=="qfq").count(),"hfq_count":coverage.iter().filter(|row|row["sufficient"]==true && row["price_adjustment"]=="hfq").count(),"timed_out":outcome.timed_out,"errors":failures.iter().map(|(code,error)|format!("{code}: {error}")).take(8).collect::<Vec<_>>(),"network_history_fetch":!requested.is_empty()});
    let mut data = crate::hk_research::project_hk_market_data(&cache)?;
    data.stocks = stock_snapshot;
    filter_calculation_histories(&mut data, &coverage);
    Ok(PreparedHistory {
        cache,
        data,
        summary,
        coverage,
        fetched_records,
        fetch_errors: failures,
        attempted_at: now,
    })
}

pub(super) async fn prepare(
    app: &tauri::AppHandle,
    payload: &Value,
    cache: HkMarketCache,
    codes: Vec<String>,
    start: &str,
    end: &str,
    minimum: usize,
) -> Result<PreparedHistory, String> {
    let policy = DataPolicy::parse(payload)?;
    if policy == DataPolicy::Refresh {
        crate::hk_market::ensure_hk_provider_ready("tencent")?;
    }
    let client = crate::market::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK research history",
        Duration::from_secs(8),
        Some(payload),
    )?;
    let adjustments = cache
        .histories
        .iter()
        .map(|(code, history)| (code.clone(), history.price_adjustment))
        .collect::<HashMap<_, _>>();
    let prepared = prepare_with_fetch(
        cache,
        codes.clone(),
        start,
        end,
        minimum,
        policy,
        move |code, start, end| {
            let client = client.clone();
            let code = code.to_owned();
            let start = start.to_owned();
            let end = end.to_owned();
            let adjustment = adjustments.get(&code).copied().unwrap_or_default();
            async move {
                crate::hk_market::fetch_tencent_hk_history_range(
                    &client, &code, &start, &end, adjustment,
                )
                .await
            }
        },
    )
    .await?;
    if prepared.summary["requested_count"].as_u64().unwrap_or(0) == 0 {
        return Ok(prepared);
    }
    let path = crate::hk_market::hk_market_data_path(app)?;
    let (cache, ()) = crate::hk_market::update_hk_market_cache(&path, |latest| {
        persist_prepared_updates(latest, &prepared);
        Ok(())
    })?;
    let canonical = codes
        .iter()
        .filter_map(|code| crate::hk_market::canonical_hk_code(code))
        .collect::<Vec<_>>();
    let coverage = coverage(&cache, &canonical, start, end, minimum)?;
    let mut data = crate::hk_research::project_hk_market_data(&cache)?;
    data.stocks = prepared.data.stocks.clone();
    filter_calculation_histories(&mut data, &coverage);
    let mut summary = prepared.summary.clone();
    summary["available_count"] = json!(coverage
        .iter()
        .filter(|row| row["sufficient"] == true)
        .count());
    summary["suspended_count"] = json!(coverage
        .iter()
        .filter(|row| row["suspended"] == true)
        .count());
    summary["qfq_count"] = json!(coverage
        .iter()
        .filter(|row| row["sufficient"] == true && row["price_adjustment"] == "qfq")
        .count());
    summary["hfq_count"] = json!(coverage
        .iter()
        .filter(|row| row["sufficient"] == true && row["price_adjustment"] == "hfq")
        .count());
    Ok(PreparedHistory {
        cache,
        data,
        summary,
        coverage,
        ..prepared
    })
}

pub(super) fn require_usable(
    prepared: &PreparedHistory,
    operation: &str,
    require_all: bool,
) -> Result<(), String> {
    if require_all || prepared.summary["available_count"].as_u64().unwrap_or(0) == 0 {
        local_data::require_coverage(operation, &prepared.coverage)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache() -> HkMarketCache {
        let mut cache = HkMarketCache::default();
        for code in ["00700", "01171"] {
            cache.securities.push(serde_json::from_value(json!({"code":code,"name":code,"currency":"HKD","security_type":"Equity","classification_status":"exchange_listed_equity"})).unwrap());
            cache.quotes.insert(code.to_owned(),serde_json::from_value(json!({"code":code,"name":code,"currency":"HKD","price":10.0,"price_semantics_version":1,"source":"fixture","quote_time":"2026/11/06 16:00:00"})).unwrap());
        }
        cache
    }
    fn bars() -> Vec<Value> {
        ["2020-01-02","2026-11-05","2026-11-06"].iter().map(|date|json!({"date":date,"open":10.0,"close":10.0,"high":11.0,"low":9.0,"volume_shares":1000})).collect()
    }
    fn fetched(rows: Vec<Value>) -> HkFetchedHistory {
        HkFetchedHistory {
            bars: serde_json::from_value(Value::Array(rows)).unwrap(),
            price_adjustment: HkHistoryPriceAdjustment::Backward,
            suspended_as_of: None,
        }
    }
    fn record(fetched_at: &str, start: &str, end: &str, rows: Vec<Value>) -> HkHistoryRecord {
        serde_json::from_value(
            json!({"bars":rows,"source":"fixture","fetched_at":fetched_at,
            "request_range":{"start_date":start,"end_date":end}}),
        )
        .unwrap()
    }
    #[test]
    fn hk_history_merge_keeps_newer_overlapping_prices() {
        let mut latest_rows = bars();
        latest_rows[2] = price_bar("2026-11-06", 20.0);
        let latest = record("200", "20260101", "20261107", latest_rows);
        let older_fetch = record("100", "20200101", "20261107", bars());
        let merged = merge_record(Some(&latest), older_fetch);
        assert_eq!(merged.bars.last().unwrap().close, 20.0);
        assert_eq!(merged.fetched_at, "200");
        assert!(!range_covers(&merged, "20200101", "20261106"));
        assert!(range_covers(&merged, "20260101", "20261106"));
    }
    #[test]
    fn hk_history_merge_retains_disjoint_prepared_ranges_without_claiming_the_gap() {
        let first = record("100", "20200101", "20211231", bars());
        let second = record("200", "20230101", "20241231", bars());
        let merged = merge_record(Some(&first), second);
        assert!(range_covers(&merged, "20200101", "20211231"));
        assert!(range_covers(&merged, "20230101", "20241231"));
        assert!(!range_covers(&merged, "20210101", "20231231"));
    }
    fn price_bar(date: &str, close: f64) -> Value {
        json!({"date":date,"open":close,"close":close,"high":close+1.0,"low":close-1.0,"volume_shares":1000})
    }
    #[tokio::test]
    async fn hk_history_accepts_a_confirmed_suspension_without_inventing_a_new_trade_bar() {
        let mut initial = cache();
        initial.quotes.get_mut("00700").unwrap().volume_shares = Some(0);
        let history: HkHistoryRecord = serde_json::from_value(
            json!({"bars":[price_bar("2020-01-02",10.0),price_bar("2026-10-12",10.0)],
            "source":"fixture","fetched_at":"100","suspended_as_of":"20261106",
            "request_range":{"start_date":"20200101","end_date":"20261108"}}),
        )
        .unwrap();
        initial.histories.insert("00700".to_owned(), history);
        let rows = coverage(&initial, &["00700".to_owned()], "20200101", "20261108", 2).unwrap();
        assert_eq!(rows[0]["sufficient"], true);
        assert_eq!(rows[0]["as_of"], "20261012");
        assert_eq!(initial.histories["00700"].bars.len(), 2);
        initial.quotes.get_mut("00700").unwrap().quote_time =
            Some("2026/11/09 10:00:00".to_owned());
        initial.quotes.get_mut("00700").unwrap().volume_shares = Some(1000);
        let rows = coverage(&initial, &["00700".to_owned()], "20200101", "20261109", 2).unwrap();
        assert_eq!(rows[0]["sufficient"], false);
    }
    #[test]
    fn hk_history_local_refresh_does_not_age_unfetched_archive_prices() {
        let archive = record(
            "100",
            "20200101",
            "20200102",
            vec![price_bar("2020-01-02", 10.0)],
        );
        let recent = record(
            "200",
            "20261101",
            "20261107",
            vec![price_bar("2026-11-06", 20.0)],
        );
        let latest = merge_record(Some(&archive), recent);
        let older_request = record(
            "150",
            "20200101",
            "20200102",
            vec![price_bar("2020-01-02", 11.0)],
        );
        let merged = merge_record(Some(&latest), older_request);
        assert_eq!(merged.bars.first().unwrap().close, 11.0);
        assert_eq!(merged.bars.last().unwrap().close, 20.0);
        assert_eq!(merged.fetched_at, "200");
    }
    #[test]
    fn hk_history_recalculated_adjustments_invalidate_unfetched_archive_coverage() {
        let archive = record(
            "100",
            "20200101",
            "20261107",
            vec![
                price_bar("2020-01-02", 10.0),
                price_bar("2024-04-02", 20.0),
                price_bar("2026-11-06", 30.0),
            ],
        );
        let updated = record(
            "200",
            "20240402",
            "20261107",
            vec![price_bar("2024-04-02", 19.0), price_bar("2026-11-06", 29.0)],
        );
        let merged = merge_record(Some(&archive), updated);
        assert!(!range_covers(&merged, "20200101", "20261106"));
        assert!(range_covers(&merged, "20240402", "20261106"));
        let usable = crate::hk_market::hk_history_bars_for_quote(&merged, None);
        assert!(usable.iter().all(|bar| bar.date.as_str() >= "2024-04-02"));
    }
    #[test]
    fn hk_history_checks_a_closed_previous_last_bar_when_a_new_day_arrives() {
        let archive = record(
            "100",
            "20200101",
            "20261106",
            vec![price_bar("2020-01-02", 10.0), price_bar("2026-11-06", 20.0)],
        );
        let updated = record(
            "200",
            "20261106",
            "20261109",
            vec![price_bar("2026-11-06", 19.0), price_bar("2026-11-09", 21.0)],
        );
        let merged = merge_record(Some(&archive), updated);
        assert!(!range_covers(&merged, "20200101", "20261109"));
        assert!(crate::hk_market::hk_history_bars_for_quote(&merged, None)
            .iter()
            .all(|bar| bar.date.as_str() >= "2026-11-06"));
    }
    #[test]
    fn hk_history_does_not_extend_coverage_with_an_older_recalculated_price_version() {
        let latest = record(
            "200",
            "20240402",
            "20261107",
            vec![price_bar("2024-04-02", 20.0), price_bar("2026-11-06", 30.0)],
        );
        let stale = record(
            "100",
            "20200101",
            "20261107",
            vec![
                price_bar("2020-01-02", 10.0),
                price_bar("2024-04-02", 19.0),
                price_bar("2026-11-06", 29.0),
            ],
        );
        let merged = merge_record(Some(&latest), stale);
        assert!(!range_covers(&merged, "20200101", "20261106"));
        assert_eq!(merged.bars.first().unwrap().date, "2024-04-02");
        assert_eq!(merged.bars.first().unwrap().close, 20.0);
    }
    #[tokio::test]
    async fn hk_history_partial_fetch_excludes_incomplete_cached_candidates_from_calculation() {
        let mut initial = cache();
        let recent = (0..60)
            .rev()
            .map(|offset| {
                let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
                    - chrono::Duration::days(offset);
                json!({"date":date.format("%Y-%m-%d").to_string(),"open":10.0,
                "close":10.0,"high":11.0,"low":9.0,"volume_shares":1000})
            })
            .collect();
        initial.histories.insert(
            "01171".to_owned(),
            record("100", "20260908", "20261107", recent),
        );
        let prepared = prepare_with_fetch(
            initial,
            vec!["00700".to_owned(), "01171".to_owned()],
            "20200101",
            "20261107",
            2,
            DataPolicy::Refresh,
            |code, _, _| {
                let code = code.to_owned();
                async move {
                    if code == "00700" {
                        Ok(fetched(bars()))
                    } else {
                        Err("provider unavailable".to_owned())
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(prepared.summary["candidate_count"], 2);
        assert_eq!(prepared.summary["available_count"], 1);
        assert_eq!(prepared.data.stocks.len(), 2);
        assert!(prepared.cache.histories.contains_key("01171"));
        assert!(!prepared.data.histories.contains_key("01171.HK"));
    }
    #[tokio::test]
    async fn hk_history_persistence_does_not_rewrite_unfetched_concurrent_bars() {
        let mut initial = cache();
        let mut recent = record("100", "20261101", "20261107", vec![bars()[2].clone()]);
        recent.price_adjustment = HkHistoryPriceAdjustment::Backward;
        initial.histories.insert("00700".to_owned(), recent);
        let mut latest = initial.clone();
        latest.histories.get_mut("00700").unwrap().bars[0].close = 20.0;
        let prepared = prepare_with_fetch(
            initial,
            vec!["00700".to_owned()],
            "20200101",
            "20211231",
            2,
            DataPolicy::Refresh,
            |_, _, _| async {
                Ok(fetched(
                    ["2020-01-02", "2021-12-31"]
                        .iter()
                        .map(|date| {
                            json!({"date":date,
                    "open":5.0,"close":5.0,"high":6.0,"low":4.0,"volume_shares":1000})
                        })
                        .collect(),
                ))
            },
        )
        .await
        .unwrap();
        persist_prepared_updates(&mut latest, &prepared);
        assert_eq!(latest.histories["00700"].bars.first().unwrap().close, 5.0);
        assert_eq!(latest.histories["00700"].bars.last().unwrap().close, 20.0);
        assert!(range_covers(
            &latest.histories["00700"],
            "20261101",
            "20261106"
        ));
    }

    #[tokio::test]
    async fn hk_history_extends_a_historical_fetch_to_include_the_existing_archive() {
        let mut initial = cache();
        initial.histories.insert(
            "00700".to_owned(),
            record("100", "20261101", "20261107", vec![bars()[2].clone()]),
        );
        let prepared = prepare_with_fetch(
            initial,
            vec!["00700".to_owned()],
            "20200101",
            "20211231",
            2,
            DataPolicy::Refresh,
            |_, start, end| {
                assert_eq!(start, "20200101");
                assert_eq!(end, "20261107");
                async { Ok(fetched(bars())) }
            },
        )
        .await
        .unwrap();
        assert_eq!(
            prepared.cache.histories["00700"]
                .request_range
                .as_ref()
                .unwrap()
                .end_date,
            "20261107"
        );
    }
    #[tokio::test]
    #[ignore = "manual public HK data probe; requires a copied cache"]
    async fn live_hk_trend_and_backtest_history_probe() {
        let root = std::path::PathBuf::from(
            std::env::var("GP_HK_RESEARCH_CACHE_ROOT").expect("copied cache root required"),
        );
        let output = std::path::PathBuf::from(
            std::env::var("GP_HK_RESEARCH_OUTPUT").expect("output root required"),
        );
        let path = crate::hk_market::hk_market_cache_path(&root);
        let mut cache = crate::hk_market::read_hk_market_cache(&path).unwrap();
        if std::env::var("GP_HK_PROBE_FORCE_HISTORY_REFRESH").as_deref() == Ok("1") {
            cache.histories.clear();
            cache.history_errors.clear();
        }
        let data = crate::hk_research::project_hk_market_data(&cache).unwrap();
        let request: stock_optimizer_core::TrendScreenRequest = serde_json::from_value(json!({"criteria":{"limit":100,"include_st":true},"limit":10,"start_date":"20260706","end_date":"20261107"})).unwrap();
        let codes = stock_optimizer_core::trend_screen_candidate_codes(&data.stocks, &request);
        let fetcher = || {
            let client = crate::market::build_http_client_with_proxy(
                "Mozilla/5.0 GuXuanYou HK live research probe",
                Duration::from_secs(8),
                None,
            )
            .unwrap();
            move |code: &str, start: &str, end: &str| {
                let client = client.clone();
                let code = code.to_owned();
                let start = start.to_owned();
                let end = end.to_owned();
                async move {
                    crate::hk_market::fetch_tencent_hk_history_range(
                        &client,
                        &code,
                        &start,
                        &end,
                        HkHistoryPriceAdjustment::Forward,
                    )
                    .await
                }
            }
        };
        let trend_data = prepare_with_fetch(
            cache,
            codes,
            &request.start_date,
            &request.end_date,
            45,
            DataPolicy::Refresh,
            fetcher(),
        )
        .await
        .unwrap();
        println!("live HK trend preparation: {}", trend_data.summary);
        require_usable(&trend_data, "hk_trend_screen", false).unwrap();
        let trend =
            stock_optimizer_core::trend_screen_with_data(&trend_data.data, &request).unwrap();
        let mut trend_value = serde_json::to_value(&trend).unwrap();
        crate::hk_research::attach_cached_hk_financial_metrics(&mut trend_value, &trend_data.cache);
        super::super::append_hk_history_note(&mut trend_value, &trend_data.summary);
        local_data::attach_metadata(
            &mut trend_value,
            super::super::hk_history_metadata(
                &trend_data.cache,
                &trend_data.data,
                &trend_data.summary,
                &trend_data.coverage,
                "refresh",
            ),
            &json!({"market":"hk","data_policy":"refresh"}),
        );
        std::fs::write(
            output.join("trend-real.json"),
            serde_json::to_vec_pretty(&trend_value).unwrap(),
        )
        .unwrap();
        let backtest_request=super::super::hk_backtest_request(&json!({"source":"criteria","criteria":{"limit":100},"start_date":"20200101","end_date":"20261107","top_n":10,"benchmark":"candidate_equal_weight","transaction_cost_bps":10}),&trend_data.data).unwrap();
        let selected = stock_optimizer_core::backtest_selected_symbols(
            &trend_data.data.stocks,
            &backtest_request,
        );
        let backtest_data = prepare_with_fetch(
            trend_data.cache,
            selected,
            &backtest_request.start_date,
            &backtest_request.end_date,
            2,
            DataPolicy::Refresh,
            fetcher(),
        )
        .await
        .unwrap();
        println!("live HK backtest preparation: {}", backtest_data.summary);
        crate::hk_market::write_hk_market_cache(&path, &backtest_data.cache).unwrap();
        require_usable(&backtest_data, "hk_backtest", true).unwrap();
        let mut backtest =
            super::super::hk_candidate_backtest_value(&backtest_data.data, &backtest_request)
                .unwrap();
        backtest["market"] = json!("hk");
        super::super::append_hk_history_note(&mut backtest, &backtest_data.summary);
        local_data::attach_metadata(
            &mut backtest,
            super::super::hk_history_metadata(
                &backtest_data.cache,
                &backtest_data.data,
                &backtest_data.summary,
                &backtest_data.coverage,
                "refresh",
            ),
            &json!({"market":"hk","data_policy":"refresh"}),
        );
        assert!(backtest["metrics"]["total_return"]
            .as_f64()
            .unwrap()
            .is_finite());
        assert!(backtest["equity_curve"].as_array().unwrap().len() > 365);
        assert!(backtest.get("Ok").is_none());
        std::fs::write(
            output.join("backtest-real.json"),
            serde_json::to_vec_pretty(&backtest).unwrap(),
        )
        .unwrap();
        crate::hk_market::write_hk_market_cache(&path, &backtest_data.cache).unwrap();
        let report = json!({"trend":trend_data.summary,"trend_returned":trend.returned,"backtest":backtest_data.summary,"curve_points":backtest["equity_curve"].as_array().unwrap().len(),"symbols":backtest["symbols"]});
        std::fs::write(
            output.join("research-live-verification.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{}", report);
    }

    #[tokio::test]
    async fn hk_history_cache_only_reports_missing_without_calling_a_provider() {
        let result = prepare_with_fetch(
            cache(),
            vec!["00700.HK".to_owned()],
            "20200101",
            "20261107",
            2,
            DataPolicy::CacheOnly,
            |_, _, _| async { panic!("cache-only must not fetch") },
        )
        .await;
        assert!(result.err().unwrap().contains("LOCAL_DATA_MISSING"));
    }
    #[tokio::test]
    async fn hk_history_prepares_requested_symbols_and_reuses_the_requested_date_range() {
        let prepared = prepare_with_fetch(
            cache(),
            vec!["00700.HK".to_owned()],
            "20200101",
            "20261107",
            2,
            DataPolicy::Refresh,
            |_, _, _| async { Ok(fetched(bars())) },
        )
        .await
        .unwrap();
        assert_eq!(prepared.summary["updated_count"], 1);
        assert_eq!(prepared.summary["available_count"], 1);
        let cached = prepare_with_fetch(
            prepared.cache,
            vec!["00700.HK".to_owned()],
            "20200101",
            "20261107",
            2,
            DataPolicy::Refresh,
            |_, _, _| async { panic!("valid range must be cached") },
        )
        .await
        .unwrap();
        assert_eq!(cached.summary["requested_count"], 0);
    }
    #[tokio::test]
    async fn hk_history_partial_fetch_preserves_good_data_and_identifies_failed_symbols() {
        let prepared = prepare_with_fetch(
            cache(),
            vec!["00700.HK".to_owned(), "01171.HK".to_owned()],
            "20200101",
            "20261107",
            2,
            DataPolicy::Refresh,
            |code, _, _| {
                let code = code.to_owned();
                async move {
                    if code == "00700" {
                        Ok(fetched(bars()))
                    } else {
                        Err("provider unavailable".to_owned())
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(prepared.summary["available_count"], 1);
        assert_eq!(prepared.summary["failed_count"], 1);
        assert!(prepared.cache.histories.contains_key("00700"));
        assert!(require_usable(&prepared, "hk_backtest", true).is_err());
        assert!(require_usable(&prepared, "hk_trend_screen", false).is_ok());
    }
}
