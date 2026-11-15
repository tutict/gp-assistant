use crate::research::app_store::open_app_store;
use crate::research::app_store::recover_research_files;
use crate::research::app_store::with_app_store_for_market;
use crate::research::app_store::with_exclusive_app_database;
use crate::research::recovery;
use crate::research::sources::legacy_documents_from_file;
use crate::research::storage::documents::string_field;
use crate::research::storage::ResearchStore;
use crate::research::time::epoch_millis;
use crate::research::time::unique_suffix;
use crate::research::validation;
use rusqlite::params;
use rusqlite::Connection;
use serde_json::json;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use tauri::Manager;

const RESEARCH_PACK_SCHEMA_VERSION: i64 = 2;

const RESEARCH_PACK_FORMAT: &str = "gp-research-pack-v2";

pub(in crate::research) const MAX_PACK_BYTES: u64 = 64 * 1024 * 1024;

pub(in crate::research) const MAX_PACK_DOCUMENTS: usize = 50_000;

pub(in crate::research) const MAX_PACK_DOCUMENT_CHARS: usize = 4 * 1024 * 1024;

impl ResearchStore {
    pub(crate) fn export_portable_pack(&self, destination: &Path) -> Result<Value, String> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create pack directory: {error}"))?;
        }
        let temporary = destination.with_extension(format!(
            "sqlite.tmp-{}-{}",
            epoch_millis(),
            unique_suffix()
        ));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .map_err(|error| format!("failed to clear stale pack temporary file: {error}"))?;
        }
        let documents = self.portable_documents()?;
        validate_portable_documents(&documents)?;
        let mut pack = Connection::open(&temporary)
            .map_err(|error| format!("failed to create SQLite v2 pack: {error}"))?;
        pack.execute_batch(
            "PRAGMA journal_mode = DELETE;
             PRAGMA synchronous = FULL;
             CREATE TABLE pack_metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE pack_documents (
                 document_id TEXT PRIMARY KEY,
                 payload_json TEXT NOT NULL
             );
             PRAGMA user_version = 2;",
        )
        .map_err(|error| format!("failed to initialize SQLite v2 pack: {error}"))?;
        let transaction = pack
            .transaction()
            .map_err(|error| format!("failed to start SQLite v2 pack: {error}"))?;
        transaction
            .execute(
                "INSERT INTO pack_metadata (key, value) VALUES
                    ('schema_version', '2'),
                    ('format', 'gp-research-pack-v2'),
                    ('contains_vectors', 'false'),
                    ('contains_chat_history', 'false')",
                [],
            )
            .map_err(|error| format!("failed to write pack metadata: {error}"))?;
        for document in &documents {
            let document_id = string_field(document, "document_id")
                .ok_or_else(|| "portable document is missing document_id".to_string())?;
            transaction
                .execute(
                    "INSERT INTO pack_documents (document_id, payload_json) VALUES (?1, ?2)",
                    params![document_id, document.to_string()],
                )
                .map_err(|error| format!("failed to write portable document: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit SQLite v2 pack: {error}"))?;
        pack.execute_batch("VACUUM;")
            .map_err(|error| format!("failed to finalize SQLite v2 pack: {error}"))?;
        drop(pack);
        let temporary_bytes = fs::metadata(&temporary)
            .map_err(|error| format!("failed to inspect SQLite v2 pack: {error}"))?
            .len();
        if temporary_bytes > MAX_PACK_BYTES {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "exported research pack exceeds the {} byte safety limit",
                MAX_PACK_BYTES
            ));
        }

        let previous = destination.with_extension("sqlite.previous");
        if previous.exists() {
            fs::remove_file(&previous)
                .map_err(|error| format!("failed to replace previous portable pack: {error}"))?;
        }
        if destination.exists() {
            fs::rename(destination, &previous)
                .map_err(|error| format!("failed to preserve previous portable pack: {error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, destination) {
            if previous.exists() && !destination.exists() {
                let _ = fs::rename(&previous, destination);
            }
            return Err(format!("failed to publish SQLite v2 pack: {error}"));
        }
        let bytes = fs::metadata(destination)
            .map(|value| value.len())
            .unwrap_or(0);
        Ok(json!({
            "schema_version": RESEARCH_PACK_SCHEMA_VERSION,
            "format": RESEARCH_PACK_FORMAT,
            "path": destination.display().to_string(),
            "document_count": documents.len(),
            "bytes": bytes,
            "contains_vectors": false,
            "contains_chat_history": false
        }))
    }
}

