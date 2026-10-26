use crate::market_domain::{
    CapabilityId, CapabilityStatus, CoverageSummary, MarketCapability, MarketId,
};
use serde_json::Value;
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
        reason: reason.map(str::to_owned),
    }
}

fn count_value(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn bool_value(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn cn_evidence(app: &AppHandle) -> Result<Vec<CapabilityEvidence>, String> {
    let status = crate::market::market_data_status(app)?;
    let universe_count = count_value(&status, "universe_count");
    let quote_requested = count_value(&status, "quote_requested");
    let quote_observed = count_value(&status, "quote_observed");
    let directory_ready = universe_count > 0;
    let directory_coverage = Some(CoverageSummary {
        requested_count: universe_count,
        available_count: universe_count,
        missing_count: 0,
        start_date: None,
        end_date: None,
        missing_fields: vec!["security_universe_completeness".to_owned()],
    });

    Ok(vec![
        evidence(
            CapabilityId::SecurityDirectory,
            true,
            directory_ready,
            false,
            universe_count,
            Some(universe_count),
            directory_coverage,
            Some("CN directory cache is readable, but complete market-universe coverage is not reported"),
        ),
        evidence(
            CapabilityId::Observe,
            directory_ready,
            directory_ready,
            false,
            quote_observed,
            Some(quote_requested),
            Some(CoverageSummary {
                requested_count: quote_requested,
                available_count: quote_observed,
                missing_count: quote_requested.saturating_sub(quote_observed),
                start_date: None,
                end_date: None,
                missing_fields: vec!["per_security_history_coverage".to_owned()],
            }),
            Some("market-wide quote freshness does not report per-security observation history coverage"),
        ),
        evidence(
            CapabilityId::FinancialSnapshot,
            true,
            true,
            false,
            0,
            Some(universe_count),
            Some(CoverageSummary {
                requested_count: universe_count,
                available_count: 0,
                missing_count: universe_count,
                start_date: None,
                end_date: None,
                missing_fields: vec!["point_in_time_financials".to_owned()],
            }),
            Some("CN point-in-time financial coverage is not reported to the capability registry"),
        ),
        evidence(
            CapabilityId::Screening,
            directory_ready,
            directory_ready,
            false,
            quote_observed,
            Some(quote_requested),
            Some(CoverageSummary {
                requested_count: quote_requested,
                available_count: quote_observed,
                missing_count: quote_requested.saturating_sub(quote_observed),
                start_date: None,
                end_date: None,
                missing_fields: vec!["screening_field_coverage".to_owned()],
            }),
            Some("screening field coverage is not reported for the current quote snapshot"),
        ),
        evidence(
            CapabilityId::Backtest,
            true,
            true,
            false,
            0,
            None,
            None,
            Some("point-in-time history and corporate-action coverage is not reported"),
        ),
        evidence(
            CapabilityId::NewsResearch,
            true,
            true,
            false,
            0,
            None,
            None,
            Some("news source coverage and reuse rights are not registered"),
        ),
        evidence(
            CapabilityId::Sentiment,
            true,
            true,
            false,
            0,
            None,
            None,
            Some("sentiment evidence coverage is not registered"),
        ),
        evidence(
            CapabilityId::ResearchAgent,
            true,
            true,
            false,
            0,
            None,
            None,
            Some("market-scoped agent tool evidence is not registered"),
        ),
    ])
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
        "HKEX directory cache is readable; live refresh remains disabled pending written automated-access permission"
    } else if expected > 0 {
        "HKEX directory cache is stale or its previous refresh failed; live refresh remains disabled pending permission review"
    } else {
        "HKEX directory cache is empty; live refresh remains disabled pending written automated-access permission"
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
pub(crate) fn api_market_capabilities(
    app: AppHandle,
    market: String,
) -> Result<Vec<MarketCapability>, String> {
    let market = parse_market(&market)
        .ok_or_else(|| "unsupported market; expected one of: cn, hk".to_owned())?;
    let evidence = collect_market_capability_evidence(&app, market)?;
    Ok(capabilities_for_market(market, &evidence))
}

fn parse_market(value: &str) -> Option<MarketId> {
    MarketId::parse(value)
}

#[cfg(test)]
mod tests {
    use super::{capabilities_for_market, parse_market, CapabilityEvidence};
    use crate::market_domain::{CapabilityId, CapabilityStatus, CoverageSummary, MarketId};

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
