use super::{DataDomain, Market, RecordMetadata, RefreshRequest, StoredRecord};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SCHEMA_VERSION: i64 = 1;
const APPLICATION_ID: i64 = 0x4750_4558; // GPEX: external provider data.
const MAX_RECORD_BYTES: usize = 2 * 1024 * 1024;
const MAX_KEY_BYTES: usize = 512;
const MAX_SOURCE_ID_BYTES: usize = 128;
const MAX_ERROR_BYTES: usize = 2 * 1024;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(300);
const NORMALIZED_RECORD_DATE: &str = "CASE
    WHEN coalesce(observed_at, disclosed_at, fetched_at) LIKE '____-__-__%' THEN substr(coalesce(observed_at, disclosed_at, fetched_at), 1, 10)
    WHEN coalesce(observed_at, disclosed_at, fetched_at) LIKE '____/__/__%' THEN replace(substr(coalesce(observed_at, disclosed_at, fetched_at), 1, 10), '/', '-')
    WHEN length(coalesce(observed_at, disclosed_at, fetched_at)) = 13
         AND coalesce(observed_at, disclosed_at, fetched_at) NOT GLOB '*[^0-9]*'
      THEN date(CAST(coalesce(observed_at, disclosed_at, fetched_at) AS INTEGER) / 1000, 'unixepoch')
    WHEN length(coalesce(observed_at, disclosed_at, fetched_at)) = 10
         AND coalesce(observed_at, disclosed_at, fetched_at) NOT GLOB '*[^0-9]*'
      THEN date(CAST(coalesce(observed_at, disclosed_at, fetched_at) AS INTEGER), 'unixepoch')
    ELSE NULL END";

const SCHEMA_V1: &str = "
CREATE TABLE store_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO store_metadata(key, value) VALUES ('store_kind', 'external_provider_data');

CREATE TABLE records (
    domain TEXT NOT NULL,
    market TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    record_key TEXT NOT NULL,
    source_id TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK(schema_version > 0),
    source_tier TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    observed_at TEXT,
    disclosed_at TEXT,
    verification_status TEXT NOT NULL,
    quality_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    payload_hash TEXT NOT NULL CHECK(length(payload_hash) = 64),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(domain, market, entity_key, record_key, source_id)
);
CREATE INDEX idx_external_records_entity
    ON records(domain, market, entity_key, record_key);
CREATE INDEX idx_external_records_observed
    ON records(domain, market, observed_at);
CREATE INDEX idx_external_records_disclosed
    ON records(domain, market, disclosed_at);

CREATE TABLE source_state (
    source_id TEXT NOT NULL,
    domain TEXT NOT NULL,
    market TEXT NOT NULL,
    last_attempt_at TEXT,
    last_success_at TEXT,
    last_error_category TEXT,
    last_error TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    record_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(source_id, domain, market)
);

CREATE TABLE refresh_runs (
    run_id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    scope_json TEXT NOT NULL,
    origin TEXT NOT NULL,
    state TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    recovery_count INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    last_error_category TEXT,
    last_error TEXT
);
CREATE INDEX idx_external_refresh_state
    ON refresh_runs(state, updated_at_ms);

CREATE TABLE legacy_imports (
    domain TEXT NOT NULL,
    market TEXT NOT NULL,
    source_path TEXT NOT NULL,
    checksum_sha256 TEXT NOT NULL CHECK(length(checksum_sha256) = 64),
    parsed_count INTEGER NOT NULL DEFAULT 0,
    imported_count INTEGER NOT NULL DEFAULT 0,
    validation_state TEXT NOT NULL,
    promoted INTEGER NOT NULL DEFAULT 0 CHECK(promoted IN (0, 1)),
    attempted_at_ms INTEGER NOT NULL,
    last_error TEXT,
    PRIMARY KEY(domain, market, source_path, checksum_sha256)
);
";

