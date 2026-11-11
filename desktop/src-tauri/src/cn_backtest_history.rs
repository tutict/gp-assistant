use chrono::Datelike;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use stock_optimizer_core as gp_core;

const PAGE_SIZE: usize = 2000;
const MAX_PAGES: usize = 8;

pub(super) fn known_through(raw: &Value, epoch_ms: u128) -> Result<String, String> {
    let today_key =
        crate::market::local_yyyymmdd_from_epoch_ms(epoch_ms).ok_or("无法确定日线可知日期")?;
    let today = date(&today_key)?;
    let calendar = raw.pointer("/cn_reference/calendar");
    let nontrading = matches!(today.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
        || calendar.is_some_and(|calendar| {
            calendar["checked"] == true
                && calendar["coverage_end"] == today_key
                && calendar["is_trading_day"] == false
        });
    let minutes = ((epoch_ms / 1000 + 8 * 60 * 60) % 86400) / 60;
    let complete = if nontrading || minutes >= 16 * 60 {
        today
    } else {
        today.pred_opt().ok_or("日期超出可查询范围")?
    };
    Ok(complete.format("%Y%m%d").to_string())
}

fn date(raw: &str) -> Result<chrono::NaiveDate, String> {
    let key = crate::market::compact_date_key(raw).ok_or_else(|| format!("无效日线日期：{raw}"))?;
    chrono::NaiveDate::parse_from_str(&key, "%Y%m%d").map_err(|_| format!("无效日线日期：{raw}"))
}

fn series(rows: &[gp_core::HistoryBar]) -> Result<(String, String, String), String> {
    let mut canonical = BTreeMap::new();
    for row in rows {
        let key = date(&row.date)?.format("%Y%m%d").to_string();
        if !row.close.is_finite()
            || row.close <= 0.0
            || [row.open, row.high, row.low]
                .into_iter()
                .flatten()
                .any(|value| !value.is_finite() || value <= 0.0)
            || [row.volume, row.capital]
                .into_iter()
                .flatten()
                .any(|value| !value.is_finite())
        {
            return Err("日线包含无效价格或数值".to_string());
        }
        let value = json!([
            key,
            row.open,
            row.high,
            row.low,
            row.close,
            row.volume,
            row.capital
        ]);
        if canonical.insert(key, value).is_some() {
            return Err("日线包含重复日期".to_string());
        }
    }
    let first = canonical.keys().next().ok_or("所选区间没有日线")?.clone();
    let last = canonical
        .keys()
        .next_back()
        .expect("nonempty series")
        .clone();
    let bytes = serde_json::to_vec(&canonical.into_values().collect::<Vec<_>>())
        .map_err(|error| error.to_string())?;
    Ok((first, last, crate::market::sha256_hex(&bytes)))
}

pub(super) fn evidence(
    code: &str,
    rows: &[gp_core::HistoryBar],
    start: &str,
    end: &str,
    provider: &str,
) -> Result<Value, String> {
    let code = crate::market::normalize_stock_code(code).ok_or("无效股票代码")?;
    let start = date(start)?.format("%Y%m%d").to_string();
    let end = date(end)?.format("%Y%m%d").to_string();
    let (first, last, hash) = series(rows)?;
    let fetched_at = crate::market::epoch_millis();
    let fetched_date =
        crate::market::local_yyyymmdd_from_epoch_ms(fetched_at).ok_or("无法确定日线抓取日期")?;
    if start > end
        || first < start
        || last > end
        || end > fetched_date
        || !matches!(provider, "tencent" | "eastmoney")
    {
        return Err("日线响应未满足所请求的区间或数据源口径".to_string());
    }
    Ok(
        json!({"schema_version":1,"code":code,"requested_start":start,"requested_end":end,
        "provider":provider,"price_adjustment":"unadjusted","fetched_at_epoch_ms":fetched_at,"coverage_known_through":end,
        "first_date":first,"last_date":last,"bar_count":rows.len(),"bars_hash":hash}),
    )
}

pub(super) fn listing_date(raw: &Value, code: &str) -> Option<String> {
    let code = crate::market::normalize_stock_code(code)?;
    raw.get("stocks")
        .and_then(Value::as_array)?
        .iter()
        .find(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(crate::market::normalize_stock_code)
                .as_ref()
                == Some(&code)
        })?
        .get("listing_date")
        .and_then(Value::as_str)
        .and_then(|value| date(value).ok())
        .map(|date| date.format("%Y%m%d").to_string())
}

