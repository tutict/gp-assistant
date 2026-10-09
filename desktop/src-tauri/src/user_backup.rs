//! Manual encrypted user backups, distinct from research sync packs.
//! Only recognized native schemas support explicit no-replace restore or tombstone-aware merge.
//! Generic staged SQLite datasets must never be installed as live databases.
use aes_gcm::{
    aead::{rand_core::RngCore, Aead, OsRng, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"GPUBAK\0\0";
const VERSION: u16 = 1;
const HEADER_BYTES: usize = 42; // magic(8), version LE(2), salt(16), nonce(12), ciphertext length LE(4)
const MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;
const MAX_BASE64_BYTES: usize = MAX_BACKUP_BYTES.div_ceil(3) * 4;
const MAX_STORE_BYTES: usize = 16 * 1024 * 1024;
const MAX_CELL_BYTES: usize = 2 * 1024 * 1024;
const MAX_ROWS: u64 = 250_000;
const RECOVERY_APPLICATION_ID: i64 = 0x47505542; // GPUB: cannot be mistaken for an unmarked live store.
const REDACTED: &str = "[REDACTED]";
// Bound simultaneous KDF / SQLite work; commands fail busy rather than queue secrets.
static OPERATION: Mutex<()> = Mutex::new(());

struct StoreSpec {
    id: &'static str,
    relative_path: &'static str,
}
const STORE_SPECS: [StoreSpec; 5] = [
    StoreSpec {
        id: "watchlist",
        relative_path: "watchlist/watchlist.sqlite",
    },
    StoreSpec {
        id: "agent_ledger",
        relative_path: "agent/agent-runs.sqlite",
    },
    StoreSpec {
        id: "research",
        relative_path: "research/research.sqlite",
    },
    StoreSpec {
        id: "sentiment",
        relative_path: "research/sentiment.sqlite",
    },
    StoreSpec {
        id: "client_state",
        relative_path: "client-state.sqlite",
    },
];
fn spec(id: &str) -> Result<&'static StoreSpec, String> {
    STORE_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "unsupported backup store".into())
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilterStats {
    tables: u64,
    rows: u64,
    filtered_fields: u64,
    omitted_tables: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveStore {
    id: String,
    sqlite_base64: String,
    stats: FilterStats,
}
impl Drop for ArchiveStore {
    fn drop(&mut self) {
        self.sqlite_base64.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    version: u16,
    created_at_epoch_ms: u64,
    stores: Vec<ArchiveStore>,
}

#[derive(Clone, Serialize)]
pub struct BackupStorePreview {
    id: String,
    relative_path: String,
    bytes: u64,
    tables: u64,
    rows: u64,
    filtered_fields: u64,
    omitted_tables: u64,
    current: String,
    conflict: bool,
    restore_to_empty: bool,
    restore_status: String,
    merge_available: bool,
    receipt_id: Option<String>,
}
#[derive(Serialize)]
pub struct BackupExport {
    blob_base64: String,
    filename: String,
    stores: Vec<BackupStorePreview>,
    missing_stores: Vec<String>,
}
#[derive(Serialize)]
pub struct BackupPreview {
    state: &'static str,
    imported: bool,
    staged: bool,
    recovery_id: Option<String>,
    restored_stores: Vec<String>,
    restoration_note: String,
    created_at_epoch_ms: u64,
    stores: Vec<BackupStorePreview>,
    missing_stores: Vec<String>,
}

fn check_password(password: &str) -> Result<(), String> {
    if password.len() < 12 || password.len() > 1024 || password.trim().is_empty() {
        return Err("backup passphrase must contain 12 to 1024 UTF-8 bytes".into());
    }
    Ok(())
}
fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    // Fixed v1 Argon2id: 19 MiB, two passes, one lane. No attacker-controlled cost.
    let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| "backup KDF unavailable")?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|_| "backup key derivation failed")?;
    Ok(key)
}
fn encrypt_archive(archive: &Archive, password: &str) -> Result<String, String> {
    check_password(password)?;
    let plaintext =
        Zeroizing::new(serde_json::to_vec(archive).map_err(|_| "backup encoding failed")?);
    if plaintext.len() + HEADER_BYTES + 16 > MAX_BACKUP_BYTES {
        return Err("backup exceeds 64 MiB".into());
    }
    let mut salt = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| "secure random generator unavailable")?;
    let mut nonce_bytes = [0u8; 12];
    OsRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|_| "secure random generator unavailable")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let mut header = MAGIC.to_vec();
    header.extend_from_slice(&VERSION.to_le_bytes());
    header.extend_from_slice(&salt);
    header.extend_from_slice(nonce);
    header.extend_from_slice(&((plaintext.len() + 16) as u32).to_le_bytes());
    let key = derive_key(password, &salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| "backup cipher unavailable")?;
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &plaintext,
                aad: &header,
            },
        )
        .map_err(|_| "backup encryption failed")?;
    header.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(header))
}
fn decrypt_archive(blob: &str, password: &str) -> Result<Archive, String> {
    if blob.len() > MAX_BASE64_BYTES {
        return Err("backup exceeds 64 MiB".into());
    }
    let bytes = STANDARD
        .decode(blob)
        .map_err(|_| "invalid backup encoding")?;
    if bytes.len() < HEADER_BYTES + 16 || bytes.len() > MAX_BACKUP_BYTES {
        return Err("invalid backup size".into());
    }
    if &bytes[..8] != MAGIC || u16::from_le_bytes([bytes[8], bytes[9]]) != VERSION {
        return Err("unsupported user backup format or version".into());
    }
    let length = u32::from_le_bytes(
        bytes[38..42]
            .try_into()
            .map_err(|_| "invalid backup header")?,
    ) as usize;
    if length != bytes.len() - HEADER_BYTES {
        return Err("invalid backup length".into());
    }
    // No filesystem or database work before successful authenticated decryption.
    check_password(password)?;
    let key = derive_key(password, &bytes[10..26])?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| "backup cipher unavailable")?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&bytes[26..38]),
                Payload {
                    msg: &bytes[HEADER_BYTES..],
                    aad: &bytes[..HEADER_BYTES],
                },
            )
            .map_err(|_| "wrong passphrase or damaged backup")?,
    );
    let archive: Archive =
        serde_json::from_slice(&plaintext).map_err(|_| "invalid backup contents")?;
    validate_archive(&archive)?;
    Ok(archive)
}
fn validate_archive(archive: &Archive) -> Result<(), String> {
    if archive.version != VERSION
        || archive.stores.is_empty()
        || archive.stores.len() > STORE_SPECS.len()
    {
        return Err("unsupported backup manifest".into());
    }
    let mut seen = HashSet::new();
    for store in &archive.stores {
        spec(&store.id)?;
        if !seen.insert(&store.id) {
            return Err("duplicate backup store".into());
        }
        if store.sqlite_base64.len() > MAX_STORE_BYTES.div_ceil(3) * 4 {
            return Err("database exceeds 16 MiB".into());
        }
    }
    Ok(())
}

