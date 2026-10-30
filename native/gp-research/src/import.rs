use crate::*;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::{Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn file_hash(path: &Path) -> ResearchResult<String> {
    let mut input = BufReader::new(File::open(path)?);
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hex(hash.finalize().as_slice()))
}
fn check_headers(path: &Path, role: &str) -> ResearchResult<()> {
    let expected = match role {
        "securities" => "code,name,security_type,listed_date,delisted_date",
        "daily_bars" => "code,date,open,high,low,close,volume,amount,total_shares,float_shares,turnover_rate,volume_ratio",
        "financial_events" => "code,report_date,available_at,revision,source,eps_ttm,bps,roe,deducted_net_profit_billion,deducted_net_profit_growth_rate,deducted_net_profit_margin,dividend_yield",
        "daily_status" => "code,date,name,industry,is_listed,is_st,can_buy,can_sell",
        "corporate_actions" => "code,event_id,kind,known_at,ex_date,pay_date,cash_per_share,share_multiplier",
        "trading_calendar" => "date",
        _ => return Err(ResearchError::new("manifest", "unknown file role")),
    };
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(path)
        .map_err(input_csv_error)?;
    let headers = reader.headers().map_err(input_csv_error)?;
    let unique = headers.iter().collect::<std::collections::HashSet<_>>();
    if unique.len() != headers.len() || expected.split(',').any(|field| !unique.contains(field)) {
        return Err(ResearchError::new(
            "csv_headers",
            format!("missing/duplicate CSV headers for {role}; required: {expected}"),
        ));
    }
    Ok(())
}
fn safe_file(bundle: &Path, path: &str) -> ResearchResult<PathBuf> {
    let relative = Path::new(path);
    if path.trim().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(ResearchError::new(
            "unsafe_path",
            format!("bundle path is not a plain relative path: {path}"),
        ));
    }
    let canonical = fs::canonicalize(bundle.join(relative))?;
    if !canonical.starts_with(bundle) || !canonical.is_file() {
        return Err(ResearchError::new(
            "unsafe_path",
            format!("file escapes bundle: {path}"),
        ));
    }
    Ok(canonical)
}
pub(crate) fn check_version(conn: &Connection) -> ResearchResult<()> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 1 {
        Err(ResearchError::new(
            "newer_schema",
            format!("schema {version} is newer than supported 1; preserved"),
        ))
    } else {
        Ok(())
    }
}
pub(crate) const DATA_SCHEMA:&str="
 PRAGMA user_version=1;
 CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
 CREATE TABLE calendar(date TEXT PRIMARY KEY);
 CREATE TABLE securities(code TEXT PRIMARY KEY,security_type TEXT NOT NULL,listed_date TEXT NOT NULL,delisted_date TEXT,payload TEXT NOT NULL);
 CREATE TABLE bars(code TEXT NOT NULL,date TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(code,date),FOREIGN KEY(code) REFERENCES securities(code),FOREIGN KEY(date) REFERENCES calendar(date));
 CREATE INDEX bars_date ON bars(date,code);
 CREATE TABLE statuses(code TEXT NOT NULL,date TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(code,date),FOREIGN KEY(code) REFERENCES securities(code),FOREIGN KEY(date) REFERENCES calendar(date));
 CREATE INDEX statuses_date ON statuses(date,code);
 CREATE TABLE financial(code TEXT NOT NULL,report_date TEXT NOT NULL,revision INTEGER NOT NULL,visible_date TEXT,payload TEXT NOT NULL,PRIMARY KEY(code,report_date,revision),FOREIGN KEY(code) REFERENCES securities(code));
 CREATE INDEX financial_visible ON financial(code,report_date DESC,visible_date DESC,revision DESC);
 CREATE TABLE actions(code TEXT NOT NULL,event_id TEXT NOT NULL,ex_date TEXT NOT NULL,pay_date TEXT NOT NULL,visible_date TEXT,payload TEXT NOT NULL,PRIMARY KEY(code,event_id),FOREIGN KEY(code) REFERENCES securities(code));
 CREATE INDEX actions_ex ON actions(ex_date,code,event_id);
 CREATE INDEX actions_code ON actions(code,ex_date,event_id);
";