pub(crate) struct DataStore {
    connection: Connection,
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SourceStatus {
    pub source_id: String,
    pub domain: String,
    pub market: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_category: Option<String>,
    pub last_error: Option<String>,
    pub attempt_count: u64,
    pub record_count: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RefreshRunState {
    Queued,
    Running,
    Completed,
    Partial,
    Retryable,
    NeedsUserRetry,
    Failed,
    Cancelled,
}

impl RefreshRunState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Retryable => "retryable",
            Self::NeedsUserRetry => "needs_user_retry",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RefreshRunStatus {
    pub run_id: String,
    pub idempotency_key: String,
    pub request: RefreshRequest,
    pub origin: String,
    pub state: RefreshRunState,
    pub attempt_count: u32,
    pub recovery_count: u32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_error_category: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LegacyImportStatus {
    pub domain: String,
    pub market: String,
    pub source_path: String,
    pub checksum_sha256: String,
    pub parsed_count: usize,
    pub imported_count: usize,
    pub validation_state: String,
    pub promoted: bool,
    pub last_error: Option<String>,
}

static REFRESH_RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

impl DataStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create external data directory: {error}"))?;
        }
        if path.exists() {
            crate::durability::validate_sqlite_header(&path)?;
        }

        let mut connection = Connection::open(&path)
            .map_err(|error| format!("open external data database: {error}"))?;
        let version = pragma_i64(&connection, "user_version")?;
        let application_id = pragma_i64(&connection, "application_id")?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "external data schema version {version} is newer than supported version {SCHEMA_VERSION}; database preserved"
            ));
        }
        if version < 0 {
            return Err("external data schema version is invalid; database preserved".into());
        }
        if application_id != 0 && application_id != APPLICATION_ID {
            return Err("database belongs to another application; database preserved".into());
        }
        if version == SCHEMA_VERSION && application_id != APPLICATION_ID {
            return Err("external data application id is missing; database preserved".into());
        }
        if version == 0 {
            let has_tables: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| format!("inspect external data database: {error}"))?;
            if has_tables {
                return Err(
                    "unversioned database is not an empty external data store; database preserved"
                        .into(),
                );
            }
        }

        crate::durability::configure_user_connection(&connection)
            .map_err(|error| format!("configure external data database: {error}"))?;
        connection
            .busy_timeout(SQLITE_BUSY_TIMEOUT)
            .map_err(|error| format!("configure external data lock timeout: {error}"))?;

        if version == 0 {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| format!("begin external data schema creation: {error}"))?;
            transaction
                .execute_batch(SCHEMA_V1)
                .map_err(|error| format!("create external data schema: {error}"))?;
            transaction
                .pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(|error| format!("set external data application id: {error}"))?;
            transaction
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(|error| format!("set external data schema version: {error}"))?;
            transaction
                .commit()
                .map_err(|error| format!("commit external data schema: {error}"))?;
        }

        Ok(Self { connection, path })
    }

    pub(crate) fn upsert_batch(&mut self, records: &[StoredRecord]) -> Result<usize, String> {
        if records.is_empty() {
            return Ok(0);
        }

        // Validate and serialize the whole batch before acquiring a SQLite write lock.
        let prepared = records
            .iter()
            .map(prepare_record)
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin external data batch: {error}"))?;
        for row in &prepared {
            transaction
                .execute(
                    "INSERT INTO records (
                        domain, market, entity_key, record_key, source_id, schema_version,
                        source_tier, fetched_at, observed_at, disclosed_at,
                        verification_status, quality_json, payload_json, payload_hash, updated_at_ms
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                     ) ON CONFLICT(domain, market, entity_key, record_key, source_id) DO UPDATE SET
                        schema_version=excluded.schema_version,
                        source_tier=excluded.source_tier,
                        fetched_at=excluded.fetched_at,
                        observed_at=excluded.observed_at,
                        disclosed_at=excluded.disclosed_at,
                        verification_status=excluded.verification_status,
                        quality_json=excluded.quality_json,
                        payload_json=excluded.payload_json,
                        payload_hash=excluded.payload_hash,
                        updated_at_ms=excluded.updated_at_ms",
                    params![
                        row.domain,
                        row.market,
                        row.entity_key,
                        row.record_key,
                        row.source_id,
                        row.schema_version,
                        row.source_tier,
                        row.fetched_at,
                        row.observed_at,
                        row.disclosed_at,
                        row.verification_status,
                        row.quality_json,
                        row.payload_json,
                        row.payload_hash,
                        now_ms(),
                    ],
                )
                .map_err(|error| format!("write external data record: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("commit external data batch: {error}"))?;
        Ok(prepared.len())
    }

    pub(crate) fn replace_domain_batch(
        &mut self,
        domain: DataDomain,
        market: Market,
        records: &[StoredRecord],
    ) -> Result<usize, String> {
        if records
            .iter()
            .any(|record| record.domain != domain || record.market != market)
        {
            return Err("replacement batch contains records from another domain or market".into());
        }
        let prepared = records
            .iter()
            .map(prepare_record)
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin external domain replacement: {error}"))?;
        transaction
            .execute(
                "DELETE FROM records WHERE domain=?1 AND market=?2",
                params![domain.as_str(), market.as_str()],
            )
            .map_err(|error| format!("replace external domain records: {error}"))?;
        for row in &prepared {
            transaction
                .execute(
                    "INSERT INTO records (
                        domain, market, entity_key, record_key, source_id, schema_version,
                        source_tier, fetched_at, observed_at, disclosed_at,
                        verification_status, quality_json, payload_json, payload_hash, updated_at_ms
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        row.domain,
                        row.market,
                        row.entity_key,
                        row.record_key,
                        row.source_id,
                        row.schema_version,
                        row.source_tier,
                        row.fetched_at,
                        row.observed_at,
                        row.disclosed_at,
                        row.verification_status,
                        row.quality_json,
                        row.payload_json,
                        row.payload_hash,
                        now_ms(),
                    ],
                )
                .map_err(|error| format!("write replacement external record: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("commit external domain replacement: {error}"))?;
        Ok(prepared.len())
    }

    pub(crate) fn clear_domain(
        &mut self,
        domain: DataDomain,
        market: Market,
    ) -> Result<u64, String> {
        let changed = self
            .connection
            .execute(
                "DELETE FROM records WHERE domain=?1 AND market=?2",
                params![domain.as_str(), market.as_str()],
            )
            .map_err(|error| format!("clear external data domain: {error}"))?;
        Ok(changed as u64)
    }

    pub(crate) fn read_records(
        &self,
        domain: DataDomain,
        market: Market,
        entity_key: &str,
    ) -> Result<Vec<StoredRecord>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT record_key, schema_version, source_id, source_tier, fetched_at,
                        observed_at, disclosed_at, verification_status, quality_json,
                        payload_json, payload_hash
                 FROM records
                 WHERE domain=?1 AND market=?2 AND entity_key=?3
                 ORDER BY record_key, source_id",
            )
            .map_err(|error| format!("prepare external data read: {error}"))?;
        let rows = statement
            .query_map(
                params![domain.as_str(), market.as_str(), entity_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .map_err(|error| format!("query external data records: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read external data records: {error}"))?;

        rows.into_iter()
            .map(
                |(
                    record_key,
                    schema_version,
                    source_id,
                    source_tier,
                    fetched_at,
                    observed_at,
                    disclosed_at,
                    verification_status,
                    quality_json,
                    payload_json,
                    payload_hash,
                )| {
                    let quality_flags = serde_json::from_str(&quality_json)
                        .map_err(|error| format!("decode external data quality flags: {error}"))?;
                    let payload = serde_json::from_str(&payload_json)
                        .map_err(|error| format!("decode external data payload: {error}"))?;
                    Ok(StoredRecord {
                        domain,
                        market,
                        entity_key: entity_key.to_string(),
                        record_key,
                        metadata: RecordMetadata {
                            schema_version,
                            source_id,
                            source_tier,
                            fetched_at,
                            observed_at,
                            disclosed_at,
                            verification_status,
                            quality_flags,
                            payload_hash: Some(payload_hash),
                        },
                        payload,
                    })
                },
            )
            .collect()
    }

    pub(crate) fn record_source_attempt(
        &self,
        source_id: &str,
        domain: DataDomain,
        market: Market,
        at: &str,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO source_state(source_id, domain, market, last_attempt_at, attempt_count)
                 VALUES(?1, ?2, ?3, ?4, 1)
                 ON CONFLICT(source_id, domain, market) DO UPDATE SET
                    last_attempt_at=excluded.last_attempt_at,
                    attempt_count=source_state.attempt_count + 1",
                params![source_id, domain.as_str(), market.as_str(), at],
            )
            .map_err(|error| format!("record external source attempt: {error}"))?;
        Ok(())
    }

    pub(crate) fn domain_has_successful_source(
        &self,
        domain: DataDomain,
        market: Market,
    ) -> Result<bool, String> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM source_state
                 WHERE domain=?1 AND market=?2 AND last_success_at IS NOT NULL)",
                params![domain.as_str(), market.as_str()],
                |row| row.get(0),
            )
            .map_err(|error| format!("read external domain activation state: {error}"))
    }

    pub(crate) fn record_source_success(
        &self,
        source_id: &str,
        domain: DataDomain,
        market: Market,
        at: &str,
        record_count: usize,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO source_state(
                    source_id, domain, market, last_attempt_at, last_success_at,
                    record_count, attempt_count
                 ) VALUES(?1, ?2, ?3, ?4, ?4, ?5, 1)
                 ON CONFLICT(source_id, domain, market) DO UPDATE SET
                    last_attempt_at=excluded.last_attempt_at,
                    last_success_at=excluded.last_success_at,
                    last_error_category=NULL,
                    last_error=NULL,
                    record_count=excluded.record_count",
                params![
                    source_id,
                    domain.as_str(),
                    market.as_str(),
                    at,
                    record_count.min(i64::MAX as usize) as i64,
                ],
            )
            .map_err(|error| format!("record external source success: {error}"))?;
        Ok(())
    }

    pub(crate) fn record_source_failure(
        &self,
        source_id: &str,
        domain: DataDomain,
        market: Market,
        at: &str,
        category: &str,
        error: &str,
    ) -> Result<(), String> {
        let error_end = error
            .char_indices()
            .take_while(|(index, character)| index + character.len_utf8() <= MAX_ERROR_BYTES)
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(0);
        let error = &error[..error_end];
        self.connection
            .execute(
                "INSERT INTO source_state(
                    source_id, domain, market, last_attempt_at,
                    last_error_category, last_error, attempt_count
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 1)
                 ON CONFLICT(source_id, domain, market) DO UPDATE SET
                    last_attempt_at=excluded.last_attempt_at,
                    last_error_category=excluded.last_error_category,
                    last_error=excluded.last_error",
                params![
                    source_id,
                    domain.as_str(),
                    market.as_str(),
                    at,
                    category,
                    error
                ],
            )
            .map_err(|error| format!("record external source failure: {error}"))?;
        Ok(())
    }

    pub(crate) fn refresh_status(
        &self,
        domain: DataDomain,
        market: Market,
    ) -> Result<Vec<SourceStatus>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT source_id, domain, market, last_attempt_at, last_success_at,
                        last_error_category, last_error, attempt_count, record_count
                 FROM source_state
                 WHERE domain=?1 AND market=?2
                 ORDER BY source_id",
            )
            .map_err(|error| format!("prepare external source status read: {error}"))?;
        let rows = statement
            .query_map(params![domain.as_str(), market.as_str()], |row| {
                Ok(SourceStatus {
                    source_id: row.get(0)?,
                    domain: row.get(1)?,
                    market: row.get(2)?,
                    last_attempt_at: row.get(3)?,
                    last_success_at: row.get(4)?,
                    last_error_category: row.get(5)?,
                    last_error: row.get(6)?,
                    attempt_count: row.get::<_, i64>(7)?.max(0) as u64,
                    record_count: row.get::<_, i64>(8)?.max(0) as u64,
                })
            })
            .map_err(|error| format!("query external source status: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read external source status: {error}"))
    }

    pub(crate) fn create_refresh_run(
        &mut self,
        request: &RefreshRequest,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(RefreshRunStatus, bool), String> {
        validate_idempotency_key(idempotency_key)?;
        let scope_json = serde_json::to_string(request)
            .map_err(|error| format!("encode external refresh scope: {error}"))?;
        if scope_json.len() > MAX_RECORD_BYTES {
            return Err("external refresh scope is too large".into());
        }

        if let Some(existing) = self
            .connection
            .query_row(
                "SELECT run_id, scope_json FROM refresh_runs WHERE idempotency_key=?1",
                [idempotency_key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| format!("read external refresh idempotency key: {error}"))?
        {
            if existing.1 != scope_json {
                return Err("idempotency key is already bound to a different refresh scope".into());
            }
            let status = self
                .refresh_run_status(&existing.0)?
                .ok_or_else(|| "external refresh run disappeared".to_string())?;
            return Ok((status, false));
        }

        let run_id = next_refresh_run_id(now);
        self.connection
            .execute(
                "INSERT INTO refresh_runs(
                    run_id, idempotency_key, scope_json, origin, state,
                    created_at_ms, updated_at_ms
                 ) VALUES(?1, ?2, ?3, 'explicit', 'queued', ?4, ?4)",
                params![run_id, idempotency_key, scope_json, now],
            )
            .map_err(|error| format!("create external refresh run: {error}"))?;
        let status = self
            .refresh_run_status(&run_id)?
            .ok_or_else(|| "created external refresh run could not be read".to_string())?;
        Ok((status, true))
    }

    pub(crate) fn start_refresh_run(
        &mut self,
        run_id: &str,
        foreground_recovery: bool,
        now: i64,
    ) -> Result<(), String> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin external refresh run start: {error}"))?;
        let changed = if foreground_recovery {
            transaction
                .execute(
                    "UPDATE refresh_runs
                     SET state='running', attempt_count=attempt_count+1,
                         recovery_count=recovery_count+1, updated_at_ms=?2
                     WHERE run_id=?1 AND state='retryable' AND origin='explicit'
                       AND recovery_count=0",
                    params![run_id, now],
                )
                .map_err(|error| format!("start external foreground recovery: {error}"))?
        } else {
            transaction
                .execute(
                    "UPDATE refresh_runs
                     SET state='running', attempt_count=attempt_count+1, updated_at_ms=?2
                     WHERE run_id=?1 AND (
                        state IN ('queued', 'needs_user_retry')
                        OR (state='retryable' AND origin='explicit')
                     )",
                    params![run_id, now],
                )
                .map_err(|error| format!("start external refresh run: {error}"))?
        };
        if changed != 1 {
            return Err("external refresh run cannot start from its current state".into());
        }
        transaction
            .commit()
            .map_err(|error| format!("commit external refresh run start: {error}"))
    }

    pub(crate) fn finish_refresh_run(
        &mut self,
        run_id: &str,
        requested_state: RefreshRunState,
        now: i64,
        error_category: Option<&str>,
        error: Option<&str>,
    ) -> Result<RefreshRunState, String> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin external refresh run finish: {error}"))?;
        let current: Option<(String, i64)> = transaction
            .query_row(
                "SELECT state, recovery_count FROM refresh_runs WHERE run_id=?1",
                [run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("read external refresh run before finish: {error}"))?;
        let Some((current_state, recovery_count)) = current else {
            return Err("external refresh run was not found".into());
        };
        if current_state != RefreshRunState::Running.as_str() {
            return Err("external refresh run is not running".into());
        }
        let final_state = if requested_state == RefreshRunState::Retryable && recovery_count > 0 {
            RefreshRunState::NeedsUserRetry
        } else {
            requested_state
        };
        let category = error_category.map(|value| truncate_utf8(value, MAX_SOURCE_ID_BYTES));
        let error = error.map(|value| truncate_utf8(value, MAX_ERROR_BYTES));
        transaction
            .execute(
                "UPDATE refresh_runs SET state=?2, updated_at_ms=?3,
                    last_error_category=?4, last_error=?5 WHERE run_id=?1",
                params![run_id, final_state.as_str(), now, category, error],
            )
            .map_err(|error| format!("finish external refresh run: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit external refresh run finish: {error}"))?;
        Ok(final_state)
    }

    pub(crate) fn foreground_recoveries(&self) -> Result<Vec<RefreshRunStatus>, String> {
        let run_ids = {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT run_id FROM refresh_runs
                     WHERE state='retryable' AND origin='explicit' AND recovery_count=0
                     ORDER BY updated_at_ms, run_id LIMIT 64",
                )
                .map_err(|error| format!("prepare external foreground recovery query: {error}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("query external foreground recovery runs: {error}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("read external foreground recovery runs: {error}"))?
        };
        run_ids
            .iter()
            .map(|run_id| {
                self.refresh_run_status(run_id)?
                    .ok_or_else(|| "external recovery run disappeared".to_string())
            })
            .collect()
    }

    pub(crate) fn refresh_run_status(
        &self,
        run_id: &str,
    ) -> Result<Option<RefreshRunStatus>, String> {
        let row: Option<(
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            i64,
            i64,
            Option<String>,
            Option<String>,
        )> = self
            .connection
            .query_row(
                "SELECT run_id, idempotency_key, scope_json, origin, state,
                            attempt_count, recovery_count, created_at_ms, updated_at_ms,
                            last_error_category, last_error
                     FROM refresh_runs WHERE run_id=?1",
                [run_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("read external refresh run status: {error}"))?;
        row.map(
            |(
                run_id,
                idempotency_key,
                request_json,
                origin,
                state,
                attempt_count,
                recovery_count,
                created_at_ms,
                updated_at_ms,
                last_error_category,
                last_error,
            )| {
                let request = serde_json::from_str(&request_json)
                    .map_err(|error| format!("decode external refresh scope: {error}"))?;
                let state = parse_refresh_run_state(&state)?;
                Ok(RefreshRunStatus {
                    run_id,
                    idempotency_key,
                    request,
                    origin,
                    state,
                    attempt_count: attempt_count.max(0) as u32,
                    recovery_count: recovery_count.max(0) as u32,
                    created_at_ms,
                    updated_at_ms,
                    last_error_category,
                    last_error,
                })
            },
        )
        .transpose()
    }

    pub(crate) fn refresh_run_by_idempotency_key(
        &self,
        key: &str,
    ) -> Result<Option<RefreshRunStatus>, String> {
        let run_id = self
            .connection
            .query_row(
                "SELECT run_id FROM refresh_runs WHERE idempotency_key=?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("find external refresh run by idempotency key: {error}"))?;
        run_id
            .map(|id| self.refresh_run_status(&id))
            .transpose()
            .map(Option::flatten)
    }

    pub(crate) fn read_domain_records(
        &self,
        domain: DataDomain,
        market: Market,
    ) -> Result<Vec<StoredRecord>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT entity_key FROM records WHERE domain=?1 AND market=?2
                 GROUP BY entity_key ORDER BY entity_key",
            )
            .map_err(|error| format!("prepare external domain entity read: {error}"))?;
        let entities = statement
            .query_map(params![domain.as_str(), market.as_str()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| format!("query external domain entities: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read external domain entities: {error}"))?;
        entities.iter().try_fold(Vec::new(), |mut all, entity| {
            all.extend(self.read_records(domain, market, entity)?);
            Ok(all)
        })
    }

    pub(crate) fn record_legacy_import(
        &self,
        domain: DataDomain,
        market: Market,
        source_path: &str,
        checksum: &str,
        parsed_count: usize,
        imported_count: usize,
        validation_state: &str,
        promoted: bool,
        error: Option<&str>,
    ) -> Result<(), String> {
        let source_path = truncate_utf8(source_path, MAX_KEY_BYTES);
        let error = error.map(|value| truncate_utf8(value, MAX_ERROR_BYTES));
        self.connection
            .execute(
                "INSERT INTO legacy_imports(
                    domain, market, source_path, checksum_sha256, parsed_count,
                    imported_count, validation_state, promoted, attempted_at_ms, last_error
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(domain, market, source_path, checksum_sha256) DO UPDATE SET
                    parsed_count=excluded.parsed_count,
                    imported_count=excluded.imported_count,
                    validation_state=excluded.validation_state,
                    promoted=excluded.promoted,
                    attempted_at_ms=excluded.attempted_at_ms,
                    last_error=excluded.last_error",
                params![
                    domain.as_str(),
                    market.as_str(),
                    source_path,
                    checksum,
                    parsed_count.min(i64::MAX as usize) as i64,
                    imported_count.min(i64::MAX as usize) as i64,
                    validation_state,
                    i64::from(promoted),
                    now_ms(),
                    error,
                ],
            )
            .map_err(|error| format!("record legacy external data import: {error}"))?;
        Ok(())
    }

    pub(crate) fn legacy_import_status(
        &self,
        domain: DataDomain,
        market: Market,
        source_path: &str,
        checksum: &str,
    ) -> Result<Option<LegacyImportStatus>, String> {
        self.connection
            .query_row(
                "SELECT domain, market, source_path, checksum_sha256, parsed_count,
                        imported_count, validation_state, promoted, last_error
                 FROM legacy_imports
                 WHERE domain=?1 AND market=?2 AND source_path=?3 AND checksum_sha256=?4",
                params![domain.as_str(), market.as_str(), source_path, checksum],
                |row| {
                    Ok(LegacyImportStatus {
                        domain: row.get(0)?,
                        market: row.get(1)?,
                        source_path: row.get(2)?,
                        checksum_sha256: row.get(3)?,
                        parsed_count: row.get::<_, i64>(4)?.max(0) as usize,
                        imported_count: row.get::<_, i64>(5)?.max(0) as usize,
                        validation_state: row.get(6)?,
                        promoted: row.get::<_, i64>(7)? != 0,
                        last_error: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("read legacy external data import status: {error}"))
    }

    pub(crate) fn latest_promoted_legacy_import(
        &self,
        domain: DataDomain,
        market: Market,
        source_path: &str,
    ) -> Result<Option<LegacyImportStatus>, String> {
        self.connection
            .query_row(
                "SELECT domain, market, source_path, checksum_sha256, parsed_count,
                        imported_count, validation_state, promoted, last_error
                 FROM legacy_imports
                 WHERE domain=?1 AND market=?2 AND source_path=?3 AND promoted=1
                 ORDER BY attempted_at_ms DESC LIMIT 1",
                params![domain.as_str(), market.as_str(), source_path],
                |row| {
                    Ok(LegacyImportStatus {
                        domain: row.get(0)?,
                        market: row.get(1)?,
                        source_path: row.get(2)?,
                        checksum_sha256: row.get(3)?,
                        parsed_count: row.get::<_, i64>(4)?.max(0) as usize,
                        imported_count: row.get::<_, i64>(5)?.max(0) as usize,
                        validation_state: row.get(6)?,
                        promoted: row.get::<_, i64>(7)? != 0,
                        last_error: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("read promoted legacy external data import: {error}"))
    }

    pub(crate) fn store_bytes(&self) -> Result<u64, String> {
        let mut bytes = fs::metadata(&self.path)
            .map_err(|error| format!("inspect external data database size: {error}"))?
            .len();
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", self.path.to_string_lossy()));
            if let Ok(metadata) = fs::metadata(sidecar) {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        Ok(bytes)
    }

    pub(crate) fn status_json(&self) -> Result<Value, String> {
        let record_count: i64 = self
            .connection
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
            .map_err(|error| format!("count external data records: {error}"))?;
        let sources = self
            .connection
            .prepare(
                "SELECT state.source_id, state.domain, state.market, state.last_attempt_at, state.last_success_at,
                        state.last_error_category, state.last_error, state.attempt_count,
                        (SELECT count(*) FROM records AS records_count
                         WHERE records_count.domain=state.domain AND records_count.market=state.market AND records_count.source_id=state.source_id),
                        (SELECT MIN(coalesce(observed_at, disclosed_at, fetched_at)) FROM records AS records_min
                         WHERE records_min.domain=state.domain AND records_min.market=state.market AND records_min.source_id=state.source_id),
                        (SELECT MAX(coalesce(observed_at, disclosed_at, fetched_at)) FROM records AS records_max
                         WHERE records_max.domain=state.domain AND records_max.market=state.market AND records_max.source_id=state.source_id)
                 FROM source_state AS state ORDER BY state.domain, state.market, state.source_id",
            )
            .map_err(|error| format!("prepare external source status: {error}"))?
            .query_map([], |row| {
                Ok(json!({
                    "source_id": row.get::<_, String>(0)?,
                    "domain": row.get::<_, String>(1)?,
                    "market": row.get::<_, String>(2)?,
                    "last_attempt_at": row.get::<_, Option<String>>(3)?,
                    "last_success_at": row.get::<_, Option<String>>(4)?,
                    "last_error_category": row.get::<_, Option<String>>(5)?,
                    "last_error": row.get::<_, Option<String>>(6)?,
                    "attempt_count": row.get::<_, i64>(7)?.max(0),
                    "record_count": row.get::<_, i64>(8)?.max(0),
                    "observed_from": row.get::<_, Option<String>>(9)?,
                    "observed_through": row.get::<_, Option<String>>(10)?,
                }))
            })
            .map_err(|error| format!("query external source status: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read external source status: {error}"))?;
        let migrations = self
            .connection
            .prepare(
                "SELECT domain, market, validation_state, promoted, parsed_count,
                        imported_count, attempted_at_ms
                 FROM legacy_imports AS current
                 WHERE attempted_at_ms=(
                    SELECT MAX(candidate.attempted_at_ms) FROM legacy_imports AS candidate
                    WHERE candidate.domain=current.domain AND candidate.market=current.market
                 )
                 GROUP BY domain, market
                 ORDER BY domain, market",
            )
            .map_err(|error| format!("prepare external migration status: {error}"))?
            .query_map([], |row| {
                Ok(json!({
                    "domain": row.get::<_, String>(0)?,
                    "market": row.get::<_, String>(1)?,
                    "state": row.get::<_, String>(2)?,
                    "promoted": row.get::<_, i64>(3)? != 0,
                    "parsed_count": row.get::<_, i64>(4)?.max(0),
                    "imported_count": row.get::<_, i64>(5)?.max(0),
                    "checked_at_ms": row.get::<_, i64>(6)?,
                }))
            })
            .map_err(|error| format!("query external migration status: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read external migration status: {error}"))?;
        let pending_retries: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM refresh_runs WHERE state='retryable' AND recovery_count=0",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("count external refresh retries: {error}"))?;
        let needs_user_retry: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM refresh_runs WHERE state='needs_user_retry'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("count external user retries: {error}"))?;
        let bytes = self.store_bytes()?;
        const WARNING_BYTES: u64 = 200 * 1024 * 1024;
        Ok(json!({
            "schema_version": SCHEMA_VERSION,
            "database_path": self.path.to_string_lossy(),
            "database_bytes": bytes,
            "warning_threshold_bytes": WARNING_BYTES,
            "capacity_warning": bytes >= WARNING_BYTES,
            "record_count": record_count.max(0),
            "sources": sources,
            "migrations": migrations,
            "pending_foreground_retries": pending_retries.max(0),
            "needs_user_retry": needs_user_retry.max(0),
            "automatic_cleanup": false,
        }))
    }

    pub(crate) fn cleanup_preview(
        &self,
        domains: &[DataDomain],
        cutoff: Option<&str>,
    ) -> Result<Value, String> {
        cleanup_preview_connection(&self.connection, domains, cutoff)
    }

    pub(crate) fn cleanup_confirmed(
        &mut self,
        domains: &[DataDomain],
        cutoff: Option<&str>,
        preview_token: &str,
    ) -> Result<u64, String> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin external data cleanup: {error}"))?;
        let preview = cleanup_preview_connection(&transaction, domains, cutoff)?;
        let expected = preview["preview_token"]
            .as_str()
            .ok_or_else(|| "cleanup preview token is unavailable".to_string())?;
        if expected != preview_token {
            return Err("cleanup scope changed; generate a new preview before confirming".into());
        }
        let mut removed = 0_u64;
        for domain in domains {
            let delete_sql = format!(
                "DELETE FROM records WHERE domain=?1 AND (?2 IS NULL OR ({NORMALIZED_RECORD_DATE}) < ?2)"
            );
            let changed = transaction
                .execute(&delete_sql, params![domain.as_str(), cutoff])
                .map_err(|error| format!("delete external data records: {error}"))?;
            removed = removed.saturating_add(changed as u64);
        }
        transaction
            .commit()
            .map_err(|error| format!("commit external data cleanup: {error}"))?;
        Ok(removed)
    }

    pub(crate) fn checkpoint(&self) -> Result<(), String> {
        crate::durability::checkpoint(&self.connection)
    }
}

fn validate_idempotency_key(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_KEY_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err("external refresh idempotency key is invalid".into());
    }
    Ok(())
}

fn next_refresh_run_id(now: i64) -> String {
    format!(
        "gp-data-run-{now}-{}-{}",
        std::process::id(),
        REFRESH_RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn parse_refresh_run_state(value: &str) -> Result<RefreshRunState, String> {
    match value {
        "queued" => Ok(RefreshRunState::Queued),
        "running" => Ok(RefreshRunState::Running),
        "completed" => Ok(RefreshRunState::Completed),
        "partial" => Ok(RefreshRunState::Partial),
        "retryable" => Ok(RefreshRunState::Retryable),
        "needs_user_retry" => Ok(RefreshRunState::NeedsUserRetry),
        "failed" => Ok(RefreshRunState::Failed),
        "cancelled" => Ok(RefreshRunState::Cancelled),
        other => Err(format!("unknown external refresh state: {other}")),
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn validate_cutoff(cutoff: Option<&str>) -> Result<(), String> {
    if let Some(cutoff) = cutoff {
        let digits = cutoff
            .chars()
            .filter(|character| character.is_ascii_digit())
            .count();
        if digits < 8
            || digits > 14
            || !cutoff.bytes().all(|byte| {
                byte.is_ascii_digit() || matches!(byte, b'-' | b'T' | b':' | b'Z' | b'.')
            })
        {
            return Err("cleanup cutoff must be an ISO date or timestamp".into());
        }
    }
    Ok(())
}

fn cleanup_preview_connection(
    connection: &Connection,
    domains: &[DataDomain],
    cutoff: Option<&str>,
) -> Result<Value, String> {
    if domains.is_empty() {
        return Err("select at least one external data domain".into());
    }
    let unique_domains = domains
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    if unique_domains.len() != domains.len() {
        return Err("external data cleanup domains must be unique".into());
    }
    validate_cutoff(cutoff)?;
    let mut grouped = Vec::new();
    let mut total_records = 0_u64;
    let mut total_bytes = 0_u64;
    let mut fingerprint = Sha256::new();
    fingerprint.update(cutoff.unwrap_or("all").as_bytes());
    for domain in domains {
        fingerprint.update(domain.as_str().as_bytes());
        let count_sql = format!(
            "SELECT count(*), coalesce(sum(length(payload_json)+length(quality_json)),0)
             FROM records WHERE domain=?1 AND (?2 IS NULL OR ({NORMALIZED_RECORD_DATE}) < ?2)"
        );
        let (records, bytes): (i64, i64) = connection
            .query_row(&count_sql, params![domain.as_str(), cutoff], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|error| format!("preview external data cleanup: {error}"))?;
        let records = records.max(0) as u64;
        let bytes = bytes.max(0) as u64;
        total_records = total_records.saturating_add(records);
        total_bytes = total_bytes.saturating_add(bytes);
        let fingerprint_sql = format!(
            "SELECT market, entity_key, record_key, source_id, payload_hash
             FROM records WHERE domain=?1 AND (?2 IS NULL OR ({NORMALIZED_RECORD_DATE}) < ?2)
             ORDER BY market, entity_key, record_key, source_id"
        );
        let mut statement = connection
            .prepare(&fingerprint_sql)
            .map_err(|error| format!("prepare external cleanup fingerprint: {error}"))?;
        let rows = statement
            .query_map(params![domain.as_str(), cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|error| format!("query external cleanup fingerprint: {error}"))?;
        for row in rows {
            let (market, entity, record, source, hash) =
                row.map_err(|error| format!("read external cleanup fingerprint: {error}"))?;
            for item in [market, entity, record, source, hash] {
                fingerprint.update(item.as_bytes());
                fingerprint.update([0]);
            }
        }
        grouped.push(json!({
            "domain": domain.as_str(),
            "record_count": records,
            "payload_bytes": bytes,
            "cutoff": cutoff,
        }));
    }
    let preview_token = fingerprint
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(json!({
        "preview_token": preview_token,
        "domains": grouped,
        "record_count": total_records,
        "payload_bytes": total_bytes,
        "cutoff": cutoff,
        "requires_confirmation": true,
    }))
}

struct PreparedRecord<'a> {
    domain: &'static str,
    market: &'static str,
    entity_key: &'a str,
    record_key: &'a str,
    source_id: &'a str,
    schema_version: i64,
    source_tier: &'a str,
    fetched_at: &'a str,
    observed_at: Option<&'a str>,
    disclosed_at: Option<&'a str>,
    verification_status: &'a str,
    quality_json: String,
    payload_json: String,
    payload_hash: String,
}

fn prepare_record(record: &StoredRecord) -> Result<PreparedRecord<'_>, String> {
    for (label, value, max_bytes) in [
        ("entity_key", record.entity_key.as_str(), MAX_KEY_BYTES),
        ("record_key", record.record_key.as_str(), MAX_KEY_BYTES),
        (
            "source_id",
            record.metadata.source_id.as_str(),
            MAX_SOURCE_ID_BYTES,
        ),
    ] {
        if value.trim().is_empty() {
            return Err(format!("external record {label} must not be empty"));
        }
        if value.len() > max_bytes {
            return Err(format!("external record {label} exceeds {max_bytes} bytes"));
        }
    }
    if record.metadata.schema_version == 0 {
        return Err("external record schema version must be positive".into());
    }
    if contains_non_finite_number(&record.payload) {
        return Err("external record contains a non-finite numeric value".into());
    }
    let payload_bytes = serde_json::to_vec(&record.payload)
        .map_err(|error| format!("encode external record payload: {error}"))?;
    if payload_bytes.len() > MAX_RECORD_BYTES {
        return Err(format!(
            "external record exceeds the {MAX_RECORD_BYTES}-byte payload limit"
        ));
    }
    let quality_json = serde_json::to_string(&record.metadata.quality_flags)
        .map_err(|error| format!("encode external record quality flags: {error}"))?;
    let payload_hash = sha256_hex(&payload_bytes);
    let payload_json = String::from_utf8(payload_bytes)
        .map_err(|error| format!("encode external record payload as UTF-8: {error}"))?;

    Ok(PreparedRecord {
        domain: record.domain.as_str(),
        market: record.market.as_str(),
        entity_key: &record.entity_key,
        record_key: &record.record_key,
        source_id: &record.metadata.source_id,
        schema_version: record.metadata.schema_version as i64,
        source_tier: &record.metadata.source_tier,
        fetched_at: &record.metadata.fetched_at,
        observed_at: record.metadata.observed_at.as_deref(),
        disclosed_at: record.metadata.disclosed_at.as_deref(),
        verification_status: &record.metadata.verification_status,
        quality_json,
        payload_json,
        payload_hash,
    })
}

fn contains_non_finite_number(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_f64().is_some_and(|value| !value.is_finite()),
        Value::Array(items) => items.iter().any(contains_non_finite_number),
        Value::Object(items) => items.values().any(contains_non_finite_number),
        _ => false,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn pragma_i64(connection: &Connection, name: &str) -> Result<i64, String> {
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .map_err(|error| format!("read external data {name}: {error}"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
