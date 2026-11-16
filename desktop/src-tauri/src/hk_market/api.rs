use super::*;

#[tauri::command]
pub(crate) fn api_hk_market_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = hk_market_data_path(&app)?;
    let cache = read_hk_market_cache(&path)?;
    let external_data = crate::external_data::status_for_app(&app)?;
    hk_market_status_value(
        &path,
        &cache,
        external_data,
        crate::market_domain::dates::epoch_millis(),
    )
}

pub(super) fn hk_market_status_value(
    path: &Path,
    cache: &HkMarketCache,
    external_data: serde_json::Value,
    now: u128,
) -> Result<serde_json::Value, String> {
    let data = crate::hk_research::project_hk_market_data(cache)?;
    let quote_trade_date = data
        .stocks
        .iter()
        .filter_map(|stock| stock.quote_time.as_deref())
        .filter_map(crate::market_domain::dates::compact_date_key)
        .max();
    let quote_generated_at = data
        .stocks
        .iter()
        .filter_map(|stock| stock.code.strip_suffix(".HK"))
        .filter_map(|code| cache.quotes.get(code))
        .filter_map(|quote| quote.fetched_at.as_deref())
        .max();
    let current_trade_date =
        crate::market_domain::dates::expected_market_quote_date_from_epoch_ms(now);
    let cache_bytes = external_data
        .get("database_bytes")
        .and_then(serde_json::Value::as_u64);
    let supported_securities = cache
        .securities
        .iter()
        .filter(|security| security.supports_hkd_research())
        .collect::<Vec<_>>();
    let quote_refresh_failed = supported_securities.iter().any(|security| {
        cache.quote_errors.contains_key(&security.code)
            || cache
                .quotes
                .get(&security.code)
                .is_some_and(|quote| quote.last_error.is_some())
    });
    let stale = data.stocks.is_empty()
        || data.stocks.len() < supported_securities.len()
        || quote_trade_date.is_none()
        || quote_trade_date != current_trade_date
        || cache.securities_last_error.is_some()
        || quote_refresh_failed;
    Ok(serde_json::json!({
        "market": "hk",
        "source": "hk_market_cache",
        "cache_dir": path.parent().map(|parent| parent.to_string_lossy()),
        "cache_bytes": cache_bytes,
        "cache_path": path.to_string_lossy(),
        "universe_count": data.stocks.len(),
        "universe_updated_at": cache.securities_updated_at,
        "quote_trade_date": quote_trade_date,
        "current_trade_date": current_trade_date,
        "quote_generated_at": quote_generated_at,
        "security_count": cache.securities.len(),
        "securities_source": cache.securities_source,
        "securities_updated_at": cache.securities_updated_at,
        "securities_last_attempt_at": cache.securities_last_attempt_at,
        "securities_last_error": cache.securities_last_error,
        "quote_count": cache.quotes.len(),
        "history_count": cache.histories.len(),
        "quote_updated_at": cache.quotes.values().filter_map(|quote| quote.fetched_at.as_ref()).max(),
        "history_updated_at": cache.histories.values().map(|history| history.fetched_at.as_str()).max(),
        "financial_updated_at": cache.financials.values().map(|snapshot| snapshot.candidate_fetched_at.as_str()).max(),
        "industry_options": data.stocks.iter().map(|stock| stock.industry.trim()).filter(|industry| !industry.is_empty()).collect::<std::collections::BTreeSet<_>>(),
        "quote_errors": cache.quote_errors,
        "history_errors": cache.history_errors,
        "external_data": external_data,
        "stale": stale,
    }))
}