/// Date-only observations enter on the strictly next session. RFC3339 timestamps
/// enter at Shanghai close, or on the first following session when later/non-session.
pub(crate) fn visible_date(
    observation: &str,
    calendar: &[NaiveDate],
) -> ResearchResult<Option<NaiveDate>> {
    let (date, after) = if observation.len() == 10 {
        (parse_date(observation)?, true)
    } else {
        let timestamp = DateTime::parse_from_rfc3339(observation)
            .map_err(|e| ResearchError::new("timestamp", e.to_string()))?;
        let local = timestamp.with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap());
        (
            local.date_naive(),
            local.time() > NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
        )
    };
    let index = calendar.partition_point(|d| *d < date || (*d == date && after));
    Ok(calendar.get(index).copied())
}
fn parse_date(s: &str) -> ResearchResult<NaiveDate> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| ResearchError::new("date", e.to_string()))?;
    if d.to_string() != s {
        return Err(ResearchError::new("date", "canonical YYYY-MM-DD required"));
    }
    Ok(d)
}
fn identity(s: &str) -> ResearchResult<()> {
    let (number, suffix) = s
        .split_once('.')
        .ok_or_else(|| ResearchError::new("identity", format!("invalid mainland code {s}")))?;
    if number.len() != 6
        || !number.bytes().all(|b| b.is_ascii_digit())
        || !matches!(suffix, "SH" | "SZ" | "BJ")
    {
        return Err(ResearchError::new(
            "identity",
            format!("invalid mainland code {s}"),
        ));
    }
    Ok(())
}
fn nonempty(s: &str, field: &str) -> ResearchResult<()> {
    if s.trim().is_empty() {
        Err(ResearchError::new(
            "required_field",
            format!("{field} is required"),
        ))
    } else {
        Ok(())
    }
}
fn finite(values: &[Option<f64>]) -> ResearchResult<()> {
    if values.iter().flatten().any(|v| !v.is_finite()) {
        Err(ResearchError::new(
            "input_numeric",
            "nonfinite numeric observation",
        ))
    } else {
        Ok(())
    }
}
fn rate(
    value: Option<f64>,
    field: &str,
    m: &BundleManifest,
    percentage: bool,
) -> ResearchResult<Option<f64>> {
    let Some(v) = value else { return Ok(None) };
    let ratio = match m.rate_units.get(field).map(String::as_str) {
        Some("ratio") => v,
        Some("percentage_points") => v / 100.0,
        _ => {
            return Err(ResearchError::new(
                "rate_unit",
                format!("explicit ratio/percentage_points unit required for {field}"),
            ))
        }
    };
    Ok(Some(if percentage { ratio * 100.0 } else { ratio }))
}
// CSV is consumed row-by-row. Rehash the same streamed parse to reject file changes
// between verification and import, without buffering full histories.
struct VerifiedRead {
    input: BufReader<File>,
    hash: Sha256,
}
fn input_csv_error(error: csv::Error) -> ResearchError {
    ResearchError::new(
        if error.is_io_error() {
            "io"
        } else {
            "input_csv"
        },
        error.to_string(),
    )
}
fn input_sqlite_error(error: rusqlite::Error) -> ResearchError {
    if matches!(&error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation)
    {
        ResearchError::new("input_record", error.to_string())
    } else {
        error.into()
    }
}
impl Read for VerifiedRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.input.read(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
}
fn rows<T: DeserializeOwned>(
    path: &Path,
    expected: &str,
    mut consume: impl FnMut(T) -> ResearchResult<()>,
) -> ResearchResult<()> {
    let source = VerifiedRead {
        input: BufReader::new(File::open(path)?),
        hash: Sha256::new(),
    };
    let mut csv = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(source);
    for row in csv.deserialize::<T>() {
        consume(row.map_err(input_csv_error)?)?;
    }
    let source = csv.into_inner();
    if hex(source.hash.finalize().as_slice()) != expected {
        return Err(ResearchError::new(
            "hash_mismatch",
            "file changed during import",
        ));
    }
    Ok(())
}
fn insert_json<T: Serialize>(
    conn: &Connection,
    select: &str,
    keys: &[&dyn rusqlite::ToSql],
    sql: &str,
    values: &[&dyn rusqlite::ToSql],
    row: &T,
) -> ResearchResult<()> {
    let payload = serde_json::to_string(row)?;
    if let Some(old) = conn
        .query_row(select, keys, |r| r.get::<_, String>(0))
        .optional()?
    {
        if old != payload {
            return Err(ResearchError::new(
                "duplicate_conflict",
                "conflicting duplicate observation identity",
            ));
        }
        return Ok(());
    }
    let mut p = values.to_vec();
    p.push(&payload);
    conn.execute(sql, p.as_slice())
        .map_err(input_sqlite_error)?;
    Ok(())
}

