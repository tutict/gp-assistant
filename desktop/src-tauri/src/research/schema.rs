use crate::research::time::normalized_research_timestamp;
use rusqlite::Connection;

pub(crate) const RESEARCH_SCHEMA_VERSION: i64 = 3;

pub(in crate::research) fn initialize_schema(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("failed to read research schema version: {error}"))?;
    if version > RESEARCH_SCHEMA_VERSION {
        return Err(format!(
            "research database schema {version} is newer than supported version {RESEARCH_SCHEMA_VERSION}"
        ));
    }
    configure_connection(connection)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| format!("failed to begin research migration: {e}"))?;
    let connection: &Connection = &transaction;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS research_metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS documents (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 content TEXT NOT NULL,
                 source_tier TEXT NOT NULL,
                 source_name TEXT NOT NULL,
                 url TEXT,
                 published_at TEXT,
                 imported_at_epoch_ms INTEGER NOT NULL,
                 updated_at_epoch_ms INTEGER NOT NULL,
                 content_hash TEXT NOT NULL,
                 user_imported INTEGER NOT NULL DEFAULT 0,
                 remote_export_allowed INTEGER NOT NULL DEFAULT 1,
                 pinned INTEGER NOT NULL DEFAULT 0,
                 cited_count INTEGER NOT NULL DEFAULT 0,
                 metadata_json TEXT NOT NULL DEFAULT '{}'
             );
             CREATE TABLE IF NOT EXISTS chunks (
                 id TEXT PRIMARY KEY,
                 document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 ordinal INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 content TEXT NOT NULL,
                 page_number INTEGER,
                 stock_codes_json TEXT NOT NULL DEFAULT '[]',
                 entities_json TEXT NOT NULL DEFAULT '[]',
                 relation_types_json TEXT NOT NULL DEFAULT '[]',
                 sentiment TEXT NOT NULL DEFAULT 'uncertain',
                 source_tier TEXT NOT NULL,
                 source_name TEXT NOT NULL,
                 url TEXT,
                 published_at TEXT,
                 content_hash TEXT NOT NULL,
                 created_at_epoch_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_chunks_document ON chunks(document_id, ordinal);
             CREATE INDEX IF NOT EXISTS idx_chunks_published ON chunks(published_at DESC);
             CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                 chunk_id UNINDEXED,
                 title_terms,
                 entity_terms,
                 body_terms,
                 tokenize='unicode61'
             );
             CREATE TABLE IF NOT EXISTS embeddings (
                 chunk_id TEXT PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
                 model_id TEXT NOT NULL,
                 dimensions INTEGER NOT NULL,
                 vector BLOB NOT NULL,
                 normalized INTEGER NOT NULL DEFAULT 1,
                 content_hash TEXT NOT NULL,
                 created_at_epoch_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS research_messages (
                 id TEXT PRIMARY KEY,
                 document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 stock_code TEXT,
                 title TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 sentiment TEXT NOT NULL,
                 source_tier TEXT NOT NULL,
                 published_at TEXT,
                 unread INTEGER NOT NULL DEFAULT 1,
                 created_at_epoch_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_messages_stock ON research_messages(stock_code, unread);
             CREATE TABLE IF NOT EXISTS research_threads (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 stock_code TEXT,
                 created_at_epoch_ms INTEGER NOT NULL,
                 updated_at_epoch_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS research_answers (
                 id TEXT PRIMARY KEY,
                 thread_id TEXT REFERENCES research_threads(id) ON DELETE CASCADE,
                 question TEXT NOT NULL,
                 answer TEXT NOT NULL,
                 mode TEXT NOT NULL,
                 created_at_epoch_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS research_answer_citations (
                 answer_id TEXT NOT NULL REFERENCES research_answers(id) ON DELETE CASCADE,
                 citation_id TEXT NOT NULL,
                 document_id TEXT NOT NULL REFERENCES documents(id),
                 chunk_id TEXT NOT NULL REFERENCES chunks(id),
                 ordinal INTEGER NOT NULL,
                 lexical_score REAL NOT NULL DEFAULT 0,
                 vector_score REAL,
                 retrieval_score REAL NOT NULL DEFAULT 0,
                 PRIMARY KEY(answer_id, citation_id)
             );
             CREATE TABLE IF NOT EXISTS research_unavailable_citations (
                 answer_id TEXT NOT NULL REFERENCES research_answers(id) ON DELETE CASCADE,
                 citation_id TEXT NOT NULL,
                 ordinal INTEGER NOT NULL,
                 payload_json TEXT NOT NULL,
                 PRIMARY KEY(answer_id, citation_id)
             );
             PRAGMA user_version = 3;
             ",
        )
        .map_err(|error| format!("failed to initialize research schema: {error}"))?;
    let remote_permission_column_added = ensure_column(
        connection,
        "documents",
        "remote_export_allowed",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    if remote_permission_column_added {
        connection.execute(
            "UPDATE documents SET remote_export_allowed = CASE WHEN user_imported = 1 THEN 0 ELSE 1 END",
            [],
        ).map_err(|error| format!("failed to apply remote evidence migration default: {error}"))?;
    }
    ensure_column(
        connection,
        "research_answer_citations",
        "lexical_score",
        "REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "research_answer_citations",
        "vector_score",
        "REAL",
    )?;
    ensure_column(
        connection,
        "research_answer_citations",
        "retrieval_score",
        "REAL NOT NULL DEFAULT 0",
    )?;
    transaction
        .pragma_update(None, "user_version", RESEARCH_SCHEMA_VERSION)
        .map_err(|e| e.to_string())?;
    transaction
        .commit()
        .map_err(|e| format!("failed to commit research migration: {e}"))?;
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("failed to inspect {table}: {error}"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to query {table} columns: {error}"))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    drop(statement);
    if !exists {
        connection
            .execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition};"
            ))
            .map_err(|error| format!("failed to add {table}.{column}: {error}"))?;
    }
    Ok(!exists)
}

pub(in crate::research) fn configure_connection(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if !(0..=RESEARCH_SCHEMA_VERSION).contains(&version) {
        return Err(format!(
            "research database schema {version} is unsupported; original preserved"
        ));
    }
    // One normalization function keeps SQL filtering, vector caches and frozen
    // evidence comparisons at the same precision, before any candidate limit.
    use rusqlite::functions::FunctionFlags;
    connection
        .create_scalar_function(
            "research_timestamp_utc",
            1,
            FunctionFlags::SQLITE_UTF8
                | FunctionFlags::SQLITE_DETERMINISTIC
                | FunctionFlags::SQLITE_INNOCUOUS,
            |context| {
                let value = context.get::<Option<String>>(0)?;
                Ok(value
                    .as_deref()
                    .and_then(|value| normalized_research_timestamp(value, false)))
            },
        )
        .map_err(|error| format!("failed to register research timestamp normalization: {error}"))?;
    crate::durability::configure_user_connection(connection)
}
