use crate::market_domain::{
    CapabilityFreshness, CapabilityId, CapabilitySourceEvidence, CapabilityStatus, CoverageSummary,
    MarketCapability, MarketId, QuoteCoverageScope,
};
use serde_json::Value;
use stock_optimizer_core as gp_core;
use tauri::AppHandle;

#[derive(Clone, Debug)]
pub(crate) struct CapabilityEvidence {
    pub(crate) id: CapabilityId,
    pub(crate) adapter_registered: bool,
    pub(crate) release_gate_open: bool,
    pub(crate) coverage_complete: bool,
    pub(crate) usable_records: u64,
    pub(crate) expected_records: Option<u64>,
    pub(crate) coverage: Option<CoverageSummary>,
    pub(crate) sources: Vec<CapabilitySourceEvidence>,
    pub(crate) quote_scope: Option<QuoteCoverageScope>,
    pub(crate) reason: Option<String>,
}

const CAPABILITY_IDS: [CapabilityId; 8] = [
    CapabilityId::SecurityDirectory,
    CapabilityId::Observe,
    CapabilityId::FinancialSnapshot,
    CapabilityId::Screening,
    CapabilityId::Backtest,
    CapabilityId::NewsResearch,
    CapabilityId::Sentiment,
    CapabilityId::ResearchAgent,
];

fn unavailable_reason(id: CapabilityId) -> &'static str {
    match id {
        CapabilityId::Screening
        | CapabilityId::Backtest
        | CapabilityId::NewsResearch
        | CapabilityId::Sentiment
        | CapabilityId::ResearchAgent => "strategy adapter or required data is not registered",
        CapabilityId::SecurityDirectory => "security directory adapter is not registered",
        CapabilityId::Observe => "observation adapter or required market data is not registered",
        CapabilityId::FinancialSnapshot => {
            "financial snapshot adapter or required data is not registered"
        }
    }
}

pub(crate) fn capabilities_for_market(
    market: MarketId,
    evidence: &[CapabilityEvidence],
) -> Vec<MarketCapability> {
    CAPABILITY_IDS
        .iter()
        .copied()
        .map(|id| {
            let item = evidence.iter().find(|item| item.id == id);
            let Some(item) = item else {
                return MarketCapability {
                    market,
                    id,
                    status: CapabilityStatus::Unavailable,
                    reason: Some(unavailable_reason(id).to_owned()),
                    coverage: None,
                    sources: Vec::new(),
                    quote_scope: None,
                };
            };

            if !item.adapter_registered {
                return MarketCapability {
                    market,
                    id,
                    status: CapabilityStatus::Unavailable,
                    reason: Some(
                        item.reason
                            .clone()
                            .unwrap_or_else(|| unavailable_reason(id).to_owned()),
                    ),
                    coverage: item.coverage.clone(),
                    sources: item.sources.clone(),
                    quote_scope: item.quote_scope.clone(),
                };
            }

            if !item.release_gate_open {
                return MarketCapability {
                    market,
                    id,
                    status: if item.usable_records > 0 {
                        CapabilityStatus::Limited
                    } else {
                        CapabilityStatus::Unavailable
                    },
                    reason: Some(item.reason.clone().unwrap_or_else(|| {
                        "provider, freshness, or classification evidence has not met the release gate".to_owned()
                    })),
                    coverage: item.coverage.clone(),
                    sources: item.sources.clone(),
                    quote_scope: item.quote_scope.clone(),
                };
            }

            let count_matches = item
                .expected_records
                .map(|expected| expected > 0 && expected == item.usable_records)
                .unwrap_or(true);
            let available = item.coverage_complete && count_matches;
            MarketCapability {
                market,
                id,
                status: if available {
                    CapabilityStatus::Available
                } else {
                    CapabilityStatus::Limited
                },
                reason: if available {
                    None
                } else {
                    Some(
                        item.reason
                            .clone()
                            .unwrap_or_else(|| "required data coverage is incomplete".to_owned()),
                    )
                },
                coverage: item.coverage.clone(),
                sources: item.sources.clone(),
                quote_scope: item.quote_scope.clone(),
            }
        })
        .collect()
}

fn evidence(
    id: CapabilityId,
    adapter_registered: bool,
    release_gate_open: bool,
    coverage_complete: bool,
    usable_records: u64,
    expected_records: Option<u64>,
    coverage: Option<CoverageSummary>,
    reason: Option<&str>,
) -> CapabilityEvidence {
    CapabilityEvidence {
        id,
        adapter_registered,
        release_gate_open,
        coverage_complete,
        usable_records,
        expected_records,
        coverage,
        sources: Vec::new(),
        quote_scope: None,
        reason: reason.map(str::to_owned),
    }
}

