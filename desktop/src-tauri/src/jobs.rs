//! Bounded, opt-in native job lifecycle. See docs/reliability/jobs-delivery.md.
//! No request, result, native error text, credential, or URL is persisted.
use futures::FutureExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    future::Future,
    io::Write,
    panic::AssertUnwindSafe,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};

const SCHEMA_VERSION: i64 = 1;
const MAX_RECORDS: usize = 10_000;
const MAX_ACTIVE: usize = 16;
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_PACK_REQUEST_BYTES: usize = (64 * 1024 * 1024 / 3 + 1) * 4 + 16 * 1024;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
const STORE_ERROR: &str = "job_store_unavailable";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    MarketRefresh,
    Backtest,
    PackImport,
    Index,
}
impl Kind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "market_refresh" => Ok(Self::MarketRefresh),
            "backtest" => Ok(Self::Backtest),
            "research_pack_import" => Ok(Self::PackImport),
            "research_rebuild_index" => Ok(Self::Index),
            _ => Err("job_kind_not_supported".into()),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::MarketRefresh => "market_refresh",
            Self::Backtest => "backtest",
            Self::PackImport => "research_pack_import",
            Self::Index => "research_rebuild_index",
        }
    }
    fn slot(self) -> usize {
        match self {
            Self::MarketRefresh => 0,
            Self::Backtest => 1,
            Self::PackImport => 2,
            Self::Index => 3,
        }
    }
    fn resource(self) -> Option<&'static str> {
        match self {
            Self::MarketRefresh => Some("market"),
            Self::Backtest => None,
            Self::PackImport | Self::Index => Some("research"),
        }
    }
    fn timeout(self) -> Duration {
        Duration::from_secs(match self {
            Self::MarketRefresh => 90,
            Self::Backtest => 180,
            Self::PackImport => 120,
            Self::Index => 120,
        })
    }
}

#[derive(Clone)]
struct Request {
    kind: Kind,
    payload: Value,
    fingerprint: String,
}
impl Request {
    fn new(kind: &str, mut payload: Value) -> Result<Self, String> {
        let kind = Kind::parse(kind)?;
        let request_limit = if kind == Kind::PackImport {
            MAX_PACK_REQUEST_BYTES
        } else {
            MAX_REQUEST_BYTES
        };
        // Bound before canonicalization/cloning. The JSON IPC parser itself is outside this module.
        bounded_json(&payload, request_limit).map_err(|_| "job_request_too_large")?;
        let object = payload
            .as_object_mut()
            .ok_or("job_payload_must_be_object")?;
        match kind {
            Kind::MarketRefresh => {
                // One bounded page group, never an automatic refresh loop. Explicit values are
                // validated rather than silently clamped, so the fingerprint matches execution.
                for (key, default, max, min) in [
                    ("batch_count", 4, 1000, 1),
                    ("max_candidates", 2000, 100000, 1),
                    ("max_failed_batches", 3, 3, 1),
                    ("batch_start", 0, 100_000, 0),
                ] {
                    let value = object.entry(key).or_insert(Value::from(default));
                    if !value.as_u64().is_some_and(|v| v >= min && v <= max) {
                        return Err("job_refresh_budget_exceeded".into());
                    }
                }
            }
            Kind::PackImport => {
                // Path mode remains bounded by the native portable-pack reader (64 MiB).
                if object
                    .keys()
                    .any(|key| key != "path" && key != "bytes_base64")
                    || object.len() != 1
                    || !object
                        .values()
                        .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
                {
                    return Err("job_pack_payload_invalid".into());
                }
            }
            Kind::Index if !object.is_empty() => {
                return Err("job_index_payload_must_be_empty".into())
            }
            _ => {}
        }
        fn canonical(value: Value, depth: usize) -> Result<Value, String> {
            if depth > 32 {
                return Err("job_payload_too_deep".into());
            }
            Ok(match value {
                Value::Object(map) => {
                    let sorted: std::collections::BTreeMap<_, _> = map.into_iter().collect();
                    Value::Object(
                        sorted
                            .into_iter()
                            .map(|(k, v)| Ok((k, canonical(v, depth + 1)?)))
                            .collect::<Result<_, String>>()?,
                    )
                }
                Value::Array(items) => Value::Array(
                    items
                        .into_iter()
                        .map(|v| canonical(v, depth + 1))
                        .collect::<Result<_, _>>()?,
                ),
                other => other,
            })
        }
        let payload = canonical(payload, 0)?;
        let mut hash = Sha256::new();
        hash.update(b"gp-native-job-v1\0");
        hash.update(kind.name());
        hash.update(b"\0");
        serialize_bounded(&payload, request_limit, Some(&mut hash))
            .map_err(|_| "job_request_too_large")?;
        let fingerprint = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self {
            kind,
            payload,
            fingerprint,
        })
    }
}