fn calendar_end(raw: &Value, end: &str) -> Option<String> {
    let calendar = raw.pointer("/cn_reference/calendar")?;
    if calendar["checked"] != true
        || calendar["coverage_start"].as_str()? > end
        || calendar["coverage_end"].as_str()? < end
    {
        return None;
    }
    calendar["trading_dates"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|day| *day <= end)
        .max()
        .map(str::to_owned)
}

pub(super) fn coverage(
    data: &gp_core::CoreDataSet,
    raw: &Value,
    overrides: &HashMap<String, Vec<gp_core::HistoryBar>>,
    proofs: &Map<String, Value>,
    codes: &[String],
    start: &str,
    end: &str,
    minimum: usize,
) -> Vec<Value> {
    let mut items = super::local_history_coverage(data, overrides, codes, start, end, minimum);
    for item in &mut items {
        let code = item["code"].as_str().unwrap_or_default().to_string();
        let normalized = crate::market::normalize_stock_code(&code).unwrap_or_else(|| code.clone());
        let rows = overrides
            .get(&code)
            .or_else(|| overrides.get(&normalized))
            .or_else(|| data.histories.get(&code))
            .or_else(|| data.histories.get(&normalized))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let proof = proofs
            .get(&code)
            .or_else(|| proofs.get(&normalized))
            .or_else(|| {
                raw.pointer("/cn_reference/history_ranges")
                    .and_then(|ranges| ranges.get(&normalized))
            });
        let requested_start = item["start_date"].as_str().unwrap_or_default();
        let requested_end = item["end_date"].as_str().unwrap_or_default();
        let verified = proof.is_some_and(|proof| {
            proof["schema_version"] == 1
                && proof["code"] == normalized
                && proof["price_adjustment"] == "unadjusted"
                && matches!(proof["provider"].as_str(), Some("tencent" | "eastmoney"))
                && proof["fetched_at_epoch_ms"].as_u64().is_some_and(|time| {
                    time > 0
                        && time as u128 <= crate::market::epoch_millis()
                        && known_through(raw, time as u128).is_ok_and(|known| {
                            proof["coverage_known_through"]
                                .as_str()
                                .is_some_and(|value| {
                                    date(value).is_ok()
                                        && requested_end <= value
                                        && value <= known.as_str()
                                })
                        })
                })
                && proof["requested_start"]
                    .as_str()
                    .is_some_and(|value| date(value).is_ok() && value <= requested_start)
                && proof["requested_end"]
                    .as_str()
                    .is_some_and(|value| date(value).is_ok() && value >= requested_end)
                && series(rows).is_ok_and(|(first, last, hash)| {
                    proof["first_date"] == first
                        && proof["last_date"] == last
                        && proof["bar_count"] == rows.len()
                        && proof["bars_hash"] == hash
                })
        });
        let listed = listing_date(raw, &code);
        let effective_start = listed
            .as_deref()
            .map(|listed| listed.max(requested_start))
            .unwrap_or(requested_start)
            .to_string();
        let effective_end =
            calendar_end(raw, requested_end).unwrap_or_else(|| requested_end.to_string());
        item["range_verified"] = json!(verified);
        item["listing_date"] = json!(listed);
        item["effective_start_date"] = json!(effective_start);
        item["effective_end_date"] = json!(effective_end);
        if let Some(proof) = proof {
            item["provider"] = proof["provider"].clone();
            item["price_adjustment"] = proof["price_adjustment"].clone();
            item["verified_start_date"] = proof["requested_start"].clone();
            item["verified_end_date"] = proof["requested_end"].clone();
        }
        if !verified {
            item["sufficient"] = json!(false);
            item["reason"] = json!(
                "本地日线尚未验证覆盖所选回测区间，需按日期补齐；短期趋势缓存不能代替完整区间"
            );
        }
    }
    items
}