impl ResearchStore {
    #[cfg(test)]
    pub(crate) fn import_portable_pack(&self, path: &Path) -> Result<Value, String> {
        let documents = read_portable_pack(path)?;
        let result = self.ingest_documents(&documents)?;
        Ok(json!({
            "schema_version": RESEARCH_PACK_SCHEMA_VERSION,
            "format": RESEARCH_PACK_FORMAT,
            "document_count": documents.len(),
            "imported": result,
            "fts_rebuilt": true,
            "vectors_rebuild_required": cfg!(target_os = "windows")
        }))
    }
}

impl ResearchStore {
    fn portable_documents(&self) -> Result<Vec<Value>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT d.id, d.title, d.content, d.source_tier, d.source_name,
                        d.url, d.published_at, d.user_imported, d.remote_export_allowed,
                        d.pinned, d.metadata_json, c.page_number, c.stock_codes_json, c.entities_json,
                        c.relation_types_json, c.sentiment
                 FROM documents d
                 LEFT JOIN chunks c ON c.id = (
                    SELECT first_chunk.id FROM chunks first_chunk
                    WHERE first_chunk.document_id = d.id
                    ORDER BY first_chunk.ordinal LIMIT 1
                 )
                 ORDER BY d.imported_at_epoch_ms, d.id",
            )
            .map_err(|error| format!("failed to prepare portable documents: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<u32>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                ))
            })
            .map_err(|error| format!("failed to query portable documents: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read portable documents: {error}"))?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    document_id,
                    title,
                    content,
                    source_tier,
                    source_name,
                    url,
                    published_at,
                    user_imported,
                    remote_export_allowed,
                    pinned,
                    metadata_json,
                    page_number,
                    stock_codes_json,
                    entities_json,
                    relation_types_json,
                    sentiment,
                )| {
                    json!({
                        "document_id": document_id,
                        "title": title,
                        "content": content,
                        "source_tier": source_tier,
                        "source_name": source_name,
                        "url": url,
                        "published_at": published_at,
                        "user_imported": user_imported != 0,
                        "remote_export_allowed": remote_export_allowed != 0,
                        "pinned": pinned != 0,
                        "metadata": serde_json::from_str::<Value>(&metadata_json).unwrap_or_else(|_| json!({})),
                        "page_number": page_number,
                        "stock_codes": stock_codes_json.and_then(|value| serde_json::from_str::<Value>(&value).ok()).unwrap_or_else(|| json!([])),
                        "entities": entities_json.and_then(|value| serde_json::from_str::<Value>(&value).ok()).unwrap_or_else(|| json!([])),
                        "relation_types": relation_types_json.and_then(|value| serde_json::from_str::<Value>(&value).ok()).unwrap_or_else(|| json!([])),
                        "sentiment": sentiment.unwrap_or_else(|| "uncertain".to_string())
                    })
                },
            )
            .collect())
    }
}

pub(crate) fn export_app_pack(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
    export_app_pack_for_market(app, payload, "cn")
}

pub(crate) fn export_app_pack_for_market(
    app: &tauri::AppHandle,
    payload: &Value,
    market: &str,
) -> Result<Value, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let destination = string_field(payload, "path")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            app_data
                .join("research")
                .join("exports")
                .join(if market == "hk" {
                    "research-pack-hk-v2.sqlite"
                } else {
                    "research-pack-v2.sqlite"
                })
        });
    with_app_store_for_market(app, market, |store| {
        store.export_portable_pack(&destination)
    })
}