#[tauri::command]
pub(crate) async fn api_hk_network_probe(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let code = match payload.get("code").and_then(serde_json::Value::as_str) {
        Some(raw_code) => canonical_hk_code(raw_code)
            .ok_or_else(|| "请输入有效的一至五位港股代码。".to_string())?,
        None => "00700".to_string(),
    };
    let path = hk_market_data_path(&app)?;
    let cache = read_hk_market_cache(&path)?;
    let currency = cache
        .securities
        .iter()
        .find(|security| security.code == code)
        .and_then(|security| CurrencyCode::new(&security.currency).ok())
        .unwrap_or_else(|| CurrencyCode::new("HKD").expect("HKD is a valid currency code"));
    let timeout = std::time::Duration::from_secs(7);
    let client = crate::network::http::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK network probe",
        timeout,
        Some(&payload),
    )?;
    let codes = vec![code.clone()];
    let tencent_codes = codes.clone();
    let tencent_currency = currency.clone();
    let tencent_client = client.clone();
    let tencent_timeout = timeout;
    let tencent = async move {
        let (text, _) =
            fetch_tencent_hk_quote_batch_text(&tencent_client, &tencent_codes, tencent_timeout)
                .await?;
        parse_tencent_hk_quote(
            &text,
            &format!("{}.HK", tencent_codes[0]),
            &tencent_currency,
        )?;
        Ok::<(), String>(())
    };
    let history_client = client.clone();
    let history_code = code.clone();
    let history_timeout = timeout;
    let tencent_history = async move {
        let url = tencent_hk_daily_history_url(&history_code)?;
        let response = history_client
            .get(url)
            .timeout(history_timeout)
            .send()
            .await
            .map_err(|error| format!("Tencent daily-history request failed: {error}"))?;
        let body = crate::external_data::provider::read_bounded_response(response, 8 * 1024 * 1024)
            .await
            .map_err(|error| {
                format!(
                    "Tencent daily-history response failed: {}: {}",
                    error.category(),
                    error.message()
                )
            })?;
        parse_tencent_hk_history(&String::from_utf8_lossy(&body), &history_code)?;
        Ok::<(), String>(())
    };
    let sina_client = client.clone();
    let sina_code = code.clone();
    let sina_fetched_at = current_epoch_millis();
    let sina = async move {
        fetch_sina_hk_financial_snapshot_from(
            &sina_client,
            &SinaHkFinancialProvider,
            &sina_code,
            &sina_fetched_at,
        )
        .await?;
        Ok::<(), String>(())
    };
    let eastmoney_client = client.clone();
    let eastmoney_code = code.clone();
    let eastmoney_fetched_at = current_epoch_millis();
    let eastmoney = async move {
        fetch_hk_financial_snapshot_from(
            &eastmoney_client,
            &EastmoneyHkFinancialProvider,
            &eastmoney_code,
            &eastmoney_fetched_at,
        )
        .await?;
        Ok::<(), String>(())
    };
    let checks = tokio::join!(
        probe_hk_source("tencent", tencent_timeout, tencent),
        probe_hk_source("tencent_history", history_timeout, tencent_history),
        probe_hk_source("sina_hk_financial", timeout, sina),
        probe_hk_source("eastmoney_hk", timeout, eastmoney),
    );
    Ok(serde_json::json!({
        "market": "hk",
        "code": code,
        "proxy_mode": crate::network::proxy::proxy_mode_from_payload(Some(&payload)),
        "proxy_configured": crate::network::proxy::proxy_from_payload(Some(&payload)).is_some(),
        "sources": [checks.0, checks.1, checks.2, checks.3],
    }))
}

pub(super) async fn probe_hk_source<F>(
    source_id: &'static str,
    timeout: std::time::Duration,
    request: F,
) -> serde_json::Value
where
    F: std::future::Future<Output = Result<(), String>>,
{
    let started = std::time::Instant::now();
    let (ok, error) = match tokio::time::timeout(timeout, request).await {
        Ok(Ok(())) => (true, String::new()),
        Ok(Err(error)) => (false, error.chars().take(240).collect::<String>()),
        Err(_) => (false, format!("连接测试超过 {} 秒", timeout.as_secs())),
    };
    serde_json::json!({
        "source_id": source_id,
        "ok": ok,
        "error": error,
        "elapsed_ms": started.elapsed().as_millis(),
    })
}

