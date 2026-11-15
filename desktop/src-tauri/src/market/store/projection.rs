use crate::market_domain::codes::normalize_stock_code;
use crate::market_domain::datasets::parse_mobile_market_data_snapshot;
use crate::market_domain::dates::cache_epoch_ms;
use crate::market_domain::dates::epoch_millis;
use crate::wire_values::sha256_hex;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use stock_optimizer_core as gp_core;

pub(crate) fn external_market_domains() -> [(
    crate::external_data::DataDomain,
    crate::external_data::Market,
); 7] {
    use crate::external_data::{DataDomain, Market};
    [
        (DataDomain::SecurityMaster, Market::China),
        (DataDomain::Quote, Market::China),
        (DataDomain::DailyHistory, Market::China),
        (DataDomain::FinancialSnapshot, Market::China),
        (DataDomain::Relations, Market::China),
        (DataDomain::FactorSnapshot, Market::China),
        (DataDomain::CapitalEvidence, Market::China),
    ]
}

type ExternalMarketRecordSet = HashMap<
    (
        crate::external_data::DataDomain,
        crate::external_data::Market,
    ),
    Vec<crate::external_data::StoredRecord>,
>;

fn external_market_records(
    data: &Value,
    source_id: &str,
) -> Result<ExternalMarketRecordSet, String> {
    let (typed, _) = parse_mobile_market_data_snapshot(data, "external market dataset")?;
    external_market_records_from_typed(data, &typed, source_id)
}