pub(crate) fn import_app_pack(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
    with_exclusive_app_database(|| {
        let result = import_app_pack_unlocked(app, payload)?;
        open_app_store(app)?.reset_sentiment_generation()?;
        Ok(result)
    })
}

pub(crate) fn import_app_pack_for_market(
    app: &tauri::AppHandle,
    payload: &Value,
    market: &str,
) -> Result<Value, String> {
    if market == "cn" {
        return import_app_pack(app, payload);
    }
    if market != "hk" {
        return Err(format!("unsupported research market: {market}"));
    }
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let research_dir = app_data.join("research");
    fs::create_dir_all(&research_dir)
        .map_err(|error| format!("failed to create research directory: {error}"))?;
    let mut temporary_source = None;
    let source_path = if let Some(path) = string_field(payload, "path") {
        PathBuf::from(path)
    } else if let Some(encoded) = string_field(payload, "bytes_base64") {
        use base64::{engine::general_purpose, Engine as _};
        if encoded.len() > (MAX_PACK_BYTES as usize * 4 / 3 + 4) {
            return Err("research pack exceeds the 64 MB safety limit".to_string());
        }
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| format!("invalid research pack base64: {error}"))?;
        if bytes.len() as u64 > MAX_PACK_BYTES {
            return Err("research pack exceeds the 64 MB safety limit".to_string());
        }
        let path = research_dir.join(format!(
            "incoming-hk-pack-{}-{}.sqlite",
            epoch_millis(),
            unique_suffix()
        ));
        crate::durability::atomic_write(&path, &bytes)?;
        temporary_source = Some(path.clone());
        path
    } else {
        return Err("path or bytes_base64 is required".to_string());
    };
    let result = read_portable_pack(&source_path).and_then(|documents| {
        with_app_store_for_market(app, market, |store| {
            let imported = store.ingest_documents(&documents)?;
            store.reset_sentiment_generation()?;
            Ok(json!({
                "schema_version": RESEARCH_PACK_SCHEMA_VERSION,
                "format": RESEARCH_PACK_FORMAT,
                "document_count": documents.len(),
                "imported": imported,
                "fts_rebuilt": true,
                "vectors_rebuild_required": cfg!(target_os = "windows")
            }))
        })
    });
    if let Some(path) = temporary_source {
        let _ = fs::remove_file(path);
    }
    result
}

fn import_app_pack_unlocked(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let research_dir = app_data.join("research");
    fs::create_dir_all(&research_dir)
        .map_err(|error| format!("failed to create research directory: {error}"))?;
    recover_research_files(&research_dir)?;
    let mut temporary_source = None;
    let source_path = if let Some(path) = string_field(payload, "path") {
        PathBuf::from(path)
    } else if let Some(encoded) = string_field(payload, "bytes_base64") {
        use base64::{engine::general_purpose, Engine as _};
        if encoded.len() > (MAX_PACK_BYTES as usize * 4 / 3 + 4) {
            return Err("research pack exceeds the 64 MB safety limit".to_string());
        }
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| format!("invalid research pack base64: {error}"))?;
        let path = research_dir.join(format!(
            "incoming-pack-{}-{}.sqlite",
            epoch_millis(),
            unique_suffix()
        ));
        crate::durability::atomic_write(&path, &bytes)?;
        temporary_source = Some(path.clone());
        path
    } else {
        return Err("path or bytes_base64 is required".to_string());
    };

    let result = read_portable_pack(&source_path)
        .and_then(|documents| import_research_documents(&research_dir, &documents));
    if let Some(path) = temporary_source {
        let _ = fs::remove_file(path);
    }
    result
}

