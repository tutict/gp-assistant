//! Crash-safe local file commits and consistent SQLite snapshots. No delete-before-rename.
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn unique_temporary(path: &Path) -> Result<(PathBuf, File), String> {
    let parent = path.parent().ok_or("target has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("create storage directory: {e}"))?;
    let name = path
        .file_name()
        .ok_or("target has no filename")?
        .to_string_lossy();
    for _ in 0..64 {
        let temporary = parent.join(format!(
            ".{name}.{}-{}.pending",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create temporary file: {error}")),
        }
    }
    Err("could not reserve a unique temporary file".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitPhase {
    Written,
    Synced,
    Committed,
}

/// Success means the new file was synced and replaced in its own directory.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write_with_hook(path, bytes, |_| Ok(()))
}
pub(crate) fn atomic_write_json(path: &Path, bytes: &[u8]) -> Result<(), String> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|e| format!("invalid JSON snapshot: {e}"))?;
    atomic_write(path, bytes)
}
fn atomic_write_with_hook(
    path: &Path,
    bytes: &[u8],
    mut hook: impl FnMut(CommitPhase) -> Result<(), String>,
) -> Result<(), String> {
    let (temporary, mut file) = unique_temporary(path)?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|e| format!("write temporary file: {e}"))?;
        hook(CommitPhase::Written)?;
        file.sync_all()
            .map_err(|e| format!("sync temporary file: {e}"))?;
        drop(file);
        if fs::read(&temporary).map_err(|e| format!("verify temporary file: {e}"))? != bytes {
            return Err("temporary file verification failed".into());
        }
        hook(CommitPhase::Synced)?;
        replace_file(&temporary, path)?;
        sync_parent(path)?;
        hook(CommitPhase::Committed)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Stream an immutable, already validated snapshot without buffering a database in RAM.
pub(crate) fn atomic_copy(source: &Path, destination: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let mut input = File::open(source).map_err(|e| format!("open snapshot: {e}"))?;
    let (temporary, mut output) = unique_temporary(destination)?;
    let result = (|| {
        let mut buffer = [0u8; 64 * 1024];
        let mut original = Sha256::new();
        loop {
            let count = input
                .read(&mut buffer)
                .map_err(|e| format!("read snapshot: {e}"))?;
            if count == 0 {
                break;
            }
            original.update(&buffer[..count]);
            output
                .write_all(&buffer[..count])
                .map_err(|e| format!("write snapshot: {e}"))?;
        }
        output
            .sync_all()
            .map_err(|e| format!("sync snapshot: {e}"))?;
        drop(output);
        let mut verify = File::open(&temporary).map_err(|e| e.to_string())?;
        let mut copied = Sha256::new();
        loop {
            let count = verify.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            copied.update(&buffer[..count]);
        }
        drop(verify);
        if original.finalize() != copied.finalize() {
            return Err("snapshot copy checksum mismatch".into());
        }
        replace_file(&temporary, destination)?;
        sync_parent(destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Same-volume replacement with write-through. No copy fallback and no deletion gap.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        return Err(format!(
            "atomic file replacement failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|e| format!("atomic file replacement failed: {e}"))
}
#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), String> {
    File::open(path.parent().ok_or("target has no parent")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("sync storage directory: {e}"))
}
#[cfg(not(unix))]
fn sync_parent(_: &Path) -> Result<(), String> {
    Ok(())
}

pub(crate) fn configure_user_connection(connection: &Connection) -> Result<(), String> {
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
        .map_err(|e| format!("configure durable user database: {e}"))
}
/// A successful SQL statement is not necessarily a successful checkpoint (SQLITE_BUSY).
pub(crate) fn checkpoint(connection: &Connection) -> Result<(), String> {
    let (busy, total, done): (i64, i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| format!("checkpoint failed: {e}"))?;
    if busy != 0 || (total >= 0 && done < total) {
        return Err("database is busy; checkpoint did not complete".into());
    }
    Ok(())
}
pub(crate) fn validate_sqlite_header(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| format!("cannot inspect database: {e}"))?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header)
        .map_err(|_| "database header is incomplete; original preserved".to_string())?;
    if &header != b"SQLite format 3\0" {
        return Err("invalid database header; original preserved".into());
    }
    Ok(())
}
pub(crate) fn validate_sqlite(path: &Path) -> Result<(), String> {
    validate_sqlite_header(path)?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("cannot open database for validation: {e}"))?;
    let result: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| format!("database integrity check failed: {e}"))?;
    if result != "ok" {
        return Err("database integrity check failed; original preserved".into());
    }
    Ok(())
}
/// Preserve existing tables before a schema-changing migration; never overwrite an earlier backup.
pub(crate) fn backup_before_migration(
    connection: &Connection,
    path: &Path,
    current: i64,
    target: i64,
) -> Result<(), String> {
    if current >= target {
        return Ok(());
    }
    let has_tables: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if !has_tables {
        return Ok(());
    }
    let backup = path.with_extension(format!(
        "before-v{target}-{}-{}.sqlite",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    if backup.exists() {
        return Err("migration snapshot already exists; refusing to overwrite".into());
    }
    snapshot_sqlite(connection, &backup)
}
/// VACUUM INTO takes a SQLite-consistent snapshot, including committed WAL pages.
pub(crate) fn snapshot_sqlite(connection: &Connection, destination: &Path) -> Result<(), String> {
    let (temporary, file) = unique_temporary(destination)?;
    drop(file);
    let result = (|| {
        connection
            .execute("VACUUM INTO ?1", [temporary.to_string_lossy().as_ref()])
            .map_err(|e| format!("create consistent database snapshot: {e}"))?;
        validate_sqlite(&temporary)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temporary)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("sync database snapshot: {e}"))?;
        replace_file(&temporary, destination)?;
        sync_parent(destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[test]
    fn invalid_market_snapshot_never_replaces_last_good_data() {
        let root =
            std::env::temp_dir().join(format!("gp-durability-invalid-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("market.json");
        let temporary = root.join("market.tmp");
        fs::write(&path, br#"{"stocks":[]}"#).unwrap();
        let result = crate::market::write_mobile_market_data_once(&temporary, &path, b"{truncated");
        assert!(
            result.is_err(),
            "invalid JSON must fail before replacing the valid snapshot"
        );
        assert_eq!(fs::read(&path).unwrap(), br#"{"stocks":[]}"#);
        let _ = fs::remove_dir_all(&root);
    }
    #[test]
    fn failures_around_commit_leave_a_whole_old_or_new_file() {
        use super::*;
        let root =
            std::env::temp_dir().join(format!("gp-durability-phases-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        for phase in [
            CommitPhase::Written,
            CommitPhase::Synced,
            CommitPhase::Committed,
        ] {
            atomic_write(&path, b"old").unwrap();
            assert!(atomic_write_with_hook(&path, b"new", |at| if at == phase {
                Err("injected failure".into())
            } else {
                Ok(())
            })
            .is_err());
            let current = fs::read(&path).unwrap();
            assert_eq!(
                current,
                if phase == CommitPhase::Committed {
                    b"new"
                } else {
                    b"old"
                }
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn snapshot_includes_committed_wal_without_copying_live_files() {
        use super::*;
        let root =
            std::env::temp_dir().join(format!("gp-durability-snapshot-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let live = Connection::open(root.join("live.sqlite")).unwrap();
        configure_user_connection(&live).unwrap();
        live.execute_batch("CREATE TABLE IF NOT EXISTS test (value TEXT); DELETE FROM test; INSERT INTO test VALUES ('saved');").unwrap();
        let copy = root.join("backup.sqlite");
        snapshot_sqlite(&live, &copy).unwrap();
        let reader = Connection::open(&copy).unwrap();
        assert_eq!(
            reader
                .query_row("SELECT value FROM test", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "saved"
        );
        drop(reader);
        drop(live);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corruption_is_reported_without_replacing_original() {
        use super::*;
        let path = std::env::temp_dir().join(format!(
            "gp-durability-corrupt-{}.sqlite",
            std::process::id()
        ));
        fs::write(&path, b"not a database").unwrap();
        assert!(validate_sqlite(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not a database");
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn crash_writer_child() {
        let Ok(directory) = std::env::var("GP_DURABILITY_TEST_ROOT") else {
            return;
        };
        let phase = std::env::var("GP_DURABILITY_TEST_PHASE").unwrap();
        let path = std::path::PathBuf::from(directory).join("state.json");
        super::atomic_write_with_hook(&path, br#"{"version":2}"#, |at| {
            if format!("{at:?}") == phase {
                std::process::exit(79);
            }
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn process_exit_at_each_commit_stage_preserves_valid_generation() {
        use super::*;
        let root = std::env::temp_dir().join(format!("gp-durability-crash-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        for phase in [
            CommitPhase::Written,
            CommitPhase::Synced,
            CommitPhase::Committed,
        ] {
            atomic_write(&path, br#"{"version":1}"#).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "durability::tests::crash_writer_child"])
                .env("GP_DURABILITY_TEST_ROOT", &root)
                .env("GP_DURABILITY_TEST_PHASE", format!("{phase:?}"))
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(79));
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                value["version"],
                if phase == CommitPhase::Committed {
                    2
                } else {
                    1
                }
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn empty_file_is_not_a_recovery_database() {
        let path =
            std::env::temp_dir().join(format!("gp-durability-empty-{}.sqlite", std::process::id()));
        fs::write(&path, []).unwrap();
        let result = super::validate_sqlite(&path);
        fs::remove_file(path).unwrap();
        assert!(
            result.is_err(),
            "SQLite accepts zero bytes as empty DB but recovery must reject it"
        );
    }
    #[test]
    fn busy_checkpoint_fails_without_removing_wal() {
        use super::*;
        let root = std::env::temp_dir().join(format!("gp-durability-busy-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("live.sqlite");
        let writer = Connection::open(&path).unwrap();
        configure_user_connection(&writer).unwrap();
        writer
            .execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES(1);")
            .unwrap();
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN; SELECT * FROM t;").unwrap();
        writer.execute("INSERT INTO t VALUES(2)", []).unwrap();
        writer.busy_timeout(Duration::ZERO).unwrap();
        assert!(checkpoint(&writer).is_err());
        assert!(root.join("live.sqlite-wal").exists());
        reader.execute_batch("ROLLBACK;").unwrap();
        checkpoint(&writer).unwrap();
        drop(reader);
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }
}