pub(crate) fn import(store: &ResearchStore, bundle: &Path) -> ResearchResult<DatasetRecord> {
    store.require_write()?;
    let bundle = fs::canonicalize(bundle)?;
    let manifest_path = safe_file(&bundle, "manifest.json")?;
    let bytes = fs::read(&manifest_path)?;
    let manifest: BundleManifest = serde_json::from_slice(&bytes)
        .map_err(|e| ResearchError::new("input_json", format!("manifest JSON: {e}")))?;
    if manifest.format != "gp-pit/v1"
        || manifest.market != "cn"
        || manifest.timezone != "Asia/Shanghai"
        || !matches!(manifest.money_unit.as_str(), "CNY" | "CNY_100m")
        || manifest.coverage.start > manifest.coverage.end
    {
        return Err(ResearchError::new(
            "manifest",
            "gp-pit/v1 cn/Asia/Shanghai and explicit valid coverage/money units required",
        ));
    }
    nonempty(&manifest.name, "name")?;
    nonempty(&manifest.source, "source")?;
    for (field, unit) in &manifest.rate_units {
        if ![
            "roe",
            "dividend_yield",
            "deducted_net_profit_growth_rate",
            "deducted_net_profit_margin",
            "turnover_rate",
        ]
        .contains(&field.as_str())
            || !matches!(unit.as_str(), "ratio" | "percentage_points")
        {
            return Err(ResearchError::new(
                "rate_unit",
                format!("invalid unit declaration {field}={unit}"),
            ));
        }
    }
    let roles = [
        "trading_calendar",
        "securities",
        "daily_bars",
        "financial_events",
        "daily_status",
        "corporate_actions",
    ];
    let mut files = std::collections::BTreeMap::new();
    for file in &manifest.files {
        if !roles.contains(&file.role.as_str()) || files.contains_key(&file.role) {
            return Err(ResearchError::new(
                "manifest",
                "exactly one file per required role",
            ));
        }
        let path = safe_file(&bundle, &file.path)?;
        if file.sha256.len() != 64 || file_hash(&path)? != file.sha256 {
            return Err(ResearchError::new(
                "hash_mismatch",
                format!("SHA256 mismatch for {}", file.role),
            ));
        }
        check_headers(&path, &file.role)?;
        files.insert(file.role.clone(), (path, file.sha256.clone()));
    }
    if files.len() != roles.len() {
        return Err(ResearchError::new(
            "manifest",
            "all six required roles must be declared",
        ));
    }
    let mut fingerprint = Sha256::new();
    fingerprint.update(b"gp-pit-dataset/v1\0");
    fingerprint.update((bytes.len() as u64).to_le_bytes());
    fingerprint.update(&bytes);
    for (role, (_, hash)) in &files {
        fingerprint.update(role.as_bytes());
        fingerprint.update(hash.as_bytes());
    }
    let id = hex(fingerprint.finalize().as_slice());
    if let Some(record) = store.lookup_dataset(&id)? {
        return Ok(record);
    }
    let datasets = store.root().join("datasets");
    fs::create_dir_all(&datasets)?;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let temp = datasets.join(format!(
        ".import-{id}-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut conn = Connection::open(&temp)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;PRAGMA foreign_keys=ON;")?;
        let tx = conn.transaction()?;
        tx.execute_batch(DATA_SCHEMA)?;
        #[derive(serde::Deserialize)]
        struct Session {
            date: NaiveDate,
        }
        let mut calendar = Vec::new();
        let (path, hash) = &files["trading_calendar"];
        rows::<Session>(path, hash, |row| {
            calendar.push(row.date);
            Ok(())
        })?;
        if calendar.is_empty() {
            return Err(ResearchError::new("calendar", "calendar is empty"));
        }
        calendar.sort();
        if calendar.windows(2).any(|w| w[0] == w[1]) {
            return Err(ResearchError::new(
                "calendar",
                "calendar contains duplicate sessions",
            ));
        }
        if calendar.first() != Some(&manifest.coverage.start)
            || calendar.last() != Some(&manifest.coverage.end)
        {
            return Err(ResearchError::new(
                "calendar",
                "manifest coverage must exactly match calendar bounds",
            ));
        }
        for date in &calendar {
            tx.execute("INSERT INTO calendar VALUES(?)", [date.to_string()])?;
        }
        let (path, hash) = &files["securities"];
        rows::<Security>(path, hash, |row| {
            identity(&row.code)?;
            nonempty(&row.name, "security.name")?;
            if !matches!(row.security_type.as_str(), "equity" | "index")
                || row.delisted_date.is_some_and(|d| d < row.listed_date)
            {
                return Err(ResearchError::new(
                    "security",
                    "invalid type or listing interval",
                ));
            }
            insert_json(
                &tx,
                "SELECT payload FROM securities WHERE code=?",
                &[&row.code],
                "INSERT INTO securities VALUES(?,?,?,?,?)",
                &[
                    &row.code,
                    &row.security_type,
                    &row.listed_date.to_string(),
                    &row.delisted_date.map(|d| d.to_string()),
                ],
                &row,
            )
        })?;
        let (path, hash) = &files["daily_bars"];
        rows::<DailyBar>(path, hash, |mut row| {
            let listing: Option<(String, Option<String>)> = tx
                .query_row(
                    "SELECT listed_date,delisted_date FROM securities WHERE code=?",
                    [&row.code],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if listing.as_ref().is_some_and(|(listed, delisted)| {
                row.date.to_string() < *listed
                    || delisted
                        .as_ref()
                        .is_some_and(|d| row.date.to_string() >= *d)
            }) {
                return Err(ResearchError::new(
                    "listing_interval",
                    format!("bar outside listed interval {} {}", row.code, row.date),
                ));
            }
            finite(&[
                Some(row.open),
                Some(row.high),
                Some(row.low),
                Some(row.close),
                row.volume,
                row.amount,
                row.total_shares,
                row.float_shares,
                row.turnover_rate,
                row.volume_ratio,
            ])?;
            if row.low <= 0.0
                || row.high < row.low
                || row.open < row.low
                || row.open > row.high
                || row.close < row.low
                || row.close > row.high
                || [
                    row.volume,
                    row.amount,
                    row.total_shares,
                    row.float_shares,
                    row.turnover_rate,
                    row.volume_ratio,
                ]
                .iter()
                .flatten()
                .any(|v| *v < 0.0)
            {
                return Err(ResearchError::new(
                    "bar",
                    format!("invalid OHLCV {} {}", row.code, row.date),
                ));
            }
            row.turnover_rate = rate(row.turnover_rate, "turnover_rate", &manifest, false)?;
            insert_json(
                &tx,
                "SELECT payload FROM bars WHERE code=? AND date=?",
                &[&row.code, &row.date.to_string()],
                "INSERT INTO bars VALUES(?,?,?)",
                &[&row.code, &row.date.to_string()],
                &row,
            )
        })?;
        let (path, hash) = &files["financial_events"];
        rows::<FinancialEvent>(path, hash, |mut row| {
            nonempty(&row.available_at, "available_at")?;
            nonempty(&row.source, "financial.source")?;
            finite(&[
                row.eps_ttm,
                row.bps,
                row.roe,
                row.deducted_net_profit_billion,
                row.deducted_net_profit_growth_rate,
                row.deducted_net_profit_margin,
                row.dividend_yield,
            ])?;
            let visible = visible_date(&row.available_at, &calendar)?.map(|d| d.to_string());
            let disclosure = if row.available_at.len() == 10 {
                parse_date(&row.available_at)?
            } else {
                DateTime::parse_from_rfc3339(&row.available_at)
                    .map_err(|e| ResearchError::new("timestamp", e.to_string()))?
                    .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
                    .date_naive()
            };
            if row.report_date > disclosure {
                return Err(ResearchError::new(
                    "financial",
                    "report date after disclosure",
                ));
            }
            row.roe = rate(row.roe, "roe", &manifest, false)?;
            row.dividend_yield = rate(row.dividend_yield, "dividend_yield", &manifest, false)?;
            row.deducted_net_profit_growth_rate = rate(
                row.deducted_net_profit_growth_rate,
                "deducted_net_profit_growth_rate",
                &manifest,
                true,
            )?;
            row.deducted_net_profit_margin = rate(
                row.deducted_net_profit_margin,
                "deducted_net_profit_margin",
                &manifest,
                true,
            )?;
            if manifest.money_unit == "CNY" {
                row.deducted_net_profit_billion = row.deducted_net_profit_billion.map(|v| v / 1e8);
            }
            insert_json(
                &tx,
                "SELECT payload FROM financial WHERE code=? AND report_date=? AND revision=?",
                &[&row.code, &row.report_date.to_string(), &row.revision],
                "INSERT INTO financial VALUES(?,?,?,?,?)",
                &[
                    &row.code,
                    &row.report_date.to_string(),
                    &row.revision,
                    &visible,
                ],
                &row,
            )
        })?;
        let (path, hash) = &files["daily_status"];
        rows::<DailyStatus>(path, hash, |row| {
            nonempty(&row.name, "daily_status.name")?;
            if !row.is_listed && (row.can_buy || row.can_sell) {
                return Err(ResearchError::new(
                    "status",
                    "unlisted security cannot trade",
                ));
            }
            insert_json(
                &tx,
                "SELECT payload FROM statuses WHERE code=? AND date=?",
                &[&row.code, &row.date.to_string()],
                "INSERT INTO statuses VALUES(?,?,?)",
                &[&row.code, &row.date.to_string()],
                &row,
            )
        })?;
        let (path, hash) = &files["corporate_actions"];
        rows::<CorporateAction>(path, hash, |row| {
            nonempty(&row.event_id, "event_id")?;
            nonempty(&row.known_at, "known_at")?;
            finite(&[row.cash_per_share, row.share_multiplier])?;
            if row.pay_date < row.ex_date
                || row.cash_per_share.is_some_and(|v| v < 0.0)
                || row.share_multiplier.is_some_and(|v| v <= 0.0)
                || !matches!(
                    row.kind.as_str(),
                    "cash_dividend" | "share_split" | "delisting_settlement" | "unsupported"
                )
            {
                return Err(ResearchError::new(
                    "action",
                    "invalid action kind/date/value",
                ));
            }
            let visible = visible_date(&row.known_at, &calendar)?.map(|d| d.to_string());
            insert_json(
                &tx,
                "SELECT payload FROM actions WHERE code=? AND event_id=?",
                &[&row.code, &row.event_id],
                "INSERT INTO actions VALUES(?,?,?,?,?,?)",
                &[
                    &row.code,
                    &row.event_id,
                    &row.ex_date.to_string(),
                    &row.pay_date.to_string(),
                    &visible,
                ],
                &row,
            )
        })?;
        let action_count: i64 = tx.query_row("SELECT count(*) FROM actions", [], |r| r.get(0))?;
        if action_count == 0 && !manifest.coverage.corporate_actions_complete {
            return Err(ResearchError::new(
                "action_coverage",
                "empty corporate actions require complete declared source coverage",
            ));
        }
        let record = DatasetRecord {
            dataset_id: id.clone(),
            name: manifest.name.clone(),
            data_fingerprint: id.clone(),
            start_date: manifest.coverage.start,
            end_date: manifest.coverage.end,
            real_data: manifest.real_data,
            full_universe: manifest.full_universe,
            security_count: tx.query_row("SELECT count(*) FROM securities", [], |r| {
                r.get::<_, i64>(0)
            })? as usize,
            bar_count: tx.query_row("SELECT count(*) FROM bars", [], |r| r.get::<_, i64>(0))?
                as usize,
            source: manifest.source.clone(),
        };
        tx.execute(
            "INSERT INTO metadata VALUES('record',?)",
            [serde_json::to_string(&record)?],
        )?;
        tx.execute(
            "INSERT INTO metadata VALUES('manifest',?)",
            [serde_json::to_string(&manifest)?],
        )?;
        let integrity: String = tx.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(ResearchError::new("integrity", integrity));
        }
        tx.commit()?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(conn);
        let final_dir = datasets.join(&id);
        fs::create_dir_all(&final_dir)?;
        let final_file = final_dir.join("dataset.sqlite");
        if final_file.exists() {
            let existing = Connection::open_with_flags(
                &final_file,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            check_version(&existing)?;
            let old: String =
                existing.query_row("SELECT value FROM metadata WHERE key='record'", [], |r| {
                    r.get(0)
                })?;
            if serde_json::from_str::<DatasetRecord>(&old)?.data_fingerprint != id {
                return Err(ResearchError::new(
                    "immutable_conflict",
                    "existing dataset fingerprint conflict",
                ));
            }
        } else {
            fs::rename(&temp, &final_file)?;
        }
        store.register_dataset(&record)?;
        Ok(record)
    })();
    // Only files generated by this import are removed; no recursive deletion.
    for path in [
        &temp,
        &PathBuf::from(format!("{}-wal", temp.display())),
        &PathBuf::from(format!("{}-shm", temp.display())),
    ] {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
    result
}