#[tauri::command]
pub(crate) async fn api_hk_market_refresh(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_hk_provider_ready("hkex")?;
    let path = hk_market_data_path(&app)?;
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    let attempt_at = current_epoch_millis();
    let request_id = format!("hk-directory-{attempt_at}");
    let request = crate::external_data::RefreshRequest {
        domain: crate::external_data::DataDomain::SecurityMaster,
        market: crate::external_data::Market::HongKong,
        scope: crate::external_data::RefreshScope::MarketWide,
        request_id: Some(request_id.clone()),
    };
    let client = crate::network::http::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK market",
        timeout,
        Some(&payload),
    )?;
    let quote_client = client.clone();
    let provider = HkexSecurityDirectoryProvider::new(client);
    let context = crate::external_data::provider::ProviderContext {
        max_response_bytes: HKEX_MAX_WORKBOOK_BYTES,
        ..crate::external_data::provider::ProviderContext::new(&request_id, timeout)
    };
    let records = match fetch_explicit_hk_batch(
        &app,
        &provider,
        &request,
        &context,
        &attempt_at,
        false,
    )
    .await
    {
        Ok(records) => records,
        Err(error) => {
            record_hkex_refresh_error(&path, &attempt_at, &error)?;
            return Err(error);
        }
    };
    let directory_cache = read_hk_market_cache(&path)?;
    let listed_equities = directory_cache
        .securities
        .iter()
        .filter(|security| security.classification_status.allows_observation())
        .cloned()
        .collect::<Vec<_>>();
    let quote_fetched_at = current_epoch_millis();
    let quote_outcome =
        fetch_hk_quote_refresh(&quote_client, &listed_equities, &quote_fetched_at, timeout).await;
    let (_, quote_refresh) = update_hk_market_cache(&path, |cache| {
        Ok(merge_hk_quote_refresh(cache, quote_outcome))
    })?;
    let refresh_stale = quote_refresh.is_stale();
    let cache = read_hk_market_cache(&path)?;
    let run = crate::external_data::open_app_store(&app)?
        .refresh_run_by_idempotency_key(&hk_refresh_idempotency_key(
            "hkex",
            request.domain,
            &request_id,
        ))?
        .ok_or_else(|| "HKEX refresh run status is unavailable".to_string())?;
    Ok(serde_json::json!({
        "market": "hk",
        "security_count": cache.securities.len(),
        "quote_count": cache.quotes.len(),
        "quote_refresh": quote_refresh,
        "securities_source": cache.securities_source,
        "securities_updated_at": cache.securities_updated_at,
        "run_id": run.run_id,
        "state": run.state,
        "refreshed_records": records.len(),
        "stale": refresh_stale,
    }))
}

pub(crate) fn record_hkex_refresh_error(
    path: &Path,
    attempt_at: &str,
    error: &str,
) -> Result<(), String> {
    update_hk_market_cache(path, |cache| {
        cache.securities_last_attempt_at = Some(attempt_at.to_string());
        cache.securities_last_error = Some(error.to_string());
        Ok(())
    })
    .map(|_| ())
}