fn count_value(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn bool_value(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn string_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn cache_coverage(
    requested_count: u64,
    available_count: u64,
    missing_fields: &[&str],
    start_date: Option<String>,
    end_date: Option<String>,
) -> CoverageSummary {
    CoverageSummary {
        requested_count,
        available_count,
        missing_count: requested_count.saturating_sub(available_count),
        start_date,
        end_date,
        missing_fields: missing_fields
            .iter()
            .map(|field| (*field).to_owned())
            .collect(),
    }
}

fn factor_snapshot_symbol_count(data: &gp_core::CoreDataSet) -> u64 {
    data.stocks
        .iter()
        .filter(|stock| {
            data.factor_snapshots
                .get(&stock.code)
                .is_some_and(|snapshots| !snapshots.is_empty())
        })
        .count() as u64
}

fn valid_history_bar_count(data: &gp_core::CoreDataSet, code: &str) -> usize {
    data.histories
        .get(code)
        .into_iter()
        .flatten()
        .filter(|bar| bar.close.is_finite() && bar.close > 0.0)
        .count()
}

fn source_evidence(
    source_id: impl Into<String>,
    domain: impl Into<String>,
    market: MarketId,
    fetched_at: Option<String>,
    observed_at: Option<String>,
    disclosed_at: Option<String>,
    report_period: Option<String>,
    freshness: CapabilityFreshness,
    quality_flags: &[&str],
    coverage: Option<CoverageSummary>,
) -> CapabilitySourceEvidence {
    CapabilitySourceEvidence {
        source_id: source_id.into(),
        domain: domain.into(),
        market,
        fetched_at,
        observed_at,
        disclosed_at,
        report_period,
        freshness,
        verification_status: "unverified".to_owned(),
        quality_flags: quality_flags
            .iter()
            .map(|flag| (*flag).to_owned())
            .collect(),
        coverage,
    }
}

fn external_cn_sources(
    external_status: &Value,
    relevant_domains: &[&str],
) -> Vec<CapabilitySourceEvidence> {
    external_status
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|source| {
            source.get("market").and_then(Value::as_str) == Some("cn")
                && source
                    .get("domain")
                    .and_then(Value::as_str)
                    .is_some_and(|domain| relevant_domains.contains(&domain))
        })
        .filter_map(|source| {
            let source_id = source.get("source_id").and_then(Value::as_str)?;
            let domain = source.get("domain").and_then(Value::as_str)?;
            let last_error = source
                .get("last_error_category")
                .and_then(Value::as_str)
                .is_some();
            let freshness = if last_error {
                CapabilityFreshness::Stale
            } else {
                CapabilityFreshness::Unknown
            };
            Some(source_evidence(
                source_id,
                domain,
                MarketId::Cn,
                source.get("last_success_at").and_then(string_value),
                source.get("observed_through").and_then(string_value),
                None,
                None,
                freshness,
                &["provider_rights_unverified", "external_cache_status_only"],
                None,
            ))
        })
        .collect()
}

fn deduplicate_sources(sources: Vec<CapabilitySourceEvidence>) -> Vec<CapabilitySourceEvidence> {
    let mut seen = std::collections::HashSet::new();
    sources
        .into_iter()
        .filter(|source| {
            let market = match source.market {
                MarketId::Cn => "cn",
                MarketId::Hk => "hk",
            };
            seen.insert((
                source.source_id.clone(),
                source.domain.clone(),
                market,
                source.report_period.clone(),
            ))
        })
        .collect()
}

pub(crate) fn cn_evidence_from_cache(
    status: &Value,
    data: &gp_core::CoreDataSet,
    external_status: &Value,
) -> Vec<CapabilityEvidence> {
    let reported_universe = count_value(status, "universe_count");
    let universe_count = if reported_universe > 0 {
        reported_universe
    } else {
        data.stocks.len() as u64
    };
    let stale = bool_value(status, "stale");
    let reference = status.get("cn_reference");
    let directory_checked = reference.and_then(|reference| reference.get("directory"))
        .is_some_and(|directory| bool_value(directory,"checked") && count_value(directory,"count") == universe_count
            && data.stocks.len() as u64 == universe_count);
    let financial_checked = reference.and_then(|reference| reference.get("financial"))
        .is_some_and(|financial| count_value(financial,"fetched_count") > 0);
    let source_id = status
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.trim().is_empty())
        .unwrap_or("legacy_market_cache")
        .to_owned();
    let fetched_at = status.get("quote_generated_at").and_then(string_value);
    let directory_fetched_at = reference.and_then(|reference| reference.pointer("/directory/fetched_at")).and_then(string_value)
        .or_else(|| status.get("universe_updated_at").and_then(string_value));
    let directory_freshness = freshness_from_timestamp(crate::market::epoch_millis(),directory_fetched_at.as_deref().and_then(cache_timestamp_epoch_ms));
    let financial_freshness = freshness_from_timestamp(crate::market::epoch_millis(),reference.and_then(|reference|reference.pointer("/financial/fetched_at")).and_then(string_value).as_deref().and_then(cache_timestamp_epoch_ms));
    let quote_date = status.get("quote_trade_date").and_then(string_value);
    let quote_requested = count_value(status, "quote_requested");
    let quote_reported = count_value(status, "quote_observed");
    let quote_observed = data
        .stocks
        .iter()
        .filter(|stock| stock.price.is_finite() && stock.price > 0.0 && stock.quote_time.is_some())
        .count() as u64;
    let quote_expected = if universe_count > 0 {
        universe_count
    } else {
        quote_requested.max(data.stocks.len() as u64)
    };
    let quote_observed = if quote_observed > 0 {
        quote_observed
    } else {
        quote_reported.min(quote_expected)
    };
    let history_symbols = data
        .stocks
        .iter()
        .filter(|stock| {
            stock.price.is_finite()
                && stock.price > 0.0
                && valid_history_bar_count(data, &stock.code)
                    >= crate::observe::MIN_OBSERVE_HISTORY_BARS
        })
        .count() as u64;
    let financial_symbols = data
        .stocks
        .iter()
        .filter(|stock| {
            data.financials.get(&stock.code).is_some_and(|snapshot| {
                if financial_checked {
                    snapshot.latest_eps.is_some() && snapshot.latest_bps.is_some()
                        && snapshot.operating_revenue_billion.is_some() && snapshot.parent_net_profit_billion.is_some()
                        && (snapshot.roe.is_some() || snapshot.latest_bps.is_some_and(|bps|bps<=0.0))
                        && snapshot.report_notice_date.is_some()
                } else { snapshot.latest_eps.is_some()
                    || snapshot.latest_bps.is_some()
                    || snapshot.operating_revenue_billion.is_some()
                    || snapshot.parent_net_profit_billion.is_some()
                    || snapshot.roe.is_some() }
            })
        })
        .count() as u64;
    let pit_symbols = data
        .stocks
        .iter()
        .filter(|stock| {
            data.factor_snapshots
                .get(&stock.code)
                .is_some_and(|snapshots| {
                    snapshots.iter().any(|snapshot| {
                        snapshot.available_date.is_some()
                            && snapshot.is_listed.is_some()
                            && snapshot.is_tradable.is_some()
                    })
                })
                && valid_history_bar_count(data, &stock.code)
                    >= crate::screening::MIN_ADAPTIVE_SCREEN_HISTORY_BARS
        })
        .count() as u64;
    let directory_coverage = cache_coverage(
        universe_count,
        (data.stocks.len() as u64).min(universe_count),
        if directory_checked { &[] } else { &["complete_exchange_universe", "provider_rights"] },
        None,
        quote_date.clone(),
    );
    let quote_coverage = cache_coverage(
        quote_expected,
        quote_observed.min(quote_expected),
        &["provider_rights"],
        None,
        quote_date.clone(),
    );
    let mut observation_missing_fields = vec!["provider_rights", "history_freshness_per_security"];
    if history_symbols < universe_count {
        observation_missing_fields.push("daily_history");
    }
    if data.stocks.iter().any(|stock| {
        stock.price.is_finite()
            && stock.price > 0.0
            && valid_history_bar_count(data, &stock.code) < crate::observe::MIN_OBSERVE_HISTORY_BARS
    }) {
        observation_missing_fields.push("minimum_history_depth");
    }
    let observe_coverage = cache_coverage(
        universe_count,
        history_symbols.min(universe_count),
        &observation_missing_fields,
        data.histories
            .values()
            .flat_map(|bars| bars.iter())
            .map(|bar| bar.date.clone())
            .min(),
        data.histories
            .values()
            .flat_map(|bars| bars.iter())
            .map(|bar| bar.date.clone())
            .max(),
    );
    let financial_coverage = cache_coverage(
        universe_count,
        financial_symbols.min(universe_count),
        if financial_checked {
            if financial_symbols == universe_count { &[] } else { &["per_security_financial_field_coverage"] }
        } else { &[
            "provider_rights",
            "point_in_time_publication_date",
            "per_security_financial_field_coverage",
        ] },
        None,
        None,
    );
    let screening_symbols = data
        .stocks
        .iter()
        .filter(|stock| {
            stock.price.is_finite()
                && stock.price > 0.0
                && stock.pe.is_some()
                && (stock.pb.is_some() || (financial_checked && data.financials.get(&stock.code).and_then(|report| report.latest_bps).is_some_and(|bps|bps <= 0.0)))
                && (stock.roe.is_some() || (financial_checked && data.financials.get(&stock.code).and_then(|report| report.latest_bps).is_some_and(|bps|bps <= 0.0)))
        })
        .count() as u64;
    let screening_coverage = cache_coverage(
        universe_count,
        screening_symbols.min(universe_count),
        if financial_checked {
            if screening_symbols == universe_count { &[] } else { &["complete_screening_factor_coverage"] }
        } else { &["provider_rights", "complete_screening_factor_coverage"] },
        None,
        quote_date.clone(),
    );
    let mut backtest_missing_fields = vec![
        "point_in_time_financial_availability",
        "delisted_universe",
        "corporate_actions",
        "exchange_calendar_and_trading_restrictions",
        "strict_out_of_sample_fold_count",
        "provider_rights",
    ];
    if pit_symbols < factor_snapshot_symbol_count(data) {
        backtest_missing_fields.push("pit_history_depth");
    }
    let backtest_coverage = cache_coverage(
        universe_count,
        pit_symbols.min(universe_count),
        &backtest_missing_fields,
        data.factor_snapshots
            .values()
            .flatten()
            .map(|snapshot| snapshot.date.clone())
            .min(),
        data.factor_snapshots
            .values()
            .flatten()
            .map(|snapshot| snapshot.date.clone())
            .max(),
    );

    let mut directory_sources = if universe_count > 0 {
        vec![source_evidence(
            if directory_checked { "sse_szse_listed_bse_quote" } else { "legacy_market_cache" },
            "security_master",
            MarketId::Cn,
            directory_fetched_at,
            quote_date.clone(),
            None,
            None,
            if directory_checked {
                directory_freshness
            } else if stale {
                CapabilityFreshness::Stale
            } else {
                CapabilityFreshness::Unknown
            },
            if directory_checked { &["provider_rights_unverified", "current_listed_universe_checked", "bse_quote_directory_scope"] }
                else { &["provider_rights_unverified", "complete_universe_unverified"] },
            Some(directory_coverage.clone()),
        )]
    } else {
        Vec::new()
    };
    let quote_freshness = if stale {
        CapabilityFreshness::Stale
    } else if quote_observed > 0 && fetched_at.is_some() {
        CapabilityFreshness::Fresh
    } else {
        CapabilityFreshness::Unknown
    };
    let quote_quality_flags = if stale {
        vec!["provider_rights_unverified", "cached_quote_stale"]
    } else if quote_observed == 0 {
        vec!["provider_rights_unverified", "usable_quote_missing"]
    } else if fetched_at.is_none() {
        vec!["provider_rights_unverified", "quote_fetch_time_missing"]
    } else {
        vec!["provider_rights_unverified"]
    };
    let quote_source = source_evidence(
        source_id,
        "quote",
        MarketId::Cn,
        fetched_at,
        quote_date.clone(),
        None,
        None,
        quote_freshness,
        &quote_quality_flags,
        Some(quote_coverage.clone()),
    );
    if directory_checked {
        for source in &mut directory_sources { source.verification_status = "scope_checked".to_owned(); }
    }
    let current_codes: std::collections::HashSet<_> = data.stocks.iter().map(|stock| stock.code.as_str()).collect();
    let mut financial_source_groups = std::collections::BTreeMap::<(String,Option<String>),CapabilitySourceEvidence>::new();
    for (code, snapshot) in &data.financials {
        if !current_codes.contains(code.as_str()) {
            continue;
        }
        let Some(source) = snapshot
            .source
            .as_deref()
            .filter(|source| !source.is_empty())
        else {
            continue;
        };
        let period = snapshot.period.clone();
        let mut flags = vec!["provider_rights_unverified"];
        if snapshot.report_notice_date.is_none() {flags.push("point_in_time_publication_date_missing");}
        let mut item = source_evidence(
                source,
                "financial_snapshot",
                MarketId::Cn,
                snapshot.report_fetched_at.clone(),
                None,
                snapshot.report_notice_date.clone(),
                period.clone(),
                freshness_from_timestamp(crate::market::epoch_millis(),snapshot.report_fetched_at.as_deref().and_then(cache_timestamp_epoch_ms)),
                &flags,
                None,
            );
        if snapshot.report_fetched_at.is_some() && snapshot.report_notice_date.is_some() {
            item.verification_status = "source_reported".to_owned();
            if snapshot.roe.is_none() {
                item.quality_flags.push(if snapshot.latest_bps.is_some_and(|bps|bps <= 0.0) {
                    "roe_not_applicable_non_positive_equity"
                } else { "reported_roe_missing" }.to_owned());
            }
            if snapshot.latest_eps.is_none() {item.quality_flags.push("reported_eps_missing".to_owned());}
            if reference.and_then(|reference| reference.pointer("/financial/latest_report_period")).and_then(Value::as_str)
                .is_some_and(|latest| Some(latest) != period.as_deref()) {
                item.quality_flags.push("latest_available_report".to_owned());
            }
        }
        let key = (source.to_string(),period);
        if let Some(existing) = financial_source_groups.get_mut(&key) {
            existing.fetched_at = existing.fetched_at.clone().max(item.fetched_at);
            existing.disclosed_at = existing.disclosed_at.clone().max(item.disclosed_at);
            existing.quality_flags.extend(item.quality_flags);
            existing.quality_flags.sort();existing.quality_flags.dedup();
        } else {financial_source_groups.insert(key,item);}
    }
    let mut financial_sources: Vec<_> = financial_source_groups.into_values().collect();
    financial_sources.sort_by(|left,right| right.report_period.cmp(&left.report_period));
    let mut backtest_sources = Vec::new();
    let factor_source_ids = data
        .factor_snapshots
        .values()
        .flatten()
        .filter_map(|snapshot| snapshot.source.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for source in factor_source_ids {
        backtest_sources.push(source_evidence(
            source,
            "factor_snapshot",
            MarketId::Cn,
            None,
            None,
            None,
            None,
            CapabilityFreshness::Unknown,
            &[
                "provider_rights_unverified",
                "delisted_universe_unverified",
                "corporate_actions_unverified",
            ],
            Some(backtest_coverage.clone()),
        ));
    }
    if !data.histories.is_empty() {
        backtest_sources.push(source_evidence(
            "legacy_market_cache",
            "daily_history",
            MarketId::Cn,
            None,
            backtest_coverage.end_date.clone(),
            None,
            None,
            CapabilityFreshness::Unknown,
            &["provider_rights_unverified"],
            Some(backtest_coverage.clone()),
        ));
    }
    let cn_external = |domains: &[&str]| external_cn_sources(external_status, domains);
    directory_sources.extend(cn_external(&["security_master"]));

    let mut output = vec![
        evidence(
            CapabilityId::SecurityDirectory,
            universe_count > 0,
            false,
            false,
            data.stocks.len() as u64,
            Some(universe_count),
            Some(directory_coverage),
            Some("A-share cache is readable, but complete exchange-universe coverage and public-app data rights are not evidenced"),
        ),
        evidence(
            CapabilityId::Observe,
            universe_count > 0,
            false,
            false,
            history_symbols,
            Some(universe_count),
            Some(observe_coverage.clone()),
            Some("A-share observation uses cached quote/history; provider rights and per-security history freshness are not evidenced"),
        ),
        evidence(
            CapabilityId::FinancialSnapshot,
            universe_count > 0,
            false,
            false,
            financial_symbols,
            Some(universe_count),
            Some(financial_coverage),
            Some("Cached financial snapshots exist for only part of the universe; provider rights and point-in-time publication dates are not evidenced"),
        ),
        evidence(
            CapabilityId::Screening,
            universe_count > 0,
            false,
            false,
            screening_symbols,
            Some(universe_count),
            Some(screening_coverage),
            Some("Cached quotes and financial fields do not establish complete screening coverage or public-app data rights"),
        ),
        evidence(
            CapabilityId::Backtest,
            universe_count > 0,
            false,
            false,
            pit_symbols,
            Some(universe_count),
            Some(backtest_coverage.clone()),
            Some("Strict release backtests still require verified point-in-time publication dates, delisted securities, corporate actions, and authorized history"),
        ),
        evidence(
            CapabilityId::NewsResearch,
            false,
            false,
            false,
            0,
            None,
            Some(cache_coverage(0, 0, &["market_scoped_news_source", "cache_and_rag_rights"], None, None)),
            Some("No authorized market-scoped news source and reuse permission are registered"),
        ),
        evidence(
            CapabilityId::Sentiment,
            false,
            false,
            false,
            0,
            None,
            Some(cache_coverage(0, 0, &["market_scoped_news_evidence", "model_processing_rights"], None, None)),
            Some("A-share sentiment evidence coverage and permitted model-processing scope are not registered"),
        ),
        evidence(
            CapabilityId::ResearchAgent,
            false,
            false,
            false,
            0,
            None,
            Some(cache_coverage(0, 0, &["market_scoped_agent_tools", "source_rights_policy"], None, None)),
            Some("The research Agent does not yet report a verified market-scoped tool/source contract"),
        ),
    ];
    if directory_checked {
        if let Some(item)=output.iter_mut().find(|item|item.id==CapabilityId::SecurityDirectory) {
            item.release_gate_open=directory_freshness==CapabilityFreshness::Fresh;item.coverage_complete=true;
            if !item.release_gate_open {item.reason=Some("证券目录缓存已过期，请刷新股票池。".to_string());}
        }
        if let Some(item)=output.iter_mut().find(|item|item.id==CapabilityId::Observe) {
            item.reason=Some("覆盖数量为已缓存日线的证券数；选择股票后按需补取历史行情。".to_string());
        }
        let research_symbols=data.stocks.iter().filter(|stock|stock.price.is_finite() && stock.price>0.0
            && valid_history_bar_count(data,&stock.code)>=crate::screening::MIN_BACKTEST_HISTORY_BARS).count() as u64;
        if let Some(item)=output.iter_mut().find(|item|item.id==CapabilityId::Backtest) {
            item.release_gate_open=quote_observed>0;
            item.coverage_complete=false;
            item.usable_records=quote_observed;
            item.coverage=Some(cache_coverage(universe_count,research_symbols,
                &["research_history_on_demand","point_in_time_financial_availability","delisted_universe","corporate_actions","exchange_calendar_and_trading_restrictions","strict_out_of_sample_fold_count"],
                backtest_coverage.start_date.clone(),backtest_coverage.end_date.clone()));
            item.reason=Some("当前名单研究回测可运行，日线按所选股票与区间准备；严格回测仍需历史名单、点时财报与公司行动。".to_string());
            for source in &mut backtest_sources {
                source.quality_flags.push("research_backtest_current_universe".to_string());
            }
        }
    }
    if financial_checked {
        for item in &mut output {
            match item.id {
                CapabilityId::FinancialSnapshot => {
                    item.release_gate_open=financial_freshness==CapabilityFreshness::Fresh;item.coverage_complete=financial_symbols==universe_count;
                    item.reason=Some(if item.release_gate_open {"部分证券的核心财务指标暂缺；已有指标使用最近公开报告。"}else{"财务快照检查时间已过期，请刷新股票池。"}.to_string());
                }
                CapabilityId::Screening => {
                    item.release_gate_open=directory_checked && directory_freshness==CapabilityFreshness::Fresh && !stale;
                    item.coverage_complete=screening_symbols==universe_count;
                    item.reason=Some(if stale {"A 股行情已过期，请刷新行情后筛选。"} else {"财务源未提供部分筛选指标；亏损或负净资产指标按不适用处理。"}.to_string());
                }
                _ => {}
            }
        }
    }
    for item in &mut output {
        item.sources = deduplicate_sources(match item.id {
            CapabilityId::SecurityDirectory => directory_sources.clone(),
            CapabilityId::Observe => {
                let mut sources = if quote_expected > 0 {
                    vec![quote_source.clone()]
                } else {
                    Vec::new()
                };
                if !data.histories.is_empty() {
                    sources.push(source_evidence(
                        "legacy_market_cache",
                        "daily_history",
                        MarketId::Cn,
                        None,
                        observe_coverage.end_date.clone(),
                        None,
                        None,
                        CapabilityFreshness::Unknown,
                        &[
                            "provider_rights_unverified",
                            "per_security_freshness_unknown",
                        ],
                        item.coverage.clone(),
                    ));
                }
                sources.extend(cn_external(&["quote", "daily_history"]));
                sources
            }
            CapabilityId::FinancialSnapshot => {
                let mut sources = financial_sources.clone();
                sources.extend(cn_external(&["financial_snapshot"]));
                sources
            }
            CapabilityId::Screening => {
                let mut sources = vec![quote_source.clone()];
                sources.extend(financial_sources.clone());
                sources.extend(cn_external(&[
                    "quote",
                    "financial_snapshot",
                    "factor_snapshot",
                ]));
                sources
            }
            CapabilityId::Backtest => {
                let mut sources = backtest_sources.clone();
                sources.extend(financial_sources.clone());
                sources.extend(cn_external(&[
                    "daily_history",
                    "factor_snapshot",
                    "financial_snapshot",
                ]));
                sources
            }
            CapabilityId::NewsResearch => cn_external(&["news", "disclosure"]),
            CapabilityId::Sentiment => cn_external(&["news", "capital_evidence"]),
            CapabilityId::ResearchAgent => Vec::new(),
        });
    }
    output
}

fn cn_evidence(app: &AppHandle) -> Result<Vec<CapabilityEvidence>, String> {
    let status = crate::market::market_data_status(app)?;
    let data = crate::market::cached_market_data_snapshot(app)
        .map(|snapshot| snapshot.as_ref().clone())
        .unwrap_or_default();
    let external_status = crate::external_data::status_for_app(app)?;
    Ok(cn_evidence_from_cache(&status, &data, &external_status))
}

fn hk_evidence(app: &AppHandle) -> Result<Vec<CapabilityEvidence>, String> {
    let path = crate::hk_market::hk_market_data_path(app)?;
    let cache = crate::hk_market::read_hk_market_cache(&path)?;
    let store = crate::external_data::open_app_store(app)?;
    let news = store.read_domain_records(
        crate::external_data::DataDomain::News,
        crate::external_data::Market::HongKong,
    )?;
    hk_evidence_from_cache_with_news(&cache, &news)
}

fn cache_timestamp_epoch_ms(value: &str) -> Option<u128> {
    value
        .parse::<u128>()
        .ok()
        .or_else(|| crate::market::parse_cache_datetime_epoch_ms(value))
}

fn freshness_from_timestamp(now: u128, fetched_at: Option<u128>) -> CapabilityFreshness {
    match fetched_at {
        Some(fetched) if now.saturating_sub(fetched) <= 24 * 60 * 60 * 1000 => {
            CapabilityFreshness::Fresh
        }
        Some(_) => CapabilityFreshness::Stale,
        None => CapabilityFreshness::Unknown,
    }
}

fn hk_quote_observed_date(raw: &str) -> Option<String> {
    if let Ok(observed) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(observed.with_timezone(&chrono::FixedOffset::east_opt(8 * 3600)?).format("%Y%m%d").to_string());
    }
    let date = crate::market::compact_date_key(raw)?;
    chrono::NaiveDate::parse_from_str(&date, "%Y%m%d").ok().map(|date| date.format("%Y%m%d").to_string())
}