pub(in crate::research) fn import_research_documents(
    research_dir: &Path,
    documents: &[Value],
) -> Result<Value, String> {
    if documents.is_empty() {
        return Err("research pack does not contain any documents".to_string());
    }
    recovery::deliver(research_dir, |staging_path| {
        let staging = ResearchStore::open(staging_path)?;
        staging.ingest_documents(documents)?;
        preserve_local_research_state(&research_dir.join("research.sqlite"), &staging)?;
        staging.connection()?.execute(
            "INSERT OR REPLACE INTO research_metadata(key,value) VALUES('legacy_v1_import_complete','true')", []
        ).map_err(|error| error.to_string())?;
        staging.rebuild_fts().map(|_| ())
    })?;
    Ok(json!({
        "schema_version": RESEARCH_PACK_SCHEMA_VERSION,
        "format": RESEARCH_PACK_FORMAT,
        "document_count": documents.len(),
        "imported": true,
        "fts_rebuilt": true,
        "vectors_rebuild_required": cfg!(target_os = "windows"),
        "rollback_available": true
    }))
}

pub(in crate::research) fn preserve_local_research_state(
    previous_path: &Path,
    staging: &ResearchStore,
) -> Result<(), String> {
    // The source must exist: a missing authoritative DB is not an empty history.
    validation::validate_database(previous_path)?;
    let previous = ResearchStore::open(previous_path)?;
    let source = previous.connection()?;
    let mut destination = staging.connection()?;
    destination
        .execute(
            "ATTACH DATABASE ?1 AS previous_research",
            params![previous_path.to_string_lossy()],
        )
        .map_err(|error| format!("failed to attach previous research database: {error}"))?;
    let result = (|| {
        let transaction = destination
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction.execute_batch(
            "DELETE FROM research_unavailable_citations;
             DELETE FROM research_answer_citations;
             DELETE FROM research_answers;
             DELETE FROM research_threads;
             INSERT INTO research_threads
                 SELECT id,title,stock_code,created_at_epoch_ms,updated_at_epoch_ms FROM previous_research.research_threads;
             INSERT INTO research_answers
                 SELECT id,thread_id,question,answer,mode,created_at_epoch_ms FROM previous_research.research_answers;
             INSERT INTO research_answer_citations
                 (answer_id,citation_id,document_id,chunk_id,ordinal,lexical_score,vector_score,retrieval_score)
                 SELECT ac.answer_id,ac.citation_id,ac.document_id,ac.chunk_id,ac.ordinal,ac.lexical_score,ac.vector_score,ac.retrieval_score
                 FROM previous_research.research_answer_citations ac
                 JOIN previous_research.chunks old ON old.id=ac.chunk_id AND old.document_id=ac.document_id
                 JOIN previous_research.documents od ON od.id=old.document_id
                 JOIN chunks fresh ON fresh.id=old.id AND fresh.document_id=old.document_id
                 JOIN documents fd ON fd.id=fresh.document_id
                 WHERE fresh.content_hash=old.content_hash AND fresh.content=old.content
                   AND fd.content_hash=od.content_hash AND fd.content=od.content
                   AND fresh.title=old.title AND fresh.source_tier=old.source_tier
                   AND fresh.source_name=old.source_name AND fresh.url IS old.url
                   AND fresh.published_at IS old.published_at AND fresh.page_number IS old.page_number;
             UPDATE research_messages SET unread=COALESCE((
                 SELECT old.unread FROM previous_research.research_messages old
                 WHERE old.id=research_messages.id AND old.document_id=research_messages.document_id
             ),1);
             UPDATE documents SET cited_count=(SELECT COUNT(*) FROM research_answer_citations ac WHERE ac.document_id=documents.id);
             INSERT OR REPLACE INTO research_metadata(key,value)
                 SELECT key,value FROM previous_research.research_metadata
                 WHERE key IN ('legacy_v1_import_complete','legacy_v1_archive_complete');"
        ).map_err(|error| format!("failed to replace local research state: {error}"))?;
        let mut answers = source
            .prepare("SELECT id FROM research_answers")
            .map_err(|error| error.to_string())?;
        let ids = answers
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        for id in ids {
            let id = id.map_err(|error| error.to_string())?;
            for (ordinal, mut citation) in previous
                .answer_citations(&source, &id)?
                .into_iter()
                .enumerate()
            {
                let retained: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM research_answer_citations WHERE answer_id=?1 AND citation_id=?2)",
                    params![id, citation.citation_id], |row| row.get(0)
                ).map_err(|error| error.to_string())?;
                if !retained {
                    citation.unavailable = true;
                    transaction.execute(
                        "INSERT INTO research_unavailable_citations(answer_id,citation_id,ordinal,payload_json) VALUES(?1,?2,?3,?4)",
                        params![id, citation.citation_id, ordinal as i64, serde_json::to_string(&citation).map_err(|error| error.to_string())?]
                    ).map_err(|error| error.to_string())?;
                }
            }
        }
        transaction.commit().map_err(|error| error.to_string())
    })();
    let detach = destination.execute_batch("DETACH DATABASE previous_research;");
    result?;
    detach.map_err(|error| format!("failed to detach previous research database: {error}"))
}

