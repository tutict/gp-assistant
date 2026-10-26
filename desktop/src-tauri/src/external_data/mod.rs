mod migration;
pub(crate) mod provider;
mod refresh;
mod store;

pub(crate) use migration::{import_legacy_json, LegacyImportReport};
pub(crate) use refresh::shared_refresh_coordinator;
pub(crate) use refresh::{execute_with_retry, RetryPolicy};
pub(crate) use store::{DataStore, RefreshRunState, RefreshRunStatus};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DataDomain {
    SecurityMaster,
    Quote,
    DailyHistory,
    FinancialSnapshot,
    Relations,
    FactorSnapshot,
    CapitalEvidence,
    FxRate,
    Disclosure,
    News,
}

impl DataDomain {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SecurityMaster => "security_master",
            Self::Quote => "quote",
            Self::DailyHistory => "daily_history",
            Self::FinancialSnapshot => "financial_snapshot",
            Self::Relations => "relations",
            Self::FactorSnapshot => "factor_snapshot",
            Self::CapitalEvidence => "capital_evidence",
            Self::FxRate => "fx_rate",
            Self::Disclosure => "disclosure",
            Self::News => "news",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Market {
    China,
    HongKong,
    Global,
}

impl Market {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::China => "cn",
            Self::HongKong => "hk",
            Self::Global => "global",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DataPolicy {
    CacheOnly,
    Refresh,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RecordMetadata {
    pub schema_version: u32,
    pub source_id: String,
    pub source_tier: String,
    pub fetched_at: String,
    pub observed_at: Option<String>,
    pub disclosed_at: Option<String>,
    pub verification_status: String,
    pub quality_flags: Vec<String>,
    pub payload_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StoredRecord {
    pub domain: DataDomain,
    pub market: Market,
    pub entity_key: String,
    pub record_key: String,
    pub metadata: RecordMetadata,
    pub payload: Value,
}

impl StoredRecord {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        domain: DataDomain,
        market: Market,
        entity_key: impl Into<String>,
        record_key: impl Into<String>,
        source_id: impl Into<String>,
        source_tier: impl Into<String>,
        fetched_at: impl Into<String>,
        observed_at: Option<&str>,
        disclosed_at: Option<&str>,
        verification_status: impl Into<String>,
        quality_flags: Vec<String>,
        payload: Value,
    ) -> Self {
        Self {
            domain,
            market,
            entity_key: entity_key.into(),
            record_key: record_key.into(),
            metadata: RecordMetadata {
                schema_version: 1,
                source_id: source_id.into(),
                source_tier: source_tier.into(),
                fetched_at: fetched_at.into(),
                observed_at: observed_at.map(str::to_owned),
                disclosed_at: disclosed_at.map(str::to_owned),
                verification_status: verification_status.into(),
                quality_flags,
                payload_hash: None,
            },
            payload,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RefreshScope {
    Entities {
        codes: Vec<String>,
    },
    DateRange {
        start_date: String,
        end_date: String,
    },
    MarketWide,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct RefreshRequest {
    pub domain: DataDomain,
    pub market: Market,
    pub scope: RefreshScope,
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RefreshOutcome {
    pub run_id: String,
    pub state: String,
    pub committed_records: usize,
    pub last_error: Option<String>,
}

pub(crate) fn store_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join("external-data")
        .join("external-data.sqlite")
}

pub(crate) fn open_app_store(app: &tauri::AppHandle) -> Result<DataStore, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve external data directory: {error}"))?;
    DataStore::open(store_path(&root))
}

#[tauri::command]
pub(crate) fn api_data_status(app: tauri::AppHandle) -> Result<Value, String> {
    status_for_app(&app)
}

pub(crate) fn status_for_app(app: &tauri::AppHandle) -> Result<Value, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve external data directory: {error}"))?;
    let path = store_path(&root);
    if !path.exists() {
        return Ok(serde_json::json!({
            "available": false,
            "schema_version": 0,
            "database_path": path.to_string_lossy(),
            "database_bytes": 0,
            "record_count": 0,
            "sources": [],
            "migrations": [],
            "pending_foreground_retries": 0,
            "needs_user_retry": 0,
            "automatic_cleanup": false,
        }));
    }
    let mut status = open_app_store(app)?.status_json()?;
    status["available"] = Value::Bool(true);
    Ok(status)
}

#[tauri::command]
pub(crate) fn api_data_cleanup_preview(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let domains = parse_domains(&payload)?;
    let cutoff = payload
        .get("cutoff")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    open_app_store(&app)?.cleanup_preview(&domains, cutoff)
}

#[tauri::command]
pub(crate) fn api_data_cleanup(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let domains = parse_domains(&payload)?;
    let cutoff = payload
        .get("cutoff")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let token = payload
        .get("preview_token")
        .and_then(Value::as_str)
        .ok_or_else(|| "cleanup requires a confirmed preview token".to_string())?;
    let mut store = open_app_store(&app)?;
    let removed_records = store.cleanup_confirmed(&domains, cutoff, token)?;
    let _ = store.checkpoint();
    Ok(serde_json::json!({
        "removed_records": removed_records,
        "status": store.status_json()?,
    }))
}

#[tauri::command]
pub(crate) async fn api_data_resume_explicit(app: tauri::AppHandle) -> Result<Value, String> {
    let hk = crate::hk_market::resume_explicit_hk_refreshes(app.clone()).await?;
    let cn = crate::market::resume_explicit_cn_refreshes(app).await?;
    Ok(serde_json::json!({ "hk": hk, "cn": cn }))
}

#[tauri::command]
pub(crate) async fn api_data_refresh(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = match payload.get("market").and_then(Value::as_str).unwrap_or("cn") {
        "hk" => return crate::hk_market::api_explicit_hk_refresh(app, payload).await,
        "cn" => Market::China,
        other => return Err(format!("unsupported refresh market: {other}")),
    };
    let requested_domain = payload
        .get("domain")
        .and_then(Value::as_str)
        .unwrap_or("quote");
    if requested_domain != "quote" {
        return Err(format!("the current A-share provider adapter only refreshes the quote/universe bundle; {requested_domain} was not started"));
    }
    let codes = payload
        .get("entity_codes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .and_then(crate::market::normalize_stock_code)
                        .ok_or_else(|| "entity_codes contains an invalid A-share code".to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let scope = if !codes.is_empty() {
        RefreshScope::Entities { codes }
    } else if let (Some(start_date), Some(end_date)) = (
        payload.get("start_date").and_then(Value::as_str),
        payload.get("end_date").and_then(Value::as_str),
    ) {
        RefreshScope::DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        }
    } else {
        RefreshScope::MarketWide
    };
    if market == Market::China && scope != RefreshScope::MarketWide {
        return Err("the current A-share provider adapter supports market-wide refresh only; requested scope was not started".into());
    }
    let request_id = payload
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("cn-market-{}", crate::agent_ledger::current_epoch_millis()));
    let mut refresh_payload = payload;
    if let Some(object) = refresh_payload.as_object_mut() {
        object.insert("request_id".into(), Value::String(request_id));
        object.insert("data_policy".into(), Value::String("refresh".into()));
    }
    crate::market::api_market_refresh(app, refresh_payload).await
}

#[tauri::command]
pub(crate) fn api_data_refresh_status(
    app: tauri::AppHandle,
    run_id: String,
) -> Result<Value, String> {
    let store = open_app_store(&app)?;
    store
        .refresh_run_status(&run_id)?
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| format!("encode external refresh status: {error}"))?
        .ok_or_else(|| "external refresh run was not found".into())
}

fn parse_domains(payload: &Value) -> Result<Vec<DataDomain>, String> {
    let values = payload
        .get("domains")
        .and_then(Value::as_array)
        .ok_or_else(|| "domains must be an array".to_string())?;
    values
        .iter()
        .map(|value| {
            let name = value
                .as_str()
                .ok_or_else(|| "domain names must be strings".to_string())?;
            match name {
                "security_master" => Ok(DataDomain::SecurityMaster),
                "quote" => Ok(DataDomain::Quote),
                "daily_history" => Ok(DataDomain::DailyHistory),
                "financial_snapshot" => Ok(DataDomain::FinancialSnapshot),
                "relations" => Ok(DataDomain::Relations),
                "factor_snapshot" => Ok(DataDomain::FactorSnapshot),
                "capital_evidence" => Ok(DataDomain::CapitalEvidence),
                "fx_rate" => Ok(DataDomain::FxRate),
                "disclosure" => Ok(DataDomain::Disclosure),
                "news" => Ok(DataDomain::News),
                _ => Err(format!("unsupported external data domain: {name}")),
            }
        })
        .collect()
}

#[cfg(test)]
mod store_tests;
