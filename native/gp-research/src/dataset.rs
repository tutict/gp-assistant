use crate::*;
use chrono::NaiveDate;
use gp_core::{AdaptiveScreenRequest, HistoryBar, StockItem};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct ResearchStore {
    pub(crate) root: PathBuf,
    pub(crate) conn: Connection,
    readonly: bool,
}
pub struct DatasetReader {
    pub(crate) conn: Connection,
    record: DatasetRecord,
    calendar: Vec<NaiveDate>,
    pub(crate) manifest: BundleManifest,
}
type FrameInputs = (
    Vec<StockItem>,
    HashMap<String, Vec<HistoryBar>>,
    HashMap<String, Vec<HistoryBar>>,
);
type CheckedEquity = (Security, Option<DailyStatus>, Option<DailyBar>);
// A checkpointed database can be read as an immutable SQLite snapshot without
// creating WAL/SHM files. Active WAL readers require both existing sidecars.
fn readonly_connection(path: &Path) -> ResearchResult<Connection> {
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let shm = PathBuf::from(format!("{}-shm", path.display()));
    if shm.exists() && !wal.exists() {
        return Err(ResearchError::new(
            "readonly_wal",
            "shared-memory sidecar exists without its WAL; explicit writable recovery is required",
        ));
    }
    if wal.exists() && (shm.exists() || fs::metadata(&wal)?.len() > 0) {
        if !shm.exists() {
            return Err(ResearchError::new("readonly_wal", "active WAL has no existing shared-memory sidecar; open a writable CLI store to recover/checkpoint it"));
        }
        return Ok(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?);
    }
    let canonical = fs::canonicalize(path)?;
    let text = canonical.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let text = if text.starts_with('/') {
        text.to_owned()
    } else {
        format!("/{text}")
    };
    let uri_path = text
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"/:-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect::<String>();
    Ok(Connection::open_with_flags(
        format!("file:{uri_path}?mode=ro&immutable=1"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?)
}
pub(crate) const REGISTRY_SCHEMA:&str="PRAGMA user_version=2; PRAGMA application_id=1196446258; CREATE TABLE IF NOT EXISTS datasets(dataset_id TEXT PRIMARY KEY,record TEXT NOT NULL); CREATE TABLE IF NOT EXISTS reports(run_id TEXT PRIMARY KEY,summary TEXT NOT NULL,report TEXT NOT NULL); CREATE TABLE IF NOT EXISTS parameter_sets(id TEXT PRIMARY KEY,payload TEXT NOT NULL,dataset_id TEXT NOT NULL,core TEXT NOT NULL,research TEXT NOT NULL,fingerprint TEXT NOT NULL); CREATE TABLE IF NOT EXISTS calibration_receipts(run_id TEXT PRIMARY KEY,payload TEXT NOT NULL); CREATE TABLE IF NOT EXISTS test_locks(dataset_id TEXT NOT NULL,test_start TEXT NOT NULL,test_end TEXT NOT NULL,signature TEXT NOT NULL,parameters TEXT NOT NULL,split TEXT NOT NULL,PRIMARY KEY(dataset_id,test_start,test_end)); CREATE TABLE IF NOT EXISTS qualifications(parameter_id TEXT PRIMARY KEY,run_id TEXT NOT NULL); CREATE TABLE IF NOT EXISTS active_parameters(singleton INTEGER PRIMARY KEY CHECK(singleton=1),parameter_id TEXT NOT NULL);";
impl ResearchStore {
    pub fn open(app_data_dir: &Path) -> ResearchResult<Self> {
        let root = app_data_dir.join("stock-research");
        let db = root.join("research.sqlite");
        if db.exists() {
            let conn = match readonly_connection(&db) {
                Ok(conn) => conn,
                // The writable CLI entrypoint is authorized to recover WAL
                // shared-memory evidence; app readers must fail instead.
                Err(error) if error.code == "readonly_wal" => {
                    Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?
                }
                Err(error) => return Err(error),
            };
            crate::registry::check_registry_version(&conn)?;
        }
        fs::create_dir_all(&root)?;
        let conn = Connection::open(&db)?;
        conn.busy_timeout(Duration::from_secs(10))?;
        crate::registry::check_registry_version(&conn)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;BEGIN IMMEDIATE;")?;
        if let Err(e) = conn.execute_batch(REGISTRY_SCHEMA) {
            let _ = conn.execute_batch("ROLLBACK;");
            return Err(e.into());
        }
        conn.execute_batch("COMMIT;")?;
        Ok(Self {
            root,
            conn,
            readonly: false,
        })
    }
    pub fn open_readonly(app_data_dir: &Path) -> ResearchResult<Self> {
        let root = app_data_dir.join("stock-research");
        let db = root.join("research.sqlite");
        let conn = readonly_connection(&db)?;
        crate::registry::check_registry_version(&conn)?;
        conn.execute_batch("PRAGMA query_only=ON;")?;
        Ok(Self {
            root,
            conn,
            readonly: true,
        })
    }
    pub(crate) fn require_write(&self) -> ResearchResult<()> {
        if self.readonly {
            Err(ResearchError::new("readonly", "store was opened read-only"))
        } else {
            crate::registry::check_registry_version(&self.conn)
        }
    }
    /// Fresh read snapshots observe publication after this store was opened.
    /// Future registry/status readers must use this rather than caching conn.
    pub(crate) fn registry_reader(&self) -> ResearchResult<Connection> {
        let conn = readonly_connection(&self.root.join("research.sqlite"))?;
        crate::registry::check_registry_version(&conn)?;
        Ok(conn)
    }
    pub(crate) fn lookup_dataset(&self, id: &str) -> ResearchResult<Option<DatasetRecord>> {
        let value = self
            .registry_reader()?
            .query_row(
                "SELECT record FROM datasets WHERE dataset_id=?",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub(crate) fn register_dataset(&self, r: &DatasetRecord) -> ResearchResult<()> {
        self.require_write()?;
        self.conn.execute(
            "INSERT OR IGNORE INTO datasets VALUES(?,?)",
            params![r.dataset_id, serde_json::to_string(r)?],
        )?;
        Ok(())
    }
    pub fn import_bundle(&self, bundle_dir: &Path) -> ResearchResult<DatasetRecord> {
        crate::import::import(self, bundle_dir)
    }
    pub fn dataset(&self, dataset_id: &str) -> ResearchResult<DatasetReader> {
        if dataset_id.len() != 64
            || !dataset_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ResearchError::new("dataset_id", "invalid fingerprint"));
        }
        let record = self
            .lookup_dataset(dataset_id)?
            .ok_or_else(|| ResearchError::new("not_found", "dataset not found"))?;
        let conn = readonly_connection(
            &self
                .root
                .join("datasets")
                .join(dataset_id)
                .join("dataset.sqlite"),
        )?;
        crate::import::check_version(&conn)?;
        conn.execute_batch("PRAGMA query_only=ON;")?;
        let saved: String =
            conn.query_row("SELECT value FROM metadata WHERE key='record'", [], |r| {
                r.get(0)
            })?;
        let saved: DatasetRecord = serde_json::from_str(&saved)?;
        if saved.data_fingerprint != record.data_fingerprint {
            return Err(ResearchError::new(
                "immutable_conflict",
                "registry and dataset fingerprint differ",
            ));
        }
        let manifest: String =
            conn.query_row("SELECT value FROM metadata WHERE key='manifest'", [], |r| {
                r.get(0)
            })?;
        let calendar = {
            let mut stmt = conn.prepare("SELECT date FROM calendar ORDER BY date")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.map(|r| {
                let s = r?;
                NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .map_err(|e| ResearchError::new("date", e.to_string()))
            })
            .collect::<ResearchResult<Vec<_>>>()?
        };
        Ok(DatasetReader {
            conn,
            record,
            calendar,
            manifest: serde_json::from_str(&manifest)?,
        })
    }
    pub fn datasets(&self) -> ResearchResult<Vec<DatasetRecord>> {
        query_json(
            &self.registry_reader()?,
            "SELECT record FROM datasets ORDER BY dataset_id",
            &[],
        )
    }
    pub fn save_report(&self, kind: &str, report: &serde_json::Value) -> ResearchResult<String> {
        self.require_write()?;
        if kind.trim().is_empty() {
            return Err(ResearchError::new("report", "report kind required"));
        }
        let id = report
            .get("run_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                crate::import::digest(serde_json::to_string(report).unwrap().as_bytes())
            });
        let summary = RunSummary {
            run_id: id.clone(),
            dataset_id: report
                .get("dataset_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            kind: kind.into(),
            status: report
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("completed")
                .into(),
            parameter_set_id: report
                .pointer("/parameters/parameter_set_id")
                .or_else(|| report.get("parameter_set_id"))
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        };
        let payload = serde_json::to_string(report)?;
        if let Some((old, old_summary)) = self
            .conn
            .query_row(
                "SELECT report,summary FROM reports WHERE run_id=?",
                [&id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != payload || old_summary != serde_json::to_string(&summary)? {
                return Err(ResearchError::new(
                    "immutable_conflict",
                    "report run id already has different content/kind",
                ));
            }
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO reports VALUES(?,?,?)",
            params![id, serde_json::to_string(&summary)?, payload],
        )?;
        Ok(id)
    }
    pub fn reports(&self) -> ResearchResult<Vec<RunSummary>> {
        query_json(
            &self.registry_reader()?,
            "SELECT summary FROM reports ORDER BY run_id",
            &[],
        )
    }
    pub fn report(&self, run_id: &str) -> ResearchResult<serde_json::Value> {
        one_json(
            &self.registry_reader()?,
            "SELECT report FROM reports WHERE run_id=?",
            &[&run_id],
        )?
        .ok_or_else(|| ResearchError::new("not_found", "report not found"))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub(crate) fn query_json<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> ResearchResult<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next()? {
        let payload: String = row.get(0)?;
        values.push(serde_json::from_str(&payload)?);
    }
    Ok(values)
}
fn one_json<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> ResearchResult<Option<T>> {
    conn.query_row(sql, params, |r| r.get::<_, String>(0))
        .optional()?
        .map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
impl DatasetReader {
    pub fn record(&self) -> &DatasetRecord {
        &self.record
    }
    pub fn calendar(&self) -> &[NaiveDate] {
        &self.calendar
    }
    pub(crate) fn bar(&self, code: &str, date: NaiveDate) -> ResearchResult<Option<DailyBar>> {
        one_json(
            &self.conn,
            "SELECT payload FROM bars WHERE code=? AND date=?",
            &[&code, &date.to_string()],
        )
    }
    pub(crate) fn status(
        &self,
        code: &str,
        date: NaiveDate,
    ) -> ResearchResult<Option<DailyStatus>> {
        one_json(
            &self.conn,
            "SELECT payload FROM statuses WHERE code=? AND date=?",
            &[&code, &date.to_string()],
        )
    }
    pub(crate) fn financial(
        &self,
        code: &str,
        date: NaiveDate,
    ) -> ResearchResult<Option<FinancialEvent>> {
        one_json(&self.conn,"SELECT payload FROM financial WHERE code=? AND visible_date<=? ORDER BY report_date DESC,visible_date DESC,revision DESC LIMIT 1",&[&code,&date.to_string()])
    }
    pub(crate) fn securities(&self) -> ResearchResult<Vec<Security>> {
        query_json(
            &self.conn,
            "SELECT payload FROM securities ORDER BY code",
            &[],
        )
    }
    pub(crate) fn listed_on(&self, date: NaiveDate) -> ResearchResult<Vec<Security>> {
        query_json(&self.conn,"SELECT payload FROM securities WHERE security_type='equity' AND listed_date<=? AND (delisted_date IS NULL OR delisted_date>?) ORDER BY code",&[&date.to_string(),&date.to_string()])
    }
    pub(crate) fn actions_on(&self, date: NaiveDate) -> ResearchResult<Vec<CorporateAction>> {
        query_json(
            &self.conn,
            "SELECT payload FROM actions WHERE ex_date=? ORDER BY code,event_id",
            &[&date.to_string()],
        )
    }
    pub(crate) fn actions_between(
        &self,
        code: &str,
        from: NaiveDate,
        to: NaiveDate,
        as_of: Option<NaiveDate>,
    ) -> ResearchResult<Vec<CorporateAction>> {
        if let Some(as_of) = as_of {
            query_json(&self.conn,"SELECT payload FROM actions WHERE code=? AND ex_date>? AND ex_date<=? AND visible_date<=? ORDER BY ex_date,event_id",&[&code,&from.to_string(),&to.to_string(),&as_of.to_string()])
        } else {
            query_json(&self.conn,"SELECT payload FROM actions WHERE code=? AND ex_date>? AND ex_date<=? ORDER BY ex_date,event_id",&[&code,&from.to_string(),&to.to_string()])
        }
    }
    pub(crate) fn request(&self, date: NaiveDate, c: &EvaluationConfig) -> AdaptiveScreenRequest {
        AdaptiveScreenRequest {
            mode: c.mode.clone(),
            primary_limit: c.top_n,
            exploration_limit: 10,
            as_of_date: Some(date.to_string()),
            criteria: c.criteria.clone(),
            ..Default::default()
        }
    }
    fn stock(
        &self,
        security: &Security,
        status: &DailyStatus,
        bar: &DailyBar,
    ) -> ResearchResult<StockItem> {
        let event = self.financial(&security.code, bar.date)?;
        let previous: Option<DailyBar> = one_json(
            &self.conn,
            "SELECT payload FROM bars WHERE code=? AND date<? ORDER BY date DESC LIMIT 1",
            &[&security.code, &bar.date.to_string()],
        )?;
        let previous_close = if let Some(previous) = previous {
            let actions =
                self.actions_between(&security.code, previous.date, bar.date, Some(bar.date))?;
            let (divisor, subtract) = adjustment(&actions, previous.date)?;
            let close = (previous.close - subtract) / divisor;
            (close > 0.0).then_some(close)
        } else {
            None
        };
        Ok(StockItem {
            code: security.code.clone(),
            name: status.name.clone(),
            industry: status.industry.clone(),
            is_st: status.is_st,
            price: bar.close,
            pe: event
                .as_ref()
                .and_then(|e| e.eps_ttm)
                .filter(|v| *v > 0.0)
                .map(|v| bar.close / v),
            pb: event
                .as_ref()
                .and_then(|e| e.bps)
                .filter(|v| *v > 0.0)
                .map(|v| bar.close / v),
            roe: event.as_ref().and_then(|e| e.roe),
            market_cap_billion: bar
                .total_shares
                .filter(|v| *v > 0.0)
                .map(|v| bar.close * v / 1e8),
            dividend_yield: event.as_ref().and_then(|e| e.dividend_yield),
            latest_eps: event.as_ref().and_then(|e| e.eps_ttm),
            deducted_net_profit_billion: event.as_ref().and_then(|e| e.deducted_net_profit_billion),
            deducted_net_profit_growth_rate: event
                .as_ref()
                .and_then(|e| e.deducted_net_profit_growth_rate),
            deducted_net_profit_margin: event.as_ref().and_then(|e| e.deducted_net_profit_margin),
            change_pct: previous_close.map(|p| bar.close / p - 1.0),
            volume: bar.volume,
            amount: bar.amount,
            turnover_rate: bar.turnover_rate,
            volume_ratio: bar.volume_ratio,
            quote_time: Some(format!("{} 15:00:00", bar.date)),
        })
    }
    pub(crate) fn history(&self, code: &str, date: NaiveDate) -> ResearchResult<Vec<HistoryBar>> {
        let end = self.calendar.partition_point(|session| *session <= date);
        let Some(first_session) = self.calendar.get(end.saturating_sub(120)) else {
            return Ok(vec![]);
        };
        let mut bars: Vec<DailyBar> = query_json(
            &self.conn,
            "SELECT payload FROM bars WHERE code=? AND date>=? AND date<=? ORDER BY date DESC LIMIT 120",
            &[&code, &first_session.to_string(), &date.to_string()],
        )?;
        bars.reverse();
        let Some(first) = bars.first() else {
            return Ok(vec![]);
        };
        let actions = self.actions_between(code, first.date, date, Some(date))?;
        let mut output = Vec::with_capacity(bars.len());
        for bar in bars {
            // Transform old prices into signal-date share units. Only then-known/effective
            // cash/split events participate; future disclosures never alter past factors.
            let (divisor, subtract) = adjustment(&actions, bar.date)?;
            let adjusted = |v: f64| (v - subtract) / divisor;
            if adjusted(bar.low) <= 0.0 {
                return Err(ResearchError::new(
                    "action",
                    "action adjustment produced nonpositive historical price",
                ));
            }
            output.push(HistoryBar {
                date: bar.date.to_string(),
                open: Some(adjusted(bar.open)),
                high: Some(adjusted(bar.high)),
                low: Some(adjusted(bar.low)),
                close: adjusted(bar.close),
                volume: bar.volume.map(|v| v * divisor),
                capital: bar.amount,
            });
        }
        Ok(output)
    }
    pub(crate) fn frame(
        &self,
        date: NaiveDate,
        c: &EvaluationConfig,
    ) -> ResearchResult<FrameInputs> {
        self.request_frame(date, &self.request(date, c))
    }
    fn request_frame(
        &self,
        date: NaiveDate,
        request: &AdaptiveScreenRequest,
    ) -> ResearchResult<FrameInputs> {
        if self.calendar.binary_search(&date).is_err() {
            return Err(ResearchError::new(
                "signal_date",
                "signal date is not a calendar session",
            ));
        }
        let mut universe = Vec::new();
        let mut histories = HashMap::new();
        for security in self.listed_on(date)? {
            let status = self.status(&security.code, date)?.ok_or_else(|| {
                ResearchError::new(
                    "data_quality",
                    format!("missing status {} {date}", security.code),
                )
            })?;
            if !status.is_listed {
                return Err(ResearchError::new(
                    "data_quality",
                    "listing/status mismatch",
                ));
            }
            if let Some(bar) = self.bar(&security.code, date)? {
                universe.push(self.stock(&security, &status, &bar)?);
            } else if status.can_buy || status.can_sell {
                return Err(ResearchError::new(
                    "data_quality",
                    format!("missing tradable bar {} {date}", security.code),
                ));
            }
        }
        // The public core stage-one function applies criteria before histories
        // are requested. Its authoritative pool limit is 80 in this revision.
        for code in gp_core::adaptive_candidate_codes(&universe, &request.criteria, 80) {
            histories.insert(code.clone(), self.history(&code, date)?);
        }
        let mut indices = HashMap::new();
        for code in ["000001.SH", "399001.SZ", "399006.SZ"] {
            let bars = if self.bar(code, date)?.is_some() {
                self.history(code, date)?
            } else {
                vec![]
            };
            if !bars.is_empty() {
                indices.insert(code.into(), bars);
            }
        }
        Ok((universe, histories, indices))
    }
    pub fn features_on(
        &self,
        signal_date: NaiveDate,
        config: &EvaluationConfig,
    ) -> ResearchResult<gp_core::AdaptiveFeatureSet> {
        validate_config(config)?;
        let (universe, histories, benchmarks) = self.frame(signal_date, config)?;
        Ok(gp_core::adaptive_scoring_features(
            &universe,
            &histories,
            &benchmarks,
            &self.request(signal_date, config),
        )?)
    }
    pub fn screen_on(
        &self,
        signal_date: NaiveDate,
        request: &gp_core::AdaptiveScreenRequest,
        parameters: &gp_core::ScoringParameters,
    ) -> ResearchResult<gp_core::AdaptiveScreenResult> {
        gp_core::validate_scoring_parameters(parameters)?;
        if request.as_of_date.as_ref().is_some_and(|date| {
            date != &signal_date.to_string() && date != &signal_date.format("%Y%m%d").to_string()
        }) {
            return Err(ResearchError::new(
                "signal_date",
                "request as-of date must match the explicit PIT signal session",
            ));
        }
        let config = EvaluationConfig {
            start_date: signal_date,
            end_date: signal_date,
            mode: request.mode.clone(),
            top_n: request.primary_limit.clamp(1, 50),
            ..Default::default()
        };
        validate_config(&config)?;
        if !self.manifest.coverage.corporate_actions_complete
            || (request.mode == "auto" && !self.record.full_universe)
        {
            return Err(ResearchError::new(
                "data_quality",
                "complete action source coverage and auto full_universe evidence are required",
            ));
        }
        let signal_index = self.calendar.binary_search(&signal_date).map_err(|_| {
            ResearchError::new("signal_date", "signal date is not a calendar session")
        })?;
        let mut missing = Vec::new();
        let indices = self.required_indices(&request.mode, &mut missing)?;
        for date in &self.calendar[signal_index.saturating_sub(119)..=signal_index] {
            self.checked_equities_on(*date, &mut missing)?;
            self.check_index_session(*date, &indices, &mut missing)?;
        }
        if !missing.is_empty() {
            return Err(ResearchError {
                code: "data_quality".into(),
                message: "strict as-of historical session coverage is incomplete".into(),
                details: missing,
            });
        }
        let action_details = self.action_missing_details(
            signal_date,
            self.calendar[signal_index.saturating_sub(119)],
            true,
        )?;
        if !action_details.is_empty() {
            return Err(ResearchError {
                code: "data_quality".into(),
                message: "strict as-of corporate-action evidence is incomplete".into(),
                details: action_details,
            });
        }
        let (universe, histories, indices) = self.request_frame(signal_date, request)?;
        let mut requested = 0;
        let mut usable = 0;
        for security in self.listed_on(signal_date)? {
            let status = self
                .status(&security.code, signal_date)?
                .ok_or_else(|| ResearchError::new("data_quality", "missing exact signal status"))?;
            if !status.is_st && (status.can_buy || status.can_sell) {
                requested += 1;
                if universe
                    .iter()
                    .find(|s| s.code == security.code)
                    .is_some_and(slow_available)
                {
                    usable += 1;
                }
            }
        }
        if requested == 0 || usable as f64 / (requested as f64) + f64::EPSILON < 0.95 {
            return Err(ResearchError::new(
                "data_quality",
                format!("signal slow-factor coverage {usable}/{requested} is below 95%"),
            ));
        }
        let mut request = request.clone();
        request.as_of_date = Some(signal_date.to_string());
        Ok(gp_core::adaptive_screen_stocks_with_parameters(
            &universe,
            &histories,
            &indices,
            &[],
            &request,
            parameters,
        )?)
    }
    // Shared dataset-wide evidence boundary. Screening may only inspect events
    // known and effective by the signal; evaluation also checks realized outcomes.
    fn action_missing_details(
        &self,
        end_date: NaiveDate,
        from_date: NaiveDate,
        as_of_only: bool,
    ) -> ResearchResult<Vec<String>> {
        let actions: Vec<CorporateAction> = if as_of_only {
            query_json(&self.conn, "SELECT payload FROM actions WHERE ex_date<=? AND visible_date<=? ORDER BY ex_date,code,event_id", &[&end_date.to_string(), &end_date.to_string()])?
        } else {
            query_json(
                &self.conn,
                "SELECT payload FROM actions WHERE ex_date<=? ORDER BY ex_date,code,event_id",
                &[&end_date.to_string()],
            )?
        };
        let mut details = Vec::new();
        for action in &actions {
            if action.kind == "unsupported"
                || ((action.kind == "cash_dividend" || action.kind == "delisting_settlement")
                    && action.cash_per_share.is_none())
                || (action.kind == "share_split" && action.share_multiplier.is_none())
            {
                details.push(format!(
                    "unsupported/incomplete action {} {}",
                    action.code, action.event_id
                ));
            }
            if self.calendar.binary_search(&action.ex_date).is_err() {
                details.push(format!(
                    "action ex date not a calendar session {} {}",
                    action.code, action.ex_date
                ));
            }
        }
        for security in self
            .securities()?
            .into_iter()
            .filter(|s| s.security_type == "equity")
        {
            if let Some(date) = security
                .delisted_date
                .filter(|d| *d <= end_date && *d >= from_date)
            {
                if !actions.iter().any(|a| {
                    a.code == security.code
                        && a.kind == "delisting_settlement"
                        && a.ex_date == date
                        && a.cash_per_share.is_some()
                }) {
                    details.push(format!(
                        "missing delisting settlement evidence {} {date}",
                        security.code
                    ));
                }
            }
        }
        Ok(details)
    }

    fn checked_equities_on(
        &self,
        date: NaiveDate,
        missing: &mut Vec<String>,
    ) -> ResearchResult<Vec<CheckedEquity>> {
        self.listed_on(date)?
            .into_iter()
            .map(|security| {
                let status = self.status(&security.code, date)?;
                let bar = self.bar(&security.code, date)?;
                match &status {
                    None => missing.push(format!("missing status {} {date}", security.code)),
                    Some(status) => {
                        if !status.is_listed {
                            missing
                                .push(format!("listing/status mismatch {} {date}", security.code));
                        }
                        if (status.can_buy || status.can_sell) && bar.is_none() {
                            missing.push(format!("missing tradable bar {} {date}", security.code));
                        }
                    }
                }
                Ok((security, status, bar))
            })
            .collect()
    }

    fn required_indices(
        &self,
        mode: &str,
        missing: &mut Vec<String>,
    ) -> ResearchResult<Vec<String>> {
        if mode != "auto" {
            return Ok(vec![]);
        }
        let indices = self
            .securities()?
            .into_iter()
            .filter(|s| {
                s.security_type == "index"
                    && ["000001.SH", "399001.SZ", "399006.SZ"].contains(&s.code.as_str())
            })
            .map(|s| s.code)
            .collect::<Vec<_>>();
        if indices.len() < 2 {
            missing.push("auto requires at least two declared mainland benchmark indices".into());
        }
        Ok(indices)
    }

    fn check_index_session(
        &self,
        date: NaiveDate,
        indices: &[String],
        missing: &mut Vec<String>,
    ) -> ResearchResult<()> {
        for code in indices {
            if self.bar(code, date)?.is_none() {
                missing.push(format!("missing benchmark bar {code} {date}"));
            }
        }
        Ok(())
    }
    pub fn validate(&self, c: &EvaluationConfig) -> ResearchResult<QualityReport> {
        validate_config(c)?;
        // Global structure/action/history quality must never be hidden by a
        // narrower research filter. Selection later uses the actual criteria.
        let global = EvaluationConfig {
            criteria: gp_core::ScreenCriteria::default(),
            ..c.clone()
        };
        let c = &global;
        let mut q = QualityReport {
            real_data: self.record.real_data,
            full_universe: self.record.full_universe,
            slow_factor_coverage: 1.0,
            ..Default::default()
        };
        if c.mode == "auto" && !self.record.full_universe {
            q.issues.push(
                "auto requires declared full_universe coverage in addition to structural checks"
                    .into(),
            );
        }
        if c.start_date < self.record.start_date || !self.covers_end_date(c.end_date) {
            q.issues
                .push("requested evaluation outside source/calendar coverage".into());
        }
        let first = self.calendar.partition_point(|d| *d < c.start_date);
        let end = self.calendar.partition_point(|d| *d <= c.end_date);
        if first == 0 || first >= end {
            q.issues
                .push("evaluation needs a prior signal session and nonempty session range".into());
            q.passed = false;
            return Ok(q);
        }
        let signal = first - 1;
        let warmup = signal.saturating_sub(119);
        let all = self.securities()?;
        let indices = self.required_indices(&c.mode, &mut q.missing_details)?;
        if all.iter().filter(|s| s.security_type == "equity").count() == 0 {
            q.issues.push("no equity identities".into());
        }
        if !self.manifest.coverage.corporate_actions_complete {
            q.issues
                .push("corporate action source coverage is not declared complete".into());
        }
        q.missing_details.extend(self.action_missing_details(
            c.end_date,
            self.calendar[warmup],
            false,
        )?);
        let mut wanted = 0;
        let mut usable = 0;
        for index in warmup..end {
            let date = self.calendar[index];
            if index >= signal {
                q.checked_sessions += 1;
            }
            let listed = self.checked_equities_on(date, &mut q.missing_details)?;
            let mut day_wanted = 0;
            let mut day_usable = 0;
            for (security, status, bar) in listed {
                let Some(status) = status else {
                    continue;
                };
                if index >= signal && !status.is_st && (status.can_buy || status.can_sell) {
                    day_wanted += 1;
                    let stock = match bar
                        .as_ref()
                        .map(|b| self.stock(&security, &status, b))
                        .transpose()
                    {
                        Ok(stock) => stock,
                        Err(error) => {
                            q.missing_details.push(format!(
                                "invalid historical feature input {} {date}: {error}",
                                security.code
                            ));
                            None
                        }
                    };
                    if stock.as_ref().is_some_and(slow_available) {
                        day_usable += 1;
                    } else {
                        q.missing_details.push(format!("slow factors unavailable {} {date} (first disclosure/valuation/ROE/cap)",security.code));
                    }
                }
            }
            if index >= signal {
                wanted += day_wanted;
                usable += day_usable;
                let coverage = if day_wanted == 0 {
                    0.0
                } else {
                    day_usable as f64 / day_wanted as f64
                };
                if coverage + f64::EPSILON < 0.95 {
                    q.issues.push(format!(
                        "slow-factor coverage {coverage:.4} below 0.95 on {date}"
                    ));
                }
            }
            self.check_index_session(date, &indices, &mut q.missing_details)?;
            if index >= signal {
                if let Err(e) = self.features_on(date, c) {
                    q.missing_details.push(format!("feature frame {date}: {e}"));
                }
            }
        }
        q.slow_factor_coverage = if wanted == 0 {
            0.0
        } else {
            usable as f64 / wanted as f64
        };
        if q.missing_details
            .iter()
            .any(|detail| !detail.starts_with("slow factors unavailable "))
        {
            q.issues
                .push("structural PIT coverage or feature/action evidence is incomplete".into());
        }
        q.issues.sort();
        q.issues.dedup();
        q.missing_details.sort();
        q.missing_details.dedup();
        q.passed = q.issues.is_empty();
        Ok(q)
    }
    pub(crate) fn covers_end_date(&self, end: NaiveDate) -> bool {
        if end <= self.record.end_date {
            return true;
        }
        // The manifest ends at its last actual session. Only a trailing
        // Saturday/Sunday can be inferred closed without external calendars.
        let mut date = self.record.end_date.succ_opt().unwrap();
        while date <= end {
            if !matches!(
                chrono::Datelike::weekday(&date),
                chrono::Weekday::Sat | chrono::Weekday::Sun
            ) {
                return false;
            }
            date = date.succ_opt().unwrap();
        }
        true
    }
    pub fn forward_return(
        &self,
        code: &str,
        signal_date: NaiveDate,
        horizon: usize,
    ) -> ResearchResult<Option<f64>> {
        if ![10, 20, 30].contains(&horizon) {
            return Err(ResearchError::new(
                "horizon",
                "forward horizon must be 10/20/30 trading sessions",
            ));
        }
        let Ok(signal) = self.calendar.binary_search(&signal_date) else {
            return Err(ResearchError::new(
                "signal_date",
                "signal date is not a calendar session",
            ));
        };
        let Some(entry_date) = self.calendar.get(signal + 1).copied() else {
            return Ok(None);
        };
        let Some(exit_date) = self.calendar.get(signal + 1 + horizon).copied() else {
            return Ok(None);
        };
        let Some(entry) = self.bar(code, entry_date)? else {
            return Ok(None);
        };
        let Some(status) = self.status(code, entry_date)? else {
            return Ok(None);
        };
        if !status.can_buy || !status.is_listed {
            return Ok(None);
        }
        let actions = self.actions_between(code, entry_date, exit_date, None)?;
        let mut shares = 1.0;
        let mut cash = 0.0;
        let mut settled = false;
        let mut action_date = None;
        let mut previous_shares = shares;
        for a in actions {
            if action_date != Some(a.ex_date) {
                action_date = Some(a.ex_date);
                previous_shares = shares;
            }
            match a.kind.as_str() {
                "cash_dividend" => {
                    cash += previous_shares
                        * a.cash_per_share.ok_or_else(|| {
                            ResearchError::new("action", "missing dividend entitlement")
                        })?
                }
                "share_split" => {
                    shares *= a
                        .share_multiplier
                        .ok_or_else(|| ResearchError::new("action", "missing split multiplier"))?
                }
                "delisting_settlement" => {
                    cash += previous_shares
                        * a.cash_per_share
                            .ok_or_else(|| ResearchError::new("action", "missing settlement"))?;
                    shares = 0.0;
                    settled = true;
                }
                _ => {
                    return Err(ResearchError::new(
                        "action",
                        "unsupported realized corporate action",
                    ))
                }
            }
        }
        let value = if settled {
            cash
        } else {
            let Some(exit) = self.bar(code, exit_date)? else {
                return Ok(None);
            };
            cash + shares * exit.close
        };
        Ok(Some(value / entry.close - 1.0))
    }
}
// Cash amounts on a shared ex-date are per shares held before that day's
// transformations. Grouping makes adjustment independent of event-id order.
fn adjustment(actions: &[CorporateAction], after: NaiveDate) -> ResearchResult<(f64, f64)> {
    let mut divisor = 1.0;
    let mut subtract = 0.0;
    let mut index = actions.partition_point(|a| a.ex_date <= after);
    while index < actions.len() {
        let date = actions[index].ex_date;
        let mut cash = 0.0;
        let mut multiplier = 1.0;
        while index < actions.len() && actions[index].ex_date == date {
            let action = &actions[index];
            match action.kind.as_str() {
                "cash_dividend" => {
                    cash += action.cash_per_share.ok_or_else(|| {
                        ResearchError::new("action", "missing dividend adjustment")
                    })?
                }
                "share_split" => {
                    multiplier *= action
                        .share_multiplier
                        .ok_or_else(|| ResearchError::new("action", "missing split adjustment"))?
                }
                "delisting_settlement" => {}
                _ => return Err(ResearchError::new("action", "unsupported adjustment")),
            }
            index += 1;
        }
        subtract += cash * divisor;
        divisor *= multiplier;
    }
    if !divisor.is_finite() || !subtract.is_finite() {
        return Err(ResearchError::new("action", "nonfinite action adjustment"));
    }
    Ok((divisor, subtract))
}
fn slow_available(s: &StockItem) -> bool {
    s.roe.is_some()
        && (s.pe.is_some_and(|v| v > 0.0) || s.pb.is_some_and(|v| v > 0.0))
        && s.market_cap_billion.is_some_and(|v| v > 0.0)
}
pub(crate) fn validate_config(c: &EvaluationConfig) -> ResearchResult<()> {
    if c.start_date > c.end_date
        || !c.initial_cash.is_finite()
        || c.initial_cash <= 0.0
        || !c.transaction_cost_bps.is_finite()
        || c.transaction_cost_bps < 0.0
        || c.transaction_cost_bps >= 10000.0
        || !(1..=50).contains(&c.top_n)
        || !matches!(
            c.mode.as_str(),
            "auto" | "range" | "trend" | "defensive" | "transition"
        )
        || !matches!(c.strategy.as_str(), "adaptive" | "legacy_balanced")
        || c.rebalance_frequency != "monthly"
    {
        Err(ResearchError::new("config","invalid evaluation dates/cash/cost/top_n/mode/strategy or unsupported rebalance frequency"))
    } else {
        Ok(())
    }
}