pub(in crate::market::store) fn external_market_records_from_typed(
    data: &Value,
    typed: &gp_core::CoreDataSet,
    source_id: &str,
) -> Result<ExternalMarketRecordSet, String> {
    use crate::external_data::{DataDomain, Market, StoredRecord};
    let mut grouped: HashMap<_, Vec<StoredRecord>> = HashMap::new();
    let fetched_at = data
        .get("updated_at_epoch_ms")
        .and_then(Value::as_u64)
        .or_else(|| data.get("generated_at_epoch_ms").and_then(Value::as_u64))
        .map(u128::from)
        .unwrap_or_else(epoch_millis);
    let fetched_at = fetched_at.to_string();
    let metadata = Value::Object(
        data.as_object()
            .into_iter()
            .flat_map(|object| object.iter())
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "stocks"
                        | "relations"
                        | "histories"
                        | "financials"
                        | "factor_snapshots"
                        | "capital_evidence"
                )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );
    let raw_stocks = data
        .get("stocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let code = row
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)?;
            Some((code, row))
        })
        .collect::<HashMap<_, _>>();
    grouped
        .entry((DataDomain::SecurityMaster, Market::China))
        .or_default()
        .push(StoredRecord::new(
            DataDomain::SecurityMaster,
            Market::China,
            "cn",
            "metadata",
            source_id,
            "public",
            &fetched_at,
            None,
            None,
            "unverified",
            vec![],
            metadata,
        ));
    for stock in &typed.stocks {
        let base =
            serde_json::to_value(stock).map_err(|error| format!("encode market stock: {error}"))?;
        let mut security = serde_json::json!({
            "code": stock.code,
            "name": stock.name,
            "industry": stock.industry,
            "is_st": stock.is_st,
        });
        let raw_security = raw_stocks.get(&stock.code);
        if let Some(raw) = raw_security {
            for field in ["listing_date", "directory_source"] {
                if let Some(value) = raw.get(field) {
                    security[field] = value.clone();
                }
            }
        }
        grouped
            .entry((DataDomain::SecurityMaster, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::SecurityMaster,
                Market::China,
                &stock.code,
                "stock",
                raw_security
                    .and_then(|row| row.get("directory_source"))
                    .and_then(Value::as_str)
                    .unwrap_or(source_id),
                "public",
                &fetched_at,
                None,
                None,
                "unverified",
                vec![],
                security,
            ));
        let mut quote = serde_json::json!({
            "price": base["price"], "change_pct": base["change_pct"], "volume": base["volume"],
            "amount": base["amount"], "turnover_rate": base["turnover_rate"],
            "volume_ratio": base["volume_ratio"], "quote_time": base["quote_time"],
            "source": base["source"],
        });
        if let Some(raw) = normalize_stock_code(&stock.code).and_then(|code| raw_stocks.get(&code))
        {
            for field in [
                "circulating_market_cap_billion",
                "total_shares",
                "circulating_shares",
            ] {
                if let Some(value) = raw
                    .get(field)
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite())
                {
                    quote[field] = json!(value);
                }
            }
        }
        grouped
            .entry((DataDomain::Quote, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Quote,
                Market::China,
                &stock.code,
                "quote",
                source_id,
                "public",
                &fetched_at,
                stock.quote_time.as_deref(),
                None,
                "unverified",
                vec![],
                quote,
            ));
        let financial_fields = [
            "pe",
            "pb",
            "roe",
            "market_cap_billion",
            "dividend_yield",
            "latest_eps",
            "deducted_net_profit_billion",
            "deducted_net_profit_margin",
            "deducted_net_profit_growth_rate",
        ];
        let mut fundamentals = financial_fields
            .iter()
            .filter_map(|field| {
                base.get(*field)
                    .map(|value| ((*field).to_string(), value.clone()))
            })
            .collect::<serde_json::Map<_, _>>();
        if let Some(raw) = raw_stocks.get(&stock.code) {
            for field in [
                "period",
                "latest_bps",
                "financial_source",
                "financial_report_notice_date",
                "financial_coverage",
            ] {
                if let Some(value) = raw.get(field) {
                    fundamentals.insert(field.to_string(), value.clone());
                }
            }
        }
        grouped
            .entry((DataDomain::FinancialSnapshot, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FinancialSnapshot,
                Market::China,
                &stock.code,
                "stock_factors",
                source_id,
                "public",
                &fetched_at,
                None,
                None,
                "unverified",
                vec![],
                Value::Object(fundamentals),
            ));
    }
    for (index, relation) in typed.relations.iter().enumerate() {
        let payload = serde_json::to_value(relation)
            .map_err(|error| format!("encode market relation: {error}"))?;
        let key = format!("{index:08}-{}", sha256_hex(payload.to_string().as_bytes()));
        grouped
            .entry((DataDomain::Relations, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::Relations,
                Market::China,
                "relations",
                key,
                source_id,
                "derived_public",
                &fetched_at,
                None,
                None,
                "unverified",
                vec![],
                payload,
            ));
    }
    for (code, bars) in &typed.histories {
        grouped
            .entry((DataDomain::DailyHistory, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::DailyHistory,
                Market::China,
                code,
                "history",
                source_id,
                "public",
                &fetched_at,
                bars.last().map(|bar| bar.date.as_str()),
                None,
                "unverified",
                vec![],
                serde_json::to_value(bars)
                    .map_err(|error| format!("encode market history: {error}"))?,
            ));
    }
    for (code, financial) in &typed.financials {
        let mut payload = serde_json::to_value(financial)
            .map_err(|error| format!("encode market financials: {error}"))?;
        let raw = data.get("financials").and_then(|items| items.get(code));
        if let Some(updated_at) =
            cache_epoch_ms(raw.and_then(|row| row.get("supplement_updated_at_epoch_ms")))
        {
            payload["supplement_updated_at_epoch_ms"] = json!(updated_at);
        }
        grouped
            .entry((DataDomain::FinancialSnapshot, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FinancialSnapshot,
                Market::China,
                code,
                "financial",
                financial.source.as_deref().unwrap_or(source_id),
                "public",
                financial
                    .report_fetched_at
                    .as_deref()
                    .unwrap_or(&fetched_at),
                None,
                financial.report_notice_date.as_deref(),
                if financial.report_fetched_at.is_some() && financial.report_notice_date.is_some() {
                    "source_reported"
                } else {
                    "unverified"
                },
                vec!["provider_rights_unverified".to_string()],
                payload,
            ));
    }
    for (code, factors) in &typed.factor_snapshots {
        grouped
            .entry((DataDomain::FactorSnapshot, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::FactorSnapshot,
                Market::China,
                code,
                "factor_snapshots",
                source_id,
                "public",
                &fetched_at,
                factors.last().map(|factor| factor.date.as_str()),
                None,
                "unverified",
                vec![],
                serde_json::to_value(factors)
                    .map_err(|error| format!("encode market factor snapshots: {error}"))?,
            ));
    }
    for (code, evidence) in &typed.capital_evidence {
        let mut payload = serde_json::to_value(evidence)
            .map_err(|error| format!("encode market capital evidence: {error}"))?;
        let raw = data
            .get("capital_evidence")
            .and_then(|items| items.get(code));
        for field in ["market_active_brokerages", "institutional_holdings"] {
            if let Some(value) = raw
                .and_then(|row| row.get(field))
                .filter(|value| value.is_object())
            {
                payload[field] = value.clone();
            }
        }
        grouped
            .entry((DataDomain::CapitalEvidence, Market::China))
            .or_default()
            .push(StoredRecord::new(
                DataDomain::CapitalEvidence,
                Market::China,
                code,
                "capital_evidence",
                source_id,
                "public",
                &fetched_at,
                Some(&evidence.generated_at),
                None,
                "unverified",
                vec![],
                payload,
            ));
    }
    Ok(grouped)
}