fn sql_error(_: rusqlite::Error) -> String {
    "backup database validation or filtering failed".into()
}
fn io_error(_: std::io::Error) -> String {
    "backup file operation failed".into()
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn normalized(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn sensitive(name: &str) -> bool {
    let name = normalized(name);
    [
        "password",
        "passwd",
        "passphrase",
        "secret",
        "credential",
        "apikey",
        "accesskey",
        "privatekey",
        "authorization",
        "authentication",
        "cookie",
        "settings",
        "configuration",
        "connectionstring",
    ]
    .iter()
    .any(|part| name.contains(part))
        || name == "auth"
        || name == "config"
        || name == "headers"
        || (name.contains("token")
            && ![
                "tokencount",
                "totaltokens",
                "inputtokens",
                "outputtokens",
                "maxtokens",
            ]
            .contains(&name.as_str()))
}
fn looks_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "sk-",
        "bearer ",
        "basic ",
        "-----begin ",
        "api_key",
        "apikey",
        "api-key",
        "access_token",
        "accesstoken",
        "password",
        "passphrase",
        "client_secret",
        "credential",
        "authorization",
        "?token=",
        "&token=",
        "secret=",
        "secret:",
    ]
    .iter()
    .any(|s| lower.contains(s))
        || (lower.contains("://") && lower.contains('@'))
}
fn clean_json(value: &mut serde_json::Value, depth: usize, filtered: &mut u64) {
    if depth > 24 {
        *value = serde_json::Value::String(REDACTED.into());
        *filtered += 1;
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            // Credentials can be serialized as {key: "api_key", value: "..."}.
            if map.iter().any(|(key, value)| {
                ["key", "name", "field", "settingkey", "settingname"]
                    .contains(&normalized(key).as_str())
                    && value.as_str().is_some_and(sensitive)
            }) {
                for value in map.values_mut() {
                    *value = serde_json::Value::String(REDACTED.into());
                    *filtered += 1;
                }
                return;
            }
            for (key, value) in map.iter_mut() {
                if sensitive(key) {
                    *value = serde_json::Value::String(REDACTED.into());
                    *filtered += 1;
                } else {
                    clean_json(value, depth + 1, filtered);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                clean_json(value, depth + 1, filtered);
            }
        }
        serde_json::Value::String(text) => {
            *text = clean_text(text, depth + 1, filtered);
        }
        _ => {}
    }
}
fn clean_text(text: &str, depth: usize, filtered: &mut u64) -> String {
    if depth > 24 {
        *filtered += 1;
        return REDACTED.into();
    }
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
            clean_json(&mut value, depth + 1, filtered);
            return value.to_string();
        }
        // Malformed/truncated structured content cannot be inspected reliably.
        *filtered += 1;
        return REDACTED.into();
    }
    if looks_secret(text) {
        *filtered += 1;
        REDACTED.into()
    } else {
        text.into()
    }
}

