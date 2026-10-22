use crate::market_domain::{
    CapabilityId, CapabilityStatus, CoverageSummary, MarketCapability, MarketId,
};
use serde_json::Value;
use tauri::AppHandle;

#[derive(Clone, Debug)]
pub(crate) struct CapabilityEvidence {
    pub(crate) id: CapabilityId,
    pub(crate) adapter_registered: bool,
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
    coverage_complete: bool,
    usable_records: u64,
    expected_records: Option<u64>,
    coverage: Option<CoverageSummary>,
    reason: Option<&str>,
) -> CapabilityEvidence {
    CapabilityEvidence {
        id,
        adapter_registered,
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
    let directory_ready = universe_count > 0;
    let observe_ready = directory_ready && !bool_value(&status, "stale");
    let coverage = Some(CoverageSummary {
        requested_count: universe_count,
        available_count: universe_count,
        missing_count: 0,
        start_date: None,
        end_date: None,
        missing_fields: Vec::new(),
    });

    Ok(vec![
        evidence(
            CapabilityId::SecurityDirectory,
            true,
            directory_ready,
            universe_count,
            Some(universe_count),
            coverage.clone(),
            (!directory_ready).then_some("CN security directory cache is empty"),
        ),
        evidence(
            CapabilityId::Observe,
            true,
            observe_ready,
            universe_count,
            Some(universe_count),
            coverage,
            (!observe_ready).then_some("CN quote/history cache is stale or incomplete"),
        ),
        evidence(
            CapabilityId::FinancialSnapshot,
            true,
            true,
            1,
            None,
            None,
            Some("CN financial snapshot follows the existing refresh contract"),
        ),
        evidence(CapabilityId::Screening, true, true, 1, None, None, None),
        evidence(CapabilityId::Backtest, true, true, 1, None, None, None),
        evidence(CapabilityId::NewsResearch, true, true, 1, None, None, None),
        evidence(CapabilityId::Sentiment, true, true, 1, None, None, None),
        evidence(CapabilityId::ResearchAgent, true, true, 1, None, None, None),
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
    let observe_count = cache
        .securities
        .iter()
        .filter(|security| {
            let has_quote = cache
                .quotes
                .get(&security.code)
                .map(|quote| quote.last_error.is_none())
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
    let observe_ready = expected > 0
        && observe_count == expected
        && cache.quote_errors.is_empty()
        && cache.history_errors.is_empty();
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
    let financial_ready = expected > 0
        && financial_count == expected
        && cache.financial_errors.is_empty()
        && cache.disclosure_errors.is_empty();
    let coverage = |available: u64| CoverageSummary {
        requested_count: expected,
        available_count: available,
        missing_count: expected.saturating_sub(available),
        start_date: None,
        end_date: None,
        missing_fields: Vec::new(),
    };

    Ok(vec![
        evidence(
            CapabilityId::SecurityDirectory,
            true,
            directory_ready,
            if directory_ready { expected } else { 0 },
            Some(expected),
            Some(coverage(if directory_ready { expected } else { 0 })),
            (!directory_ready).then_some("HKEX security directory cache is empty or stale"),
        ),
        evidence(
            CapabilityId::Observe,
            true,
            observe_ready,
            observe_count,
            Some(expected),
            Some(coverage(observe_count)),
            (!observe_ready).then_some("HK quote/history coverage is incomplete"),
        ),
        evidence(
            CapabilityId::FinancialSnapshot,
            true,
            financial_ready,
            financial_count,
            Some(expected),
            Some(coverage(financial_count)),
            (!financial_ready)
                .then_some("HK verified financial, FX, or disclosure coverage is incomplete"),
        ),
        evidence(
            CapabilityId::Screening,
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
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::Backtest)),
        ),
        evidence(
            CapabilityId::NewsResearch,
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
            0,
            None,
            None,
            Some(unavailable_reason(CapabilityId::Sentiment)),
        ),
        evidence(
            CapabilityId::ResearchAgent,
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
    fn rejects_unknown_and_noncanonical_market_ids() {
        assert_eq!(parse_market("cn"), Some(MarketId::Cn));
        assert_eq!(parse_market("hk"), Some(MarketId::Hk));
        assert_eq!(parse_market("hk "), None);
        assert_eq!(parse_market("global"), None);
    }
}