// serde_json::to_vec would allocate the entire oversized output before rejecting it.
fn bounded_json(value: &Value, limit: usize) -> Result<(), String> {
    serialize_bounded(value, limit, None)
}
fn serialize_bounded(value: &Value, limit: usize, hash: Option<&mut Sha256>) -> Result<(), String> {
    struct Bounded<'a> {
        written: usize,
        limit: usize,
        hash: Option<&'a mut Sha256>,
    }
    impl Write for Bounded<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.written) {
                return Err(std::io::Error::other("size limit"));
            }
            self.written += bytes.len();
            if let Some(hash) = self.hash.as_mut() {
                hash.update(bytes);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded {
        written: 0,
        limit,
        hash,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| "job_value_too_large".into())
}

fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err("job_id_invalid".into())
    } else {
        Ok(())
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn db_error(_: rusqlite::Error) -> String {
    STORE_ERROR.into()
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct JobStatus {
    job_id: String,
    kind: String,
    state: String,
    reason: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    cancel_requested: bool,
    /// Conservative: a dropped native future may have left a non-abortable blocking worker.
    blocking_may_continue: bool,
    side_effects_may_have_occurred: bool,
}
struct Store {
    conn: Connection,
    limit: usize,
}
impl Store {
    /// Call exactly once per process, before exposing commands. Never on a status read.
    fn open(path: &Path) -> Result<Self, String> {
        if path.exists() {
            crate::durability::validate_sqlite_header(path).map_err(|_| STORE_ERROR)?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| STORE_ERROR)?;
        }
        let mut conn = Connection::open(path).map_err(db_error)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(db_error)?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err("job_schema_unsupported".into());
        }
        if version == 0 {
            let tables: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if tables != 0 {
                return Err("job_schema_unsupported".into());
            }
        }
        crate::durability::configure_user_connection(&conn).map_err(|_| STORE_ERROR)?;
        conn.busy_timeout(Duration::from_millis(250))
            .map_err(db_error)?;
        conn.execute_batch("PRAGMA max_page_count=16384; PRAGMA wal_autocheckpoint=64; PRAGMA journal_size_limit=4194304;").map_err(db_error)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE jobs (
                job_id TEXT PRIMARY KEY CHECK(length(job_id) BETWEEN 1 AND 80),
                kind TEXT NOT NULL CHECK(length(kind)<=40),
                fingerprint TEXT NOT NULL CHECK(length(fingerprint) IN (0,64)),
                state TEXT NOT NULL CHECK(state IN ('queued','running','completed','failed','cancelled','interrupted')),
                reason TEXT NOT NULL CHECK(length(reason)<=40),
                created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
                cancel_requested INTEGER NOT NULL DEFAULT 0,
                blocking_may_continue INTEGER NOT NULL DEFAULT 0,
                side_effects_may_have_occurred INTEGER NOT NULL DEFAULT 0
            ); PRAGMA user_version=1;").map_err(db_error)?;
        }
        // A new process does not replay requests. Old workers cannot still exist under the
        // application's single-instance contract. Side-effect uncertainty survives restart.
        tx.execute("UPDATE jobs SET state='interrupted', reason='process_restarted', updated_at_ms=?1 WHERE state IN ('queued','running')", [now()]).map_err(db_error)?;
        tx.execute(
            "UPDATE jobs SET blocking_may_continue=0 WHERE blocking_may_continue!=0",
            [],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(Self {
            conn,
            limit: MAX_RECORDS,
        })
    }
    fn duplicate(&mut self, id: &str, request: &Request) -> Result<(), String> {
        let existing: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT fingerprint,state FROM jobs WHERE job_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_error)?;
        if let Some((fingerprint, state)) = existing {
            // An early cancel has no request yet; bind the first arrival, but never execute it.
            if fingerprint.is_empty() {
                self.conn
                    .execute(
                        "UPDATE jobs SET fingerprint=?2,kind=?3 WHERE job_id=?1 AND fingerprint=''",
                        params![id, request.fingerprint, request.kind.name()],
                    )
                    .map_err(db_error)?;
            } else if fingerprint != request.fingerprint {
                return Err("job_id_conflict".into());
            }
            return Err(match state.as_str() {
                "completed" => "job_already_completed",
                "cancelled" => "job_cancelled",
                "interrupted" => "job_interrupted",
                "failed" => "job_already_failed",
                _ => "job_in_progress",
            }
            .into());
        }
        Ok(())
    }
    fn capacity(&self) -> Result<(), String> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
            .map_err(db_error)?;
        if count >= self.limit as i64 {
            Err("job_store_full".into())
        } else {
            Ok(())
        }
    }
    fn enqueue(&mut self, id: &str, request: &Request) -> Result<(), String> {
        valid_id(id)?;
        self.duplicate(id, request)?;
        self.capacity()?;
        self.conn.execute("INSERT INTO jobs(job_id,kind,fingerprint,state,reason,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,'queued','queued',?4,?4)", params![id,request.kind.name(),request.fingerprint,now()]).map_err(db_error)?;
        Ok(())
    }
    fn running(&self, id: &str) -> Result<(), String> {
        let changed = self.conn.execute("UPDATE jobs SET state='running',reason='running',blocking_may_continue=1,side_effects_may_have_occurred=1,updated_at_ms=?2 WHERE job_id=?1 AND state='queued'", params![id,now()]).map_err(db_error)?;
        if changed != 1 {
            Err("job_cancelled".into())
        } else {
            Ok(())
        }
    }
    fn finish(
        &self,
        id: &str,
        state: &str,
        reason: &str,
        uncertain: bool,
    ) -> Result<JobStatus, String> {
        self.conn.execute("UPDATE jobs SET state=?2,reason=?3,blocking_may_continue=?4,cancel_requested=CASE WHEN ?2='cancelled' THEN 1 ELSE cancel_requested END,updated_at_ms=?5 WHERE job_id=?1 AND state IN ('queued','running')", params![id,state,reason,uncertain,now()]).map_err(db_error)?;
        self.status(id)
    }
    fn cancel(&mut self, id: &str) -> Result<JobStatus, String> {
        valid_id(id)?;
        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE job_id=?1)",
                [id],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            self.capacity()?;
            self.conn.execute("INSERT INTO jobs(job_id,kind,fingerprint,state,reason,created_at_ms,updated_at_ms,cancel_requested) VALUES(?1,'','','cancelled','cancelled_before_submit',?2,?2,1)", params![id,now()]).map_err(db_error)?;
        } else {
            self.conn.execute("UPDATE jobs SET cancel_requested=1,blocking_may_continue=(state='running'),state='cancelled',reason='cancelled',updated_at_ms=?2 WHERE job_id=?1 AND state IN ('queued','running')", params![id,now()]).map_err(db_error)?;
        }
        self.status(id)
    }
    fn status(&self, id: &str) -> Result<JobStatus, String> {
        valid_id(id)?;
        self.conn.query_row("SELECT job_id,kind,state,reason,created_at_ms,updated_at_ms,cancel_requested,blocking_may_continue,side_effects_may_have_occurred FROM jobs WHERE job_id=?1", [id], |r| Ok(JobStatus {
            job_id:r.get(0)?, kind:r.get(1)?, state:r.get(2)?, reason:r.get(3)?, created_at_ms:r.get(4)?, updated_at_ms:r.get(5)?, cancel_requested:r.get(6)?, blocking_may_continue:r.get(7)?, side_effects_may_have_occurred:r.get(8)?,
        })).optional().map_err(db_error)?.ok_or_else(|| "job_not_found".into())
    }
}