/// Rebuild ordinary tables, never execute source schema SQL, triggers, or views.
/// BLOBs (including derived embeddings), FTS/shadow tables and settings tables are
/// deliberately omitted/redacted. This is data for an owner-mediated merge, not a
/// replacement schema. Rebuilding also removes freelist/deleted-page secret bytes.
fn sanitized_database(source: &Connection) -> Result<(Connection, FilterStats), String> {
    source
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(sql_error)?;
    let clean = Connection::open_in_memory().map_err(sql_error)?;
    clean
        .execute_batch("PRAGMA trusted_schema=OFF; BEGIN;")
        .map_err(sql_error)?;
    clean
        .pragma_update(None, "application_id", RECOVERY_APPLICATION_ID)
        .map_err(sql_error)?;
    let version: i64 = source
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql_error)?;
    clean
        .pragma_update(None, "user_version", version)
        .map_err(sql_error)?;
    let mut stats = FilterStats::default();
    let mut tables = source.prepare("PRAGMA table_list").map_err(sql_error)?;
    let entries: Vec<(String, String)> = tables
        .query_map([], |row| Ok((row.get(1)?, row.get(2)?)))
        .map_err(sql_error)?
        .collect::<Result<_, _>>()
        .map_err(sql_error)?;
    if entries.len() > 128 {
        return Err("too many backup tables".into());
    }
    for (table, kind) in entries {
        if table.starts_with("sqlite_") {
            continue;
        }
        if kind != "table" || sensitive(&table) {
            stats.omitted_tables += 1;
            continue;
        }
        if table.len() > 256 {
            return Err("invalid backup table name".into());
        }
        let mut columns = source
            .prepare(&format!("PRAGMA table_info({})", quote(&table)))
            .map_err(sql_error)?;
        let names: Vec<String> = columns
            .query_map([], |r| r.get(1))
            .map_err(sql_error)?
            .collect::<Result<_, _>>()
            .map_err(sql_error)?;
        if names.is_empty() || names.len() > 128 || names.iter().any(|n| n.len() > 256) {
            return Err("invalid backup columns".into());
        }
        // No affinity/coercion: preserve each SQLite cell's native storage class.
        clean
            .execute_batch(&format!(
                "CREATE TABLE {} ({})",
                quote(&table),
                names.iter().map(|n| quote(n)).collect::<Vec<_>>().join(",")
            ))
            .map_err(sql_error)?;
        let fields = names.iter().map(|n| quote(n)).collect::<Vec<_>>().join(",");
        let mut select = source
            .prepare(&format!("SELECT {fields} FROM {}", quote(&table)))
            .map_err(sql_error)?;
        let mut insert = clean
            .prepare(&format!(
                "INSERT INTO {} VALUES ({})",
                quote(&table),
                vec!["?"; names.len()].join(",")
            ))
            .map_err(sql_error)?;
        let mut rows = select.query([]).map_err(sql_error)?;
        let mut visited = 0u64;
        while let Some(row) = rows.next().map_err(sql_error)? {
            visited += 1;
            if visited > MAX_ROWS || stats.rows >= MAX_ROWS {
                return Err("too many backup rows".into());
            }
            let mut values = (0..names.len())
                .map(|i| row.get::<_, SqlValue>(i))
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_error)?;
            for value in &values {
                let len = match value {
                    SqlValue::Text(s) => s.len(),
                    SqlValue::Blob(b) => b.len(),
                    _ => 0,
                };
                if len > MAX_CELL_BYTES {
                    return Err("backup cell exceeds 2 MiB".into());
                }
            }
            // Key/value stores can hold credential fields in rows rather than columns.
            if names.iter().zip(&values).any(|(name, value)| {
                ["key", "name", "field", "settingkey", "settingname"]
                    .contains(&normalized(name).as_str())
                    && matches!(value, SqlValue::Text(text) if sensitive(text))
            }) {
                stats.filtered_fields += 1;
                continue;
            }
            for (name, value) in names.iter().zip(&mut values) {
                if sensitive(name) {
                    *value = SqlValue::Text(REDACTED.into());
                    stats.filtered_fields += 1;
                } else {
                    match value {
                        SqlValue::Text(text) => {
                            *text = clean_text(text, 0, &mut stats.filtered_fields)
                        }
                        SqlValue::Blob(_) => {
                            *value = SqlValue::Null;
                            stats.filtered_fields += 1;
                        }
                        _ => {}
                    }
                }
            }
            insert
                .execute(rusqlite::params_from_iter(values))
                .map_err(sql_error)?;
            stats.rows += 1;
        }
        stats.tables += 1;
    }
    clean.execute_batch("COMMIT;").map_err(sql_error)?;
    Ok((clean, stats))
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(sql_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(sql_error)?;
    conn.execute_batch("PRAGMA trusted_schema=OFF;")
        .map_err(sql_error)?;
    let pages: i64 = conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .map_err(sql_error)?;
    let size: i64 = conn
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(sql_error)?;
    if pages < 0 || size <= 0 || pages.saturating_mul(size) > MAX_STORE_BYTES as i64 {
        return Err("database exceeds 16 MiB".into());
    }
    Ok(conn)
}
// All paths come from static specs or random identifiers, never from a manifest.
fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("invalid backup path".into());
        }
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("linked backup paths are not supported".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(path)
}
fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "secure random generator unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
struct Scratch(PathBuf);
impl Scratch {
    fn new(parent: &Path) -> Result<Self, String> {
        let path = parent.join(format!(".user-backup-{}.pending", random_id()?));
        fs::create_dir(&path).map_err(io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
        }
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn read_bounded(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    let file = fs::File::open(path).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > MAX_STORE_BYTES as u64 {
        return Err("database exceeds 16 MiB".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_STORE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_STORE_BYTES {
        return Err("database exceeds 16 MiB".into());
    }
    Ok(bytes)
}
fn snapshot(conn: &Connection, path: &Path) -> Result<(), String> {
    crate::durability::snapshot_sqlite(conn, path)
        .map_err(|_| "consistent backup snapshot failed".into())
}
fn validate(path: &Path) -> Result<(), String> {
    crate::durability::validate_sqlite(path).map_err(|_| "backup SQLite validation failed".into())
}
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::durability::atomic_write(path, bytes).map_err(|_| "atomic backup write failed".into())
}
fn missing(stores: &[ArchiveStore]) -> Vec<String> {
    STORE_SPECS
        .iter()
        .filter(|s| !stores.iter().any(|v| v.id == s.id))
        .map(|s| s.id.into())
        .collect()
}
fn summary(
    store: &ArchiveStore,
    bytes: usize,
    current: &str,
) -> Result<BackupStorePreview, String> {
    Ok(BackupStorePreview {
        id: store.id.clone(),
        relative_path: spec(&store.id)?.relative_path.into(),
        bytes: bytes as u64,
        tables: store.stats.tables,
        rows: store.stats.rows,
        filtered_fields: store.stats.filtered_fields,
        omitted_tables: store.stats.omitted_tables,
        current: current.into(),
        conflict: current != "absent",
        restore_to_empty: false,
        restore_status: "staged_only".into(),
        merge_available: false,
        receipt_id: None,
    })
}
fn export_backup(root: &Path, password: &str) -> Result<BackupExport, String> {
    check_password(password)?;
    let scratch = Scratch::new(root)?;
    let mut archive = Archive {
        version: VERSION,
        created_at_epoch_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "invalid system clock")?
            .as_millis() as u64,
        stores: Vec::new(),
    };
    let mut previews = Vec::new();
    let mut total = 0;
    for s in &STORE_SPECS {
        let path = checked_path(root, s.relative_path)?;
        if !path.try_exists().map_err(io_error)? {
            continue;
        }
        let live = open_readonly(&path)?;
        let raw_path = scratch.0.join("raw.sqlite");
        snapshot(&live, &raw_path)?;
        drop(live);
        let raw = open_readonly(&raw_path)?;
        let (clean, stats) = sanitized_database(&raw)?;
        let clean = native_candidate(s.id, &clean).unwrap_or(clean);
        let clean_path = scratch.0.join("clean.sqlite");
        snapshot(&clean, &clean_path)?;
        let bytes = read_bounded(&clean_path)?;
        let store = ArchiveStore {
            id: s.id.into(),
            sqlite_base64: STANDARD.encode(&*bytes),
            stats,
        };
        total += store.sqlite_base64.len();
        if total > MAX_BACKUP_BYTES - 8192 {
            return Err("backup exceeds 64 MiB".into());
        }
        previews.push(summary(&store, bytes.len(), "present_valid")?);
        archive.stores.push(store);
        drop(raw);
        drop(clean);
        fs::remove_file(&raw_path).map_err(io_error)?;
        fs::remove_file(&clean_path).map_err(io_error)?;
    }
    if archive.stores.is_empty() {
        return Err("no user SQLite stores available to back up".into());
    }
    Ok(BackupExport {
        blob_base64: encrypt_archive(&archive, password)?,
        filename: format!("gp-user-backup-{}.gpbackup", archive.created_at_epoch_ms),
        stores: previews,
        missing_stores: missing(&archive.stores),
    })
}
fn inspect_backup(
    root: &Path,
    blob: &str,
    password: &str,
    stage: bool,
) -> Result<BackupPreview, String> {
    let archive = decrypt_archive(blob, password)?;
    let scratch = Scratch::new(root)?;
    let mut previews = Vec::new();
    let mut datasets = Vec::new();
    // Validate ALL stores and re-filter even authenticated data before persistent staging.
    for store in &archive.stores {
        let bytes = Zeroizing::new(
            STANDARD
                .decode(&store.sqlite_base64)
                .map_err(|_| "invalid database encoding")?,
        );
        if bytes.len() > MAX_STORE_BYTES || !bytes.starts_with(b"SQLite format 3\0") {
            return Err("invalid backup SQLite file".into());
        }
        let raw_path = scratch.0.join("incoming.sqlite");
        write(&raw_path, &bytes)?;
        validate(&raw_path)?;
        let conn = open_readonly(&raw_path)?;
        let (clean, mut stats) = sanitized_database(&conn)?;
        // Recompute table/row counts; preserve advisory counts of the export's removals.
        stats.filtered_fields = stats
            .filtered_fields
            .saturating_add(store.stats.filtered_fields);
        stats.omitted_tables = stats
            .omitted_tables
            .saturating_add(store.stats.omitted_tables);
        let clean_path = scratch.0.join(format!("{}.sqlite", store.id));
        snapshot(&clean, &clean_path)?;
        validate(&clean_path)?;
        let clean_bytes = read_bounded(&clean_path)?;
        let current_path = checked_path(root, spec(&store.id)?.relative_path)?;
        let current = if !current_path.try_exists().map_err(io_error)? {
            "absent"
        } else if validate(&current_path).is_ok() {
            "present_valid"
        } else {
            "present_unreadable"
        };
        let sanitized = ArchiveStore {
            id: store.id.clone(),
            sqlite_base64: String::new(),
            stats,
        };
        let mut preview = summary(&sanitized, clean_bytes.len(), current)?;
        preview.restore_to_empty = ["watchlist", "agent_ledger"].contains(&store.id.as_str())
            && current == "absent"
            && destination_absent(&current_path)?
            && native_candidate(&store.id, &clean).is_ok();
        let pristine = store.id == "watchlist"
            && current == "present_valid"
            && native_candidate(&store.id, &clean).is_ok()
            && open_readonly(&current_path)
                .and_then(|db| pristine_watchlist(&db))
                .unwrap_or(false);
        preview.restore_to_empty |= pristine;
        preview.merge_available = store.id == "agent_ledger"
            && current == "present_valid"
            && native_candidate(&store.id, &clean).is_ok()
            && open_readonly(&current_path)
                .and_then(|db| agent_candidate(&db))
                .is_ok();
        preview.restore_status = if current != "absent" {
            "preserved_current"
        } else if preview.restore_to_empty {
            "eligible_absent_native_schema"
        } else {
            "unsupported_schema_or_store_or_sidecars"
        }
        .into();
        if pristine {
            preview.restore_status = "eligible_pristine_watchlist_v2".into();
        }
        let receipt = receipt_path(root, blob, &store.id)?;
        if receipt.try_exists().map_err(io_error)? {
            preview.restore_to_empty = false;
            preview.merge_available = false;
            preview.restore_status = "already_attempted_receipt".into();
            preview.receipt_id = receipt
                .file_name()
                .map(|s| s.to_string_lossy().into_owned());
        }
        previews.push(preview);
        datasets.push((store.id.clone(), clean_bytes));
        drop(conn);
        drop(clean);
        fs::remove_file(&raw_path).map_err(io_error)?;
    }
    let mut result = BackupPreview {
        state: "preview_only",
        imported: false,
        staged: false,
        recovery_id: None,
        restored_stores: Vec::new(),
        restoration_note: "No live stores imported".into(),
        created_at_epoch_ms: archive.created_at_epoch_ms,
        stores: previews,
        missing_stores: missing(&archive.stores),
    };
    if stage {
        let parent = checked_path(root, "user-backup-recovery")?;
        fs::create_dir_all(&parent).map_err(io_error)?;
        let pending = Scratch::new(&parent)?;
        for (id, bytes) in &datasets {
            write(&pending.0.join(format!("{id}.sqlite")), bytes)?;
        }
        let id = format!("recovery-{}", random_id()?);
        result.staged = true;
        result.recovery_id = Some(id.clone());
        write(
            &pending.0.join("preview.json"),
            &serde_json::to_vec(&result).map_err(|_| "preview encoding failed")?,
        )?;
        // New isolated name only; never rename/copy anything into an active store path.
        let destination = parent.join(id);
        if destination.try_exists().map_err(io_error)? {
            return Err("recovery directory collision".into());
        }
        fs::rename(&pending.0, &destination).map_err(io_error)?;
    }
    Ok(result)
}
// Recovery accepts legacy watchlist v1 and the owner's explicit pristine-aware v2 contract.
// Unknown tables/columns/versions (including future tombstones) are never ignored.
const WATCHLIST_COLUMNS: &[&str] = &[
    "code",
    "name",
    "industry",
    "added_at",
    "source",
    "screen_criteria_summary",
    "updated_at",
];
const STATE_COLUMNS: &[&str] = &["singleton", "revision", "migration_complete"];
const PRISTINE_STATE_COLUMNS: &[&str] = &[
    "singleton",
    "revision",
    "migration_complete",
    "restore_pristine",
];
const OPERATIONS_COLUMNS: &[&str] = &["operation_id", "payload", "response"];
fn watchlist_candidate(source: &Connection) -> Result<Connection, String> {
    let version: i64 = source
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql_error)?;
    if ![1, 2].contains(&version) {
        return Err("only watchlist schema v1/v2 supports recovery".into());
    }
    let tables: Vec<String> = source.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(sql_error)?.query_map([], |r| r.get(0)).map_err(sql_error)?.collect::<Result<_, _>>().map_err(sql_error)?;
    if tables != ["watchlist", "watchlist_operations", "watchlist_state"] {
        return Err("unknown watchlist tables; owner migration required".into());
    }
    let target = Connection::open_in_memory().map_err(sql_error)?;
    target.execute_batch("BEGIN;
        CREATE TABLE watchlist (code TEXT PRIMARY KEY NOT NULL, name TEXT, industry TEXT, added_at TEXT NOT NULL, source TEXT, screen_criteria_summary TEXT, updated_at INTEGER NOT NULL);
        CREATE INDEX idx_watchlist_added_at ON watchlist(added_at DESC);
        CREATE TABLE watchlist_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0), migration_complete INTEGER NOT NULL CHECK(migration_complete IN (0,1)), restore_pristine INTEGER NOT NULL DEFAULT 0 CHECK(restore_pristine IN (0,1)));
        CREATE TABLE watchlist_operations (operation_id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL, response TEXT NOT NULL);
        PRAGMA user_version=2;").map_err(sql_error)?;
    for (table, expected) in [
        ("watchlist", WATCHLIST_COLUMNS),
        ("watchlist_state", STATE_COLUMNS),
        ("watchlist_operations", OPERATIONS_COLUMNS),
    ] {
        let names: Vec<String> = source
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
            .map_err(sql_error)?
            .query_map([], |r| r.get(1))
            .map_err(sql_error)?
            .collect::<Result<_, _>>()
            .map_err(sql_error)?;
        let expected = if table == "watchlist_state" && version == 2 {
            PRISTINE_STATE_COLUMNS
        } else {
            expected
        };
        if names != expected {
            return Err("unknown watchlist columns; owner migration required".into());
        }
        let fields = expected
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = source
            .prepare(&format!("SELECT {fields} FROM {}", quote(table)))
            .map_err(sql_error)?;
        let mut rows = statement.query([]).map_err(sql_error)?;
        while let Some(row) = rows.next().map_err(sql_error)? {
            let values = (0..expected.len())
                .map(|i| row.get::<_, SqlValue>(i))
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_error)?;
            if values
                .iter()
                .any(|v| matches!(v, SqlValue::Text(s) if s.contains(REDACTED)))
            {
                return Err("filtered watchlist requires manual recovery".into());
            }
            target
                .execute(
                    &format!(
                        "INSERT INTO {} ({fields}) VALUES ({})",
                        quote(table),
                        vec!["?"; values.len()].join(",")
                    ),
                    rusqlite::params_from_iter(values),
                )
                .map_err(sql_error)?;
        }
    }
    let count: i64 = target
        .query_row("SELECT count(*) FROM watchlist_state", [], |r| r.get(0))
        .map_err(sql_error)?;
    if count != 1 {
        return Err("invalid watchlist state".into());
    }
    // Never re-run stale localStorage migration after recovery. Advance the revision
    // so a client holding the exported revision must reload before submitting edits.
    let revision: i64 = target
        .query_row("SELECT revision FROM watchlist_state", [], |r| r.get(0))
        .map_err(sql_error)?;
    if revision >= 9_007_199_254_740_990 {
        return Err("watchlist revision out of range".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "invalid system clock")?
        .as_millis() as i64;
    target
        .execute(
            "UPDATE watchlist_state SET migration_complete=1, restore_pristine=0, revision=?1",
            [now.max(revision + 1)],
        )
        .map_err(sql_error)?;
    target.execute_batch("COMMIT;").map_err(sql_error)?;
    Ok(target)
}
/// Explicit owner marker, never inferred from an empty rowset or operation IDs.
fn pristine_watchlist(source: &Connection) -> Result<bool, String> {
    let version: i64 = source
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql_error)?;
    if version != 2 {
        return Ok(false);
    }
    // Validates version, exact recognized tables/columns, values and constraints.
    let _candidate = watchlist_candidate(source)?;
    let marked: i64 = source
        .query_row(
            "SELECT count(*) FROM watchlist_state WHERE singleton=1 AND restore_pristine=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    let rows: i64 = source
        .query_row("SELECT count(*) FROM watchlist", [], |r| r.get(0))
        .map_err(sql_error)?;
    Ok(marked == 1 && rows == 0)
}
fn restore_pristine_watchlist(path: &Path, incoming: &Connection) -> Result<(), String> {
    let mut current =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(sql_error)?;
    current
        .busy_timeout(Duration::from_secs(5))
        .map_err(sql_error)?;
    current
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")
        .map_err(sql_error)?;
    let tx = current
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    // Recheck inside the write transaction: preview is never authority to mutate.
    if !pristine_watchlist(&tx)? {
        return Err("current watchlist is not explicitly pristine".into());
    }
    for (table, columns) in [
        ("watchlist", WATCHLIST_COLUMNS),
        ("watchlist_operations", OPERATIONS_COLUMNS),
    ] {
        let fields = columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = incoming
            .prepare(&format!("SELECT {fields} FROM {}", quote(table)))
            .map_err(sql_error)?;
        let mut rows = statement.query([]).map_err(sql_error)?;
        while let Some(row) = rows.next().map_err(sql_error)? {
            let values = (0..columns.len())
                .map(|i| row.get::<_, SqlValue>(i))
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_error)?;
            // Startup migration receipts remain authoritative on operation-ID collision.
            let suffix = if table == "watchlist_operations" {
                " ON CONFLICT(operation_id) DO NOTHING"
            } else {
                ""
            };
            tx.execute(
                &format!(
                    "INSERT INTO {} ({fields}) VALUES ({}){suffix}",
                    quote(table),
                    vec!["?"; values.len()].join(",")
                ),
                rusqlite::params_from_iter(values),
            )
            .map_err(sql_error)?;
        }
    }
    let old_revision: i64 = tx
        .query_row(
            "SELECT revision FROM watchlist_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    let incoming_revision: i64 = incoming
        .query_row(
            "SELECT revision FROM watchlist_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if old_revision >= 9_007_199_254_740_990 || incoming_revision >= 9_007_199_254_740_990 {
        return Err("watchlist revision out of range".into());
    }
    let changed=tx.execute("UPDATE watchlist_state SET restore_pristine=0,migration_complete=1,revision=?1 WHERE singleton=1 AND restore_pristine=1",[old_revision.max(incoming_revision)+1]).map_err(sql_error)?;
    if changed != 1 {
        return Err("pristine marker changed".into());
    }
    tx.commit().map_err(sql_error)
}
fn destination_absent(path: &Path) -> Result<bool, String> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let candidate = PathBuf::from(format!("{}{suffix}", path.to_string_lossy()));
        match fs::symlink_metadata(candidate) {
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(true)
}
/// Atomically publish a complete, synced file WITHOUT replacement. A concurrent
/// owner opening/creating the destination wins. No copy/delete/rename fallback.
fn install_absent(source: &Path, destination: &Path) -> Result<bool, String> {
    if !destination_absent(destination)? {
        return Err("current database or sidecars preserved".into());
    }
    fs::hard_link(source, destination)
        .map_err(|_| "empty-store install refused; current data preserved")?;
    #[cfg(unix)]
    {
        // A sync failure occurs after installation: callers must report the partial
        // durability result, NOT return a misleading no-mutation error.
        return Ok(
            fs::File::open(destination.parent().ok_or("missing parent")?)
                .and_then(|f| f.sync_all())
                .is_ok(),
        );
    }
    #[cfg(not(unix))]
    Ok(fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)
        .and_then(|f| f.sync_all())
        .is_ok())
}
fn native_candidate(id: &str, source: &Connection) -> Result<Connection, String> {
    match id {
        "watchlist" => watchlist_candidate(source),
        "agent_ledger" => agent_candidate(source),
        _ => Err("owner-provided recovery schema required".into()),
    }
}
const AGENT_RUN_COLUMNS: &[&str] = &[
    "run_id",
    "conversation_id",
    "question",
    "mode",
    "status",
    "started_at_epoch_ms",
    "completed_at_epoch_ms",
    "duration_ms",
    "request_json",
    "events_json",
    "result_json",
    "error",
];
const AGENT_DELETED_COLUMNS: &[&str] = &["conversation_id", "deleted_at_epoch_ms"];
fn check_agent_schema(source: &Connection) -> Result<(), String> {
    let version: i64 = source
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql_error)?;
    let objects: Vec<(String,String)> = source.prepare("SELECT name,type FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND type != 'index' ORDER BY name").map_err(sql_error)?
        .query_map([], |r| Ok((r.get(0)?,r.get(1)?))).map_err(sql_error)?.collect::<Result<_, _>>().map_err(sql_error)?;
    if version != 2
        || objects
            != [
                ("agent_deleted_conversations".into(), "table".into()),
                ("agent_runs".into(), "table".into()),
            ]
    {
        return Err("unsupported agent schema or deletion history".into());
    }
    for (table, expected) in [
        ("agent_runs", AGENT_RUN_COLUMNS),
        ("agent_deleted_conversations", AGENT_DELETED_COLUMNS),
    ] {
        let names: Vec<String> = source
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
            .map_err(sql_error)?
            .query_map([], |r| r.get(1))
            .map_err(sql_error)?
            .collect::<Result<_, _>>()
            .map_err(sql_error)?;
        if names != expected {
            return Err("unknown agent columns".into());
        }
    }
    Ok(())
}
fn agent_candidate(source: &Connection) -> Result<Connection, String> {
    check_agent_schema(source)?;
    let target = Connection::open_in_memory().map_err(sql_error)?;
    target.execute_batch("BEGIN;
        CREATE TABLE agent_runs (run_id TEXT PRIMARY KEY,conversation_id TEXT,question TEXT NOT NULL,mode TEXT NOT NULL,status TEXT NOT NULL,started_at_epoch_ms INTEGER NOT NULL,completed_at_epoch_ms INTEGER,duration_ms INTEGER,request_json TEXT NOT NULL,events_json TEXT NOT NULL DEFAULT '[]',result_json TEXT,error TEXT);
        CREATE TABLE agent_deleted_conversations (conversation_id TEXT PRIMARY KEY,deleted_at_epoch_ms INTEGER NOT NULL);
        CREATE INDEX idx_agent_runs_started ON agent_runs(started_at_epoch_ms DESC,run_id DESC);
        CREATE INDEX idx_agent_runs_conversation ON agent_runs(conversation_id,started_at_epoch_ms DESC);
        PRAGMA user_version=2;").map_err(sql_error)?;
    for (table, columns) in [
        ("agent_deleted_conversations", AGENT_DELETED_COLUMNS),
        ("agent_runs", AGENT_RUN_COLUMNS),
    ] {
        let mut statement = source
            .prepare(&format!(
                "SELECT {} FROM {}",
                columns
                    .iter()
                    .map(|c| quote(c))
                    .collect::<Vec<_>>()
                    .join(","),
                quote(table)
            ))
            .map_err(sql_error)?;
        let mut rows = statement.query([]).map_err(sql_error)?;
        while let Some(row) = rows.next().map_err(sql_error)? {
            let values = (0..columns.len())
                .map(|i| row.get::<_, SqlValue>(i))
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_error)?;
            // IDs are required for replay and tombstone semantics; never recover ambiguous IDs.
            for index in if table == "agent_runs" {
                &[0, 1][..]
            } else {
                &[0][..]
            } {
                if !matches!(&values[*index],SqlValue::Text(id) if !id.trim().is_empty() && id.len()<=256 && !id.contains(REDACTED))
                {
                    return Err("invalid agent recovery identity".into());
                }
            }
            target
                .execute(
                    &format!(
                        "INSERT INTO {} VALUES ({})",
                        quote(table),
                        vec!["?"; values.len()].join(",")
                    ),
                    rusqlite::params_from_iter(values),
                )
                .map_err(sql_error)?;
        }
    }
    target.execute_batch("DELETE FROM agent_runs WHERE conversation_id IN (SELECT conversation_id FROM agent_deleted_conversations);
        UPDATE agent_runs SET status='unknown' WHERE status='running'; COMMIT;").map_err(sql_error)?;
    Ok(target)
}
fn receipt_path(root: &Path, blob: &str, store: &str) -> Result<PathBuf, String> {
    spec(store)?;
    let digest = Sha256::digest(blob.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    checked_path(
        root,
        &format!("user-backup-recovery/receipt-{digest}-{store}.json"),
    )
}
fn claim_receipt(path: &Path) -> Result<bool, String> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            // Intent is durable BEFORE mutation. A crash leaves an ambiguous intent
            // which blocks retries: recovery never guesses that no rows were imported.
            file.write_all(b"{\"state\":\"attempt_pending\",\"retry_allowed\":false}")
                .and_then(|_| file.sync_all())
                .map_err(io_error)?;
            #[cfg(unix)]
            {
                fs::File::open(path.parent().ok_or("receipt parent missing")?)
                    .and_then(|f| f.sync_all())
                    .map_err(io_error)?;
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}
fn merge_agent_rows(path: &Path, incoming: &Connection) -> Result<(), String> {
    // Read/write without CREATE. A missing/corrupt/unknown current store is never initialized.
    let mut current =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(sql_error)?;
    current
        .busy_timeout(Duration::from_secs(5))
        .map_err(sql_error)?;
    current
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")
        .map_err(sql_error)?;
    let tx = current
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    check_agent_schema(&tx)?;
    // Existing live conversations win over incoming tombstones; existing tombstones
    // win over every incoming run. This never deletes or overwrites a current row.
    let mut statement = incoming
        .prepare("SELECT conversation_id,deleted_at_epoch_ms FROM agent_deleted_conversations")
        .map_err(sql_error)?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(sql_error)?;
    for row in rows {
        let (id, at) = row.map_err(sql_error)?;
        tx.execute(
            "INSERT INTO agent_deleted_conversations SELECT ?1,?2
            WHERE NOT EXISTS(SELECT 1 FROM agent_runs WHERE conversation_id=?1)
            AND NOT EXISTS(SELECT 1 FROM agent_deleted_conversations WHERE conversation_id=?1)",
            rusqlite::params![id, at],
        )
        .map_err(sql_error)?;
    }
    let mut statement = incoming
        .prepare(&format!(
            "SELECT {} FROM agent_runs",
            AGENT_RUN_COLUMNS.join(",")
        ))
        .map_err(sql_error)?;
    let mut rows = statement.query([]).map_err(sql_error)?;
    while let Some(row) = rows.next().map_err(sql_error)? {
        let values = (0..AGENT_RUN_COLUMNS.len())
            .map(|i| row.get::<_, SqlValue>(i))
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO agent_runs SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12
            WHERE NOT EXISTS(SELECT 1 FROM agent_runs WHERE run_id=?1)
            AND NOT EXISTS(SELECT 1 FROM agent_deleted_conversations WHERE conversation_id=?2)",
            rusqlite::params_from_iter(values),
        )
        .map_err(sql_error)?;
    }
    tx.commit().map_err(sql_error)
}
fn apply_recovery(
    root: &Path,
    blob: &str,
    recovery: &Path,
    store: &mut BackupStorePreview,
    merge: bool,
) -> Result<bool, String> {
    let path = checked_path(root, spec(&store.id)?.relative_path)?;
    let source = open_readonly(&recovery.join(format!("{}.sqlite", store.id)))?;
    let candidate = native_candidate(&store.id, &source)?;
    let scratch = Scratch::new(root)?;
    let candidate_path = scratch.0.join("native.sqlite");
    snapshot(&candidate, &candidate_path)?;
    validate(&candidate_path)?;
    let receipt = receipt_path(root, blob, &store.id)?;
    store.receipt_id = receipt
        .file_name()
        .map(|s| s.to_string_lossy().into_owned());
    if !claim_receipt(&receipt)? {
        store.restore_status = "already_attempted_receipt".into();
        return Ok(false);
    }
    let result = if merge {
        merge_agent_rows(&path, &candidate).map(|_| true)
    } else if store.id == "watchlist" && path.try_exists().map_err(io_error)? {
        restore_pristine_watchlist(&path, &candidate).map(|_| true)
    } else {
        fs::create_dir_all(path.parent().ok_or("missing store directory")?).map_err(io_error)?;
        install_absent(&candidate_path, &path)
    };
    match result {
        Ok(durable) => {
            store.restore_status = if durable {
                if merge {
                    "merged_current_wins"
                } else {
                    "imported"
                }
            } else {
                "imported_durability_unconfirmed"
            }
            .into();
            store.current = "present_valid".into();
            store.conflict = true;
            let receipt_bytes=serde_json::to_vec(&serde_json::json!({"state":store.restore_status,"store":store.id,"retry_allowed":false})).map_err(|_|"receipt encoding failed")?;
            // The pending receipt remains sufficient to block duplicate resurrection.
            if write(&receipt, &receipt_bytes).is_err() {
                store.restore_status = "imported_receipt_pending".into();
            }
            Ok(true)
        }
        Err(_) => {
            store.restore_status = "attempt_blocked_receipt_retained".into();
            Ok(false)
        }
    }
}
fn recover_backup(
    root: &Path,
    blob: &str,
    password: &str,
    merge: bool,
) -> Result<BackupPreview, String> {
    let mut result = inspect_backup(root, blob, password, true)?;
    let recovery = checked_path(
        root,
        &format!(
            "user-backup-recovery/{}",
            result.recovery_id.as_deref().ok_or("missing recovery id")?
        ),
    )?;
    for store in &mut result.stores {
        let eligible = if merge {
            store.id == "agent_ledger" && store.merge_available
        } else {
            store.restore_to_empty
        };
        if !eligible {
            continue;
        }
        match apply_recovery(root, blob, &recovery, store, merge) {
            Ok(true) => result.restored_stores.push(store.id.clone()),
            Ok(false) => {}
            Err(_) => store.restore_status = "not_imported_validation_or_io_failure".into(),
        }
        store.restore_to_empty = false;
        store.merge_available = false;
    }
    result.imported = !result.restored_stores.is_empty();
    result.state = if !result.imported {
        "preview_only"
    } else if result.restored_stores.len() == result.stores.len() {
        "imported"
    } else {
        "partially_imported"
    };
    result.restoration_note="Only restored_stores were applied. Other stores are unchanged. Receipt intents block retries even after deletion; pending receipts require manual inspection. Reload recovered views before editing.".into();
    if let Ok(bytes) = serde_json::to_vec(&result) {
        if write(&recovery.join("result.json"), &bytes).is_err() {
            result
                .restoration_note
                .push_str(" Recovery result file could not be persisted.");
        }
    }
    Ok(result)
}
fn restore_empty_backup(root: &Path, blob: &str, password: &str) -> Result<BackupPreview, String> {
    recover_backup(root, blob, password, false)
}
fn merge_agent_backup(root: &Path, blob: &str, password: &str) -> Result<BackupPreview, String> {
    recover_backup(root, blob, password, true)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gp-backup-test-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn database(&self, relative: &str) -> Connection {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let conn = Connection::open(path).unwrap();
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT, metadata_json TEXT, api_key TEXT); INSERT INTO notes VALUES (1, 'kept', '{\"nested\":{\"apiKey\":\"nested-secret\",\"ok\":true}}', 'column-secret'); CREATE TABLE credentials (token TEXT); INSERT INTO credentials VALUES ('table-secret'); CREATE TABLE tombstones (id TEXT); INSERT INTO tombstones VALUES ('deleted');").unwrap();
            conn
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    const PASSWORD: &str = "correct horse battery staple";
    fn agent_v2(fixture: &Fixture) -> Connection {
        let path = fixture.0.join("agent/agent-runs.sqlite");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA user_version=2; PRAGMA journal_mode=WAL;
          CREATE TABLE agent_runs (run_id TEXT PRIMARY KEY,conversation_id TEXT,question TEXT NOT NULL,mode TEXT NOT NULL,status TEXT NOT NULL,started_at_epoch_ms INTEGER NOT NULL,completed_at_epoch_ms INTEGER,duration_ms INTEGER,request_json TEXT NOT NULL,events_json TEXT NOT NULL DEFAULT '[]',result_json TEXT,error TEXT);
          CREATE TABLE agent_deleted_conversations (conversation_id TEXT PRIMARY KEY,deleted_at_epoch_ms INTEGER NOT NULL);
          INSERT INTO agent_runs VALUES ('r1','c1','old question','quick','running',1,NULL,NULL,'{}','[]',NULL,NULL);").unwrap();
        db
    }
    #[test]
    fn agent_native_export_restore_and_once_receipt_prevent_reimport_after_clear() {
        let source = Fixture::new();
        let _live = agent_v2(&source);
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let restored = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert_eq!(restored.restored_stores, vec!["agent_ledger"]);
        let path = target.0.join("agent/agent-runs.sqlite");
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT status FROM agent_runs", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "unknown"
        );
        assert!(db
            .execute(
                "INSERT INTO agent_runs (run_id) VALUES ('missing-required')",
                []
            )
            .is_err());
        drop(db);
        fs::remove_file(&path).unwrap();
        let second = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(!second.imported);
        assert!(!path.exists());
        assert_eq!(second.stores[0].restore_status, "already_attempted_receipt");
    }
    #[test]
    fn agent_merge_is_current_wins_and_respects_both_deletion_sets() {
        let source = Fixture::new();
        let incoming = agent_v2(&source);
        incoming.execute_batch("INSERT INTO agent_runs SELECT 'r2','deleted-current','must not resurrect',mode,status,started_at_epoch_ms,completed_at_epoch_ms,duration_ms,request_json,events_json,result_json,error FROM agent_runs WHERE run_id='r1';
          INSERT INTO agent_runs SELECT 'r3','fresh','recover me',mode,status,started_at_epoch_ms,completed_at_epoch_ms,duration_ms,request_json,events_json,result_json,error FROM agent_runs WHERE run_id='r1';
          INSERT INTO agent_deleted_conversations VALUES ('deleted-backup',2),('c1',2);").unwrap();
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let current = agent_v2(&target);
        current.execute_batch("UPDATE agent_runs SET question='current question'; INSERT INTO agent_deleted_conversations VALUES ('deleted-current',3)").unwrap();
        let result = merge_agent_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(result.imported);
        assert_eq!(
            current
                .query_row(
                    "SELECT question FROM agent_runs WHERE run_id='r1'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "current question"
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT count(*) FROM agent_runs WHERE conversation_id='deleted-current'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT count(*) FROM agent_runs WHERE run_id='r3'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(current.query_row("SELECT count(*) FROM agent_deleted_conversations WHERE conversation_id='deleted-backup'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        current
            .execute("DELETE FROM agent_runs WHERE run_id='r3'", [])
            .unwrap();
        assert!(
            !merge_agent_backup(&target.0, &exported.blob_base64, PASSWORD)
                .unwrap()
                .imported
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT count(*) FROM agent_runs WHERE run_id='r3'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn mixed_backup_reports_partial_import_and_preserves_unsupported_stores() {
        let source = Fixture::new();
        let _watch = watchlist_v1(&source);
        let _research = source.database("research/research.sqlite");
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let result = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert_eq!(result.state, "partially_imported");
        assert_eq!(result.restored_stores, vec!["watchlist"]);
        assert!(!target.0.join("research/research.sqlite").exists());
    }
    #[test]
    fn pending_receipt_blocks_retry_and_native_archive_keeps_constraints() {
        let source = Fixture::new();
        let _live = watchlist_v1(&source);
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let payload = decrypt_archive(&exported.blob_base64, PASSWORD).unwrap();
        let copy = source.0.join("export-inspection.sqlite");
        write(
            &copy,
            &STANDARD.decode(&payload.stores[0].sqlite_base64).unwrap(),
        )
        .unwrap();
        let db = Connection::open(&copy).unwrap();
        assert!(db
            .execute("INSERT INTO watchlist(code) VALUES ('invalid')", [])
            .is_err());
        let target = Fixture::new();
        fs::create_dir(target.0.join("user-backup-recovery")).unwrap();
        let receipt = receipt_path(&target.0, &exported.blob_base64, "watchlist").unwrap();
        assert!(claim_receipt(&receipt).unwrap());
        let result = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(!result.imported);
        assert_eq!(result.stores[0].restore_status, "already_attempted_receipt");
        assert!(!target.0.join("watchlist/watchlist.sqlite").exists());
    }
    #[test]
    fn newer_native_schema_is_staged_but_never_installed() {
        let source = Fixture::new();
        let live = watchlist_v1(&source);
        live.pragma_update(None, "user_version", 99).unwrap();
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let result = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(!result.imported);
        assert!(result.staged);
        assert!(!result.stores[0].restore_to_empty);
        assert!(!target.0.join("watchlist/watchlist.sqlite").exists());
    }
    #[test]
    fn restores_transactionally_into_explicitly_pristine_initialized_watchlist() {
        let source = Fixture::new();
        let _live = watchlist_v1(&source);
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let current = watchlist_v1(&target);
        current.execute_batch("ALTER TABLE watchlist_state ADD COLUMN restore_pristine INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=2;
            DELETE FROM watchlist; DELETE FROM watchlist_operations;
            INSERT INTO watchlist_operations VALUES ('empty-startup','{\"mutation\":{\"kind\":\"migrate\",\"items\":[]}}','{}');
            UPDATE watchlist_state SET migration_complete=1,restore_pristine=1;").unwrap();
        let preview = inspect_backup(&target.0, &exported.blob_base64, PASSWORD, false).unwrap();
        assert!(preview.stores[0].restore_to_empty);
        assert_eq!(
            preview.stores[0].restore_status,
            "eligible_pristine_watchlist_v2"
        );
        let result = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(result.imported);
        // The original open handle sees the restored data: this was a transaction,
        // not replacement of an active file or its WAL.
        assert_eq!(
            current
                .query_row("SELECT count(*) FROM watchlist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            current
                .query_row("SELECT restore_pristine FROM watchlist_state", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT count(*) FROM watchlist_operations WHERE operation_id='empty-startup'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            current
                .query_row(
                    "SELECT count(*) FROM watchlist WHERE code='deleted'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn pristine_restore_rechecks_marker_and_rows_after_preview() {
        let fixture = Fixture::new();
        let db = watchlist_v1(&fixture);
        let candidate = watchlist_candidate(&db).unwrap();
        db.execute_batch(
            "ALTER TABLE watchlist_state ADD COLUMN restore_pristine INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=2;
            DELETE FROM watchlist; UPDATE watchlist_state SET restore_pristine=1;",
        )
        .unwrap();
        assert!(pristine_watchlist(&db).unwrap());
        // Simulate a user clear after preview: empty is still authoritative.
        db.execute("UPDATE watchlist_state SET restore_pristine=0", [])
            .unwrap();
        assert!(restore_pristine_watchlist(
            &fixture.0.join("watchlist/watchlist.sqlite"),
            &candidate
        )
        .is_err());
        assert_eq!(
            db.query_row("SELECT count(*) FROM watchlist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        // A wrongly-stale marker cannot override a new current row either.
        db.execute_batch("UPDATE watchlist_state SET restore_pristine=1; INSERT INTO watchlist VALUES ('current',NULL,NULL,'now',NULL,NULL,1)").unwrap();
        assert!(!pristine_watchlist(&db).unwrap());
        assert!(restore_pristine_watchlist(
            &fixture.0.join("watchlist/watchlist.sqlite"),
            &candidate
        )
        .is_err());
    }
    fn watchlist_v1(fixture: &Fixture) -> Connection {
        let path = fixture.0.join("watchlist/watchlist.sqlite");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA user_version=1; PRAGMA journal_mode=WAL;
          CREATE TABLE watchlist (code TEXT PRIMARY KEY NOT NULL, name TEXT, industry TEXT, added_at TEXT NOT NULL, source TEXT, screen_criteria_summary TEXT, updated_at INTEGER NOT NULL);
          CREATE TABLE watchlist_state (singleton INTEGER PRIMARY KEY, revision INTEGER, migration_complete INTEGER);
          CREATE TABLE watchlist_operations (operation_id TEXT PRIMARY KEY, payload TEXT NOT NULL, response TEXT NOT NULL);
          INSERT INTO watchlist VALUES ('kept','keep',NULL,'2026-10-06',NULL,NULL,1),('deleted','gone',NULL,'2026-10-06',NULL,NULL,1);
          DELETE FROM watchlist WHERE code='deleted';
          INSERT INTO watchlist_state VALUES (1,7,0);
          INSERT INTO watchlist_operations VALUES ('removal-op','{\"mutation\":{\"removes\":[\"deleted\"]}}','{\"revision\":7}');").unwrap();
        db
    }
    #[test]
    fn restore_to_absent_watchlist_preserves_deletions_and_installs_real_schema() {
        let source = Fixture::new();
        let _live = watchlist_v1(&source);
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let preview = inspect_backup(&target.0, &exported.blob_base64, PASSWORD, false).unwrap();
        assert!(preview.stores[0].restore_to_empty);
        let result = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(result.imported);
        assert_eq!(result.state, "imported");
        assert_eq!(result.restored_stores, vec!["watchlist"]);
        let db = Connection::open(target.0.join("watchlist/watchlist.sqlite")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM watchlist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM watchlist_operations WHERE operation_id='removal-op'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT migration_complete FROM watchlist_state", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        assert!(db
            .execute("INSERT INTO watchlist (code) VALUES ('broken')", [])
            .is_err());
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn restore_preserves_current_empty_store_tombstones_and_stale_sidecars() {
        let source = Fixture::new();
        let _live = watchlist_v1(&source);
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let current = watchlist_v1(&target);
        current.execute("DELETE FROM watchlist", []).unwrap();
        let result = restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD).unwrap();
        assert!(!result.imported);
        assert_eq!(result.state, "preview_only");
        assert_eq!(
            current
                .query_row("SELECT count(*) FROM watchlist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        let corrupt = Fixture::new();
        fs::create_dir(corrupt.0.join("watchlist")).unwrap();
        fs::write(
            corrupt.0.join("watchlist/watchlist.sqlite"),
            b"keep corrupt original",
        )
        .unwrap();
        assert!(
            !restore_empty_backup(&corrupt.0, &exported.blob_base64, PASSWORD)
                .unwrap()
                .imported
        );
        assert_eq!(
            fs::read(corrupt.0.join("watchlist/watchlist.sqlite")).unwrap(),
            b"keep corrupt original"
        );
        let sidecars = Fixture::new();
        fs::create_dir(sidecars.0.join("watchlist")).unwrap();
        fs::write(
            sidecars.0.join("watchlist/watchlist.sqlite-wal"),
            b"do not overwrite",
        )
        .unwrap();
        assert!(
            !restore_empty_backup(&sidecars.0, &exported.blob_base64, PASSWORD)
                .unwrap()
                .imported
        );
        assert!(!sidecars.0.join("watchlist/watchlist.sqlite").exists());
    }
    #[test]
    fn restore_refuses_unknown_schema_or_tombstone_tables_and_never_replaces_racing_file() {
        let source = Fixture::new();
        let live = watchlist_v1(&source);
        live.execute_batch("CREATE TABLE watchlist_tombstones (code TEXT); INSERT INTO watchlist_tombstones VALUES ('kept')").unwrap();
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        assert!(
            !restore_empty_backup(&target.0, &exported.blob_base64, PASSWORD)
                .unwrap()
                .imported
        );
        assert!(!target.0.join("watchlist/watchlist.sqlite").exists());
        let candidate = target.0.join("candidate.sqlite");
        let destination = target.0.join("current.sqlite");
        fs::write(&candidate, b"old").unwrap();
        fs::write(&destination, b"current").unwrap();
        assert!(install_absent(&candidate, &destination).is_err());
        assert_eq!(fs::read(destination).unwrap(), b"current");
    }
    #[test]
    fn nested_key_value_credentials_are_not_exported() {
        let mut value =
            serde_json::json!({"nested": [{"key": "api_key", "value": "must-not-leak"}]});
        clean_json(&mut value, 0, &mut 0);
        assert!(!value.to_string().contains("must-not-leak"));
    }

    #[test]
    fn wal_roundtrip_filters_secrets_and_stages_without_overwriting_current() {
        let fixture = Fixture::new();
        let live = fixture.database("watchlist/watchlist.sqlite");
        fs::write(fixture.0.join("settings.json"), "settings-secret").unwrap();
        let exported = export_backup(&fixture.0, PASSWORD).unwrap();
        assert_eq!(exported.stores.len(), 1);
        let payload = decrypt_archive(&exported.blob_base64, PASSWORD).unwrap();
        let bytes = STANDARD.decode(&payload.stores[0].sqlite_base64).unwrap();
        for secret in [
            "column-secret",
            "nested-secret",
            "table-secret",
            "settings-secret",
        ] {
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "leaked {secret}"
            );
        }
        live.execute("UPDATE notes SET body='new current value'", [])
            .unwrap();
        let before = fs::read(fixture.0.join("watchlist/watchlist.sqlite")).unwrap();
        let wal_before = fs::read(fixture.0.join("watchlist/watchlist.sqlite-wal")).unwrap();
        let preview = inspect_backup(&fixture.0, &exported.blob_base64, PASSWORD, false).unwrap();
        assert_eq!(preview.state, "preview_only");
        assert!(!preview.imported);
        assert!(!preview.staged);
        assert!(preview.stores[0].conflict);
        assert!(!fixture.0.join("user-backup-recovery").exists());
        let staged = inspect_backup(&fixture.0, &exported.blob_base64, PASSWORD, true).unwrap();
        assert!(staged.staged);
        assert!(!staged.imported);
        let path = fixture
            .0
            .join("user-backup-recovery")
            .join(staged.recovery_id.unwrap())
            .join("watchlist.sqlite");
        let recovered = Connection::open(&path).unwrap();
        assert_eq!(
            recovered
                .query_row("SELECT body FROM notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "kept"
        );
        assert_eq!(
            recovered
                .query_row("SELECT id FROM tombstones", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "deleted"
        );
        assert_eq!(
            live.query_row("SELECT body FROM notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "new current value"
        );
        assert_eq!(
            fs::read(fixture.0.join("watchlist/watchlist.sqlite")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(fixture.0.join("watchlist/watchlist.sqlite-wal")).unwrap(),
            wal_before
        );
    }

    #[test]
    fn password_tamper_version_and_size_fail_before_creating_directories() {
        let source = Fixture::new();
        let _live = source.database("watchlist/watchlist.sqlite");
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        assert!(inspect_backup(&target.0, &exported.blob_base64, "wrong password", true).is_err());
        let bytes = STANDARD.decode(&exported.blob_base64).unwrap();
        for offset in [10, 26, HEADER_BYTES, bytes.len() - 1] {
            let mut corrupt = bytes.clone();
            corrupt[offset] ^= 1;
            assert!(inspect_backup(&target.0, &STANDARD.encode(corrupt), PASSWORD, true).is_err());
        }
        let mut future = bytes;
        future[8] = 99;
        assert!(inspect_backup(&target.0, &STANDARD.encode(future), PASSWORD, true).is_err());
        assert!(decrypt_archive(&"x".repeat(MAX_BASE64_BYTES + 1), PASSWORD).is_err());
        assert_eq!(fs::read_dir(&target.0).unwrap().count(), 0);
    }

    #[test]
    fn authenticated_invalid_sqlite_and_unknown_or_duplicate_stores_never_stage() {
        let source = Fixture::new();
        let _live = source.database("watchlist/watchlist.sqlite");
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        let target = Fixture::new();
        let mut payload = decrypt_archive(&exported.blob_base64, PASSWORD).unwrap();
        payload.stores[0].sqlite_base64 = STANDARD.encode(b"not sqlite");
        let invalid = encrypt_archive(&payload, PASSWORD).unwrap();
        assert!(inspect_backup(&target.0, &invalid, PASSWORD, true).is_err());
        assert!(!target.0.join("user-backup-recovery").exists());
        payload.stores[0].id = "../../settings".into();
        assert!(validate_archive(&payload).is_err());
        payload.stores[0].id = "watchlist".into();
        payload.stores.push(payload.stores[0].clone());
        assert!(validate_archive(&payload).is_err());
    }

    #[test]
    fn includes_all_exact_store_paths_and_marks_absence() {
        let source = Fixture::new();
        let live: Vec<_> = STORE_SPECS
            .iter()
            .map(|s| source.database(s.relative_path))
            .collect();
        let exported = export_backup(&source.0, PASSWORD).unwrap();
        assert_eq!(exported.stores.len(), 5);
        let target = Fixture::new();
        let preview = inspect_backup(&target.0, &exported.blob_base64, PASSWORD, false).unwrap();
        assert!(preview
            .stores
            .iter()
            .all(|s| s.current == "absent" && !s.conflict));
        assert_eq!(live.len(), 5);
    }

    #[test]
    fn sanitizes_key_value_rows_json_strings_and_opaque_credentials() {
        let source = Connection::open_in_memory().unwrap();
        source.execute_batch("CREATE TABLE state (key TEXT, value TEXT); INSERT INTO state VALUES ('access_token','secret'),('theme','dark'); CREATE TABLE data (metadata_json TEXT, auth BLOB, text TEXT); INSERT INTO data VALUES ('{\"request\":\"{\\\"password\\\":\\\"secret\\\"}\"}',X'010203','Authorization: Bearer secret');").unwrap();
        let (clean, stats) = sanitized_database(&source).unwrap();
        assert_eq!(
            clean
                .query_row("SELECT count(*) FROM state", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let json: String = clean
            .query_row("SELECT metadata_json FROM data", [], |r| r.get(0))
            .unwrap();
        assert!(!json.contains("secret"));
        assert!(stats.filtered_fields >= 4);
    }

    #[test]
    fn random_salt_and_nonce_make_exports_distinct_and_errors_never_echo_password() {
        let source = Fixture::new();
        let _live = source.database("watchlist/watchlist.sqlite");
        let one = export_backup(&source.0, PASSWORD).unwrap();
        let two = export_backup(&source.0, PASSWORD).unwrap();
        assert_ne!(one.blob_base64, two.blob_base64);
        let error = decrypt_archive(&one.blob_base64, "do-not-echo-this")
            .err()
            .unwrap();
        assert!(!error.contains("do-not-echo-this"));
        assert!(export_backup(&source.0, "short").is_err());
    }
}

// Tauri adapters: parent registers these commands; no extra filesystem permissions.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    passphrase: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    blob_base64: String,
    passphrase: String,
}
impl Drop for ExportRequest {
    fn drop(&mut self) {
        self.passphrase.zeroize();
    }
}
impl Drop for ImportRequest {
    fn drop(&mut self) {
        self.passphrase.zeroize();
    }
}

#[tauri::command]
pub async fn api_user_backup_export(
    app: tauri::AppHandle,
    payload: ExportRequest,
) -> Result<BackupExport, String> {
    use tauri::Manager;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "backup data directory unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = OPERATION
            .try_lock()
            .map_err(|_| "another backup operation is running")?;
        export_backup(&root, &payload.passphrase)
    })
    .await
    .map_err(|_| "backup worker failed")?
}
#[tauri::command]
pub async fn api_user_backup_preview(
    app: tauri::AppHandle,
    payload: ImportRequest,
) -> Result<BackupPreview, String> {
    run_import(app, payload, false).await
}
#[tauri::command]
pub async fn api_user_backup_stage(
    app: tauri::AppHandle,
    payload: ImportRequest,
) -> Result<BackupPreview, String> {
    run_import(app, payload, true).await
}
async fn run_import(
    app: tauri::AppHandle,
    payload: ImportRequest,
    stage: bool,
) -> Result<BackupPreview, String> {
    use tauri::Manager;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "backup data directory unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = OPERATION
            .try_lock()
            .map_err(|_| "another backup operation is running")?;
        inspect_backup(&root, &payload.blob_base64, &payload.passphrase, stage)
    })
    .await
    .map_err(|_| "backup worker failed")?
}

#[tauri::command]
pub async fn api_user_backup_restore_empty(
    app: tauri::AppHandle,
    payload: ImportRequest,
) -> Result<BackupPreview, String> {
    use tauri::Manager;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "backup data directory unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = OPERATION
            .try_lock()
            .map_err(|_| "another backup operation is running")?;
        restore_empty_backup(&root, &payload.blob_base64, &payload.passphrase)
    })
    .await
    .map_err(|_| "backup worker failed")?
}

#[tauri::command]
pub async fn api_user_backup_merge_agent(
    app: tauri::AppHandle,
    payload: ImportRequest,
) -> Result<BackupPreview, String> {
    use tauri::Manager;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "backup data directory unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = OPERATION
            .try_lock()
            .map_err(|_| "another backup operation is running")?;
        merge_agent_backup(&root, &payload.blob_base64, &payload.passphrase)
    })
    .await
    .map_err(|_| "backup worker failed")?
}