#[tauri::command]
pub(crate) fn api_hk_stock_search(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let cache = read_hk_market_cache(&hk_market_data_path(&app)?)?;
    let query = payload
        .get("query")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let limit = payload
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 100) as usize;
    let items = cache
        .securities
        .iter()
        .filter(|security| {
            hk_security_matches_query(security, cache.quotes.get(&security.code), &query)
                || hk_financial_chinese_name(cache.financials.get(&security.code))
                    .is_some_and(|name| name.to_lowercase().contains(&query))
        })
        .take(limit)
        .map(|security| {
            let mut item = serde_json::to_value(security)
                .map_err(|error| format!("encode HK search result: {error}"))?;
            if let Some(name_zh) = hk_chinese_display_name(cache.quotes.get(&security.code))
                .or_else(|| hk_financial_chinese_name(cache.financials.get(&security.code)))
            {
                item["name_zh"] = serde_json::Value::String(name_zh.to_owned());
            }
            Ok(item)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let refresh_readiness = ensure_hk_provider_ready("hkex");
    Ok(
        serde_json::json!({ "market": "hk", "items": items, "source": cache.securities_source, "updated_at": cache.securities_updated_at, "last_attempt_at": cache.securities_last_attempt_at, "last_error": cache.securities_last_error, "refresh_enabled": refresh_readiness.is_ok(), "refresh_reason": refresh_readiness.err() }),
    )
}

#[tauri::command]
pub(crate) fn api_hk_confirm_ordinary_share(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "港股代码不能为空".to_string())?;
    let path = hk_market_data_path(&app)?;
    let (cache, ()) =
        update_hk_market_cache(&path, |cache| confirm_hk_ordinary_share(cache, code))?;
    let confirmed = canonical_hk_code(code).ok_or_else(|| "港股代码无效".to_string())?;
    let security = cache
        .securities
        .iter()
        .find(|security| security.code == confirmed)
        .ok_or_else(|| "更新后的港股目录未包含目标证券".to_string())?;
    serde_json::to_value(security).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn api_hk_observe(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let path = hk_market_data_path(&app)?;
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let policy = payload
        .get("data_policy")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("cache_only");
    let mut cache = read_hk_market_cache(&path)?;
    let security = cache
        .securities
        .iter()
        .find(|item| item.code == code)
        .cloned()
        .ok_or_else(|| format!("港股证券列表中没有找到 {code}。"))?;
    observation_security_id(&security)?;
    let provider_readiness_error = ensure_hk_provider_ready("tencent").err();
    let mut notes = Vec::new();
    if hk_observation_should_refresh(policy) {
        if let Some(error) = provider_readiness_error.as_ref() {
            return Err(error.clone());
        }
        let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
        let fetched_at = current_epoch_millis();
        let client = crate::network::http::build_http_client_with_proxy(
            "Mozilla/5.0 GuXuanYou HK observe",
            timeout,
            Some(&payload),
        )?;
        let provider = TencentHkMarketProvider::new(client, &cache.securities)
            .with_history_adjustments(&cache.histories);
        let request_id = payload
            .get("request_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("hk-observe-{code}-{fetched_at}"));
        let quote_request = crate::external_data::RefreshRequest {
            domain: crate::external_data::DataDomain::Quote,
            market: crate::external_data::Market::HongKong,
            scope: crate::external_data::RefreshScope::Entities {
                codes: vec![code.clone()],
            },
            request_id: Some(format!("{request_id}-quote")),
        };
        let history_request = crate::external_data::RefreshRequest {
            domain: crate::external_data::DataDomain::DailyHistory,
            market: crate::external_data::Market::HongKong,
            scope: crate::external_data::RefreshScope::Entities {
                codes: vec![code.clone()],
            },
            request_id: Some(format!("{request_id}-history")),
        };
        let mut quote_context = crate::external_data::provider::ProviderContext::new(
            format!("{request_id}-quote"),
            timeout,
        );
        let mut history_context = crate::external_data::provider::ProviderContext::new(
            format!("{request_id}-history"),
            timeout,
        );
        let proxy_mode = crate::network::proxy::proxy_mode_from_payload(Some(&payload));
        let proxy_url = crate::network::proxy::proxy_from_payload(Some(&payload));
        let network_policy = crate::external_data::provider::ProviderNetworkPolicy {
            proxy_mode,
            proxy_url,
            allow_direct_fallback: false,
        };
        quote_context.network_policy = crate::external_data::provider::ProviderNetworkPolicy {
            proxy_mode: network_policy.proxy_mode.clone(),
            proxy_url: network_policy.proxy_url.clone(),
            allow_direct_fallback: network_policy.allow_direct_fallback,
        };
        history_context.network_policy = network_policy;

        let (quote_batch, history_batch) = tokio::join!(
            fetch_explicit_hk_batch(
                &app,
                &provider,
                &quote_request,
                &quote_context,
                &fetched_at,
                false
            ),
            fetch_explicit_hk_batch(
                &app,
                &provider,
                &history_request,
                &history_context,
                &fetched_at,
                false
            ),
        );
        let quote = match quote_batch {
            Ok(records) => records
                .into_iter()
                .next()
                .map(|record| {
                    serde_json::from_value(record.payload)
                        .map_err(|error| format!("decode refreshed HK quote: {error}"))
                })
                .transpose()?
                .ok_or_else(|| "Tencent returned no HK quote record".to_string()),
            Err(error) => Err(error),
        };
        let history = match history_batch {
            Ok(records) => records
                .into_iter()
                .next()
                .map(|record| {
                    serde_json::from_value::<HkHistoryRecord>(record.payload)
                        .map_err(|error| format!("decode refreshed HK history: {error}"))
                })
                .transpose()?
                .ok_or_else(|| "Tencent returned no HK history record".to_string()),
            Err(error) => Err(error),
        };
        let (updated_cache, result) = update_hk_market_cache(&path, |latest| {
            Ok(merge_hk_observation_update(
                latest,
                &code,
                quote,
                history,
                &fetched_at,
            ))
        })?;
        cache = updated_cache;
        notes.extend(result.notes);
    }
    let cached_quote = cache.quotes.get(&code).cloned();
    let quote = cached_quote_for_security(&security, cached_quote.clone());
    let history = cache.histories.get(&code).cloned();
    let quote_cache_error = cached_quote
        .as_ref()
        .filter(|_| quote.is_none())
        .map(|value| {
            if !value.has_current_price_semantics() {
                return "旧版新浪报价缓存的最新价口径已修正，请刷新行情重新获取报价。".to_string();
            }
            format!(
                "cached quote currency {} does not match security-master currency {}",
                value.currency, security.currency
            )
        });
    let quote_error = quote_cache_error.or_else(|| {
        cache
            .quotes
            .get(&code)
            .and_then(|value| value.last_error.clone())
            .or_else(|| cache.quote_errors.get(&code).cloned())
    });
    let history_error = history
        .as_ref()
        .and_then(|record| record.last_error.clone())
        .or_else(|| cache.history_errors.get(&code).cloned());
    let quote_capability = quote_capability_for_security(
        &security,
        cached_quote
            .as_ref()
            .filter(|value| value.has_current_price_semantics())
            .map(|value| value.currency.as_str()),
        quote_error.as_deref(),
        provider_readiness_error.as_deref(),
    );
    if let Some(error) = quote_error.as_ref() {
        if !notes.iter().any(|note| note.contains(error)) {
            notes.push(format!("报价：{error}"));
        }
    }
    if let Some(error) = history_error.as_ref() {
        if !notes.iter().any(|note| note.contains(error)) {
            notes.push(format!("日 K：{error}"));
        }
    }
    let stale = quote.is_none() || history.is_none() || !notes.is_empty();
    if history
        .as_ref()
        .is_some_and(|history| history.price_adjustment == HkHistoryPriceAdjustment::Backward)
    {
        notes.push(
            "历史图表使用后复权价格；同日数据按最新报价归一，缩放不改变历史收益率。".to_owned(),
        );
    }
    if let Some(reason) = history
        .as_ref()
        .and_then(|history| history.revision_reason.as_ref())
    {
        if !notes.contains(reason) {
            notes.push(reason.clone());
        }
    }
    if history
        .as_ref()
        .zip(quote.as_ref())
        .is_some_and(|(history, quote)| hk_history_suspension_matches_quote(history, quote))
    {
        notes.push("该证券处于停牌状态，日线使用最后成交日行情。".to_owned());
    }
    let financial = build_hk_financial_result(&cache, &code);
    let mut security_payload = serde_json::to_value(&security)
        .map_err(|error| format!("encode HK observation security: {error}"))?;
    if let Some(name_zh) = hk_chinese_display_name(cache.quotes.get(&code))
        .or_else(|| hk_financial_chinese_name(cache.financials.get(&code)))
    {
        security_payload["name_zh"] = serde_json::Value::String(name_zh.to_owned());
    }
    Ok(serde_json::json!({
        "market": "hk",
        "security": security_payload,
        "quote": quote,
        "history": history.as_ref().map(|record| hk_history_bars_for_quote(record, quote.as_ref())).unwrap_or_default(),
        "quote_capability": quote_capability,
        "provider_readiness_error": provider_readiness_error,
        "quote_updated_at": quote.as_ref().and_then(|value| value.fetched_at.clone()),
        "history_source": history.as_ref().map(|value| if value.price_adjustment == HkHistoryPriceAdjustment::Backward { format!("{}（后复权）", value.source) } else { value.source.clone() }),
        "history_price_adjustment": history.as_ref().map(|value| value.price_adjustment),
        "history_updated_at": history.as_ref().map(|record| record.fetched_at.clone()),
        "history_error": history_error,
        "quote_error": quote_error,
        "notes": notes,
        "stale": stale,
        "financial": financial.snapshot,
        "fx_rate": financial.fx_rate,
        "official_disclosure": financial.official_disclosure,
        "quarterly_update": financial.quarterly_update,
        "quarterly_update_checked_at": financial.quarterly_update_checked_at,
        "quarterly_update_error": financial.quarterly_update_error,
        "candidate_updated_at": financial.candidate_updated_at,
        "fx_updated_at": financial.fx_updated_at,
        "official_checked_at": financial.official_checked_at,
        "financial_notes": financial.notes,
        "financial_field_errors": financial.field_errors,
        "financial_stale": financial.stale,
    }))
}

#[tauri::command]
pub(crate) fn api_hk_financial_status(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let cache = read_hk_market_cache(&hk_market_data_path(&app)?)?;
    serde_json::to_value(build_hk_financial_result(&cache, &code))
        .map_err(|error| format!("encode HK financial status failed: {error}"))
}

#[tauri::command]
pub(crate) async fn api_hk_financial_refresh(
    app: AppHandle,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let code = payload
        .get("code")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_hk_code)
        .ok_or_else(|| "请输入有效港股代码。".to_string())?;
    let path = hk_market_data_path(&app)?;
    let fetched_at = current_epoch_millis();
    let timeout = std::time::Duration::from_secs(HK_REQUEST_TIMEOUT_SECS);
    {
        let store = crate::external_data::open_app_store(&app)?;
        store.record_source_attempt(
            "eastmoney_hk",
            crate::external_data::DataDomain::FinancialSnapshot,
            crate::external_data::Market::HongKong,
            &fetched_at,
        )?;
        store.record_source_attempt(
            "sina_hk_financial",
            crate::external_data::DataDomain::FinancialSnapshot,
            crate::external_data::Market::HongKong,
            &fetched_at,
        )?;
        store.record_source_attempt(
            "frankfurter",
            crate::external_data::DataDomain::FxRate,
            crate::external_data::Market::Global,
            &fetched_at,
        )?;
        store.record_source_attempt(
            "hkexnews",
            crate::external_data::DataDomain::Disclosure,
            crate::external_data::Market::HongKong,
            &fetched_at,
        )?;
    }
    let request_client = crate::network::http::build_http_client_with_proxy(
        "Mozilla/5.0 GuXuanYou HK financial",
        timeout,
        Some(&payload),
    );
    let (financial, fx, disclosure, quarterly_update) = match request_client {
        Ok(client) => {
            let (financial, fx, disclosure) = tokio::join!(
                fetch_hk_financial_snapshot(&client, &code, &fetched_at),
                fetch_hk_fx_rate(&client, &fetched_at),
                fetch_hk_disclosure_metadata(&client, &code, &fetched_at),
            );
            let cached_disclosure = read_hk_market_cache(&path)?.disclosures.get(&code).cloned();
            let quarterly_disclosure = disclosure
                .as_ref()
                .ok()
                .filter(|value| value.source_url.is_some())
                .cloned()
                .or_else(|| cached_disclosure.filter(|value| value.source_url.is_some()));
            let quarterly_update = if let Some(disclosure) =
                quarterly_disclosure.filter(|value| value.source_url.is_some())
            {
                fetch_hkex_quarterly_update(&client, &code, &disclosure, &fetched_at).await
            } else {
                Err("HKEX issuer identifier is unavailable".to_string())
            };
            (financial, fx, disclosure, quarterly_update)
        }
        Err(error) => (
            Err(error.clone()),
            Err(error.clone()),
            Err(error.clone()),
            Err(error),
        ),
    };
    {
        let store = crate::external_data::open_app_store(&app)?;
        let eastmoney_error = financial
            .as_ref()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .field_errors
                    .get("eastmoney_supplement")
                    .map(String::as_str)
            })
            .or_else(|| financial.as_ref().err().map(String::as_str));
        let sina_error = financial
            .as_ref()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .field_errors
                    .get("sina_supplement")
                    .map(String::as_str)
            })
            .or_else(|| financial.as_ref().err().map(String::as_str));
        let eastmoney_succeeded = financial.as_ref().ok().is_some_and(|snapshot| {
            snapshot.candidate_source.contains("Eastmoney")
                && !snapshot.field_errors.contains_key("eastmoney_supplement")
        });
        let sina_succeeded = financial.as_ref().ok().is_some_and(|snapshot| {
            snapshot.supplemental_source.is_some()
                || snapshot.candidate_source == "Sina Finance HK statements"
        });
        record_hk_source_result(
            &store,
            "eastmoney_hk",
            crate::external_data::DataDomain::FinancialSnapshot,
            crate::external_data::Market::HongKong,
            &fetched_at,
            eastmoney_succeeded,
            eastmoney_error,
        )?;
        record_hk_source_result(
            &store,
            "sina_hk_financial",
            crate::external_data::DataDomain::FinancialSnapshot,
            crate::external_data::Market::HongKong,
            &fetched_at,
            sina_succeeded,
            sina_error,
        )?;
        record_hk_source_result(
            &store,
            "frankfurter",
            crate::external_data::DataDomain::FxRate,
            crate::external_data::Market::Global,
            &fetched_at,
            fx.is_ok(),
            fx.as_ref().err().map(String::as_str),
        )?;
        record_hk_source_result(
            &store,
            "hkexnews",
            crate::external_data::DataDomain::Disclosure,
            crate::external_data::Market::HongKong,
            &fetched_at,
            disclosure.is_ok(),
            disclosure.as_ref().err().map(String::as_str),
        )?;
    }
    let (cache, _) = update_hk_market_cache(&path, |latest| {
        merge_hk_financial_refresh(latest, &code, financial, fx, disclosure);
        merge_hk_quarterly_update_refresh(latest, &code, quarterly_update, &fetched_at);
        Ok(())
    })?;
    serde_json::to_value(build_hk_financial_result(&cache, &code))
        .map_err(|error| format!("encode HK financial refresh result failed: {error}"))
}

fn record_hk_source_result(
    store: &crate::external_data::DataStore,
    source_id: &str,
    domain: crate::external_data::DataDomain,
    market: crate::external_data::Market,
    at: &str,
    success: bool,
    error: Option<&str>,
) -> Result<(), String> {
    if success {
        store.record_source_success(source_id, domain, market, at, 1)
    } else {
        let error = error.unwrap_or("provider request failed");
        let lower = error.to_ascii_lowercase();
        let category = if lower.contains("timeout") || lower.contains("timed out") {
            "timeout"
        } else if lower.contains("429") {
            "http_429"
        } else if lower.contains("http 5") {
            "http_5xx"
        } else if lower.contains("connection") || lower.contains("request failed") {
            "connection"
        } else {
            "provider_error"
        };
        store.record_source_failure(source_id, domain, market, at, category, error)
    }
}