struct Active {
    cancel: watch::Sender<bool>,
    resource: Option<&'static str>,
    permit: Option<OwnedSemaphorePermit>,
}
struct Inner {
    store: Mutex<Store>,
    store_lane: Arc<Semaphore>,
    active: Mutex<HashMap<String, Active>>,
    lanes: [Arc<Semaphore>; 4],
}
#[derive(Clone)]
pub(crate) struct Jobs(Arc<Inner>);
impl Jobs {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self(Arc::new(Inner {
            store: Mutex::new(Store::open(path)?),
            store_lane: Arc::new(Semaphore::new(1)),
            active: Mutex::new(HashMap::new()),
            lanes: [
                Arc::new(Semaphore::new(1)),
                Arc::new(Semaphore::new(2)),
                Arc::new(Semaphore::new(1)),
                Arc::new(Semaphore::new(1)),
            ],
        })))
    }
    async fn store<T: Send + 'static>(
        &self,
        task: impl FnOnce(&mut Store) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let inner = self.0.clone();
        // Metadata must not wait behind the native operation it is trying to cancel.
        let permit = tokio::time::timeout(
            Duration::from_secs(2),
            inner.store_lane.clone().acquire_owned(),
        )
        .await
        .map_err(|_| STORE_ERROR)?
        .map_err(|_| STORE_ERROR)?;
        tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit; // Retained by the worker after waiter cancellation.
            let mut store = inner.store.lock().map_err(|_| STORE_ERROR)?;
            task(&mut store)
        })
        .await
        .map_err(|_| STORE_ERROR)?
    }
    async fn status(&self, id: String) -> Result<JobStatus, String> {
        self.store(move |s| s.status(&id)).await
    }
    fn signal(&self, id: &str) {
        if let Ok(active) = self.0.active.lock() {
            if let Some(entry) = active.get(id) {
                entry.cancel.send_replace(true);
            }
        }
    }
    async fn cancel(&self, id: String) -> Result<JobStatus, String> {
        valid_id(&id)?;
        // Signal without waiting for SQLite or a runtime IO permit. Store failure must not
        // prevent best-effort cooperative cancellation, but is still returned to the caller.
        self.signal(&id);
        let jobs = self.clone();
        self.store(move |s| {
            let result = s.cancel(&id);
            jobs.signal(&id);
            result
        })
        .await
    }
    async fn execute<F, Fut>(
        &self,
        id: &str,
        request: Request,
        budget: Duration,
        work: F,
    ) -> Result<Value, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Value, String>>,
    {
        valid_id(id)?;
        let deadline = tokio::time::Instant::now() + budget;
        let key = id.to_string();
        let admitted_key = key.clone();
        let kind = request.kind;
        let jobs = self.clone();
        // Admission is intentionally NOT select-cancelled: a blocking insert must resolve
        // before its owner can leave. Otherwise a late queued row could become orphaned.
        let mut cancel = self
            .store(move |store| {
                store.duplicate(&admitted_key, &request)?;
                let mut active = jobs.0.active.lock().map_err(|_| STORE_ERROR)?;
                if active.len() >= MAX_ACTIVE {
                    return Err("job_queue_full".into());
                }
                if kind.resource().is_some()
                    && active.values().any(|a| a.resource == kind.resource())
                {
                    return Err("job_resource_busy".into());
                }
                store.enqueue(&admitted_key, &request)?;
                let (sender, receiver) = watch::channel(false);
                active.insert(
                    admitted_key,
                    Active {
                        cancel: sender,
                        resource: kind.resource(),
                        permit: None,
                    },
                );
                Ok(receiver)
            })
            .await?;
        async fn cancelled(receiver: &mut watch::Receiver<bool>) {
            loop {
                if *receiver.borrow_and_update() {
                    return;
                }
                if receiver.changed().await.is_err() {
                    return;
                }
            }
        }
        let permit = tokio::select! {
            biased;
            _ = cancelled(&mut cancel) => Err("job_cancelled"),
            _ = tokio::time::sleep_until(deadline) => Err("job_timed_out"),
            permit = self.0.lanes[kind.slot()].clone().acquire_owned() => permit.map_err(|_| "job_limiter_closed"),
        };
        let permit = match permit {
            Ok(permit) => permit,
            Err(reason) => {
                return self.end(key, Err(reason.into()), None).await;
            }
        };
        {
            let mut active = self.0.active.lock().map_err(|_| STORE_ERROR)?;
            active.get_mut(&key).ok_or(STORE_ERROR)?.permit = Some(permit);
        }
        let running_key = key.clone();
        if let Err(error) = self.store(move |store| store.running(&running_key)).await {
            // No native work has started; even on storage failure it is safe to release
            // the lane. A persisted queued/cancelled row still prevents duplicate replay.
            if let Ok(mut active) = self.0.active.lock() {
                active.remove(&key);
            }
            return Err(error);
        }
        let blocking = Arc::new(crate::runtime::BlockingWork::default());
        let outcome = {
            // The native future is scoped INSIDE this block: select losers really are
            // dropped before the terminal lifecycle write, not merely stopped polling.
            let future = AssertUnwindSafe(crate::runtime::with_blocking_work(
                blocking.clone(),
                async { work().await },
            ))
            .catch_unwind();
            tokio::pin!(future);
            tokio::select! {
                biased;
                _ = cancelled(&mut cancel) => Err("job_cancelled".into()),
                _ = tokio::time::sleep_until(deadline) => Err("job_timed_out".into()),
                result = &mut future => result.unwrap_or_else(|_| Err("job_native_panicked".into())),
            }
        };
        self.end(key, outcome, Some(blocking)).await
    }
    async fn release_finished_work(&self, key: String) {
        let row = key.clone();
        // Even if this metadata update fails, duplicate IDs remain non-replayable.
        // Actual worker completion, not a database error, owns resource lifetime.
        let _ = self
            .store(move |store| {
                store
                    .conn
                    .execute(
                        "UPDATE jobs SET blocking_may_continue=0 WHERE job_id=?1",
                        [row],
                    )
                    .map_err(db_error)?;
                Ok(())
            })
            .await;
        if let Ok(mut active) = self.0.active.lock() {
            active.remove(&key);
        }
    }
    async fn end(
        &self,
        key: String,
        result: Result<Value, String>,
        blocking: Option<Arc<crate::runtime::BlockingWork>>,
    ) -> Result<Value, String> {
        let (state, reason) = match result.as_ref().map_err(String::as_str) {
            Ok(_) => ("completed", "ok"),
            Err("job_cancelled") => ("cancelled", "cancelled"),
            Err("job_timed_out") => ("failed", "timed_out"),
            Err("job_native_panicked") => ("interrupted", "native_panicked"),
            Err(_) => ("failed", "native_failed"),
        };
        let event = match state {
            "completed" => crate::diagnostics::OperationalEvent::TaskCompleted,
            "cancelled" => crate::diagnostics::OperationalEvent::TaskCancelled,
            _ => crate::diagnostics::OperationalEvent::TaskFailed,
        };
        let _ = crate::diagnostics::record(event);
        let uncertain = blocking.as_ref().is_some_and(|work| work.is_active());
        let oversized = result
            .as_ref()
            .is_ok_and(|value| bounded_json(value, MAX_RESULT_BYTES).is_err());
        let finish_key = key.clone();
        let persisted = self
            .store(move |store| {
                store.finish(
                    &finish_key,
                    state,
                    if oversized {
                        "result_too_large"
                    } else {
                        reason
                    },
                    uncertain,
                )
            })
            .await;
        if let Some(work) = blocking.filter(|work| work.is_active()) {
            let jobs = self.clone();
            tauri::async_runtime::spawn(async move {
                work.wait_idle().await;
                jobs.release_finished_work(key).await;
            });
        } else {
            self.release_finished_work(key).await;
        }
        let status = persisted?;
        if status.state == "cancelled" {
            return Err("job_cancelled".into());
        }
        if oversized {
            return Err("job_result_too_large".into());
        }
        result
    }
}