pub(crate) fn rollback_app_pack(app: &tauri::AppHandle) -> Result<Value, String> {
    with_exclusive_app_database(|| {
        let result = rollback_app_pack_unlocked(app)?;
        open_app_store(app)?.reset_sentiment_generation()?;
        Ok(result)
    })
}

fn rollback_app_pack_unlocked(app: &tauri::AppHandle) -> Result<Value, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let research_dir = app_data.join("research");
    rollback_research_documents(&research_dir)
}

pub(in crate::research) fn rollback_research_documents(
    research_dir: &Path,
) -> Result<Value, String> {
    recover_research_files(research_dir)?;
    let rollback_path = recovery::rollback_source(research_dir)?;
    validation::validate_database(&rollback_path)?;
    recovery::deliver(research_dir, |staging_path| {
        let old =
            Connection::open_with_flags(&rollback_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|error| error.to_string())?;
        crate::durability::snapshot_sqlite(&old, staging_path)?;
        drop(old);
        let staging = ResearchStore::open(staging_path)?;
        // Discard every old user row before copying CURRENT authority, including deletions.
        preserve_local_research_state(&research_dir.join("research.sqlite"), &staging)?;
        staging.rebuild_fts().map(|_| ())
    })?;
    Ok(
        json!({"rolled_back":true, "replaced_database":recovery::rollback_source(research_dir)?.display().to_string()}),
    )
}

pub(in crate::research) fn remove_sqlite_files(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            format!(
                "failed to remove stale SQLite file {}: {error}",
                path.display()
            )
        })?;
    }
    remove_sqlite_sidecars(path)
}