pub(super) struct FetchedHistory {
    pub(super) rows: Vec<gp_core::HistoryBar>,
    pub(super) proof: Value,
}

fn parse_tencent_page(
    text: &str,
    code: &str,
    cursor: chrono::NaiveDate,
) -> Result<Vec<gp_core::HistoryBar>, String> {
    let symbol = crate::market::tencent_symbol(code).ok_or("无效股票代码")?;
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("腾讯日线响应无效：{error}"))?;
    if value["code"] != 0 {
        return Err("腾讯日线数据源返回错误".to_string());
    }
    // Request raw prices; an adjusted response cannot be substituted for them.
    let raw = value["data"][&symbol]["day"]
        .as_array()
        .ok_or("腾讯未返回不复权日线")?;
    let rows = raw
        .iter()
        .map(|row| {
            crate::market::parse_tencent_kline_row(row)
                .ok_or_else(|| "腾讯日线包含无效数据".to_string())
                .and_then(|row| {
                    serde_json::from_value::<gp_core::HistoryBar>(row)
                        .map_err(|error| error.to_string())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !rows.is_empty() {
        series(&rows)?;
    }
    if rows.len() > PAGE_SIZE
        || rows
            .iter()
            .any(|row| date(&row.date).is_ok_and(|day| day > cursor))
    {
        return Err("腾讯日线未遵守请求的截止日期或分页上限".to_string());
    }
    Ok(rows)
}

async fn collect_tencent_range<F, Fut>(
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    mut fetch: F,
) -> Result<Vec<gp_core::HistoryBar>, String>
where
    F: FnMut(chrono::NaiveDate) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<gp_core::HistoryBar>, String>>,
{
    let mut cursor = end;
    let mut merged = BTreeMap::new();
    for _ in 0..MAX_PAGES {
        let rows = fetch(cursor).await?;
        if rows.is_empty() {
            return Ok(merged.into_values().collect());
        }
        let page_count = rows.len();
        if page_count > PAGE_SIZE {
            return Err("日线分页条数超出约定上限".to_string());
        }
        let first = rows
            .iter()
            .map(|row| date(&row.date))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min()
            .expect("nonempty page");
        if first > cursor
            || rows
                .iter()
                .any(|row| date(&row.date).is_ok_and(|day| day > cursor))
        {
            return Err("腾讯日线分页未向前推进，所选区间未补齐".to_string());
        }
        for row in rows {
            if date(&row.date)? >= start {
                merged.insert(row.date.clone(), row);
            }
        }
        if first <= start || page_count < PAGE_SIZE {
            return Ok(merged.into_values().collect());
        }
        cursor = first.pred_opt().ok_or("日线日期超出可查询范围")?;
    }
    Err("日线分页达到上限，所选区间未补齐，请缩短回测日期".to_string())
}

async fn tencent_range(
    client: &reqwest::Client,
    code: &str,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> Result<Vec<gp_core::HistoryBar>, String> {
    let symbol = crate::market::tencent_symbol(code).ok_or("无效股票代码")?;
    collect_tencent_range(start, end, |cursor| {
        let param = format!("{symbol},day,,{},{PAGE_SIZE},", cursor.format("%Y-%m-%d"));
        let encoded =
            percent_encoding::utf8_percent_encode(&param, percent_encoding::NON_ALPHANUMERIC)
                .to_string();
        let url = format!("https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={encoded}");
        async move {
            let text = response_text(client, &url).await?;
            parse_tencent_page(&text, code, cursor)
        }
    })
    .await
}

async fn response_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .header(reqwest::header::REFERER, "https://gu.qq.com/")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let bytes = crate::external_data::provider::read_bounded_response(response, 8 * 1024 * 1024)
        .await
        .map_err(|error| error.message().to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

async fn eastmoney_range(
    client: &reqwest::Client,
    code: &str,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> Result<Vec<gp_core::HistoryBar>, String> {
    let market = crate::market::eastmoney_market_code(code).ok_or("无效股票代码")?;
    let digits = code.get(..6).ok_or("无效股票代码")?;
    let url = format!("https://push2his.eastmoney.com/api/qt/stock/kline/get?fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&ut=7eea3edcaed734bea9cbfc24409ed989&klt=101&fqt=0&secid={market}.{digits}&beg={}&end={}&lmt=100000", start.format("%Y%m%d"), end.format("%Y%m%d"));
    let text = response_text(client, &url).await?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if value["rc"] != 0 {
        return Err("东方财富日线数据源返回错误".to_string());
    }
    if value["data"]["code"].as_str() != Some(digits) {
        return Err("东方财富日线响应证券代码不匹配".to_string());
    }
    let raw = value["data"]["klines"]
        .as_array()
        .ok_or("东方财富未返回所选区间的日线")?;
    if raw.len() >= 100000 {
        return Err("东方财富日线返回条数达到上限，所选区间未补齐".to_string());
    }
    raw.iter()
        .map(|row| {
            row.as_str()
                .and_then(crate::market::parse_eastmoney_kline_row)
                .ok_or_else(|| "东方财富日线包含无效数据".to_string())
                .and_then(|row| serde_json::from_value(row).map_err(|error| error.to_string()))
        })
        .collect()
}

pub(super) async fn fetch_range(
    code: &str,
    start: &str,
    end: &str,
    listed: Option<&str>,
    known: &str,
) -> Result<FetchedHistory, String> {
    let start_day = date(start)?;
    let end_day = date(end)?;
    if end_day > date(known)? {
        return Err(format!(
            "回测结束日期尚无完整日线，请选择 {known} 或更早日期"
        ));
    }
    let effective_start = listed
        .and_then(|listed| date(listed).ok())
        .map(|listed| listed.max(start_day))
        .unwrap_or(start_day);
    if effective_start > end_day {
        return Err("所选回测区间早于该股票上市日期".to_string());
    }
    let client = crate::market::build_tencent_http_client(
        "Mozilla/5.0 GuXuanYou/0.3 backtest range",
        Duration::from_secs(8),
    )?;
    let mut errors = Vec::new();
    for provider in ["tencent", "eastmoney"] {
        let fetched = if provider == "tencent" {
            tencent_range(&client, code, effective_start, end_day).await
        } else {
            eastmoney_range(&client, code, effective_start, end_day).await
        };
        match fetched.and_then(|rows| {
            evidence(code, &rows, start, end, provider).map(|proof| FetchedHistory { rows, proof })
        }) {
            Ok(history) => return Ok(history),
            Err(error) => errors.push(format!("{provider}：{error}")),
        }
    }
    Err(format!("无法补齐所选回测区间：{}", errors.join("；")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(dates: &[&str]) -> Vec<gp_core::HistoryBar> {
        dates
            .iter()
            .map(|date| gp_core::HistoryBar {
                date: date.to_string(),
                close: 10.0,
                open: None,
                high: None,
                low: None,
                volume: None,
                capital: None,
            })
            .collect()
    }

    #[test]
    fn cn_backtest_range_rejects_old_rolling_windows_without_request_evidence() {
        let code = "300750.SZ".to_string();
        let mut data = gp_core::CoreDataSet::default();
        data.histories
            .insert(code.clone(), bars(&["2026-03-19", "2026-08-18"]));
        let items = coverage(
            &data,
            &json!({}),
            &HashMap::new(),
            &Map::new(),
            &[code],
            "20200101",
            "20261016",
            2,
        );
        assert_eq!(items[0]["available_bars"], 2);
        assert_eq!(items[0]["sufficient"], false);
        assert_eq!(items[0]["range_verified"], false);
    }

    #[test]
    fn cn_backtest_range_keeps_ipo_and_nontrading_boundaries_without_inventing_bars() {
        let code = "301358.SZ".to_string();
        let rows = bars(&["2023-02-09", "2026-10-15", "2026-10-16"]);
        let proof = evidence(&code, &rows, "20200101", "20261018", "eastmoney").unwrap();
        assert_eq!(proof["first_date"], "20230209");
        assert_eq!(proof["last_date"], "20261016");
        assert_eq!(proof["bar_count"], 3);
        let mut data = gp_core::CoreDataSet::default();
        data.histories.insert(code.clone(), rows.clone());
        let raw = json!({"stocks":[{"code":code,"listing_date":"2023-02-09"}],"cn_reference":{"history_ranges":{code.clone():proof}}});
        let items = coverage(
            &data,
            &raw,
            &HashMap::new(),
            &Map::new(),
            &[code.clone()],
            "20200101",
            "20261018",
            2,
        );
        assert_eq!(items[0]["sufficient"], true);
        assert_eq!(items[0]["range_verified"], true);
        data.histories
            .insert(code.clone(), bars(&["2026-07-01", "2026-08-18"]));
        let stale = coverage(
            &data,
            &raw,
            &HashMap::new(),
            &Map::new(),
            &[code],
            "20200101",
            "20261018",
            2,
        );
        assert_eq!(stale[0]["sufficient"], false);
    }

    #[test]
    fn cn_backtest_range_does_not_expand_a_proof_beyond_its_requested_dates() {
        let code = "300750.SZ".to_string();
        let rows = bars(&["2026-07-01", "2026-10-16"]);
        let proof = evidence(&code, &rows, "20260701", "20261016", "eastmoney").unwrap();
        let mut data = gp_core::CoreDataSet::default();
        data.histories.insert(code.clone(), rows);
        let raw = json!({"cn_reference":{"history_ranges":{code.clone():proof}}});
        let items = coverage(
            &data,
            &raw,
            &HashMap::new(),
            &Map::new(),
            &[code],
            "20200101",
            "20261018",
            2,
        );
        assert_eq!(items[0]["sufficient"], false);
    }

    #[test]
    fn cn_backtest_range_proof_binds_prices_but_normalizes_date_format_and_order() {
        let code = "300750.SZ".to_string();
        let rows = bars(&["2026-10-15", "2026-10-16"]);
        let proof = evidence(&code, &rows, "20200101", "20261018", "tencent").unwrap();
        let mut data = gp_core::CoreDataSet::default();
        data.histories
            .insert(code.clone(), bars(&["20261016", "20261015"]));
        let raw = json!({"cn_reference":{"history_ranges":{code.clone():proof},"calendar":{
            "checked":true,"coverage_start":"20260901","coverage_end":"20261108","trading_dates":["20261015","20261016","20261106"]}}});
        let check = |data: &gp_core::CoreDataSet| {
            coverage(
                data,
                &raw,
                &HashMap::new(),
                &Map::new(),
                &[code.clone()],
                "20200101",
                "20261018",
                2,
            )
        };
        assert_eq!(check(&data)[0]["sufficient"], true);
        assert_eq!(check(&data)[0]["effective_end_date"], "20261016");
        data.histories.get_mut(&code).unwrap()[0].close = 11.0;
        assert_eq!(check(&data)[0]["sufficient"], false);
    }

    #[test]
    fn cn_backtest_range_rejects_invalid_duplicate_and_adjusted_responses() {
        assert!(evidence(
            "300750.SZ",
            &bars(&["2026-02-30"]),
            "20260101",
            "20261016",
            "tencent"
        )
        .is_err());
        assert!(evidence(
            "300750.SZ",
            &bars(&["2026-10-16", "20261016"]),
            "20260101",
            "20261016",
            "tencent"
        )
        .is_err());
        let adjusted = json!({"code":0,"data":{"sz300750":{"qfqday":[["2026-10-16","10","10","10","10","100"]]}}});
        assert!(parse_tencent_page(
            &adjusted.to_string(),
            "300750.SZ",
            date("20261016").unwrap()
        )
        .is_err());
        let ignored_end = json!({"code":0,"data":{"sz300750":{"day":[["2026-11-06","10","10","10","10","100"]]}}});
        assert!(parse_tencent_page(
            &ignored_end.to_string(),
            "300750.SZ",
            date("20261016").unwrap()
        )
        .is_err());
    }

    #[test]
    fn cn_backtest_range_never_proves_future_or_unfinished_trading_dates() {
        assert!(evidence(
            "300750.SZ",
            &bars(&["2026-10-16"]),
            "20200101",
            "20991231",
            "tencent"
        )
        .is_err());
        let morning = crate::market::parse_cache_datetime_epoch_ms("2026-11-09T02:00:00Z").unwrap();
        assert_eq!(known_through(&json!({}), morning).unwrap(), "20261108");
        let closed = crate::market::parse_cache_datetime_epoch_ms("2026-11-09T08:00:00Z").unwrap();
        assert_eq!(known_through(&json!({}), closed).unwrap(), "20261109");
        let sunday = crate::market::parse_cache_datetime_epoch_ms("2026-11-08T02:00:00Z").unwrap();
        assert_eq!(known_through(&json!({}), sunday).unwrap(), "20261108");
        let mut proof = evidence(
            "300750.SZ",
            &bars(&["2026-10-15", "2026-10-16"]),
            "20200101",
            "20261018",
            "tencent",
        )
        .unwrap();
        proof["requested_end"] = json!("20991231");
        let mut data = gp_core::CoreDataSet::default();
        data.histories
            .insert("300750.SZ".to_string(), bars(&["2026-10-15", "2026-10-16"]));
        let raw = json!({"cn_reference":{"history_ranges":{"300750.SZ":proof}}});
        let item = coverage(
            &data,
            &raw,
            &HashMap::new(),
            &Map::new(),
            &["300750.SZ".to_string()],
            "20200101",
            "20991231",
            2,
        );
        assert_eq!(item[0]["sufficient"], false);
    }

    fn page(cursor: chrono::NaiveDate, count: usize) -> Vec<gp_core::HistoryBar> {
        (0..count)
            .rev()
            .map(|offset| {
                let day = (cursor - chrono::Duration::days(offset as i64))
                    .format("%Y-%m-%d")
                    .to_string();
                bars(&[&day]).pop().unwrap()
            })
            .collect()
    }

    #[tokio::test]
    async fn cn_backtest_range_fetches_earlier_pages_and_rejects_stuck_or_truncated_pagination() {
        let end = date("20261106").unwrap();
        let start = end - chrono::Duration::days(2500);
        let mut cursors = Vec::new();
        let rows = collect_tencent_range(start, end, |cursor| {
            cursors.push(cursor);
            let count = if cursors.len() == 1 { PAGE_SIZE } else { 600 };
            std::future::ready(Ok(page(cursor, count)))
        })
        .await
        .unwrap();
        assert_eq!(cursors.len(), 2);
        assert_eq!(rows.len(), 2501);
        assert_eq!(
            rows.first().unwrap().date,
            start.format("%Y-%m-%d").to_string()
        );
        assert_eq!(rows.last().unwrap().date, "2026-11-06");
        let stuck =
            collect_tencent_range(start, end, |_| std::future::ready(Ok(page(end, PAGE_SIZE))))
                .await;
        assert!(stuck.unwrap_err().contains("未向前推进"));
        let truncated = collect_tencent_range(date("19500101").unwrap(), end, |cursor| {
            std::future::ready(Ok(page(cursor, PAGE_SIZE)))
        })
        .await;
        assert!(truncated.unwrap_err().contains("达到上限"));
    }
}