pub(crate) fn project_market_store(
    store: &crate::external_data::DataStore,
) -> Result<Option<Value>, String> {
    use crate::external_data::DataDomain;
    let mut records = Vec::new();
    for (domain, market) in external_market_domains() {
        records.extend(store.read_domain_records(domain, market)?);
    }
    if records.is_empty() {
        return Ok(None);
    }
    let metadata = records.iter().find(|record| {
        record.domain == DataDomain::SecurityMaster && record.record_key == "metadata"
    });
    let mut data = metadata
        .map(|metadata| metadata.payload.clone())
        .unwrap_or_else(|| serde_json::json!({}));
    let object = data
        .as_object_mut()
        .ok_or_else(|| "market store metadata is not a JSON object".to_string())?;
    let mut stocks: BTreeMap<String, Value> = BTreeMap::new();
    let mut relations = Vec::new();
    let mut histories = serde_json::Map::new();
    let mut financials = serde_json::Map::new();
    let mut factors = serde_json::Map::new();
    let mut capital = serde_json::Map::new();
    for record in records {
        match (record.domain, record.record_key.as_str()) {
            (DataDomain::SecurityMaster, "stock") => {
                stocks.insert(record.entity_key, record.payload);
            }
            (DataDomain::Quote, "quote") => {
                let row = stocks
                    .entry(record.entity_key)
                    .or_insert_with(|| serde_json::json!({}));
                if let (Some(target), Some(fields)) =
                    (row.as_object_mut(), record.payload.as_object())
                {
                    target.extend(fields.clone());
                }
            }
            (DataDomain::FinancialSnapshot, "stock_factors") => {
                let row = stocks
                    .entry(record.entity_key)
                    .or_insert_with(|| serde_json::json!({}));
                if let (Some(target), Some(fields)) =
                    (row.as_object_mut(), record.payload.as_object())
                {
                    target.extend(fields.clone());
                }
            }
            (DataDomain::Relations, _) => relations.push(record.payload),
            (DataDomain::DailyHistory, "history") => {
                histories.insert(record.entity_key, record.payload);
            }
            (DataDomain::FinancialSnapshot, "financial") => {
                financials.insert(record.entity_key, record.payload);
            }
            (DataDomain::FactorSnapshot, "factor_snapshots") => {
                factors.insert(record.entity_key, record.payload);
            }
            (DataDomain::CapitalEvidence, "capital_evidence") => {
                capital.insert(record.entity_key, record.payload);
            }
            _ => {}
        }
    }
    object.insert(
        "stocks".into(),
        Value::Array(stocks.into_values().collect()),
    );
    object.insert("relations".into(), Value::Array(relations));
    object.insert("histories".into(), Value::Object(histories));
    object.insert("financials".into(), Value::Object(financials));
    object.insert("factor_snapshots".into(), Value::Object(factors));
    object.insert("capital_evidence".into(), Value::Object(capital));
    Ok(Some(data))
}