/// Parent setup hook: call once on the setup thread before registering/exposing jobs.
/// Repeated init is harmless; startup reconciliation is never repeated on status/run.
pub(crate) fn init(app: &tauri::AppHandle) -> Result<(), String> {
    // The parent must serialize setup; Tauri setup is single-threaded.
    if app.try_state::<Jobs>().is_some() {
        return Ok(());
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| STORE_ERROR)?
        .join("jobs.sqlite");
    let jobs = Jobs::open(&path)?;
    if !app.manage(jobs) {
        return Err("job_already_initialized".into());
    }
    Ok(())
}
fn manager(app: &tauri::AppHandle) -> Result<Jobs, String> {
    app.try_state::<Jobs>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| "jobs_not_initialized".into())
}

#[tauri::command]
pub(crate) async fn api_job_run(
    app: tauri::AppHandle,
    job_id: String,
    kind: String,
    payload: Value,
) -> Result<Value, String> {
    let jobs = manager(&app)?;
    valid_id(&job_id)?;
    let request = Request::new(&kind, payload)?;
    let kind = request.kind;
    let payload = request.payload.clone();
    // Own the lifecycle even if the IPC receiver goes away; only api_job_cancel or the
    // native deadline cancels execution. This is NOT a replay/background retry mechanism.
    tauri::async_runtime::spawn(async move {
        jobs.execute(&job_id, request, kind.timeout(), || {
            dispatch(app, kind, payload)
        })
        .await
    })
    .await
    .map_err(|_| "job_runner_interrupted".to_string())?
}
#[tauri::command]
pub(crate) async fn api_job_status(
    app: tauri::AppHandle,
    job_id: String,
) -> Result<JobStatus, String> {
    manager(&app)?.status(job_id).await
}
#[tauri::command]
pub(crate) async fn api_job_cancel(
    app: tauri::AppHandle,
    job_id: String,
) -> Result<JobStatus, String> {
    manager(&app)?.cancel(job_id).await
}

