use crate::market_domain::{
    CapabilityFreshness, CapabilityId, CapabilitySourceEvidence, CapabilityStatus, CoverageSummary,
    MarketCapability, MarketId,
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
    let source_id = status
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.trim().is_empty())
        .unwrap_or("legacy_market_cache")
        .to_owned();
    let fetched_at = status.get("quote_generated_at").and_then(string_value);
    let directory_fetched_at = status.get("universe_updated_at").and_then(string_value);
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
                snapshot.latest_eps.is_some()
                    || snapshot.latest_bps.is_some()
                    || snapshot.operating_revenue_billion.is_some()
                    || snapshot.parent_net_profit_billion.is_some()
                    || snapshot.roe.is_some()
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
        &["complete_exchange_universe", "provider_rights"],
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
        &[
            "provider_rights",
            "point_in_time_publication_date",
            "per_security_financial_field_coverage",
        ],
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
                && stock.pb.is_some()
                && stock.roe.is_some()
        })
        .count() as u64;
    let screening_coverage = cache_coverage(
        universe_count,
        screening_symbols.min(universe_count),
        &["provider_rights", "complete_screening_factor_coverage"],
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
            "legacy_market_cache",
            "security_master",
            MarketId::Cn,
            directory_fetched_at,
            quote_date.clone(),
            None,
            None,
            if stale {
                CapabilityFreshness::Stale
            } else {
                CapabilityFreshness::Unknown
            },
            &["provider_rights_unverified", "complete_universe_unverified"],
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
    let mut financial_sources = Vec::new();
    for (code, snapshot) in &data.financials {
        if !data.stocks.iter().any(|stock| stock.code == *code) {
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
        let source_coverage = None;
        if !financial_sources
            .iter()
            .any(|existing: &CapabilitySourceEvidence| {
                existing.source_id == source && existing.report_period == period
            })
        {
            financial_sources.push(source_evidence(
                source,
                "financial_snapshot",
                MarketId::Cn,
                None,
                None,
                None,
                period,
                CapabilityFreshness::Unknown,
                &[
                    "provider_rights_unverified",
                    "point_in_time_publication_date_missing",
                ],
                source_coverage,
            ));
        }
    }
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
            Some(backtest_coverage),
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
    let expected = cache.securities.len() as u64;
    let directory_ready = expected > 0
        && !cache.securities_source.trim().is_empty()
        && cache.securities_updated_at.is_some()
        && cache.securities_last_error.is_none();
    let verified_ordinary_count = cache
        .securities
        .iter()
        .filter(|security| {
            security.classification_status
                == crate::hk_market::HkClassificationStatus::VerifiedOrdinaryShare
        })
        .count() as u64;
    let observe_count = cache
        .securities
        .iter()
        .filter(|security| {
            if security.classification_status
                != crate::hk_market::HkClassificationStatus::VerifiedOrdinaryShare
            {
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
                if security.classification_status
                    != crate::hk_market::HkClassificationStatus::VerifiedOrdinaryShare
                {
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
    let financial_expected = verified_ordinary_count;
    let coverage = |available: u64| CoverageSummary {
        requested_count: expected,
        available_count: available,
        missing_count: expected.saturating_sub(available),
        start_date: None,
        end_date: None,
        missing_fields: Vec::new(),
    };
    let directory_reason = if directory_ready {
        "HKEX directory cache is readable; live directory refresh remains disabled pending written automated-access permission"
    } else if expected > 0 {
        "HKEX directory cache is stale or its previous refresh failed; live directory refresh remains disabled pending permission review"
    } else {
        "HKEX directory cache is empty; live directory refresh remains disabled pending written automated-access permission"
    };

    Ok(vec![
        evidence(
            CapabilityId::SecurityDirectory,
            true,
            false,
            false,
            expected,
            Some(expected),
            Some(coverage(expected)),
            Some(directory_reason),
        ),
        evidence(
            CapabilityId::Observe,
            true,
            false,
            false,
            observe_count,
            Some(expected),
            Some(coverage(observe_count)),
            Some("ordinary-share classification is unverified and Tencent quote/history usage terms are unresolved"),
        ),
        evidence(
            CapabilityId::FinancialSnapshot,
            true,
            false,
            false,
            financial_count,
            Some(financial_expected),
            Some(CoverageSummary {
                requested_count: financial_expected,
                available_count: financial_count,
                missing_count: financial_expected.saturating_sub(financial_count),
                start_date: None,
                end_date: None,
                missing_fields: vec!["provider_readiness".to_owned()],
            }),
            Some("Eastmoney and HKEXnews refreshes are disabled pending usage-term review; only ECB-attributed FX refresh is enabled"),
        ),
        evidence(
            CapabilityId::Screening,
            false,
            false,
            false,
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::Screening)),
        ),
        evidence(
            CapabilityId::Backtest,
            false,
            false,
            false,
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::Backtest)),
        ),
        evidence(
            CapabilityId::NewsResearch,
            false,
            false,
            false,
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::NewsResearch)),
        ),
        evidence(
            CapabilityId::Sentiment,
            false,
            false,
            false,
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::Sentiment)),
        ),
        evidence(
            CapabilityId::ResearchAgent,
            false,
            false,
            false,
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::ResearchAgent)),
        ),
    ])
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
        capabilities_for_market, cn_evidence_from_cache, parse_market, CapabilityEvidence,
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
}