pub(crate) fn project_market_store_with_legacy(
    store: &crate::external_data::DataStore,
    legacy: Option<&Value>,
    promoted: &HashSet<(
        crate::external_data::DataDomain,
        crate::external_data::Market,
    )>,
) -> Result<Option<Value>, String> {
    use crate::external_data::{DataDomain, Market};
    let external = project_market_store(store)?;
    if external.is_none() && promoted.is_empty() {
        return Ok(legacy.cloned());
    }
    let external = external.unwrap_or_else(|| {
        serde_json::json!({
            "stocks": [], "relations": [], "histories": {}, "financials": {},
            "factor_snapshots": {}, "capital_evidence": {}
        })
    });
    let mut projected = legacy.cloned().unwrap_or_else(|| serde_json::json!({}));
    let Some(object) = projected.as_object_mut() else {
        return Err("legacy market dataset projection is not a JSON object".into());
    };
    let external_object = external
        .as_object()
        .ok_or_else(|| "external market dataset projection is not a JSON object".to_string())?;

    if promoted.contains(&(DataDomain::SecurityMaster, Market::China)) {
        for key in [
            "source",
            "generated_at",
            "generated_at_epoch_ms",
            "updated_at_epoch_ms",
            "notes",
            "cn_reference",
            "archived_stocks",
        ] {
            if let Some(value) = external_object.get(key) {
                object.insert(key.to_string(), value.clone());
            } else if matches!(key, "cn_reference" | "archived_stocks") {
                object.remove(key);
            }
        }
        let external_stocks = external_object
            .get("stocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let valid_codes = external_stocks
            .iter()
            .filter_map(|row| row.get("code").and_then(Value::as_str).map(str::to_string))
            .collect::<HashSet<_>>();
        let mut stocks = object
            .get("stocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| {
                let code = row.get("code").and_then(Value::as_str)?.to_string();
                if !valid_codes.contains(&code) {
                    return None;
                }
                Some((code, row))
            })
            .collect::<BTreeMap<_, _>>();
        for row in &external_stocks {
            let Some(code) = row.get("code").and_then(Value::as_str) else {
                continue;
            };
            let target = stocks
                .entry(code.to_string())
                .or_insert_with(|| serde_json::json!({}));
            for field in [
                "code",
                "name",
                "industry",
                "is_st",
                "listing_date",
                "directory_source",
            ] {
                if let Some(value) = row.get(field) {
                    target[field] = value.clone();
                }
            }
        }
        object.insert(
            "stocks".into(),
            Value::Array(stocks.into_values().collect()),
        );
    }

    if promoted.contains(&(DataDomain::Quote, Market::China))
        || promoted.contains(&(DataDomain::FinancialSnapshot, Market::China))
    {
        let mut stocks = object
            .get("stocks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| Some((row.get("code")?.as_str()?.to_string(), row)))
            .collect::<BTreeMap<_, _>>();
        if promoted.contains(&(DataDomain::Quote, Market::China)) {
            for row in stocks.values_mut() {
                if let Some(fields) = row.as_object_mut() {
                    for field in [
                        "price",
                        "change_pct",
                        "volume",
                        "amount",
                        "turnover_rate",
                        "volume_ratio",
                        "quote_time",
                        "source",
                        "circulating_market_cap_billion",
                        "total_shares",
                        "circulating_shares",
                    ] {
                        fields.remove(field);
                    }
                }
            }
        }
        if promoted.contains(&(DataDomain::FinancialSnapshot, Market::China)) {
            for row in stocks.values_mut() {
                if let Some(fields) = row.as_object_mut() {
                    for field in [
                        "pe",
                        "pb",
                        "roe",
                        "market_cap_billion",
                        "dividend_yield",
                        "latest_eps",
                        "deducted_net_profit_billion",
                        "deducted_net_profit_margin",
                        "deducted_net_profit_growth_rate",
                        "period",
                        "latest_bps",
                        "financial_source",
                        "financial_report_notice_date",
                        "financial_coverage",
                    ] {
                        fields.remove(field);
                    }
                }
            }
        }
        for row in external_object
            .get("stocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(code) = row.get("code").and_then(Value::as_str) else {
                continue;
            };
            let target = stocks
                .entry(code.to_string())
                .or_insert_with(|| serde_json::json!({}));
            let mut fields = Vec::new();
            if promoted.contains(&(DataDomain::Quote, Market::China)) {
                fields.extend([
                    "price",
                    "change_pct",
                    "volume",
                    "amount",
                    "turnover_rate",
                    "volume_ratio",
                    "quote_time",
                    "source",
                    "circulating_market_cap_billion",
                    "total_shares",
                    "circulating_shares",
                ]);
            }
            if promoted.contains(&(DataDomain::FinancialSnapshot, Market::China)) {
                fields.extend([
                    "pe",
                    "pb",
                    "roe",
                    "market_cap_billion",
                    "dividend_yield",
                    "latest_eps",
                    "deducted_net_profit_billion",
                    "deducted_net_profit_margin",
                    "deducted_net_profit_growth_rate",
                    "period",
                    "latest_bps",
                    "financial_source",
                    "financial_report_notice_date",
                    "financial_coverage",
                ]);
            }
            for field in fields {
                if let Some(value) = row.get(field) {
                    target[field] = value.clone();
                }
            }
        }
        object.insert(
            "stocks".into(),
            Value::Array(stocks.into_values().collect()),
        );
    }

    for (domain, key) in [
        (DataDomain::Relations, "relations"),
        (DataDomain::DailyHistory, "histories"),
        (DataDomain::FinancialSnapshot, "financials"),
        (DataDomain::FactorSnapshot, "factor_snapshots"),
        (DataDomain::CapitalEvidence, "capital_evidence"),
    ] {
        if promoted.contains(&(domain, Market::China)) {
            object.insert(
                key.to_string(),
                external_object.get(key).cloned().unwrap_or_else(|| {
                    if key == "relations" {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(serde_json::Map::new())
                    }
                }),
            );
        }
    }
    Ok(Some(projected))
}

pub(crate) fn store_market_dataset(
    store: &mut crate::external_data::DataStore,
    data: &Value,
    source_id: &str,
) -> Result<(), String> {
    let mut grouped = external_market_records(data, source_id)?;
    let mut errors = Vec::new();
    for (domain, market) in external_market_domains() {
        let at = epoch_millis().to_string();
        store.record_source_attempt(source_id, domain, market, &at)?;
        let records = grouped.remove(&(domain, market)).unwrap_or_default();
        match store.replace_domain_batch(domain, market, &records) {
            Ok(count) => store.record_source_success(source_id, domain, market, &at, count)?,
            Err(error) => {
                store.record_source_failure(
                    source_id,
                    domain,
                    market,
                    &at,
                    "store_write",
                    &error,
                )?;
                errors.push(format!("{}: {error}", domain.as_str()));
            }
        }
    }
    if !errors.is_empty() {
        return Err(format!(
            "some external market domains failed to commit: {}",
            errors.join("; ")
        ));
    }
    Ok(())
}