pub(crate) fn has_active_work(app: &tauri::AppHandle) -> bool {
    manager(app)
        .ok()
        .and_then(|jobs| jobs.0.active.lock().ok().map(|active| !active.is_empty()))
        .unwrap_or(true)
}

async fn dispatch(app: tauri::AppHandle, kind: Kind, payload: Value) -> Result<Value, String> {
    match kind {
        Kind::MarketRefresh => crate::market::api_market_refresh(app, payload).await,
        Kind::Backtest => crate::screening::api_backtest(app, payload).await,
        // Deliberately use the existing synchronous import primitive, not the command
        // that detaches embedding work. FTS is rebuilt by import_app_pack itself.
        Kind::PackImport => {
            crate::runtime::run_io_bound("job research pack import", move || {
                crate::research::import_app_pack(&app, &payload)
            })
            .await?
        }
        Kind::Index => crate::research::api_research_rebuild_index(app).await,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "gp-jobs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn db(&self) -> std::path::PathBuf {
            self.0.join("jobs.sqlite")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn request() -> Request {
        Request::new("backtest", json!({})).unwrap()
    }

    #[test]
    fn canonical_fingerprint_duplicates_and_no_replay() {
        let dir = Temp::new();
        let mut store = Store::open(&dir.db()).unwrap();
        let a = Request::new("backtest", json!({"a":1,"b":{"x":2,"y":3}})).unwrap();
        let b = Request::new("backtest", json!({"b":{"y":3,"x":2},"a":1})).unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        store.enqueue("same", &a).unwrap();
        assert_eq!(store.enqueue("same", &b).unwrap_err(), "job_in_progress");
        assert_eq!(
            store.enqueue("same", &request()).unwrap_err(),
            "job_id_conflict"
        );
        store.running("same").unwrap();
        store.finish("same", "completed", "ok", false).unwrap();
        assert_eq!(
            store.enqueue("same", &a).unwrap_err(),
            "job_already_completed"
        );
        drop(store);
        assert_eq!(
            Store::open(&dir.db())
                .unwrap()
                .enqueue("same", &a)
                .unwrap_err(),
            "job_already_completed"
        );
    }

    #[test]
    fn startup_reconciles_queued_and_running_without_replay() {
        let dir = Temp::new();
        let mut store = Store::open(&dir.db()).unwrap();
        store.enqueue("queued", &request()).unwrap();
        store.enqueue("running", &request()).unwrap();
        store.running("running").unwrap();
        drop(store);
        let mut store = Store::open(&dir.db()).unwrap();
        for id in ["queued", "running"] {
            let status = store.status(id).unwrap();
            assert_eq!(status.state, "interrupted");
            assert!(!status.blocking_may_continue);
            assert_eq!(
                store.enqueue(id, &request()).unwrap_err(),
                "job_interrupted"
            );
        }
        assert!(
            !store
                .status("queued")
                .unwrap()
                .side_effects_may_have_occurred
        );
        assert!(
            store
                .status("running")
                .unwrap()
                .side_effects_may_have_occurred
        );
    }

    #[test]
    fn early_cancel_is_durable_and_cannot_be_lost_before_admission() {
        let dir = Temp::new();
        let mut store = Store::open(&dir.db()).unwrap();
        assert_eq!(store.cancel("early").unwrap().state, "cancelled");
        drop(store);
        let mut store = Store::open(&dir.db()).unwrap();
        assert_eq!(
            store.enqueue("early", &request()).unwrap_err(),
            "job_cancelled"
        );
        assert_eq!(
            store
                .enqueue(
                    "early",
                    &Request::new("backtest", json!({"different":true})).unwrap()
                )
                .unwrap_err(),
            "job_id_conflict"
        );
    }

    #[test]
    fn rejects_versions_corruption_locks_capacity_and_oversize_without_dispatch() {
        let dir = Temp::new();
        let conn = Connection::open(dir.db()).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(Store::open(&dir.db()).is_err());
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            999
        );
        drop(conn);
        let other = Temp::new();
        std::fs::write(other.db(), b"not sqlite").unwrap();
        assert!(Store::open(&other.db()).is_err());
        assert_eq!(std::fs::read(other.db()).unwrap(), b"not sqlite");
        let clean = Temp::new();
        let mut store = Store::open(&clean.db()).unwrap();
        let lock = Connection::open(clean.db()).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert_eq!(
            store.enqueue("locked", &request()).unwrap_err(),
            "job_store_unavailable"
        );
        lock.execute_batch("ROLLBACK").unwrap();
        assert!(store.status("locked").is_err());
        store.limit = 1;
        store.enqueue("only", &request()).unwrap();
        assert_eq!(
            store.enqueue("extra", &request()).unwrap_err(),
            "job_store_full"
        );
        assert!(Request::new("backtest", json!({"x":"x".repeat(MAX_REQUEST_BYTES)})).is_err());
        assert!(Request::new("paid_ai", json!({})).is_err());
        assert!(Request::new("research_rebuild_index", json!({"ignored":1})).is_err());
    }

    #[test]
    fn lifecycle_privacy_and_storage_durability() {
        let dir = Temp::new();
        let mut store = Store::open(&dir.db()).unwrap();
        let secret = "https://secret.invalid/token?api_key=unique-secret";
        store
            .enqueue(
                "private",
                &Request::new("backtest", json!({"api_key":secret})).unwrap(),
            )
            .unwrap();
        store.running("private").unwrap();
        store
            .finish("private", "failed", "native_failed", false)
            .unwrap();
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        let dump: String = store
            .conn
            .query_row(
                "SELECT job_id || kind || fingerprint || reason FROM jobs",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!dump.contains(secret));
        assert!(!dump.contains("api_key"));
    }

    #[test]
    fn cancel_and_timeout_drop_async_work_before_followup_batches() {
        tauri::async_runtime::block_on(async {
            for timeout in [false, true] {
                let dir = Temp::new();
                let jobs = Jobs::open(&dir.db()).unwrap();
                let calls = Arc::new(AtomicUsize::new(0));
                let next = calls.clone();
                let runner = jobs.clone();
                let handle = tauri::async_runtime::spawn(async move {
                    runner
                        .execute(
                            "stop",
                            request(),
                            Duration::from_millis(if timeout { 35 } else { 5000 }),
                            || async move {
                                next.fetch_add(1, Ordering::SeqCst);
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                next.fetch_add(1, Ordering::SeqCst);
                                Ok(json!({"done":true}))
                            },
                        )
                        .await
                });
                while calls.load(Ordering::SeqCst) == 0 {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                if !timeout {
                    jobs.cancel("stop".into()).await.unwrap();
                }
                assert_eq!(
                    handle.await.unwrap().unwrap_err(),
                    if timeout {
                        "job_timed_out"
                    } else {
                        "job_cancelled"
                    }
                );
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                let status = jobs.status("stop".into()).await.unwrap();
                assert_eq!(status.state, if timeout { "failed" } else { "cancelled" });
                assert!(!status.blocking_may_continue);
                assert!(status.side_effects_may_have_occurred);
            }
        });
    }

    #[test]
    fn executor_does_not_replay_completed_or_oversized_results() {
        tauri::async_runtime::block_on(async {
            let dir = Temp::new();
            let jobs = Jobs::open(&dir.db()).unwrap();
            let count = AtomicUsize::new(0);
            let result = jobs
                .execute("done", request(), Duration::from_secs(2), || async {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"ok":true}))
                })
                .await
                .unwrap();
            assert_eq!(result, json!({"ok":true}));
            assert_eq!(
                jobs.execute("done", request(), Duration::from_secs(2), || async {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({}))
                })
                .await
                .unwrap_err(),
                "job_already_completed"
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert_eq!(
                jobs.execute("large", request(), Duration::from_secs(2), || async {
                    Ok(json!({"x":"x".repeat(MAX_RESULT_BYTES)}))
                })
                .await
                .unwrap_err(),
                "job_result_too_large"
            );
            assert_eq!(
                jobs.status("large".into()).await.unwrap().state,
                "completed"
            );
            assert_eq!(
                jobs.execute("large", request(), Duration::from_secs(2), || async {
                    panic!("must not replay")
                })
                .await
                .unwrap_err(),
                "job_already_completed"
            );
        });
    }
    #[test]
    fn queued_cancel_and_timeout_never_poll_native_work() {
        tauri::async_runtime::block_on(async {
            for timeout in [false, true] {
                let dir = Temp::new();
                let jobs = Jobs::open(&dir.db()).unwrap();
                let held = jobs.0.lanes[1].clone().acquire_many_owned(2).await.unwrap();
                let worker = jobs.clone();
                let run = tauri::async_runtime::spawn(async move {
                    worker
                        .execute(
                            "queue",
                            request(),
                            Duration::from_millis(if timeout { 40 } else { 2000 }),
                            || async { panic!("queued work must never be polled") },
                        )
                        .await
                });
                tokio::time::timeout(Duration::from_secs(2), async {
                    while jobs.status("queue".into()).await.is_err() {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                })
                .await
                .unwrap();
                if !timeout {
                    jobs.cancel("queue".into()).await.unwrap();
                }
                assert_eq!(
                    run.await.unwrap().unwrap_err(),
                    if timeout {
                        "job_timed_out"
                    } else {
                        "job_cancelled"
                    }
                );
                let status = jobs.status("queue".into()).await.unwrap();
                assert!(!status.blocking_may_continue);
                assert!(!status.side_effects_may_have_occurred);
                assert!(jobs.0.active.lock().unwrap().is_empty());
                drop(held);
            }
        });
    }

    #[test]
    fn refresh_dedupe_quarantine_and_default_fingerprint() {
        tauri::async_runtime::block_on(async {
            let dir = Temp::new();
            let jobs = Jobs::open(&dir.db()).unwrap();
            let request = Request::new("market_refresh", json!({})).unwrap();
            assert_eq!(request.fingerprint, Request::new("market_refresh", json!({"batch_count":4,"batch_start":0,"max_candidates":2000,"max_failed_batches":3})).unwrap().fingerprint);
            assert!(Request::new("market_refresh", json!({"batch_count":1001})).is_err());
            let worker = jobs.clone();
            let run = tauri::async_runtime::spawn(async move {
                worker
                    .execute("refresh", request, Duration::from_secs(2), || async {
                        std::future::pending().await
                    })
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if jobs
                        .status("refresh".into())
                        .await
                        .is_ok_and(|s| s.state == "running")
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            let duplicate = || Request::new("market_refresh", json!({"batch_start":1})).unwrap();
            assert_eq!(
                jobs.execute("other", duplicate(), Duration::from_secs(2), || async {
                    panic!("must dedupe")
                })
                .await
                .unwrap_err(),
                "job_resource_busy"
            );
            assert!(jobs.status("other".into()).await.is_err());
            assert_eq!(
                jobs.execute("refresh", duplicate(), Duration::from_secs(2), || async {
                    panic!("ID conflict")
                })
                .await
                .unwrap_err(),
                "job_id_conflict"
            );
            jobs.cancel("refresh".into()).await.unwrap();
            assert_eq!(run.await.unwrap().unwrap_err(), "job_cancelled");
            assert_eq!(jobs.0.lanes[0].available_permits(), 1);
            assert!(jobs
                .execute("later", duplicate(), Duration::from_secs(2), || async {
                    Ok(json!({}))
                })
                .await
                .is_ok());
            drop(jobs);
            let restarted = Jobs::open(&dir.db()).unwrap();
            assert!(
                !restarted
                    .status("refresh".into())
                    .await
                    .unwrap()
                    .blocking_may_continue
            );
            assert_eq!(
                restarted
                    .execute("fresh", duplicate(), Duration::from_secs(2), || async {
                        Ok(json!({}))
                    })
                    .await
                    .unwrap(),
                json!({})
            );
        });
    }

    #[test]
    fn blocked_admission_never_dispatches_and_failed_final_write_never_replays() {
        tauri::async_runtime::block_on(async {
            let dir = Temp::new();
            let jobs = Jobs::open(&dir.db()).unwrap();
            let lock = Connection::open(dir.db()).unwrap();
            lock.execute_batch("BEGIN IMMEDIATE").unwrap();
            assert_eq!(
                jobs.execute("locked", request(), Duration::from_secs(2), || async {
                    panic!("blocked admission")
                })
                .await
                .unwrap_err(),
                STORE_ERROR
            );
            lock.execute_batch("ROLLBACK").unwrap();
            assert!(jobs.status("locked".into()).await.is_err());
            let outcome = jobs
                .execute(
                    "terminal-lock",
                    request(),
                    Duration::from_secs(2),
                    || async {
                        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
                        Ok(json!({"native_write_completed":true}))
                    },
                )
                .await;
            assert_eq!(outcome.unwrap_err(), STORE_ERROR);
            lock.execute_batch("ROLLBACK").unwrap();
            assert_eq!(
                jobs.status("terminal-lock".into()).await.unwrap().state,
                "running"
            );
            assert_eq!(
                jobs.execute(
                    "terminal-lock",
                    request(),
                    Duration::from_secs(2),
                    || async { panic!("must not replay after terminal store error") }
                )
                .await
                .unwrap_err(),
                "job_in_progress"
            );
        });
    }

    #[test]
    fn cancellation_drops_future_and_blocking_worker_can_finish_later() {
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        tauri::async_runtime::block_on(async {
            let dir = Temp::new();
            let jobs = Jobs::open(&dir.db()).unwrap();
            let dropped = Arc::new(AtomicUsize::new(0));
            let committed = Arc::new(AtomicUsize::new(0));
            let entered = Arc::new(AtomicUsize::new(0));
            let (release, wait) = std::sync::mpsc::channel();
            let worker = jobs.clone();
            let (d, c, e) = (dropped.clone(), committed.clone(), entered.clone());
            let run = tauri::async_runtime::spawn(async move {
                worker
                    .execute(
                        "blocking",
                        request(),
                        Duration::from_secs(5),
                        || async move {
                            let _dropped = Dropped(d);
                            crate::runtime::run_io_bound("job test atomic worker", move || {
                                e.store(1, Ordering::SeqCst);
                                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                                c.store(1, Ordering::SeqCst);
                                Ok(json!({}))
                            })
                            .await?
                        },
                    )
                    .await
            });
            tokio::time::timeout(Duration::from_secs(3), async {
                while entered.load(Ordering::SeqCst) == 0 {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            jobs.cancel("blocking".into()).await.unwrap();
            assert_eq!(run.await.unwrap().unwrap_err(), "job_cancelled");
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
            assert_eq!(committed.load(Ordering::SeqCst), 0);
            assert!(
                jobs.status("blocking".into())
                    .await
                    .unwrap()
                    .blocking_may_continue
            );
            release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while committed.load(Ordering::SeqCst) == 0 {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while jobs.0.lanes[1].available_permits() != 2 {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            assert!(
                !jobs
                    .status("blocking".into())
                    .await
                    .unwrap()
                    .blocking_may_continue
            );
        });
    }
    #[test]
    fn failed_refresh_releases_lane_for_explicit_retry() {
        tauri::async_runtime::block_on(async {
            let dir = Temp::new();
            let jobs = Jobs::open(&dir.db()).unwrap();
            let make = || Request::new("market_refresh", json!({})).unwrap();
            assert!(jobs
                .execute("fail", make(), Duration::from_secs(2), || async {
                    Err("transient".into())
                })
                .await
                .is_err());
            assert_eq!(
                jobs.execute("retry", make(), Duration::from_secs(2), || async {
                    Ok(json!({"ok":true}))
                })
                .await
                .unwrap(),
                json!({"ok":true})
            );
        });
    }
}