pub(in crate::research) fn remove_sqlite_sidecars(path: &Path) -> Result<(), String> {
    let base = path.display().to_string();
    for sidecar in [format!("{base}-wal"), format!("{base}-shm")] {
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            fs::remove_file(&sidecar).map_err(|error| {
                format!(
                    "failed to remove SQLite sidecar {}: {error}",
                    sidecar.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(in crate::research) fn validate_legacy_pack_version(value: &Value) -> Result<(), String> {
    let supported = match value.get("schema_version") {
        Some(Value::Number(version)) => version.as_u64() == Some(1),
        Some(Value::String(version)) => matches!(
            version.as_str(),
            "1" | "rag-pack-tauri-v1" | "upstream-rag-pack-v1" | "upstream-rag-pack-tauri-v1"
        ),
        _ => false,
    };
    if !supported {
        return Err("unsupported legacy research pack schema; expected v1".to_string());
    }
    Ok(())
}

pub(in crate::research) fn validate_legacy_candidate_size(item: &Value) -> Result<(), String> {
    let Some(object) = item.as_object() else {
        return Ok(());
    };
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("旧版研究资料")
        .trim();
    let content = object
        .get("content")
        .or_else(|| object.get("text"))
        .or_else(|| object.get("evidence_text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("summary")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|summary| format!("{title}\n{summary}"))
        });
    if content.is_some_and(|content| content.chars().count() > MAX_PACK_DOCUMENT_CHARS) {
        return Err("legacy research pack contains an oversized document".to_string());
    }
    Ok(())
}

pub(in crate::research) fn validate_portable_documents(documents: &[Value]) -> Result<(), String> {
    if documents.len() > MAX_PACK_DOCUMENTS {
        return Err(format!(
            "research pack contains {} documents; the limit is {MAX_PACK_DOCUMENTS}",
            documents.len()
        ));
    }
    let mut total_chars = 0usize;
    for document in documents {
        let content = document
            .get("content")
            .or_else(|| document.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content_chars = content.chars().count();
        if content_chars > MAX_PACK_DOCUMENT_CHARS {
            return Err("research pack contains an oversized document".to_string());
        }
        total_chars = total_chars.saturating_add(content_chars);
        if total_chars > MAX_PACK_BYTES as usize {
            return Err("research pack contains too much text".to_string());
        }
    }
    Ok(())
}

pub(in crate::research) fn read_portable_pack(path: &Path) -> Result<Vec<Value>, String> {
    let size = fs::metadata(path)
        .map_err(|error| {
            format!(
                "failed to inspect research pack {}: {error}",
                path.display()
            )
        })?
        .len();
    if size > MAX_PACK_BYTES {
        return Err("research pack exceeds the 64 MB safety limit".to_string());
    }
    let mut header = [0u8; 16];
    fs::File::open(path)
        .and_then(|mut file| file.read(&mut header))
        .map_err(|error| format!("failed to read research pack {}: {error}", path.display()))?;
    if !header.starts_with(b"SQLite format 3\0") {
        return legacy_documents_from_file(path);
    }
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("failed to open SQLite v2 pack: {error}"))?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("failed to read pack schema version: {error}"))?;
    if version != RESEARCH_PACK_SCHEMA_VERSION {
        return Err(format!(
            "unsupported research pack schema {version}; expected {RESEARCH_PACK_SCHEMA_VERSION}"
        ));
    }
    let format = connection
        .query_row(
            "SELECT value FROM pack_metadata WHERE key = 'format'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("research pack metadata is invalid: {error}"))?;
    if format != "gp-research-pack-v2" {
        return Err(format!("unsupported research pack format: {format}"));
    }
    let document_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM pack_documents", [], |row| row.get(0))
        .map_err(|error| format!("failed to count research pack documents: {error}"))?;
    if document_count < 0 || document_count as usize > MAX_PACK_DOCUMENTS {
        return Err("research pack contains too many documents".to_string());
    }
    let mut statement = connection
        .prepare("SELECT payload_json FROM pack_documents ORDER BY document_id")
        .map_err(|error| format!("failed to prepare research pack documents: {error}"))?;
    let payloads = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("failed to query research pack documents: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read research pack documents: {error}"))?;
    let mut total_chars = 0usize;
    payloads
        .into_iter()
        .map(|payload| {
            if payload.len() > MAX_PACK_DOCUMENT_CHARS * 2 {
                return Err("research pack contains an oversized document payload".to_string());
            }
            let value = serde_json::from_str::<Value>(&payload)
                .map_err(|error| format!("invalid document JSON in research pack: {error}"))?;
            let content = value
                .get("content")
                .or_else(|| value.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if content.chars().count() > MAX_PACK_DOCUMENT_CHARS {
                return Err("research pack contains an oversized document".to_string());
            }
            total_chars = total_chars.saturating_add(content.chars().count());
            if total_chars > MAX_PACK_BYTES as usize {
                return Err("research pack contains too much text".to_string());
            }
            Ok(value)
        })
        .collect()
}
