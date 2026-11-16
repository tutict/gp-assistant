use crate::durability;
use rusqlite::Connection;
use std::fs;
use std::io::Read;
use std::path::Path;

pub(in crate::research) fn read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("cannot read research database: {e}"))
}

pub(in crate::research) fn validate_database(path: &Path) -> Result<(), String> {
    validate_header(path)?; // SQLite considers a zero-byte file a valid empty DB; we must not.
    durability::validate_sqlite(path)?;
    let connection = read_only(path)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
         ('research_metadata','documents','chunks','research_threads','research_answers','research_messages','research_answer_citations')",
        [], |row| row.get(0)
    ).map_err(|e| e.to_string())?;
    let legacy_v2_documents_only = version == 2
        && connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='documents'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())?
            == 1;
    if count != 7 && !legacy_v2_documents_only {
        return Err("unrecognized research schema; original preserved".into());
    }
    let foreign_key_error = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| e.to_string())?
        .exists([])
        .map_err(|e| e.to_string())?;
    if foreign_key_error {
        return Err("research foreign key validation failed; original preserved".into());
    }
    Ok(())
}

pub(in crate::research) fn validate_header(path: &Path) -> Result<(), String> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("cannot inspect research database: {e}"))?;
    let mut header = [0; 16];
    if file.metadata().map_err(|e| e.to_string())?.len() < 100
        || file.read_exact(&mut header).is_err()
        || &header != b"SQLite format 3\0"
    {
        return Err(format!(
            "invalid research database {}; original preserved",
            path.display()
        ));
    }
    Ok(())
}