fn hk_coverage_freshness(values: impl IntoIterator<Item = CapabilityFreshness>, expected: u64) -> CapabilityFreshness {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.contains(&CapabilityFreshness::Stale) {
        CapabilityFreshness::Stale
    } else if expected == 0 || values.len() as u64 != expected || values.contains(&CapabilityFreshness::Unknown) {
        CapabilityFreshness::Unknown
    } else {
        CapabilityFreshness::Fresh
    }
}

fn hk_evidence_from_cache(
    cache: &crate::hk_market::HkMarketCache,
) -> Result<Vec<CapabilityEvidence>, String> {
    hk_evidence_from_cache_with_news(cache, &[])
}

fn hk_evidence_from_cache_with_news(
    cache: &crate::hk_market::HkMarketCache,
    news_records: &[crate::external_data::StoredRecord],
) -> Result<Vec<CapabilityEvidence>, String> {
    let expected = cache.securities.len() as u64;
    let directory_readable = expected > 0
        && !cache.securities_source.trim().is_empty()
        && cache.securities_updated_at.is_some();
    let directory_ready = directory_readable && cache.securities_last_error.is_none();
    let eligible_ordinary_count = cache
        .securities
        .iter()
        .filter(|security| security.classification_status.allows_observation())
        .count() as u64;
    let eligible_hkd_count = cache
        .securities
        .iter()
        .filter(|security| security.supports_hkd_research())
        .count() as u64;
    let observe_count = cache
        .securities
        .iter()
        .filter(|security| {
            if !security.supports_hkd_research() {
                return false;
            }
            let has_quote = cache
                .quotes
                .get(&security.code)
                .map(|quote| {
                    quote.last_error.is_none()
                        && security.cached_quote_matches_security_master(quote)
                })
                .unwrap_or(false);
            let has_history = cache
                .histories
                .get(&security.code)
                .map(|history| !history.bars.is_empty() && history.last_error.is_none())
                .unwrap_or(false);
            has_quote
                && has_history
                && !cache.quote_errors.contains_key(&security.code)
                && !cache.history_errors.contains_key(&security.code)
        })
        .count() as u64;
    let projected = crate::hk_research::project_hk_market_data(cache)?;
    let projected_count = projected.stocks.len() as u64;
    let history_count = projected
        .stocks
        .iter()
        .filter(|stock| {
            projected
                .histories
                .get(&stock.code)
                .is_some_and(|bars| !bars.is_empty())
        })
        .count() as u64;
    let eligible_codes = cache
        .securities
        .iter()
        .filter(|security| security.classification_status.allows_observation())
        .map(|security| format!("{}.HK", security.code))
        .collect::<std::collections::HashSet<_>>();
    let news_codes = news_records
        .iter()
        .filter(|record| record.record_key == "item")
        .flat_map(|record| {
            record
                .payload
                .get("stock_codes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|code| eligible_codes.contains(code))
        .collect::<std::collections::HashSet<_>>();
    let news_count = news_codes.len() as u64;
    let fx_ready = cache
        .fx_rate
        .as_ref()
        .map(|rate| rate.rate.is_finite() && rate.rate > 0.0 && rate.last_error.is_none())
        .unwrap_or(false);
    let financial_count = if fx_ready {
        cache
            .securities
            .iter()
            .filter(|security| {
                if !security.supports_hkd_research() {
                    return false;
                }
                let verified_snapshot = cache
                    .financials
                    .get(&security.code)
                    .map(|snapshot| {
                        snapshot.verification_status == "verified"
                            && snapshot.field_errors.is_empty()
                    })
                    .unwrap_or(false);
                verified_snapshot
                    && cache
                        .disclosures
                        .get(&security.code)
                        .map(|disclosure| {
                            disclosure.status != "unavailable" && disclosure.error.is_none()
                        })
                        .unwrap_or(false)
                    && !cache.financial_errors.contains_key(&security.code)
                    && !cache.disclosure_errors.contains_key(&security.code)
            })
            .count() as u64
    } else {
        0
    };
    let financial_expected = eligible_hkd_count;
    let coverage = |requested: u64, available: u64| CoverageSummary {
        requested_count: requested,
        available_count: available,
        missing_count: requested.saturating_sub(available),
        start_date: None,
        end_date: None,
        missing_fields: Vec::new(),
    };
    let mut projected_coverage = CoverageSummary {
        requested_count: eligible_hkd_count,
        available_count: projected_count,
        missing_count: eligible_hkd_count.saturating_sub(projected_count),
        start_date: projected
            .histories
            .values()
            .flat_map(|bars| bars.iter().map(|bar| bar.date.clone()))
            .min(),
        end_date: projected
            .histories
            .values()
            .flat_map(|bars| bars.iter().map(|bar| bar.date.clone()))
            .max(),
        missing_fields: if projected_count < eligible_hkd_count {
            vec!["currency_matched_cached_quote".to_owned()]
        } else {
            Vec::new()
        },
    };
    let now = crate::hk_market::current_epoch_millis()
        .parse::<u128>()
        .unwrap_or_default();
    let expected_quote_date = crate::market::expected_market_quote_date_from_epoch_ms(now);
    let per_security_freshness = projected.stocks.iter().map(|stock| {
        let code = stock.code.strip_suffix(".HK").unwrap_or_default();
        let quote = cache.quotes.get(code);
        let downloaded = freshness_from_timestamp(now, quote.and_then(|quote| quote.fetched_at.as_deref()).and_then(cache_timestamp_epoch_ms));
        let observed = quote.and_then(|quote| quote.quote_time.as_deref()).and_then(hk_quote_observed_date);
        let observation_freshness = match observed.as_ref().zip(expected_quote_date.as_ref()) {
            Some((observed, expected)) if observed == expected => CapabilityFreshness::Fresh,
            Some(_) => CapabilityFreshness::Stale,
            None => CapabilityFreshness::Unknown,
        };
        let quote_freshness = if cache.quote_errors.contains_key(code) || quote.is_some_and(|quote| quote.last_error.is_some()) {
            CapabilityFreshness::Stale
        } else {
            hk_coverage_freshness([downloaded, observation_freshness], 2)
        };
        // History contributes to Observe's release gate. Every projected security needs
        // its own prepared history download; the newest record cannot cover the others.
        // Existing prepared/suspension handling in project_hk_market_data is retained.
        let history_freshness = match cache.histories.get(code).filter(|_| projected.histories.contains_key(&stock.code)) {
            Some(history) if history.last_error.is_some() || cache.history_errors.contains_key(code) => CapabilityFreshness::Stale,
            Some(history) => freshness_from_timestamp(now, cache_timestamp_epoch_ms(&history.fetched_at)),
            None => CapabilityFreshness::Unknown,
        };
        (quote_freshness, history_freshness, downloaded, observation_freshness)
    }).collect::<Vec<_>>();
    let quote_freshness = hk_coverage_freshness(per_security_freshness.iter().map(|row| row.0), eligible_hkd_count);
    let history_freshness = hk_coverage_freshness(per_security_freshness.iter().map(|row| row.1), projected_count);
    let current_quote_count = per_security_freshness.iter().filter(|row| row.0 == CapabilityFreshness::Fresh).count() as u64;
    let current_history_count = per_security_freshness.iter().filter(|row| row.1 == CapabilityFreshness::Fresh).count() as u64;
    let current_observe_count = per_security_freshness.iter().filter(|row| row.0 == CapabilityFreshness::Fresh && row.1 == CapabilityFreshness::Fresh).count() as u64;
    projected_coverage.available_count = current_quote_count;
    projected_coverage.missing_count = eligible_hkd_count.saturating_sub(current_quote_count);
    if per_security_freshness.iter().any(|row| row.3 != CapabilityFreshness::Fresh) {
        projected_coverage.missing_fields.push("current_quote_observation".to_owned());
    }
    if per_security_freshness.iter().any(|row| row.2 != CapabilityFreshness::Fresh) {
        projected_coverage.missing_fields.push("per_security_quote_fetch_time".to_owned());
    }
    if per_security_freshness.iter().any(|row| row.0 == CapabilityFreshness::Stale && row.2 == CapabilityFreshness::Fresh && row.3 == CapabilityFreshness::Fresh) {
        projected_coverage.missing_fields.push("successful_quote_refresh".to_owned());
    }
    let freshness = quote_freshness;
    let quote_fetched_at = projected
        .stocks
        .iter()
        .filter_map(|stock| stock.code.strip_suffix(".HK"))
        .filter_map(|code| cache.quotes.get(code))
        .filter_map(|quote| quote.fetched_at.clone())
        .max();
    let quote_observed_at = projected
        .stocks
        .iter()
        .filter_map(|stock| stock.code.strip_suffix(".HK"))
        .filter_map(|code| cache.quotes.get(code))
        .filter_map(|quote| quote.quote_time.clone())
        .max();
    let quote_sources = if projected_count == 0 {
        Vec::new()
    } else {
        let user_confirmed_count = cache
            .securities
            .iter()
            .filter(|security| {
                security.classification_status
                    == crate::hk_market::HkClassificationStatus::UserConfirmedOrdinaryShare
            })
            .count();
        let exchange_listed_count = cache
            .securities
            .iter()
            .filter(|security| {
                security.classification_status
                    == crate::hk_market::HkClassificationStatus::ExchangeListedEquity
            })
            .count();
        let classification_flag = if user_confirmed_count > 0 {
            "ordinary_share_user_confirmed_records_present"
        } else if exchange_listed_count > 0 {
            "hkex_listed_equity_class_unresolved"
        } else {
            "ordinary_share_source_verified"
        };
        vec![source_evidence(
            "hk_market_cache",
            "quote",
            MarketId::Hk,
            quote_fetched_at,
            quote_observed_at,
            None,
            None,
            quote_freshness,
            &[classification_flag, "currency_matched"],
            Some(projected_coverage.clone()),
        )]
    };
    let history_sources = if history_count == 0 {
        Vec::new()
    } else {
        vec![source_evidence(
            "hk_market_cache",
            "daily_history",
            MarketId::Hk,
            projected
                .histories
                .keys()
                .filter_map(|code| code.strip_suffix(".HK"))
                .filter_map(|code| cache.histories.get(code))
                .map(|history| history.fetched_at.clone())
                .max(),
            projected_coverage.end_date.clone(),
            None,
            None,
            history_freshness,
            &["ordinary_share_verified", "corporate_actions_unverified"],
            Some(CoverageSummary {
                requested_count: projected_count,
                available_count: current_history_count,
                missing_count: projected_count.saturating_sub(current_history_count),
                missing_fields: (current_history_count < projected_count).then(|| "per_security_history_fetch_time".to_owned()).into_iter().collect(),
                ..projected_coverage.clone()
            }),
        )]
    };
    let directory_reason = if directory_ready {
        "HKEX security directory is cached and ready"
    } else if directory_readable {
        "Using the last cached HKEX directory because the most recent refresh failed"
    } else {
        "HKEX directory is empty; refresh the directory to begin"
    };

    let mut output = vec![
        evidence(
            CapabilityId::SecurityDirectory,
            directory_readable,
            true,
            directory_ready,
            expected,
            Some(expected),
            Some(coverage(expected, expected)),
            (!directory_ready).then_some(directory_reason),
        ),
        evidence(
            CapabilityId::Observe,
            true,
            quote_freshness == CapabilityFreshness::Fresh && history_freshness == CapabilityFreshness::Fresh,
            observe_count > 0
                && observe_count == eligible_hkd_count
                && quote_freshness == CapabilityFreshness::Fresh
                && history_freshness == CapabilityFreshness::Fresh,
            observe_count,
            Some(eligible_hkd_count),
            Some(CoverageSummary {
                available_count: current_observe_count,
                missing_count: eligible_hkd_count.saturating_sub(current_observe_count),
                missing_fields: {
                    let mut fields = projected_coverage.missing_fields.clone();
                    if current_history_count < projected_count { fields.push("per_security_history_fetch_time".to_owned()); }
                    fields
                },
                ..projected_coverage.clone()
            }),
            if observe_count == 0 {
                Some("refresh the HKEX directory to batch-load HKD equity quotes; individual daily history loads on observation")
            } else if current_observe_count < eligible_hkd_count {
                Some("HK observation requires current per-security quote observations and fresh prepared history downloads")
            } else { None },
        ),
        evidence(
            CapabilityId::FinancialSnapshot,
            true,
            true,
            financial_count > 0 && financial_count == financial_expected,
            financial_count,
            Some(financial_expected),
            Some(CoverageSummary {
                requested_count: financial_expected,
                available_count: financial_count,
                missing_count: financial_expected.saturating_sub(financial_count),
                start_date: None,
                end_date: None,
                missing_fields: if financial_count < financial_expected {
                    vec!["per_security_financial_field_coverage".to_owned()]
                } else {
                    Vec::new()
                },
            }),
            (financial_count < financial_expected).then_some("candidate financial fields are incomplete or lack a currency reference"),
        ),
        evidence(
            CapabilityId::Screening,
            true,
            freshness == CapabilityFreshness::Fresh,
            projected_count > 0
                && projected_count == eligible_hkd_count
                && freshness == CapabilityFreshness::Fresh,
            projected_count,
            Some(eligible_hkd_count),
            Some(projected_coverage.clone()),
            if projected_count == 0 {
                Some("no HKEX-listed equity has a cached, currency-matched quote; refresh the HKEX directory to batch-load quotes")
            } else if freshness != CapabilityFreshness::Fresh {
                Some("cached HK screening quote observations or per-security download times are stale or unknown")
            } else {
                None
            },
        ),
        evidence(
            CapabilityId::Backtest,
            true,
            true,
            false,
            history_count,
            Some(projected_count),
            Some(CoverageSummary {
                requested_count: projected_count,
                available_count: history_count,
                missing_count: projected_count.saturating_sub(history_count),
                start_date: projected_coverage.start_date.clone(),
                end_date: projected_coverage.end_date.clone(),
                missing_fields: vec!["survivorship_bias".to_owned(), "corporate_action_adjustment".to_owned(), "point_in_time_financials".to_owned()],
            }),
            Some("exploratory candidate_snapshot only; survivorship, corporate-action adjustment, and point-in-time financial evidence are incomplete"),
        ),
        evidence(
            CapabilityId::NewsResearch,
            true,
            true,
            news_count > 0 && news_count == eligible_ordinary_count,
            news_count,
            Some(eligible_ordinary_count),
            Some(CoverageSummary {
                requested_count: eligible_ordinary_count,
                available_count: news_count,
                missing_count: eligible_ordinary_count.saturating_sub(news_count),
                start_date: None,
                end_date: None,
                missing_fields: if news_count < eligible_ordinary_count {
                    vec!["per_security_news_coverage".to_owned()]
                } else { Vec::new() },
            }),
            (news_count == 0).then_some("observe an HKEX-listed equity to collect Sina/Eastmoney news"),
        ),
        evidence(
            CapabilityId::Sentiment,
            true,
            true,
            news_count > 0 && news_count == eligible_ordinary_count,
            news_count,
            Some(eligible_ordinary_count),
            Some(CoverageSummary {
                requested_count: eligible_ordinary_count,
                available_count: news_count,
                missing_count: eligible_ordinary_count.saturating_sub(news_count),
                start_date: None,
                end_date: None,
                missing_fields: if news_count < eligible_ordinary_count {
                    vec!["per_security_sentiment_evidence".to_owned()]
                } else { Vec::new() },
            }),
            (news_count == 0).then_some("collect HK market-scoped news before running sentiment analysis"),
        ),
        evidence(
            CapabilityId::ResearchAgent,
            true,
            freshness == CapabilityFreshness::Fresh,
            projected_count > 0 && freshness == CapabilityFreshness::Fresh,
            projected_count,
            Some(eligible_hkd_count),
            Some(projected_coverage.clone()),
            if projected_count == 0 {
                Some("refresh the HKEX directory to batch-load HKD equity quotes before using the research Agent")
            } else if freshness != CapabilityFreshness::Fresh {
                Some("HK Agent market evidence is stale")
            } else {
                None
            },
        ),
    ];
    for item in &mut output {
        item.quote_scope = matches!(
            item.id,
            CapabilityId::Observe
                | CapabilityId::Screening
                | CapabilityId::Backtest
                | CapabilityId::ResearchAgent
        )
        .then(|| QuoteCoverageScope {
            currency: "HKD".to_owned(),
            excluded_count: eligible_ordinary_count.saturating_sub(eligible_hkd_count),
        });
        item.sources = match item.id {
            CapabilityId::Screening => quote_sources.clone(),
            CapabilityId::Observe => {
                let mut sources = quote_sources.clone();
                sources.extend(history_sources.clone());
                sources
            }
            CapabilityId::Backtest => {
                let mut sources = quote_sources.clone();
                sources.extend(history_sources.clone());
                sources
            }
            CapabilityId::NewsResearch | CapabilityId::Sentiment => {
                let mut sources = Vec::new();
                for record in news_records
                    .iter()
                    .filter(|record| record.record_key == "item")
                {
                    let source = &record.metadata.source_id;
                    if sources
                        .iter()
                        .any(|item: &CapabilitySourceEvidence| item.source_id == *source)
                    {
                        continue;
                    }
                    sources.push(source_evidence(
                        source,
                        "news",
                        MarketId::Hk,
                        Some(record.metadata.fetched_at.clone()),
                        record.metadata.observed_at.clone(),
                        record.metadata.disclosed_at.clone(),
                        None,
                        CapabilityFreshness::Unknown,
                        &["hk_market_scoped", "provider_rights_unverified"],
                        None,
                    ));
                }
                sources
            }
            CapabilityId::ResearchAgent => quote_sources.clone(),
            _ => Vec::new(),
        };
    }
    Ok(output)
}

pub(crate) fn collect_market_capability_evidence(
    app: &AppHandle,
    market: MarketId,
) -> Result<Vec<CapabilityEvidence>, String> {
    match market {
        MarketId::Cn => cn_evidence(app),
        MarketId::Hk => hk_evidence(app),
    }
}

#[tauri::command]
pub(crate) async fn api_market_capabilities(
    app: AppHandle,
    market: String,
) -> Result<Vec<MarketCapability>, String> {
    let market = parse_market(&market)
        .ok_or_else(|| "unsupported market; expected one of: cn, hk".to_owned())?;
    tauri::async_runtime::spawn_blocking(move || {
        let evidence = collect_market_capability_evidence(&app, market)?;
        Ok(capabilities_for_market(market, &evidence))
    })
    .await
    .map_err(|error| format!("market capability worker failed: {error}"))?
}

fn parse_market(value: &str) -> Option<MarketId> {
    MarketId::parse(value)
}

#[cfg(test)]
mod tests {
    use super::{
        capabilities_for_market, cn_evidence_from_cache, hk_evidence_from_cache, parse_market,
        CapabilityEvidence,
    };
    use crate::market_domain::{
        CapabilityFreshness, CapabilityId, CapabilityStatus, CoverageSummary, MarketId,
    };
    use serde_json::{json, Value};
    use stock_optimizer_core as gp_core;

    fn stock(code: &str, quote_time: &str) -> gp_core::StockItem {
        gp_core::StockItem {
            code: code.to_owned(),
            name: format!("Stock {code}"),
            industry: "industry".to_owned(),
            price: 10.0,
            quote_time: Some(quote_time.to_owned()),
            ..gp_core::StockItem::default()
        }
    }

    fn history(date: &str) -> Vec<gp_core::HistoryBar> {
        let end_day = date[6..].parse::<u32>().unwrap_or(3);
        (0..crate::observe::MIN_OBSERVE_HISTORY_BARS)
            .map(|offset| gp_core::HistoryBar {
                date: format!(
                    "{}{:02}",
                    &date[..6],
                    end_day.saturating_sub((2 - offset) as u32)
                ),
                open: Some(9.0),
                high: Some(11.0),
                low: Some(8.0),
                close: 10.0,
                volume: Some(100.0),
                capital: None,
            })
            .collect()
    }

    fn status(universe: u64, requested: u64, observed: u64, stale: bool) -> Value {
        json!({
            "source": "tencent",
            "universe_count": universe,
            "quote_requested": requested,
            "quote_observed": observed,
            "quote_generated_at": "2026-10-27T08:00:00Z",
            "quote_trade_date": "20261027",
            "stale": stale,
        })
    }

    fn evidence_for(
        data: gp_core::CoreDataSet,
        status: Value,
        external: Value,
    ) -> Vec<CapabilityEvidence> {
        cn_evidence_from_cache(&status, &data, &external)
    }

    fn capability<'a>(items: &'a [CapabilityEvidence], id: CapabilityId) -> &'a CapabilityEvidence {
        items.iter().find(|item| item.id == id).unwrap()
    }

    #[test]
    fn cn_empty_cache_reports_no_market_capability_evidence() {
        let items = evidence_for(
            gp_core::CoreDataSet::default(),
            status(0, 0, 0, true),
            json!({"sources": []}),
        );

        assert_eq!(
            capabilities_for_market(MarketId::Cn, &items)
                .iter()
                .find(|item| item.id == CapabilityId::SecurityDirectory)
                .unwrap()
                .status,
            CapabilityStatus::Unavailable
        );
        assert_eq!(capability(&items, CapabilityId::Observe).usable_records, 0);
        assert!(capability(&items, CapabilityId::Observe).sources.is_empty());
    }

    #[test]
    fn cn_data_research_backtest_reports_cached_history_separately_from_strict_pit_data() {
        let mut data=gp_core::CoreDataSet::default();
        data.stocks=vec![stock("000001.SZ","20261106"),stock("600000.SH","20261106")];
        data.histories.insert("000001.SZ".to_string(),history("20261106"));
        let now=crate::market::epoch_millis().to_string();
        let status=json!({"universe_count":2,"stale":false,"cn_reference":{"directory":{"checked":true,"count":2,"fetched_at":now}}});
        let items=evidence_for(data,status,json!({"sources":[]}));
        let backtest=capabilities_for_market(MarketId::Cn,&items).into_iter().find(|item|item.id==CapabilityId::Backtest).unwrap();
        assert_eq!(backtest.status,CapabilityStatus::Limited);
        assert_eq!(backtest.coverage.as_ref().unwrap().available_count,1);
        assert_eq!(backtest.coverage.as_ref().unwrap().requested_count,2);
        assert!(backtest.coverage.as_ref().unwrap().missing_fields.contains(&"point_in_time_financial_availability".to_string()));
        assert!(backtest.reason.as_deref().unwrap().contains("研究回测"));
    }

    #[test]
    fn cn_data_checked_local_coverage_is_available_without_claiming_distribution_rights() {
        let mut data=gp_core::CoreDataSet::default();
        let fetched_at=crate::market::epoch_millis().to_string();
        let mut company=stock("000002.SZ","20261106");
        company.pe=Some(-6.3);company.pb=Some(0.4);company.roe=Some(-0.228);
        data.stocks.push(company);
        data.financials.insert("000002.SZ".to_string(),serde_json::from_value(json!({
            "period":"2026Q3","source":"eastmoney_cn_financial","currency":"CNY","latest_eps":-2.01,"latest_bps":7.8,
            "operating_revenue_billion":1049.98,"parent_net_profit_billion":-239.43,"roe":-22.8,
            "report_notice_date":"2026-10-30","report_fetched_at":fetched_at
        })).unwrap());
        let status=json!({"source":"tencent","universe_count":1,"quote_generated_at":fetched_at,"quote_trade_date":"20261106","stale":false,
            "cn_reference":{"directory":{"checked":true,"count":1,"fetched_at":fetched_at,"scope":"sse_szse_listed_bse_quote","sources":["sse","szse","eastmoney_cn_directory"]},
                "financial":{"fetched_at":fetched_at,"latest_report_period":"2026Q3","requested_count":1,"fetched_count":1,"errors":[]}}});
        let items=evidence_for(data,status,json!({"sources":[]}));
        let capabilities=capabilities_for_market(MarketId::Cn,&items);
        for id in [CapabilityId::SecurityDirectory,CapabilityId::FinancialSnapshot,CapabilityId::Screening] {
            let capability=capabilities.iter().find(|item|item.id==id).unwrap();
            assert_eq!(capability.status,CapabilityStatus::Available);
            assert!(!capability.coverage.as_ref().unwrap().missing_fields.contains(&"provider_rights".to_string()));
            assert!(capability.sources.iter().any(|source|source.quality_flags.contains(&"provider_rights_unverified".to_string())));
        }
        let financial=capabilities.iter().find(|item|item.id==CapabilityId::FinancialSnapshot).unwrap();
        assert!(financial.sources.iter().any(|source|source.report_period.as_deref()==Some("2026Q3") && source.disclosed_at.as_deref()==Some("2026-10-30") && source.fetched_at.is_some()));
    }

    #[test]
    fn cn_fresh_quote_and_history_evidence_is_limited_until_rights_are_verified() {
        let mut data = gp_core::CoreDataSet::default();
        data.stocks = vec![stock("600000", "20261027"), stock("000001", "20261027")];
        data.histories
            .insert("600000".to_owned(), history("20261027"));
        data.histories
            .insert("000001".to_owned(), history("20261027"));
        let external = json!({"sources": [
            {"source_id":"tencent","domain":"quote","market":"cn","last_success_at":"2026-10-27T08:00:00Z","observed_through":"2026-10-27","record_count":2},
            {"source_id":"hkex","domain":"quote","market":"hk","last_success_at":"2026-10-27T08:00:00Z","record_count":1},
            {"source_id":"frankfurter","domain":"fx_rate","market":"global","last_success_at":"2026-10-27T08:00:00Z","record_count":1}
        ]});

        let items = evidence_for(data, status(2, 2, 2, false), external);
        let observe = capability(&items, CapabilityId::Observe);
        let sources = capabilities_for_market(MarketId::Cn, &items)
            .into_iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap()
            .sources;

        assert_eq!(observe.usable_records, 2);
        assert_eq!(observe.expected_records, Some(2));
        assert!(sources.iter().all(|source| source.market == MarketId::Cn));
        assert!(sources.iter().any(|source| {
            source.source_id == "tencent"
                && source.domain == "quote"
                && source.freshness == CapabilityFreshness::Fresh
                && source.verification_status == "unverified"
                && source
                    .quality_flags
                    .contains(&"provider_rights_unverified".to_owned())
        }));
        let capabilities = capabilities_for_market(MarketId::Cn, &items);
        assert_eq!(
            capabilities
                .iter()
                .find(|item| item.id == CapabilityId::Observe)
                .unwrap()
                .status,
            CapabilityStatus::Limited
        );
        assert!(capabilities
            .iter()
            .all(|item| item.status != CapabilityStatus::Available));
    }

    #[test]
    fn cn_stale_quote_cache_stays_stale_in_source_evidence() {
        let mut data = gp_core::CoreDataSet::default();
        data.stocks = vec![stock("600000", "20261024")];
        data.histories
            .insert("600000".to_owned(), history("20261024"));

        let items = evidence_for(data, status(1, 1, 1, true), json!({"sources": []}));
        let observe = capabilities_for_market(MarketId::Cn, &items)
            .into_iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap();

        assert_eq!(observe.status, CapabilityStatus::Limited);
        assert!(observe.sources.iter().any(|source| {
            source.source_id == "tencent"
                && source.freshness == CapabilityFreshness::Stale
                && source
                    .quality_flags
                    .contains(&"cached_quote_stale".to_owned())
        }));
    }

    #[test]
    fn cn_observation_coverage_uses_per_symbol_history_not_quote_request_count() {
        let mut data = gp_core::CoreDataSet::default();
        data.stocks = vec![stock("600000", "20261027"), stock("000001", "20261027")];
        data.histories
            .insert("600000".to_owned(), history("20261027"));

        let items = evidence_for(data, status(2, 100, 90, false), json!({"sources": []}));
        let observe = capabilities_for_market(MarketId::Cn, &items)
            .into_iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap();

        assert_eq!(observe.coverage.as_ref().unwrap().requested_count, 2);
        assert_eq!(observe.coverage.as_ref().unwrap().available_count, 1);
        assert_eq!(observe.coverage.as_ref().unwrap().missing_count, 1);
        assert!(observe
            .coverage
            .as_ref()
            .unwrap()
            .missing_fields
            .contains(&"daily_history".to_owned()));
    }

    #[test]
    fn cn_observation_does_not_count_history_below_the_ui_minimum_depth() {
        let mut data = gp_core::CoreDataSet::default();
        data.stocks = vec![stock("600000", "20261027")];
        data.histories.insert(
            "600000".to_owned(),
            history("20261027").into_iter().take(1).collect(),
        );

        let items = evidence_for(data, status(1, 1, 1, false), json!({"sources": []}));
        let observe = capabilities_for_market(MarketId::Cn, &items)
            .into_iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap();

        assert_eq!(observe.coverage.as_ref().unwrap().available_count, 0);
        assert!(observe
            .coverage
            .as_ref()
            .unwrap()
            .missing_fields
            .contains(&"minimum_history_depth".to_owned()));
    }

    #[test]
    fn cn_financial_and_pit_evidence_comes_from_snapshot_records() {
        let mut data = gp_core::CoreDataSet::default();
        data.stocks = vec![stock("600000", "20261027"), stock("000001", "20261027")];
        data.financials.insert(
            "600000".to_owned(),
            gp_core::StockFinancialSnapshot {
                period: Some("2025Q4".to_owned()),
                source: Some("mootdx".to_owned()),
                latest_eps: Some(0.5),
                ..gp_core::StockFinancialSnapshot::default()
            },
        );
        data.histories
            .insert("600000".to_owned(), history("20261024"));
        data.factor_snapshots.insert(
            "600000".to_owned(),
            vec![gp_core::StockFactorSnapshot {
                date: "20261024".to_owned(),
                available_date: Some("20261025".to_owned()),
                is_listed: Some(true),
                is_tradable: Some(true),
                source: Some("fixture-provider".to_owned()),
                ..gp_core::StockFactorSnapshot::default()
            }],
        );

        let items = evidence_for(data, status(2, 2, 2, false), json!({"sources": []}));
        let financial = capabilities_for_market(MarketId::Cn, &items)
            .into_iter()
            .find(|item| item.id == CapabilityId::FinancialSnapshot)
            .unwrap();
        let backtest = capabilities_for_market(MarketId::Cn, &items)
            .into_iter()
            .find(|item| item.id == CapabilityId::Backtest)
            .unwrap();

        assert_eq!(financial.coverage.as_ref().unwrap().available_count, 1);
        assert_eq!(financial.coverage.as_ref().unwrap().missing_count, 1);
        assert!(financial
            .sources
            .iter()
            .any(|source| source.source_id == "mootdx"
                && source.report_period.as_deref() == Some("2025Q4")));
        assert_eq!(backtest.coverage.as_ref().unwrap().available_count, 0);
        assert!(backtest
            .coverage
            .as_ref()
            .unwrap()
            .missing_fields
            .contains(&"pit_history_depth".to_owned()));
        assert!(backtest
            .coverage
            .as_ref()
            .unwrap()
            .missing_fields
            .contains(&"delisted_universe".to_owned()));
        assert!(backtest
            .coverage
            .as_ref()
            .unwrap()
            .missing_fields
            .contains(&"corporate_actions".to_owned()));
    }

    #[test]
    fn hk_screening_is_unavailable_without_a_registered_strategy_and_data() {
        let result = capabilities_for_market(MarketId::Hk, &[]);
        let screening = result
            .iter()
            .find(|item| item.id == CapabilityId::Screening)
            .unwrap();

        assert_eq!(screening.status, CapabilityStatus::Unavailable);
        assert!(screening.reason.as_deref().unwrap().contains("strategy"));
    }

    #[test]
    fn registered_adapter_with_partial_coverage_is_limited() {
        let result = capabilities_for_market(
            MarketId::Hk,
            &[CapabilityEvidence {
                id: CapabilityId::Observe,
                adapter_registered: true,
                release_gate_open: true,
                coverage_complete: false,
                usable_records: 1,
                expected_records: Some(2),
                coverage: Some(CoverageSummary {
                    requested_count: 2,
                    available_count: 1,
                    missing_count: 1,
                    start_date: None,
                    end_date: None,
                    missing_fields: vec!["daily_history".to_owned()],
                }),
                sources: Vec::new(),
                reason: Some("daily history is missing".to_owned()),
                quote_scope: None,
            }],
        );
        let observe = result
            .iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap();

        assert_eq!(observe.status, CapabilityStatus::Limited);
        assert_eq!(observe.reason.as_deref(), Some("daily history is missing"));
        assert_eq!(observe.coverage.as_ref().unwrap().missing_count, 1);
    }

    #[test]
    fn uncounted_capability_requires_explicit_complete_coverage() {
        let result = capabilities_for_market(
            MarketId::Hk,
            &[CapabilityEvidence {
                id: CapabilityId::FinancialSnapshot,
                adapter_registered: true,
                release_gate_open: true,
                coverage_complete: false,
                usable_records: 1,
                expected_records: None,
                coverage: None,
                sources: Vec::new(),
                reason: None,
                quote_scope: None,
            }],
        );
        let financial = result
            .iter()
            .find(|item| item.id == CapabilityId::FinancialSnapshot)
            .unwrap();

        assert_eq!(financial.status, CapabilityStatus::Limited);
    }

    #[test]
    fn complete_provider_contract_can_enable_uncounted_capability() {
        let result = capabilities_for_market(
            MarketId::Cn,
            &[CapabilityEvidence {
                id: CapabilityId::NewsResearch,
                adapter_registered: true,
                release_gate_open: true,
                coverage_complete: true,
                usable_records: 0,
                expected_records: None,
                coverage: None,
                sources: Vec::new(),
                reason: None,
                quote_scope: None,
            }],
        );
        let news = result
            .iter()
            .find(|item| item.id == CapabilityId::NewsResearch)
            .unwrap();

        assert_eq!(news.status, CapabilityStatus::Available);
    }

    #[test]
    fn complete_flag_does_not_override_a_known_record_count_gap() {
        let result = capabilities_for_market(
            MarketId::Hk,
            &[CapabilityEvidence {
                id: CapabilityId::Observe,
                adapter_registered: true,
                release_gate_open: true,
                coverage_complete: true,
                usable_records: 1,
                expected_records: Some(2),
                coverage: None,
                sources: Vec::new(),
                reason: None,
                quote_scope: None,
            }],
        );
        let observe = result
            .iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap();

        assert_eq!(observe.status, CapabilityStatus::Limited);
    }

    #[test]
    fn cached_records_never_open_a_closed_provider_release_gate() {
        let result = capabilities_for_market(
            MarketId::Hk,
            &[CapabilityEvidence {
                id: CapabilityId::Observe,
                adapter_registered: true,
                release_gate_open: false,
                coverage_complete: true,
                usable_records: 1,
                expected_records: Some(1),
                coverage: None,
                sources: Vec::new(),
                reason: Some("provider terms are unresolved".to_owned()),
                quote_scope: None,
            }],
        );
        let observe = result
            .iter()
            .find(|item| item.id == CapabilityId::Observe)
            .unwrap();

        assert_eq!(observe.status, CapabilityStatus::Limited);
        assert_eq!(
            observe.reason.as_deref(),
            Some("provider terms are unresolved")
        );
    }

    #[test]
    fn rejects_unknown_and_noncanonical_market_ids() {
        assert_eq!(parse_market("cn"), Some(MarketId::Cn));
        assert_eq!(parse_market("hk"), Some(MarketId::Hk));
        assert_eq!(parse_market("hk "), None);
        assert_eq!(parse_market("global"), None);
    }

    fn hk_currency_scope_cache() -> crate::hk_market::HkMarketCache {
        let now = crate::hk_market::current_epoch_millis();
        let mut cache = crate::hk_market::HkMarketCache::default();
        cache.securities_source = "HKEX security directory".to_owned();
        cache.securities_updated_at = Some(now.clone());
        for (code, currency) in [("00700", "HKD"), ("80700", "CNY")] {
            cache.securities.push(
                serde_json::from_value(json!({
                    "code":code,"name":code,"currency":currency,"security_type":"Equity",
                    "classification_status":"exchange_listed_equity",
                }))
                .unwrap(),
            );
        }
        cache.quotes.insert("00700".to_owned(), serde_json::from_value(json!({
            "code":"00700","name":"Tencent","currency":"HKD","price":400.0,
            "price_semantics_version":1,"source":"fixture","quote_time":current_hk_quote_date(),"fetched_at":now,
        })).unwrap());
        cache
    }

    fn current_hk_quote_date() -> String {
        crate::market::expected_market_quote_date_from_epoch_ms(crate::market::epoch_millis()).unwrap()
    }

    fn hk_freshness_cache() -> crate::hk_market::HkMarketCache {
        let mut cache = hk_currency_scope_cache();
        cache.histories.insert("00700".to_owned(), serde_json::from_value(json!({
            "bars":[{"date":current_hk_quote_date(),"open":400.0,"high":401.0,"low":399.0,"close":400.0,"volume_shares":100}],
            "source":"fixture","fetched_at":crate::hk_market::current_epoch_millis()
        })).unwrap());
        cache
    }

    #[test]
    fn hk_freshness_rejects_old_observations_despite_new_downloads() {
        let mut cache = hk_freshness_cache();
        cache.quotes.get_mut("00700").unwrap().quote_time = Some("2020-01-02T08:00:00Z".to_owned());
        let evidence = hk_evidence_from_cache(&cache).unwrap();
        for id in [CapabilityId::Observe, CapabilityId::Screening, CapabilityId::ResearchAgent] {
            assert!(!capability(&evidence, id).release_gate_open);
            assert_eq!(capability(&evidence, id).coverage.as_ref().unwrap().missing_count, 1);
        }
        let source = &capability(&evidence, CapabilityId::Screening).sources[0];
        assert_eq!(source.freshness, CapabilityFreshness::Stale);
        assert_eq!(source.observed_at.as_deref(), Some("2020-01-02T08:00:00Z"));
        assert_eq!(source.fetched_at, cache.quotes["00700"].fetched_at);
        assert!(source.coverage.as_ref().unwrap().missing_fields.contains(&"current_quote_observation".to_owned()));
    }

    #[test]
    fn hk_freshness_does_not_let_one_new_quote_or_history_hide_old_downloads() {
        for old_quote in [false, true] {
            let mut cache = hk_freshness_cache();
            let mut security = cache.securities[0].clone();
            security.code = "09988".to_owned();
            cache.securities.push(security);
            let mut quote = cache.quotes["00700"].clone();
            quote.code = "09988".to_owned();
            if old_quote { quote.fetched_at = Some("1".to_owned()); }
            cache.quotes.insert("09988".to_owned(), quote);
            let mut history = cache.histories["00700"].clone();
            if !old_quote { history.fetched_at = "1".to_owned(); }
            cache.histories.insert("09988".to_owned(), history);
            let evidence = hk_evidence_from_cache(&cache).unwrap();
            assert!(!capability(&evidence, CapabilityId::Observe).release_gate_open);
            assert_eq!(capability(&evidence, CapabilityId::Observe).coverage.as_ref().unwrap().missing_count, 1);
            if old_quote {
                let source = &capability(&evidence, CapabilityId::Screening).sources[0];
                assert_eq!(source.freshness, CapabilityFreshness::Stale);
                assert_eq!(source.coverage.as_ref().unwrap().available_count, 1);
            } else {
                assert!(capability(&evidence, CapabilityId::Screening).release_gate_open);
                let source = capability(&evidence, CapabilityId::Backtest).sources.iter().find(|source| source.domain == "daily_history").unwrap();
                assert_eq!(source.freshness, CapabilityFreshness::Stale);
                assert_eq!(source.coverage.as_ref().unwrap().available_count, 1);
            }
        }
    }

    #[test]
    fn hk_freshness_marks_unknown_observation_and_download_coverage() {
        for unknown_observation in [false, true] {
            let mut cache = hk_freshness_cache();
            let quote = cache.quotes.get_mut("00700").unwrap();
            if unknown_observation { quote.quote_time = None; } else { quote.fetched_at = None; }
            let evidence = hk_evidence_from_cache(&cache).unwrap();
            let screening = capability(&evidence, CapabilityId::Screening);
            assert!(!screening.release_gate_open);
            assert_eq!(screening.sources[0].freshness, CapabilityFreshness::Unknown);
            assert_eq!(screening.coverage.as_ref().unwrap().missing_count, 1);
        }
    }

    #[test]
    fn hk_freshness_uses_hong_kong_date_for_offset_observation() {
        let mut cache = hk_freshness_cache();
        let date = current_hk_quote_date();
        // At +14:00 midnight the observed Hong Kong date is the previous day.
        cache.quotes.get_mut("00700").unwrap().quote_time = Some(format!("{}-{}-{}T00:30:00+14:00", &date[..4], &date[4..6], &date[6..8]));
        let evidence = hk_evidence_from_cache(&cache).unwrap();
        assert_eq!(capability(&evidence, CapabilityId::Screening).sources[0].freshness, CapabilityFreshness::Stale);
    }

    #[test]
    fn hk_freshness_retains_confirmed_suspension_with_current_quote() {
        let mut cache = hk_freshness_cache();
        let date = current_hk_quote_date();
        cache.quotes.get_mut("00700").unwrap().volume_shares = Some(0);
        let history = cache.histories.get_mut("00700").unwrap();
        history.bars[0].date = "20200102".to_owned();
        history.suspended_as_of = Some(date.clone());
        history.request_range = Some(crate::hk_market::HkHistoryRequestRange { start_date:"20200102".to_owned(), end_date:date });
        assert!(crate::hk_market::hk_history_suspension_matches_quote(history, &cache.quotes["00700"]));
        let evidence = hk_evidence_from_cache(&cache).unwrap();
        assert!(capability(&evidence, CapabilityId::Observe).release_gate_open);
        cache.quotes.get_mut("00700").unwrap().quote_time = Some("2020-01-02T08:00:00Z".to_owned());
        let evidence = hk_evidence_from_cache(&cache).unwrap();
        assert!(!capability(&evidence, CapabilityId::Screening).release_gate_open, "an old suspension marker cannot establish current quote coverage");
    }

    #[test]
    fn hk_screening_does_not_count_an_rmb_counter_as_a_missing_hkd_quote() {
        let evidence = hk_evidence_from_cache(&hk_currency_scope_cache()).unwrap();
        let result = capabilities_for_market(MarketId::Hk, &evidence);
        let screening = result
            .iter()
            .find(|item| item.id == CapabilityId::Screening)
            .unwrap();
        let coverage = screening.coverage.as_ref().unwrap();
        assert_eq!(coverage.requested_count, 1);
        assert_eq!(coverage.available_count, 1);
        assert_eq!(coverage.missing_count, 0);
        assert!(coverage.missing_fields.is_empty());
        assert_eq!(screening.status, CapabilityStatus::Available);
        let payload = serde_json::to_value(screening).unwrap();
        assert_eq!(
            payload["quote_scope"],
            json!({"currency":"HKD","excluded_count":1})
        );
    }

    #[test]
    fn hk_screening_still_reports_a_genuinely_missing_hkd_quote() {
        let mut cache = hk_currency_scope_cache();
        cache.securities.push(
            serde_json::from_value(json!({
                "code":"09988","name":"Alibaba","currency":"HKD","security_type":"Equity",
                "classification_status":"exchange_listed_equity",
            }))
            .unwrap(),
        );
        let evidence = hk_evidence_from_cache(&cache).unwrap();
        let result = capabilities_for_market(MarketId::Hk, &evidence);
        let screening = result
            .iter()
            .find(|item| item.id == CapabilityId::Screening)
            .unwrap();
        let coverage = screening.coverage.as_ref().unwrap();
        assert_eq!(coverage.requested_count, 2);
        assert_eq!(coverage.available_count, 1);
        assert_eq!(coverage.missing_count, 1);
        assert_eq!(screening.status, CapabilityStatus::Limited);
        assert_eq!(coverage.missing_fields, ["currency_matched_cached_quote"]);
    }

    #[test]
    fn hk_screening_capability_uses_projected_cache_coverage_and_freshness() {
        let mut cache = crate::hk_market::HkMarketCache::default();
        cache.securities_source = "HKEX security directory".to_owned();
        cache.securities_updated_at = Some(crate::hk_market::current_epoch_millis());
        cache.securities.push(crate::hk_market::HkSecurity {
            code: "00700".to_owned(),
            name: "Tencent".to_owned(),
            currency: "HKD".to_owned(),
            board_lot: Some(100),
            security_type: "ordinary share".to_owned(),
            classification_status: crate::hk_market::HkClassificationStatus::VerifiedOrdinaryShare,
        });
        cache.quotes.insert(
            "00700".to_owned(),
            crate::hk_market::HkQuote {
                code: "00700".to_owned(),
                name: "Tencent".to_owned(),
                name_zh: None,
                price: 400.0,
                price_semantics_version: 1,
                previous_close: None,
                open: None,
                high: None,
                low: None,
                change: None,
                change_pct: None,
                volume_shares: None,
                turnover_hkd: None,
                pe: None,
                pb: None,
                dividend_yield_percent: None,
                market_cap_hkd: None,
                total_shares: None,
                circulating_shares: None,
                currency: "HKD".to_owned(),
                quote_time: Some(current_hk_quote_date()),
                source: "fixture".to_owned(),
                fetched_at: Some(crate::hk_market::current_epoch_millis()),
                last_error: None,
            },
        );
        cache.histories.insert(
            "00700".to_owned(),
            crate::hk_market::HkHistoryRecord {
                request_range: None,
                request_ranges: Vec::new(),
                price_adjustment: Default::default(),
                bar_fetched_at: Default::default(),
                revision_reason: None,
                suspended_as_of: None,
                bars: vec![crate::hk_market::HkDailyBar {
                    date: current_hk_quote_date(),
                    open: 399.0,
                    high: 401.0,
                    low: 398.0,
                    close: 400.0,
                    volume_shares: Some(10_000),
                }],
                source: "Tencent Finance".to_owned(),
                fetched_at: crate::hk_market::current_epoch_millis(),
                last_error: None,
            },
        );

        let fresh = hk_evidence_from_cache(&cache).unwrap();
        let screening = capability(&fresh, CapabilityId::Screening);
        assert_eq!(screening.usable_records, 1);
        assert_eq!(screening.coverage.as_ref().unwrap().available_count, 1);
        assert_eq!(screening.sources[0].market, MarketId::Hk);
        let capabilities = capabilities_for_market(MarketId::Hk, &fresh);
        assert_eq!(
            capabilities
                .iter()
                .find(|item| item.id == CapabilityId::SecurityDirectory)
                .unwrap()
                .status,
            CapabilityStatus::Available
        );
        assert_eq!(
            capabilities
                .iter()
                .find(|item| item.id == CapabilityId::Observe)
                .unwrap()
                .status,
            CapabilityStatus::Available
        );
        assert_eq!(
            capabilities
                .iter()
                .find(|item| item.id == CapabilityId::Screening)
                .unwrap()
                .status,
            CapabilityStatus::Available
        );
        assert_eq!(
            capabilities
                .iter()
                .find(|item| item.id == CapabilityId::Backtest)
                .unwrap()
                .status,
            CapabilityStatus::Limited
        );

        cache.quotes.get_mut("00700").unwrap().fetched_at = Some("1".to_owned());
        let stale = hk_evidence_from_cache(&cache).unwrap();
        assert_eq!(
            capabilities_for_market(MarketId::Hk, &stale)
                .iter()
                .find(|item| item.id == CapabilityId::Screening)
                .unwrap()
                .status,
            CapabilityStatus::Limited
        );

        let mut user_confirmed_cache = cache.clone();
        user_confirmed_cache.securities[0].classification_status =
            crate::hk_market::HkClassificationStatus::UserConfirmedOrdinaryShare;
        let user_confirmed = hk_evidence_from_cache(&user_confirmed_cache).unwrap();
        let user_confirmed_quote = capability(&user_confirmed, CapabilityId::Screening)
            .sources
            .first()
            .unwrap();
        assert!(user_confirmed_quote
            .quality_flags
            .contains(&"ordinary_share_user_confirmed_records_present".to_owned()));
    }
}
