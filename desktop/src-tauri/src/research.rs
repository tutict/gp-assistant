use jieba_rs::Jieba;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering as AtomicOrdering},
        Arc, Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

pub(crate) const RESEARCH_SCHEMA_VERSION: i64 = 2;
const DEFAULT_CHUNK_CHARS: usize = 520;
const DEFAULT_CHUNK_OVERLAP: usize = 80;
const MAX_CITATIONS: usize = 8;
const MAX_PACK_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACK_DOCUMENTS: usize = 50_000;
const MAX_PACK_DOCUMENT_CHARS: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResearchMessage {
    pub id: String,
    pub document_id: String,
    pub stock_code: Option<String>,
    pub title: String,
    pub summary: String,
    pub sentiment: String,
    pub source_tier: String,
    pub published_at: Option<String>,
    pub unread: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResearchCitation {
    pub citation_id: String,
    pub document_id: String,
    pub chunk_id: String,
    pub title: String,
    pub excerpt: String,
    pub source_tier: String,
    pub source_name: String,
    pub published_at: Option<String>,
    pub url: Option<String>,
    pub page_number: Option<u32>,
    pub lexical_score: f64,
    pub vector_score: Option<f64>,
    pub retrieval_score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResearchAnswer {
    pub id: String,
    pub thread_id: Option<String>,
    pub mode: String,
    pub question: String,
    pub answer: String,
    pub citations: Vec<ResearchCitation>,
    pub created_at_epoch_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResearchThread {
    pub id: String,
    pub title: String,
    pub stock_code: Option<String>,
    pub created_at_epoch_ms: i64,
    pub updated_at_epoch_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct KnowledgeDocumentStatus {
    pub document_id: String,
    pub title: String,
    pub source_tier: String,
    pub chunk_count: usize,
    pub embedding_count: usize,
    pub indexed: bool,
    pub updated_at_epoch_ms: i64,
}

pub(crate) struct ResearchStore {
    path: PathBuf,
}

impl ResearchStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create research directory: {error}"))?;
        }
        let existed = path.exists();
        let registry = INITIALIZED_DATABASES.get_or_init(|| Mutex::new(HashSet::new()));
        let mut initialized = registry
            .lock()
            .map_err(|_| "research database initialization lock is poisoned".to_string())?;
        if !existed || !initialized.contains(&path) {
            let connection = Connection::open(&path)
                .map_err(|error| format!("failed to open research database: {error}"))?;
            initialize_schema(&connection)?;
            initialized.insert(path.clone());
        }
        Ok(Self { path })
    }

    pub(crate) fn ingest_documents(&self, documents: &[Value]) -> Result<Value, String> {
        if documents.is_empty() {
            return Ok(json!({"document_count": 0, "chunk_count": 0}));
        }
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start research import: {error}"))?;
        let imported_at = epoch_millis();
        let mut chunk_count = 0usize;

        for (document_index, document) in documents.iter().enumerate() {
            let mut document_id = string_field(document, "document_id")
                .unwrap_or_else(|| format!("research-doc-{imported_at}-{document_index}"));
            let title = string_field(document, "title").unwrap_or_else(|| "Untitled".to_string());
            let content = string_field(document, "content")
                .or_else(|| string_field(document, "text"))
                .unwrap_or_default();
            if content.trim().is_empty() {
                return Err(format!("document {document_id} has no text content"));
            }
            let source_tier = normalize_source_tier(
                string_field(document, "source_tier")
                    .as_deref()
                    .unwrap_or("news"),
            );
            let source_name = string_field(document, "source_name")
                .or_else(|| string_field(document, "source"))
                .unwrap_or_else(|| source_tier.clone());
            let published_at = string_field(document, "published_at");
            let url = string_field(document, "url");
            let stock_codes = normalized_string_array(document.get("stock_codes"));
            let entities = normalized_string_array(document.get("entities"));
            let relation_types = normalized_string_array(document.get("relation_types"));
            let sentiment =
                string_field(document, "sentiment").unwrap_or_else(|| "uncertain".to_string());
            let page_number = document
                .get("page_number")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok());
            let metadata_json = document
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string();
            let content_hash = sha256_hex(content.as_bytes());
            let existing_document = transaction
                .query_row(
                    "SELECT content_hash, cited_count FROM documents WHERE id = ?1",
                    params![document_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(|error| format!("failed to inspect existing document: {error}"))?;
            if let Some((existing_hash, cited_count)) = existing_document {
                if existing_hash != content_hash && cited_count > 0 {
                    let revision = content_hash.get(..12).unwrap_or(content_hash.as_str());
                    document_id = format!("{document_id}:revision:{revision}");
                }
            }
            let content_unchanged = transaction
                .query_row(
                    "SELECT content_hash FROM documents WHERE id = ?1",
                    params![document_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| format!("failed to inspect existing document: {error}"))?
                .as_deref()
                == Some(content_hash.as_str());

            transaction
                .execute(
                    "INSERT INTO documents (
                        id, title, content, source_tier, source_name, url, published_at,
                        imported_at_epoch_ms, updated_at_epoch_ms, content_hash, user_imported,
                        pinned, cited_count, metadata_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, ?11, 0, ?12)
                    ON CONFLICT(id) DO UPDATE SET
                        title=excluded.title, content=excluded.content,
                        source_tier=excluded.source_tier, source_name=excluded.source_name,
                        url=excluded.url, published_at=excluded.published_at,
                        updated_at_epoch_ms=excluded.updated_at_epoch_ms,
                        content_hash=excluded.content_hash,
                        user_imported=MAX(documents.user_imported, excluded.user_imported),
                        pinned=MAX(documents.pinned, excluded.pinned),
                        metadata_json=excluded.metadata_json",
                    params![
                        document_id,
                        title,
                        content,
                        source_tier,
                        source_name,
                        url,
                        published_at,
                        imported_at,
                        content_hash,
                        bool_field(document, "user_imported"),
                        bool_field(document, "pinned"),
                        metadata_json
                    ],
                )
                .map_err(|error| format!("failed to store research document: {error}"))?;

            if content_unchanged {
                continue;
            }

            transaction
                .execute(
                    "DELETE FROM chunks_fts WHERE chunk_id IN (
                        SELECT id FROM chunks WHERE document_id = ?1
                    )",
                    params![document_id],
                )
                .map_err(|error| format!("failed to clear research FTS rows: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM chunks WHERE document_id = ?1",
                    params![document_id],
                )
                .map_err(|error| format!("failed to replace research chunks: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM research_messages WHERE document_id = ?1",
                    params![document_id],
                )
                .map_err(|error| format!("failed to replace research messages: {error}"))?;

            let chunks = split_text(&content, DEFAULT_CHUNK_CHARS, DEFAULT_CHUNK_OVERLAP);
            for (ordinal, chunk_text) in chunks.iter().enumerate() {
                let chunk_id = format!("{document_id}:{ordinal}");
                let stock_codes_json = serde_json::to_string(&stock_codes)
                    .map_err(|error| format!("failed to encode stock codes: {error}"))?;
                let entities_json = serde_json::to_string(&entities)
                    .map_err(|error| format!("failed to encode entities: {error}"))?;
                let relation_types_json = serde_json::to_string(&relation_types)
                    .map_err(|error| format!("failed to encode relation types: {error}"))?;
                transaction
                    .execute(
                        "INSERT INTO chunks (
                            id, document_id, ordinal, title, content, page_number,
                            stock_codes_json, entities_json, relation_types_json, sentiment,
                            source_tier, source_name, url, published_at, content_hash,
                            created_at_epoch_ms
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                            ?14, ?15, ?16
                        )",
                        params![
                            chunk_id,
                            document_id,
                            ordinal as i64,
                            title,
                            chunk_text,
                            page_number,
                            stock_codes_json,
                            entities_json,
                            relation_types_json,
                            sentiment,
                            source_tier,
                            source_name,
                            url,
                            published_at,
                            sha256_hex(chunk_text.as_bytes()),
                            imported_at
                        ],
                    )
                    .map_err(|error| format!("failed to store research chunk: {error}"))?;
                transaction
                    .execute(
                        "INSERT INTO chunks_fts (chunk_id, title_terms, entity_terms, body_terms)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            chunk_id,
                            tokenize_for_fts(&title),
                            tokenize_for_fts(&entities.join(" ")),
                            tokenize_for_fts(chunk_text)
                        ],
                    )
                    .map_err(|error| format!("failed to index research chunk: {error}"))?;
                chunk_count += 1;
            }

            let message_stocks = if stock_codes.is_empty() {
                vec![None]
            } else {
                stock_codes.iter().cloned().map(Some).collect::<Vec<_>>()
            };
            for message_stock in message_stocks {
                let message_scope = message_stock.as_deref().unwrap_or("all");
                let message_id = format!("message:{document_id}:{message_scope}");
                transaction
                    .execute(
                        "INSERT INTO research_messages (
                            id, document_id, stock_code, title, summary, sentiment, source_tier,
                            published_at, unread, created_at_epoch_ms
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
                        params![
                            message_id,
                            document_id,
                            message_stock,
                            title,
                            excerpt(&content, 180),
                            sentiment,
                            source_tier,
                            published_at,
                            imported_at
                        ],
                    )
                    .map_err(|error| format!("failed to create research message: {error}"))?;
            }
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit research import: {error}"))?;
        if chunk_count > 0 {
            invalidate_vector_cache(&self.path);
        }
        let retention = self.prune_retention()?;
        Ok(json!({
            "document_count": documents.len(),
            "chunk_count": chunk_count,
            "schema_version": RESEARCH_SCHEMA_VERSION,
            "retention": retention
        }))
    }

    pub(crate) fn prune_retention(&self) -> Result<Value, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start research retention cleanup: {error}"))?;
        let old_document_ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT id FROM documents
                     WHERE source_tier IN ('news', 'community')
                       AND user_imported = 0 AND pinned = 0 AND cited_count = 0
                       AND published_at IS NOT NULL
                       AND datetime(published_at) < datetime('now', '-365 days')",
                )
                .map_err(|error| format!("failed to prepare retention candidates: {error}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("failed to query retention candidates: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to read retention candidates: {error}"))?;
            rows
        };
        let mut removed_documents = 0usize;
        let mut removed_chunks = 0usize;
        for document_id in old_document_ids {
            removed_chunks += delete_document(&transaction, &document_id)?;
            removed_documents += 1;
        }
        let mut chunk_count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|error| format!("failed to count retained chunks: {error}"))?;
        if chunk_count > 50_000 {
            let overflow_candidates = {
                let mut statement = transaction
                    .prepare(
                        "SELECT id FROM documents
                         WHERE source_tier IN ('news', 'community', 'research_report')
                           AND user_imported = 0 AND pinned = 0 AND cited_count = 0
                         ORDER BY COALESCE(published_at, ''), imported_at_epoch_ms, id",
                    )
                    .map_err(|error| format!("failed to prepare overflow cleanup: {error}"))?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|error| format!("failed to query overflow cleanup: {error}"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("failed to read overflow cleanup: {error}"))?;
                rows
            };
            for document_id in overflow_candidates {
                if chunk_count <= 50_000 {
                    break;
                }
                let deleted = delete_document(&transaction, &document_id)?;
                chunk_count -= deleted as i64;
                removed_chunks += deleted;
                removed_documents += 1;
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit retention cleanup: {error}"))?;
        if removed_chunks > 0 {
            invalidate_vector_cache(&self.path);
        }
        Ok(json!({
            "removed_documents": removed_documents,
            "removed_chunks": removed_chunks,
            "chunk_limit": 50000,
            "news_community_days": 365
        }))
    }

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
            "schema_version": 2,
            "format": "gp-research-pack-v2",
            "path": destination.display().to_string(),
            "document_count": documents.len(),
            "bytes": bytes,
            "contains_vectors": false,
            "contains_chat_history": false
        }))
    }

    #[cfg(test)]
    pub(crate) fn import_portable_pack(&self, path: &Path) -> Result<Value, String> {
        let documents = read_portable_pack(path)?;
        let result = self.ingest_documents(&documents)?;
        Ok(json!({
            "schema_version": 2,
            "format": "gp-research-pack-v2",
            "document_count": documents.len(),
            "imported": result,
            "fts_rebuilt": true,
            "vectors_rebuild_required": cfg!(target_os = "windows")
        }))
    }

    fn portable_documents(&self) -> Result<Vec<Value>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT d.id, d.title, d.content, d.source_tier, d.source_name,
                        d.url, d.published_at, d.user_imported, d.pinned, d.metadata_json,
                        c.page_number, c.stock_codes_json, c.entities_json,
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
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<u32>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
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

    pub(crate) fn query(&self, request: &Value) -> Result<Value, String> {
        self.query_with_vector(request, None)
    }

    pub(crate) fn query_with_vector(
        &self,
        request: &Value,
        query_vector: Option<&[f32]>,
    ) -> Result<Value, String> {
        let query = string_field(request, "query").unwrap_or_default();
        if query.trim().is_empty() {
            return Err("query is required".to_string());
        }
        let stock_code =
            string_field(request, "stock_code").map(|value| value.trim().to_ascii_uppercase());
        let top_k = request
            .get("top_k")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_CITATIONS as u64)
            .clamp(1, MAX_CITATIONS as u64) as usize;
        let connection = self.connection()?;
        let match_query = build_fts_match_query(&query);
        let mut statement = connection
            .prepare(
                "SELECT id, document_id, title, content, page_number,
                        source_tier, source_name, published_at, url,
                        -bm25(chunks_fts, 0.0, 3.0, 2.0, 1.0) AS lexical_score
                 FROM chunks_fts
                 JOIN chunks ON chunks.id = chunks_fts.chunk_id
                 WHERE chunks_fts MATCH ?1
                   AND (
                     ?2 IS NULL OR EXISTS (
                       SELECT 1 FROM json_each(chunks.stock_codes_json)
                       WHERE value = ?2 COLLATE NOCASE
                     )
                   )
                 ORDER BY bm25(chunks_fts, 0.0, 3.0, 2.0, 1.0)
                 LIMIT 50",
            )
            .map_err(|error| format!("failed to prepare research query: {error}"))?;
        let rows = statement
            .query_map(params![match_query, stock_code], |row| {
                Ok((
                    Candidate {
                        chunk_id: row.get(0)?,
                        document_id: row.get(1)?,
                        title: row.get(2)?,
                        content: row.get(3)?,
                        page_number: row.get(4)?,
                        source_tier: row.get(5)?,
                        source_name: row.get(6)?,
                        published_at: row.get(7)?,
                        url: row.get(8)?,
                    },
                    row.get::<_, f64>(9)?,
                ))
            })
            .map_err(|error| format!("failed to execute research query: {error}"))?;

        let mut scored = Vec::new();
        for row in rows {
            let (candidate, lexical_score) =
                row.map_err(|error| format!("failed to read research candidate: {error}"))?;
            scored.push((candidate, lexical_score));
        }
        let lexical_ranking = scored
            .iter()
            .map(|(candidate, score)| (candidate.chunk_id.clone(), *score))
            .collect::<Vec<_>>();
        let mut candidates = scored
            .into_iter()
            .map(|(candidate, score)| {
                (
                    candidate.chunk_id.clone(),
                    (candidate, Some(score), None::<f64>),
                )
            })
            .collect::<HashMap<_, _>>();
        let vector_candidates = query_vector
            .map(|vector| self.vector_candidates(&connection, vector, stock_code.as_deref()))
            .transpose()?
            .unwrap_or_default();
        let vector_ranking = vector_candidates
            .iter()
            .map(|(candidate, score)| (candidate.chunk_id.clone(), *score))
            .collect::<Vec<_>>();
        for (candidate, score) in vector_candidates {
            candidates
                .entry(candidate.chunk_id.clone())
                .and_modify(|entry| entry.2 = Some(score))
                .or_insert((candidate, None, Some(score)));
        }
        let fused = rrf_fuse(&lexical_ranking, &vector_ranking, 60.0);
        let mut ranked = fused
            .into_iter()
            .filter_map(|(chunk_id, rrf_score)| {
                candidates
                    .remove(&chunk_id)
                    .map(|(candidate, lexical, vector)| {
                        (candidate, lexical.unwrap_or(0.0), vector, rrf_score)
                    })
            })
            .collect::<Vec<_>>();
        ranked.sort_by(
            |(left_candidate, _, _, left_score), (right_candidate, _, _, right_score)| {
                source_rank(&right_candidate.source_tier)
                    .cmp(&source_rank(&left_candidate.source_tier))
                    .then_with(|| {
                        right_score
                            .partial_cmp(left_score)
                            .unwrap_or(Ordering::Equal)
                    })
            },
        );

        let mut citations = Vec::new();
        let mut per_document = HashMap::<String, usize>::new();
        for (candidate, lexical_score, vector_score, retrieval_score) in ranked {
            let used = per_document
                .entry(candidate.document_id.clone())
                .or_default();
            if *used >= 2 {
                continue;
            }
            *used += 1;
            let citation_id = format!("C{}", citations.len() + 1);
            citations.push(ResearchCitation {
                citation_id,
                document_id: candidate.document_id,
                chunk_id: candidate.chunk_id,
                title: candidate.title,
                excerpt: excerpt(&candidate.content, 320),
                source_tier: candidate.source_tier,
                source_name: candidate.source_name,
                published_at: candidate.published_at,
                url: candidate.url,
                page_number: candidate.page_number,
                lexical_score,
                vector_score,
                retrieval_score,
            });
            if citations.len() >= top_k {
                break;
            }
        }

        let used_vector = !vector_ranking.is_empty();
        let community_only = !citations.is_empty()
            && citations
                .iter()
                .all(|citation| citation.source_tier == "community");
        let answer = if citations.is_empty() {
            "未找到足以支撑结论的本地证据。".to_string()
        } else if community_only {
            "当前只命中社区信息，不能单独作为事实结论；请补充公告、财务快照或可信新闻核验。"
                .to_string()
        } else {
            citations
                .iter()
                .map(|citation| format!("[{}] {}", citation.citation_id, citation.excerpt))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(json!({
            "mode": "evidence_only",
            "query": query,
            "answer": answer,
            "citations": citations,
            "community_only": community_only,
            "fact_supported": !community_only && !citations.is_empty(),
            "retrieval_mode": if used_vector { "hybrid_rrf" } else { "bm25" }
        }))
    }

    pub(crate) fn pending_embedding_chunks(
        &self,
        limit: usize,
    ) -> Result<Vec<EmbeddingWorkItem>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT c.id, c.content_hash, c.title, c.content
                 FROM chunks c
                 LEFT JOIN embeddings e ON e.chunk_id = c.id
                 WHERE e.chunk_id IS NULL OR e.content_hash <> c.content_hash
                 ORDER BY c.created_at_epoch_ms, c.document_id, c.ordinal
                 LIMIT ?1",
            )
            .map_err(|error| format!("failed to prepare pending embeddings: {error}"))?;
        let items = statement
            .query_map(params![limit.clamp(1, 50_000) as i64], |row| {
                let title: String = row.get(2)?;
                let content: String = row.get(3)?;
                Ok(EmbeddingWorkItem {
                    chunk_id: row.get(0)?,
                    content_hash: row.get(1)?,
                    text: format!("{title}\n{content}"),
                })
            })
            .map_err(|error| format!("failed to query pending embeddings: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read pending embeddings: {error}"))?;
        Ok(items)
    }

    pub(crate) fn store_embeddings(
        &self,
        items: &[(EmbeddingWorkItem, Vec<f32>)],
        model_id: &str,
    ) -> Result<Value, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start embedding update: {error}"))?;
        let created_at = epoch_millis();
        for (item, vector) in items {
            if vector.len() != 512 {
                return Err(format!(
                    "embedding {} has invalid dimension {}",
                    item.chunk_id,
                    vector.len()
                ));
            }
            transaction
                .execute(
                    "INSERT INTO embeddings (
                        chunk_id, model_id, dimensions, vector, normalized,
                        content_hash, created_at_epoch_ms
                     ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
                     ON CONFLICT(chunk_id) DO UPDATE SET
                        model_id=excluded.model_id, dimensions=excluded.dimensions,
                        vector=excluded.vector, normalized=excluded.normalized,
                        content_hash=excluded.content_hash,
                        created_at_epoch_ms=excluded.created_at_epoch_ms",
                    params![
                        item.chunk_id,
                        model_id,
                        vector.len() as i64,
                        encode_f32_blob(vector),
                        item.content_hash,
                        created_at
                    ],
                )
                .map_err(|error| format!("failed to store embedding: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit embeddings: {error}"))?;
        invalidate_vector_cache(&self.path);
        Ok(json!({"stored": items.len(), "model_id": model_id}))
    }

    fn vector_candidates(
        &self,
        connection: &Connection,
        query_vector: &[f32],
        stock_code: Option<&str>,
    ) -> Result<Vec<(Candidate, f64)>, String> {
        if query_vector.len() != 512 {
            return Err(format!(
                "query embedding has invalid dimension {}",
                query_vector.len()
            ));
        }
        let vectors = load_vector_cache(connection, &self.path)?;
        let mut scored = vectors
            .iter()
            .filter(|row| {
                stock_code
                    .map(|expected| {
                        row.stock_codes
                            .iter()
                            .any(|code| code.eq_ignore_ascii_case(expected))
                    })
                    .unwrap_or(true)
            })
            .filter_map(|row| {
                cosine_similarity(query_vector, &row.vector)
                    .map(|score| (row.chunk_id.clone(), score as f64))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
        scored.truncate(50);
        scored
            .into_iter()
            .map(|(chunk_id, score)| {
                load_candidate(connection, &chunk_id).map(|candidate| (candidate, score))
            })
            .collect()
    }

    pub(crate) fn overview(&self, request: &Value) -> Result<Value, String> {
        let connection = self.connection()?;
        let stock_code = string_field(request, "stock_code");
        let unread_count: i64 = if let Some(code) = stock_code.as_deref() {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM research_messages WHERE unread = 1 AND stock_code = ?1",
                    params![code],
                    |row| row.get(0),
                )
                .map_err(|error| format!("failed to count unread research messages: {error}"))?
        } else {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM research_messages WHERE unread = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| format!("failed to count unread research messages: {error}"))?
        };
        let mut unread_by_stock = HashMap::new();
        let mut unread_statement = connection
            .prepare(
                "SELECT stock_code, COUNT(*) FROM research_messages
                 WHERE unread = 1 AND stock_code IS NOT NULL AND stock_code != ''
                 GROUP BY stock_code",
            )
            .map_err(|error| format!("failed to prepare unread stock summary: {error}"))?;
        let unread_rows = unread_statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| format!("failed to query unread stock summary: {error}"))?;
        for row in unread_rows {
            let (code, count) =
                row.map_err(|error| format!("failed to read unread stock summary: {error}"))?;
            unread_by_stock.insert(code, count);
        }
        let document_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .map_err(|error| format!("failed to count research documents: {error}"))?;
        let chunk_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|error| format!("failed to count research chunks: {error}"))?;
        let mut source_statement = connection
            .prepare(
                "SELECT source_tier, COUNT(*) FROM documents GROUP BY source_tier
                 ORDER BY CASE source_tier
                    WHEN 'filing' THEN 1 WHEN 'financial_snapshot' THEN 2
                    WHEN 'news' THEN 3 WHEN 'research_report' THEN 4
                    WHEN 'community' THEN 5 ELSE 6 END",
            )
            .map_err(|error| format!("failed to prepare source summary: {error}"))?;
        let source_counts = source_statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| format!("failed to query source summary: {error}"))?
            .filter_map(Result::ok)
            .map(|(tier, count)| json!({"source_tier": tier, "count": count}))
            .collect::<Vec<_>>();
        let mut message_request = request.clone();
        message_request["limit"] = json!(20);
        let messages = self.messages(&message_request)?;
        Ok(json!({
            "schema_version": RESEARCH_SCHEMA_VERSION,
            "database_path": self.path.display().to_string(),
            "document_count": document_count,
            "chunk_count": chunk_count,
            "unread_count": unread_count,
            "unread_by_stock": unread_by_stock,
            "source_counts": source_counts,
            "messages": messages["items"].clone(),
            "retrieval": {
                "lexical": "jieba_fts5_bm25",
                "vector": vector_backend_status(),
                "rrf_k": 60,
                "citation_limit": MAX_CITATIONS
            }
        }))
    }

    pub(crate) fn messages(&self, request: &Value) -> Result<Value, String> {
        let connection = self.connection()?;
        let stock_code = string_field(request, "stock_code");
        let unread_only = bool_field(request, "unread_only");
        let limit = request
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(100)
            .clamp(1, 500) as i64;
        let offset = request
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(50_000) as i64;
        let mut statement = connection
            .prepare(
                "SELECT id, document_id, stock_code, title, summary, sentiment,
                        source_tier, published_at, unread
                 FROM research_messages
                 WHERE (?1 IS NULL OR stock_code = ?1)
                   AND (?2 = 0 OR unread = 1)
                 ORDER BY COALESCE(published_at, '') DESC, created_at_epoch_ms DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|error| format!("failed to prepare research messages: {error}"))?;
        let items = statement
            .query_map(params![stock_code, unread_only, limit, offset], |row| {
                Ok(ResearchMessage {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    stock_code: row.get(2)?,
                    title: row.get(3)?,
                    summary: row.get(4)?,
                    sentiment: row.get(5)?,
                    source_tier: row.get(6)?,
                    published_at: row.get(7)?,
                    unread: row.get::<_, i64>(8)? != 0,
                })
            })
            .map_err(|error| format!("failed to query research messages: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read research messages: {error}"))?;
        Ok(json!({"items": items, "limit": limit, "offset": offset}))
    }

    pub(crate) fn mark_read(&self, request: &Value) -> Result<Value, String> {
        let message_ids = normalized_string_array(request.get("message_ids"));
        let stock_code = string_field(request, "stock_code");
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start unread update: {error}"))?;
        let mut updated = 0usize;
        if !message_ids.is_empty() {
            for message_id in message_ids {
                updated += transaction
                    .execute(
                        "UPDATE research_messages SET unread = 0 WHERE id = ?1 AND unread = 1",
                        params![message_id],
                    )
                    .map_err(|error| format!("failed to mark research message read: {error}"))?;
            }
        } else if let Some(code) = stock_code {
            updated = transaction
                .execute(
                    "UPDATE research_messages SET unread = 0 WHERE stock_code = ?1 AND unread = 1",
                    params![code],
                )
                .map_err(|error| format!("failed to mark stock messages read: {error}"))?;
        } else if bool_field(request, "all") {
            updated = transaction
                .execute(
                    "UPDATE research_messages SET unread = 0 WHERE unread = 1",
                    [],
                )
                .map_err(|error| format!("failed to mark all messages read: {error}"))?;
        } else {
            return Err("message_ids, stock_code, or all=true is required".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit unread update: {error}"))?;
        Ok(json!({"updated": updated}))
    }

    pub(crate) fn create_thread(&self, request: &Value) -> Result<Value, String> {
        let created_at = epoch_millis();
        let id = string_field(request, "id")
            .unwrap_or_else(|| format!("thread-{created_at}-{}", unique_suffix()));
        let title = string_field(request, "title").unwrap_or_else(|| "新研究会话".to_string());
        let stock_code = string_field(request, "stock_code");
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO research_threads (
                    id, title, stock_code, created_at_epoch_ms, updated_at_epoch_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, title, stock_code, created_at],
            )
            .map_err(|error| format!("failed to create research thread: {error}"))?;
        serde_json::to_value(ResearchThread {
            id,
            title,
            stock_code,
            created_at_epoch_ms: created_at,
            updated_at_epoch_ms: created_at,
        })
        .map_err(|error| format!("failed to encode research thread: {error}"))
    }

    pub(crate) fn threads(&self) -> Result<Value, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, stock_code, created_at_epoch_ms, updated_at_epoch_ms
                 FROM research_threads ORDER BY updated_at_epoch_ms DESC",
            )
            .map_err(|error| format!("failed to prepare research threads: {error}"))?;
        let items = statement
            .query_map([], |row| {
                Ok(ResearchThread {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    stock_code: row.get(2)?,
                    created_at_epoch_ms: row.get(3)?,
                    updated_at_epoch_ms: row.get(4)?,
                })
            })
            .map_err(|error| format!("failed to query research threads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read research threads: {error}"))?;
        Ok(json!({"items": items}))
    }

    pub(crate) fn thread(&self, thread_id: &str) -> Result<Value, String> {
        let connection = self.connection()?;
        let thread = connection
            .query_row(
                "SELECT id, title, stock_code, created_at_epoch_ms, updated_at_epoch_ms
                 FROM research_threads WHERE id = ?1",
                params![thread_id],
                |row| {
                    Ok(ResearchThread {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        stock_code: row.get(2)?,
                        created_at_epoch_ms: row.get(3)?,
                        updated_at_epoch_ms: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("failed to read research thread: {error}"))?
            .ok_or_else(|| "research thread not found".to_string())?;
        let mut answer_statement = connection
            .prepare(
                "SELECT id, question, answer, mode, created_at_epoch_ms
                 FROM research_answers WHERE thread_id = ?1 ORDER BY created_at_epoch_ms",
            )
            .map_err(|error| format!("failed to prepare research answers: {error}"))?;
        let answer_rows = answer_statement
            .query_map(params![thread_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|error| format!("failed to query research answers: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read research answers: {error}"))?;
        let mut answers = Vec::new();
        for (answer_id, question, answer, mode, created_at_epoch_ms) in answer_rows {
            let citations = self.answer_citations(&connection, &answer_id)?;
            answers.push(ResearchAnswer {
                id: answer_id,
                thread_id: Some(thread_id.to_string()),
                mode,
                question,
                answer,
                citations,
                created_at_epoch_ms,
            });
        }
        Ok(json!({"thread": thread, "answers": answers}))
    }

    pub(crate) fn save_answer(
        &self,
        thread_id: &str,
        question: &str,
        response: &Value,
    ) -> Result<Value, String> {
        let answer_text = string_field(response, "answer").unwrap_or_default();
        let mode = string_field(response, "mode").unwrap_or_else(|| "evidence_only".to_string());
        let citations = response
            .get("citations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(serde_json::from_value::<ResearchCitation>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("invalid research citation: {error}"))?;
        let created_at = epoch_millis();
        let answer_id = format!("answer-{created_at}-{}", unique_suffix());
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start answer save: {error}"))?;
        let thread_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM research_threads WHERE id = ?1)",
                params![thread_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to validate research thread: {error}"))?;
        if !thread_exists {
            return Err("research thread not found".to_string());
        }
        transaction
            .execute(
                "INSERT INTO research_answers (
                    id, thread_id, question, answer, mode, created_at_epoch_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    answer_id,
                    thread_id,
                    question,
                    answer_text,
                    mode,
                    created_at
                ],
            )
            .map_err(|error| format!("failed to store research answer: {error}"))?;
        for (ordinal, citation) in citations.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO research_answer_citations (
                        answer_id, citation_id, document_id, chunk_id, ordinal,
                        lexical_score, vector_score, retrieval_score
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        answer_id,
                        citation.citation_id,
                        citation.document_id,
                        citation.chunk_id,
                        ordinal as i64,
                        citation.lexical_score,
                        citation.vector_score,
                        citation.retrieval_score
                    ],
                )
                .map_err(|error| format!("failed to store answer citation: {error}"))?;
            transaction
                .execute(
                    "UPDATE documents SET cited_count = cited_count + 1 WHERE id = ?1",
                    params![citation.document_id],
                )
                .map_err(|error| format!("failed to retain cited document: {error}"))?;
        }
        transaction
            .execute(
                "UPDATE research_threads SET updated_at_epoch_ms = ?2 WHERE id = ?1",
                params![thread_id, created_at],
            )
            .map_err(|error| format!("failed to update research thread: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit research answer: {error}"))?;
        Ok(json!({"id": answer_id, "saved": true}))
    }

    pub(crate) fn index_status(&self) -> Result<Value, String> {
        let connection = self.connection()?;
        let document_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .map_err(|error| format!("failed to count research documents: {error}"))?;
        let chunk_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|error| format!("failed to count research chunks: {error}"))?;
        let fts_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks_fts", [], |row| row.get(0))
            .map_err(|error| format!("failed to count FTS rows: {error}"))?;
        let embedding_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))
            .map_err(|error| format!("failed to count embeddings: {error}"))?;
        let database_bytes = fs::metadata(&self.path)
            .map(|value| value.len())
            .unwrap_or(0);
        Ok(json!({
            "schema_version": RESEARCH_SCHEMA_VERSION,
            "document_count": document_count,
            "chunk_count": chunk_count,
            "fts_count": fts_count,
            "embedding_count": embedding_count,
            "database_bytes": database_bytes,
            "healthy": chunk_count == fts_count,
            "vector": vector_backend_status()
        }))
    }

    pub(crate) fn document_statuses(&self) -> Result<Value, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT d.id, d.title, d.source_tier, d.updated_at_epoch_ms,
                        COUNT(DISTINCT c.id), COUNT(DISTINCT e.chunk_id)
                 FROM documents d
                 LEFT JOIN chunks c ON c.document_id = d.id
                 LEFT JOIN embeddings e ON e.chunk_id = c.id
                 GROUP BY d.id, d.title, d.source_tier, d.updated_at_epoch_ms
                 ORDER BY d.updated_at_epoch_ms DESC LIMIT 1000",
            )
            .map_err(|error| format!("failed to prepare document statuses: {error}"))?;
        let items = statement
            .query_map([], |row| {
                let chunk_count = row.get::<_, i64>(4)?.max(0) as usize;
                let embedding_count = row.get::<_, i64>(5)?.max(0) as usize;
                Ok(KnowledgeDocumentStatus {
                    document_id: row.get(0)?,
                    title: row.get(1)?,
                    source_tier: row.get(2)?,
                    chunk_count,
                    embedding_count,
                    indexed: chunk_count > 0,
                    updated_at_epoch_ms: row.get(3)?,
                })
            })
            .map_err(|error| format!("failed to query document statuses: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read document statuses: {error}"))?;
        Ok(json!({"items": items}))
    }

    pub(crate) fn rebuild_fts(&self) -> Result<Value, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start FTS rebuild: {error}"))?;
        let chunks = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, title, entities_json, content FROM chunks ORDER BY document_id, ordinal",
                )
                .map_err(|error| format!("failed to prepare chunks for FTS rebuild: {error}"))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|error| format!("failed to query chunks for FTS rebuild: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to read chunks for FTS rebuild: {error}"))?;
            rows
        };
        transaction
            .execute("DELETE FROM chunks_fts", [])
            .map_err(|error| format!("failed to clear FTS index: {error}"))?;
        for (chunk_id, title, entities_json, content) in &chunks {
            let entities = serde_json::from_str::<Vec<String>>(entities_json).unwrap_or_default();
            transaction
                .execute(
                    "INSERT INTO chunks_fts (chunk_id, title_terms, entity_terms, body_terms)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        chunk_id,
                        tokenize_for_fts(title),
                        tokenize_for_fts(&entities.join(" ")),
                        tokenize_for_fts(content)
                    ],
                )
                .map_err(|error| format!("failed to rebuild FTS row: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit FTS rebuild: {error}"))?;
        Ok(json!({"rebuilt": chunks.len(), "schema_version": RESEARCH_SCHEMA_VERSION}))
    }

    fn answer_citations(
        &self,
        connection: &Connection,
        answer_id: &str,
    ) -> Result<Vec<ResearchCitation>, String> {
        let mut statement = connection
            .prepare(
                "SELECT ac.citation_id, c.document_id, c.id, c.title, c.content,
                        c.source_tier, c.source_name, c.published_at, c.url, c.page_number,
                        ac.lexical_score, ac.vector_score, ac.retrieval_score
                 FROM research_answer_citations ac
                 JOIN chunks c ON c.id = ac.chunk_id
                 WHERE ac.answer_id = ?1 ORDER BY ac.ordinal",
            )
            .map_err(|error| format!("failed to prepare stored citations: {error}"))?;
        let citations = statement
            .query_map(params![answer_id], |row| {
                let content: String = row.get(4)?;
                Ok(ResearchCitation {
                    citation_id: row.get(0)?,
                    document_id: row.get(1)?,
                    chunk_id: row.get(2)?,
                    title: row.get(3)?,
                    excerpt: excerpt(&content, 320),
                    source_tier: row.get(5)?,
                    source_name: row.get(6)?,
                    published_at: row.get(7)?,
                    url: row.get(8)?,
                    page_number: row.get(9)?,
                    lexical_score: row.get(10)?,
                    vector_score: row.get(11)?,
                    retrieval_score: row.get(12)?,
                })
            })
            .map_err(|error| format!("failed to query stored citations: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read stored citations: {error}"))?;
        Ok(citations)
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("failed to open research database: {error}"))?;
        configure_connection(&connection)?;
        Ok(connection)
    }
}

pub(crate) fn open_app_store(app: &tauri::AppHandle) -> Result<ResearchStore, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let research_dir = app_data.join("research");
    fs::create_dir_all(&research_dir)
        .map_err(|error| format!("failed to create research directory: {error}"))?;
    recover_research_files(&research_dir)?;
    let store = ResearchStore::open(research_dir.join("research.sqlite"))?;
    let migration_lock = LEGACY_MIGRATION_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = migration_lock
        .lock()
        .map_err(|_| "legacy research migration lock is poisoned".to_string())?;
    migrate_legacy_sources(&store, &app_data)?;
    Ok(store)
}

fn recover_research_files(research_dir: &Path) -> Result<(), String> {
    let database = research_dir.join("research.sqlite");
    let rollback = research_dir.join("research.sqlite.rollback");
    let replaced = research_dir.join("research.sqlite.replaced");
    let importing = research_dir.join("research.sqlite.importing");
    if !database.exists() {
        let recovery = if rollback.exists() {
            Some(rollback)
        } else if replaced.exists() {
            Some(replaced)
        } else if importing.exists() {
            Some(importing.clone())
        } else {
            None
        };
        if let Some(recovery) = recovery {
            fs::rename(&recovery, &database).map_err(|error| {
                format!(
                    "failed to recover research database from {}: {error}",
                    recovery.display()
                )
            })?;
        }
    }
    if database.exists() && importing.exists() {
        remove_sqlite_files(&importing)?;
    }
    Ok(())
}

pub(crate) fn ingest_news_cache(app: &tauri::AppHandle) -> Result<Value, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let store = open_app_store(app)?;
    let path = app_data.join("news").join("news-cache.json");
    let documents = legacy_documents_from_file(&path)?;
    store.ingest_documents(&documents)
}

pub(crate) fn export_app_pack(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
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
                .join("research-pack-v2.sqlite")
        });
    open_app_store(app)?.export_portable_pack(&destination)
}

pub(crate) fn import_app_pack(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
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
        fs::write(&path, bytes)
            .map_err(|error| format!("failed to stage incoming research pack: {error}"))?;
        temporary_source = Some(path.clone());
        path
    } else {
        return Err("path or bytes_base64 is required".to_string());
    };

    let documents = read_portable_pack(&source_path)?;
    if documents.is_empty() {
        return Err("research pack does not contain any documents".to_string());
    }
    let database_path = research_dir.join("research.sqlite");
    let staging_path = research_dir.join("research.sqlite.importing");
    let rollback_path = research_dir.join("research.sqlite.rollback");
    remove_sqlite_files(&staging_path)?;
    {
        let staging = ResearchStore::open(&staging_path)?;
        staging.ingest_documents(&documents)?;
        preserve_local_research_state(&database_path, &staging)?;
        staging
            .connection()?
            .execute(
                "INSERT INTO research_metadata (key, value) VALUES ('legacy_v1_import_complete', 'true')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .map_err(|error| format!("failed to mark staged migration complete: {error}"))?;
        staging.rebuild_fts()?;
        staging
            .connection()?
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|error| format!("failed to finalize staged research database: {error}"))?;
    }
    remove_sqlite_sidecars(&staging_path)?;

    if database_path.exists() {
        {
            let connection = Connection::open(&database_path)
                .map_err(|error| format!("failed to open current research database: {error}"))?;
            let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        remove_sqlite_sidecars(&database_path)?;
        remove_sqlite_files(&rollback_path)?;
        fs::rename(&database_path, &rollback_path)
            .map_err(|error| format!("failed to preserve rollback database: {error}"))?;
    }
    if let Err(error) = fs::rename(&staging_path, &database_path) {
        if rollback_path.exists() && !database_path.exists() {
            let _ = fs::rename(&rollback_path, &database_path);
        }
        return Err(format!(
            "failed to atomically activate research pack: {error}"
        ));
    }
    invalidate_vector_cache(&database_path);
    if let Some(path) = temporary_source {
        let _ = fs::remove_file(path);
    }
    Ok(json!({
        "schema_version": 2,
        "format": "gp-research-pack-v2",
        "document_count": documents.len(),
        "imported": true,
        "fts_rebuilt": true,
        "vectors_rebuild_required": cfg!(target_os = "windows"),
        "rollback_available": rollback_path.exists()
    }))
}

fn preserve_local_research_state(
    previous_path: &Path,
    staging: &ResearchStore,
) -> Result<(), String> {
    if !previous_path.exists() {
        return Ok(());
    }
    {
        let previous = Connection::open(previous_path)
            .map_err(|error| format!("failed to open previous research database: {error}"))?;
        initialize_schema(&previous)?;
    }
    let destination = staging.connection()?;
    destination
        .execute(
            "ATTACH DATABASE ?1 AS previous_research",
            params![previous_path.display().to_string()],
        )
        .map_err(|error| format!("failed to attach previous research database: {error}"))?;
    let result = destination
        .execute_batch(
            "INSERT OR IGNORE INTO research_threads
                     SELECT id, title, stock_code, created_at_epoch_ms, updated_at_epoch_ms
                     FROM previous_research.research_threads;
                 INSERT OR IGNORE INTO research_answers
                     SELECT id, thread_id, question, answer, mode, created_at_epoch_ms
                     FROM previous_research.research_answers
                     WHERE thread_id IN (SELECT id FROM research_threads);
                 INSERT OR IGNORE INTO research_answer_citations (
                     answer_id, citation_id, document_id, chunk_id, ordinal,
                     lexical_score, vector_score, retrieval_score
                 )
                     SELECT previous.answer_id, previous.citation_id, previous.document_id,
                            previous.chunk_id, previous.ordinal, previous.lexical_score,
                            previous.vector_score, previous.retrieval_score
                     FROM previous_research.research_answer_citations previous
                     WHERE previous.answer_id IN (SELECT id FROM research_answers)
                       AND EXISTS (SELECT 1 FROM chunks WHERE id = previous.chunk_id);
                 UPDATE research_messages
                 SET unread = COALESCE((
                     SELECT previous.unread FROM previous_research.research_messages previous
                     WHERE previous.id = research_messages.id
                 ), unread);
                 UPDATE documents SET cited_count = (
                     SELECT COUNT(*) FROM research_answer_citations citations
                     WHERE citations.document_id = documents.id
                 );",
        )
        .map_err(|error| format!("failed to preserve local research state: {error}"));
    let detach = destination.execute_batch("DETACH DATABASE previous_research;");
    result?;
    detach.map_err(|error| format!("failed to detach previous research database: {error}"))
}

pub(crate) fn rollback_app_pack(app: &tauri::AppHandle) -> Result<Value, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let research_dir = app_data.join("research");
    recover_research_files(&research_dir)?;
    let database_path = research_dir.join("research.sqlite");
    let rollback_path = research_dir.join("research.sqlite.rollback");
    if !rollback_path.exists() {
        return Err("no research database rollback is available".to_string());
    }
    let replaced_path = research_dir.join("research.sqlite.replaced");
    if database_path.exists() {
        {
            let connection = Connection::open(&database_path)
                .map_err(|error| format!("failed to open current research database: {error}"))?;
            let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        remove_sqlite_sidecars(&database_path)?;
        remove_sqlite_files(&replaced_path)?;
        fs::rename(&database_path, &replaced_path)
            .map_err(|error| format!("failed to preserve replaced database: {error}"))?;
    }
    if let Err(error) = fs::rename(&rollback_path, &database_path) {
        if replaced_path.exists() && !database_path.exists() {
            let _ = fs::rename(&replaced_path, &database_path);
        }
        return Err(format!("failed to restore research rollback: {error}"));
    }
    invalidate_vector_cache(&database_path);
    Ok(json!({
        "rolled_back": true,
        "replaced_database": replaced_path.display().to_string()
    }))
}

fn remove_sqlite_files(path: &Path) -> Result<(), String> {
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

fn remove_sqlite_sidecars(path: &Path) -> Result<(), String> {
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

fn migrate_legacy_sources(store: &ResearchStore, app_data: &Path) -> Result<(), String> {
    let connection = store.connection()?;
    let completed = connection
        .query_row(
            "SELECT value FROM research_metadata WHERE key = 'legacy_v1_import_complete'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("failed to read migration state: {error}"))?
        .as_deref()
        == Some("true");
    if completed {
        return Ok(());
    }

    let mut paths = vec![
        app_data.join("news").join("news-cache.json"),
        app_data.join("rag").join("rag-pack.json"),
    ];
    collect_named_files(
        &app_data.join("upstream_rag_desktop"),
        "rag_pack.sqlite",
        3,
        &mut paths,
    );
    let mut documents = Vec::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        documents.extend(legacy_documents_from_file(&path)?);
    }
    if !documents.is_empty() {
        store.ingest_documents(&documents)?;
    }
    connection
        .execute(
            "INSERT INTO research_metadata (key, value) VALUES ('legacy_v1_import_complete', 'true')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .map_err(|error| format!("failed to record migration state: {error}"))?;
    Ok(())
}

fn legacy_documents_from_file(path: &Path) -> Result<Vec<Value>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let size = fs::metadata(path)
        .map_err(|error| {
            format!(
                "failed to inspect legacy source {}: {error}",
                path.display()
            )
        })?
        .len();
    if size > MAX_PACK_BYTES {
        return Err("legacy research source exceeds the 64 MB safety limit".to_string());
    }
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "failed to read legacy research source {}: {error}",
            path.display()
        )
    })?;
    if bytes.first().copied() != Some(b'{') && bytes.first().copied() != Some(b'[') {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "failed to parse legacy research source {}: {error}",
            path.display()
        )
    })?;
    let inherited_codes = normalized_string_array(value.get("stock_codes"));
    let mut candidates = Vec::new();
    for key in [
        "items",
        "documents",
        "source_documents",
        "evidence_chunks",
        "relation_edges",
    ] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            candidates.extend(items.iter().cloned());
        }
    }
    if candidates.len() > MAX_PACK_DOCUMENTS {
        return Err("legacy research source contains too many documents".to_string());
    }
    Ok(candidates
        .into_iter()
        .filter_map(|item| normalize_legacy_document(item, path, &inherited_codes))
        .collect())
}

fn read_portable_pack(path: &Path) -> Result<Vec<Value>, String> {
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
    if version != RESEARCH_SCHEMA_VERSION {
        return Err(format!(
            "unsupported research pack schema {version}; expected {RESEARCH_SCHEMA_VERSION}"
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

fn normalize_legacy_document(
    mut item: Value,
    path: &Path,
    inherited_codes: &[String],
) -> Option<Value> {
    let object = item.as_object_mut()?;
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("旧版研究资料")
        .trim()
        .to_string();
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
        })?;
    if content.chars().count() > MAX_PACK_DOCUMENT_CHARS {
        return None;
    }
    let path_hash = sha256_hex(path.display().to_string().as_bytes());
    let original_id = object
        .get("document_id")
        .or_else(|| object.get("chunk_id"))
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let stable_key = original_id.map(|id| format!("id:{id}")).unwrap_or_else(|| {
        let source = object
            .get("source_name")
            .or_else(|| object.get("source"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let published_at = object
            .get("published_at")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let url = object
            .get("url")
            .or_else(|| object.get("source_url"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!("source:{source}|published:{published_at}|url:{url}|title:{title}")
    });
    let identity_hash = sha256_hex(stable_key.as_bytes());
    let document_id = format!("legacy-{}-{}", &path_hash[..16], &identity_hash[..24]);
    let stock_codes = {
        let own = normalized_string_array(object.get("stock_codes"));
        if own.is_empty() {
            inherited_codes.to_vec()
        } else {
            own
        }
    };
    Some(json!({
        "document_id": document_id,
        "title": title,
        "content": content,
        "source_tier": object
            .get("source_tier")
            .and_then(Value::as_str)
            .unwrap_or("news"),
        "source_name": object
            .get("source_name")
            .or_else(|| object.get("source"))
            .and_then(Value::as_str)
            .unwrap_or("旧版本地资料"),
        "published_at": object.get("published_at").cloned().unwrap_or(Value::Null),
        "url": object
            .get("url")
            .or_else(|| object.get("source_url"))
            .cloned()
            .unwrap_or(Value::Null),
        "stock_codes": stock_codes,
        "relation_types": object.get("relation_types").cloned().unwrap_or_else(|| json!([])),
        "sentiment": object
            .get("sentiment")
            .and_then(Value::as_str)
            .unwrap_or("uncertain"),
        "metadata": {"legacy_path": path.display().to_string()}
    }))
}

fn collect_named_files(root: &Path, name: &str, depth: usize, output: &mut Vec<PathBuf>) {
    if depth == 0 || !root.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_named_files(&path, name, depth - 1, output);
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            output.push(path);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EmbeddingWorkItem {
    pub(crate) chunk_id: String,
    pub(crate) content_hash: String,
    pub(crate) text: String,
}

#[derive(Clone)]
struct CachedVector {
    chunk_id: String,
    stock_codes: Vec<String>,
    vector: Vec<f32>,
}

#[derive(Clone)]
struct VectorCacheEntry {
    count: i64,
    newest_epoch_ms: i64,
    rows: Arc<Vec<CachedVector>>,
}

static VECTOR_CACHE: OnceLock<Mutex<HashMap<PathBuf, VectorCacheEntry>>> = OnceLock::new();
static INITIALIZED_DATABASES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static LEGACY_MIGRATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug)]
struct Candidate {
    chunk_id: String,
    document_id: String,
    title: String,
    content: String,
    page_number: Option<u32>,
    source_tier: String,
    source_name: String,
    published_at: Option<String>,
    url: Option<String>,
}

fn load_vector_cache(
    connection: &Connection,
    path: &Path,
) -> Result<Arc<Vec<CachedVector>>, String> {
    let (count, newest_epoch_ms) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(created_at_epoch_ms), 0) FROM embeddings",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|error| format!("failed to inspect embedding cache version: {error}"))?;
    let cache = VECTOR_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache
            .lock()
            .map_err(|_| "vector cache lock is poisoned".to_string())?;
        if let Some(entry) = guard.get(path) {
            if entry.count == count && entry.newest_epoch_ms == newest_epoch_ms {
                return Ok(Arc::clone(&entry.rows));
            }
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT e.chunk_id, c.stock_codes_json, e.vector, e.dimensions
             FROM embeddings e JOIN chunks c ON c.id = e.chunk_id",
        )
        .map_err(|error| format!("failed to prepare embedding cache: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            let chunk_id: String = row.get(0)?;
            let stock_codes_json: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            let dimensions: i64 = row.get(3)?;
            Ok((chunk_id, stock_codes_json, blob, dimensions))
        })
        .map_err(|error| format!("failed to query embedding cache: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read embedding cache: {error}"))?;
    let mut decoded = Vec::with_capacity(rows.len());
    for (chunk_id, stock_codes_json, blob, dimensions) in rows {
        let vector = decode_f32_blob(&blob)?;
        if vector.len() != dimensions.max(0) as usize || vector.len() != 512 {
            continue;
        }
        decoded.push(CachedVector {
            chunk_id,
            stock_codes: serde_json::from_str(&stock_codes_json).unwrap_or_default(),
            vector,
        });
    }
    let rows = Arc::new(decoded);
    cache
        .lock()
        .map_err(|_| "vector cache lock is poisoned".to_string())?
        .insert(
            path.to_path_buf(),
            VectorCacheEntry {
                count,
                newest_epoch_ms,
                rows: Arc::clone(&rows),
            },
        );
    Ok(rows)
}

fn invalidate_vector_cache(path: &Path) {
    if let Some(cache) = VECTOR_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.remove(path);
        }
    }
}

fn load_candidate(connection: &Connection, chunk_id: &str) -> Result<Candidate, String> {
    connection
        .query_row(
            "SELECT id, document_id, title, content, page_number,
                    source_tier, source_name, published_at, url
             FROM chunks WHERE id = ?1",
            params![chunk_id],
            |row| {
                Ok(Candidate {
                    chunk_id: row.get(0)?,
                    document_id: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    page_number: row.get(4)?,
                    source_tier: row.get(5)?,
                    source_name: row.get(6)?,
                    published_at: row.get(7)?,
                    url: row.get(8)?,
                })
            },
        )
        .map_err(|error| format!("failed to load vector candidate {chunk_id}: {error}"))
}

fn delete_document(
    transaction: &rusqlite::Transaction<'_>,
    document_id: &str,
) -> Result<usize, String> {
    let chunk_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE document_id = ?1",
            params![document_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to count removable document chunks: {error}"))?;
    transaction
        .execute(
            "DELETE FROM chunks_fts WHERE chunk_id IN (
                SELECT id FROM chunks WHERE document_id = ?1
             )",
            params![document_id],
        )
        .map_err(|error| format!("failed to remove retained FTS rows: {error}"))?;
    transaction
        .execute("DELETE FROM documents WHERE id = ?1", params![document_id])
        .map_err(|error| format!("failed to remove retained document: {error}"))?;
    Ok(chunk_count.max(0) as usize)
}

fn encode_f32_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_f32_blob(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err("embedding BLOB length is not aligned to f32".to_string());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn initialize_schema(connection: &Connection) -> Result<(), String> {
    configure_connection(connection)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("failed to read research schema version: {error}"))?;
    if version > RESEARCH_SCHEMA_VERSION {
        return Err(format!(
            "research database schema {version} is newer than supported version {RESEARCH_SCHEMA_VERSION}"
        ));
    }
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS research_metadata (
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
             PRAGMA user_version = 2;
             COMMIT;",
        )
        .map_err(|error| format!("failed to initialize research schema: {error}"))?;
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
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
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
    Ok(())
}

fn configure_connection(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|error| format!("failed to configure research database: {error}"))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn normalized_string_array(value: Option<&Value>) -> Vec<String> {
    let mut values = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn normalize_source_tier(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "filing" | "official" | "announcement" => "filing",
        "financial" | "financial_snapshot" | "finance" => "financial_snapshot",
        "research" | "research_report" | "report" => "research_report",
        "community" | "social" => "community",
        _ => "news",
    }
    .to_string()
}

fn source_rank(value: &str) -> u8 {
    match normalize_source_tier(value).as_str() {
        "filing" => 5,
        "financial_snapshot" => 4,
        "news" => 3,
        "research_report" => 2,
        "community" => 1,
        _ => 0,
    }
}

fn research_jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(|| {
        let mut jieba = Jieba::new();
        for word in [
            "主力净流入",
            "主力净占比",
            "主力介入度",
            "资产负债率",
            "市盈率",
            "净资产收益率",
            "量化交易",
            "龙虎榜",
            "营业部",
            "储能订单",
            "业绩预告",
            "回购",
            "减持",
            "增持",
        ] {
            jieba.add_word(word, None, None);
        }
        jieba
    })
}

fn tokenize_for_fts(value: &str) -> String {
    let mut seen = HashSet::new();
    let lower = value.to_lowercase();
    research_jieba()
        .cut_for_search(&lower, true)
        .into_iter()
        .map(|token| token.word.trim())
        .filter(|token| !token.is_empty())
        .filter(|token| seen.insert((*token).to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_fts_match_query(value: &str) -> String {
    let tokens = tokenize_for_fts(value);
    let mut unique = HashSet::new();
    let terms = tokens
        .split_whitespace()
        .filter(|token| unique.insert((*token).to_string()))
        .map(|token| format!("{token:?}"))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        format!("{:?}", "__empty_research_query__")
    } else {
        terms.join(" OR ")
    }
}

pub(crate) fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let mut dot = 0.0f32;
    let mut left_norm = 0.0f32;
    let mut right_norm = 0.0f32;
    for (left_value, right_value) in left.iter().zip(right) {
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    (denominator > f32::EPSILON).then_some(dot / denominator)
}

pub(crate) fn rrf_fuse(
    lexical: &[(String, f64)],
    vector: &[(String, f64)],
    k: f64,
) -> Vec<(String, f64)> {
    let mut scores = HashMap::<String, f64>::new();
    for ranking in [lexical, vector] {
        for (index, (chunk_id, _)) in ranking.iter().enumerate() {
            *scores.entry(chunk_id.clone()).or_default() += 1.0 / (k + index as f64 + 1.0);
        }
    }
    let mut fused = scores.into_iter().collect::<Vec<_>>();
    fused.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    fused
}

pub(crate) fn validate_model_answer(
    answer: &str,
    allowed_citation_ids: &[String],
) -> Result<(), String> {
    let allowed = allowed_citation_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut remaining = answer;
    let mut found = 0usize;
    while let Some(start) = remaining.find("[C") {
        let after_prefix = &remaining[start + 2..];
        let digit_count = after_prefix
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        if digit_count == 0 || after_prefix.as_bytes().get(digit_count) != Some(&b']') {
            remaining = &after_prefix[after_prefix
                .char_indices()
                .nth(1)
                .map(|(index, _)| index)
                .unwrap_or(after_prefix.len())..];
            continue;
        }
        let citation_id = format!("C{}", &after_prefix[..digit_count]);
        if !allowed.contains(citation_id.as_str()) {
            return Err(format!("model returned unknown citation [{citation_id}]"));
        }
        found += 1;
        remaining = &after_prefix[digit_count + 1..];
    }
    if !allowed.is_empty() && found == 0 {
        return Err("model answer did not include any allowed citation".to_string());
    }
    Ok(())
}

fn split_text(text: &str, target_chars: usize, overlap_chars: usize) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < characters.len() {
        let end = (start + target_chars).min(characters.len());
        chunks.push(characters[start..end].iter().collect::<String>());
        if end == characters.len() {
            break;
        }
        start = end.saturating_sub(overlap_chars.min(target_chars.saturating_sub(1)));
    }
    chunks
}

fn excerpt(value: &str, max_chars: usize) -> String {
    let mut result = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        result.push('…');
    }
    result
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn unique_suffix() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed)
}

fn vector_backend_status() -> Value {
    if cfg!(target_os = "windows") {
        json!({
            "supported": true,
            "backend": "fastembed",
            "model_id": "BAAI/bge-small-zh-v1.5",
            "dimensions": 512,
            "state": "not_initialized"
        })
    } else {
        json!({
            "supported": false,
            "backend": "bm25_only",
            "state": "platform_disabled"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_database_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("gp-assistant-{name}-{unique}.sqlite"))
    }

    #[test]
    fn stores_and_queries_research_evidence() {
        let path = temporary_database_path("research-evidence");
        let store = ResearchStore::open(&path).expect("research store should open");

        store
            .ingest_documents(&[json!({
                "document_id": "filing-1",
                "title": "宁德时代订单增长公告",
                "content": "公司披露海外储能订单增长，交付计划保持稳定。",
                "source_tier": "filing",
                "source_name": "官方公告",
                "published_at": "2026-07-21T09:30:00+08:00",
                "stock_codes": ["300750.SZ"]
            })])
            .expect("document should be ingested");

        let response = store
            .query(&json!({
                "query": "订单增长",
                "stock_code": "300750.SZ",
                "top_k": 8
            }))
            .expect("query should succeed");

        assert_eq!(response["mode"], "evidence_only");
        assert_eq!(response["citations"].as_array().map(Vec::len), Some(1));
        assert_eq!(response["citations"][0]["citation_id"], "C1");
        assert_eq!(response["citations"][0]["document_id"], "filing-1");
        assert_eq!(response["citations"][0]["source_tier"], "filing");

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_quality_breaks_equal_retrieval_scores() {
        let path = temporary_database_path("source-ranking");
        let store = ResearchStore::open(&path).expect("research store should open");
        store
            .ingest_documents(&[
                json!({
                    "document_id": "community-1",
                    "title": "储能订单讨论",
                    "content": "市场传闻储能订单增长。",
                    "source_tier": "community",
                    "stock_codes": ["300750.SZ"]
                }),
                json!({
                    "document_id": "filing-1",
                    "title": "储能订单公告",
                    "content": "公司公告储能订单增长。",
                    "source_tier": "filing",
                    "stock_codes": ["300750.SZ"]
                }),
            ])
            .expect("documents should be ingested");

        let response = store
            .query(&json!({"query": "储能订单", "stock_code": "300750.SZ"}))
            .expect("query should succeed");

        assert_eq!(response["citations"][0]["source_tier"], "filing");
        assert_eq!(response["community_only"], false);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn vector_math_and_rrf_are_deterministic() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).unwrap() - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0], &[1.0, 0.0]).is_none());

        let fused = rrf_fuse(
            &[("lexical".to_string(), 0.9), ("shared".to_string(), 0.8)],
            &[("vector".to_string(), 0.9), ("shared".to_string(), 0.8)],
            60.0,
        );
        assert_eq!(fused.first().map(|item| item.0.as_str()), Some("shared"));
    }

    #[test]
    fn manages_unread_messages_threads_and_answers() {
        let path = temporary_database_path("research-state");
        let store = ResearchStore::open(&path).expect("research store should open");
        store
            .ingest_documents(&[json!({
                "document_id": "news-1",
                "title": "订单进展",
                "content": "订单交付按计划推进。",
                "source_tier": "news",
                "stock_codes": ["300750.SZ"]
            })])
            .expect("document should be ingested");

        assert_eq!(store.overview(&json!({})).unwrap()["unread_count"], 1);
        let messages = store.messages(&json!({"stock_code": "300750.SZ"})).unwrap();
        let message_id = messages["items"][0]["id"].as_str().unwrap();
        store
            .mark_read(&json!({"message_ids": [message_id]}))
            .expect("message should be marked read");
        assert_eq!(store.overview(&json!({})).unwrap()["unread_count"], 0);

        let thread = store
            .create_thread(&json!({"title": "订单跟踪", "stock_code": "300750.SZ"}))
            .expect("thread should be created");
        let thread_id = thread["id"].as_str().unwrap();
        let query = store
            .query(&json!({
                "query": "订单交付",
                "stock_code": "300750.SZ",
                "thread_id": thread_id
            }))
            .expect("query should succeed");
        store
            .save_answer(thread_id, "订单交付如何？", &query)
            .expect("answer should be persisted");

        let detail = store.thread(thread_id).expect("thread should be readable");
        assert_eq!(detail["answers"].as_array().map(Vec::len), Some(1));
        assert!(
            detail["answers"][0]["citations"][0]["lexical_score"]
                .as_f64()
                .unwrap_or_default()
                > 0.0
        );
        assert_eq!(store.index_status().unwrap()["schema_version"], 2);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unchanged_refresh_preserves_read_state_and_embeddings() {
        let path = temporary_database_path("incremental-refresh");
        let store = ResearchStore::open(&path).expect("research store should open");
        let document = json!({
            "document_id": "stable-news",
            "title": "Stable update",
            "content": "The common signal remains unchanged.",
            "source_tier": "news",
            "stock_codes": ["600000.SH"]
        });
        store
            .ingest_documents(std::slice::from_ref(&document))
            .expect("initial document should be ingested");
        let message_id = store.messages(&json!({})).unwrap()["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        store
            .mark_read(&json!({"message_ids": [message_id]}))
            .expect("message should be marked read");
        let work = store
            .pending_embedding_chunks(10)
            .expect("pending embeddings should be readable")
            .into_iter()
            .map(|item| (item, vec![1.0; 512]))
            .collect::<Vec<_>>();
        store
            .store_embeddings(&work, "test-model")
            .expect("embedding should be stored");

        let result = store
            .ingest_documents(std::slice::from_ref(&document))
            .expect("unchanged refresh should succeed");

        assert_eq!(result["chunk_count"], 0);
        assert_eq!(store.overview(&json!({})).unwrap()["unread_count"], 0);
        assert_eq!(store.index_status().unwrap()["embedding_count"], 1);
        drop(store);
        let _ = remove_sqlite_files(&path);
    }

    #[test]
    fn legacy_document_ids_do_not_depend_on_cache_order() {
        let first_item = json!({
            "id": "stable-news-id",
            "title": "Stable news item",
            "summary": "The cache position may change after a refresh.",
            "url": "https://example.com/stable-news",
            "published_at": "2026-07-22T09:00:00+08:00",
            "stock_codes": ["600000.SH"]
        });
        let second_item = json!({
            "id": "second-news-id",
            "title": "Second news item",
            "summary": "A newer item can move older cache entries.",
            "url": "https://example.com/second-news",
            "published_at": "2026-07-22T10:00:00+08:00",
            "stock_codes": ["600000.SH"]
        });
        let path = temporary_database_path("reordered-news-cache").with_extension("json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"items": [first_item.clone(), second_item.clone()]}))
                .expect("first cache should serialize"),
        )
        .expect("first cache should be written");
        let original_ids = legacy_documents_from_file(&path)
            .expect("first cache should normalize")
            .into_iter()
            .map(|item| item["document_id"].as_str().unwrap().to_string())
            .collect::<HashSet<_>>();

        fs::write(
            &path,
            serde_json::to_vec(&json!({"items": [second_item, first_item]}))
                .expect("reordered cache should serialize"),
        )
        .expect("reordered cache should be written");
        let reordered_ids = legacy_documents_from_file(&path)
            .expect("reordered cache should normalize")
            .into_iter()
            .map(|item| item["document_id"].as_str().unwrap().to_string())
            .collect::<HashSet<_>>();

        assert_eq!(original_ids, reordered_ids);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn creates_one_message_for_each_document_stock() {
        let path = temporary_database_path("multi-stock-messages");
        let store = ResearchStore::open(&path).expect("research store should open");
        store
            .ingest_documents(&[json!({
                "document_id": "multi-stock",
                "title": "Joint announcement",
                "content": "Both listed companies disclosed the same transaction.",
                "source_tier": "filing",
                "stock_codes": ["600000.SH", "000001.SZ"]
            })])
            .expect("document should be ingested");

        assert_eq!(
            store.messages(&json!({"stock_code": "600000.SH"})).unwrap()["items"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            store.messages(&json!({"stock_code": "000001.SZ"})).unwrap()["items"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        let overview = store.overview(&json!({})).expect("overview should load");
        assert_eq!(overview["unread_by_stock"]["600000.SH"], 1);
        assert_eq!(overview["unread_by_stock"]["000001.SZ"], 1);
        drop(store);
        let _ = remove_sqlite_files(&path);
    }

    #[test]
    fn applies_stock_filter_before_bm25_candidate_limit() {
        let path = temporary_database_path("stock-filter-limit");
        let store = ResearchStore::open(&path).expect("research store should open");
        let mut documents = (0..60)
            .map(|index| {
                json!({
                    "document_id": format!("popular-{index}"),
                    "title": "Common signal",
                    "content": format!("Common signal repeated {index}."),
                    "source_tier": "news",
                    "stock_codes": ["600000.SH"]
                })
            })
            .collect::<Vec<_>>();
        documents.push(json!({
            "document_id": "target-stock",
            "title": "Common signal",
            "content": "Common signal for the requested stock.",
            "source_tier": "news",
            "stock_codes": ["000001.SZ"]
        }));
        store
            .ingest_documents(&documents)
            .expect("documents should be ingested");

        let response = store
            .query(&json!({"query": "common signal", "stock_code": "000001.SZ"}))
            .expect("filtered query should succeed");
        assert_eq!(response["citations"][0]["document_id"], "target-stock");
        drop(store);
        let _ = remove_sqlite_files(&path);
    }

    #[test]
    fn versions_changed_documents_that_are_already_cited() {
        let path = temporary_database_path("cited-revision");
        let store = ResearchStore::open(&path).expect("research store should open");
        store
            .ingest_documents(&[json!({
                "document_id": "versioned-doc",
                "title": "Versioned evidence",
                "content": "Original evidence phrase.",
                "source_tier": "filing",
                "stock_codes": ["600000.SH"]
            })])
            .expect("initial document should be ingested");
        let thread = store
            .create_thread(&json!({"title": "Version test", "stock_code": "600000.SH"}))
            .unwrap();
        let thread_id = thread["id"].as_str().unwrap();
        let response = store
            .query(&json!({"query": "original evidence", "stock_code": "600000.SH"}))
            .unwrap();
        store
            .save_answer(thread_id, "What was originally disclosed?", &response)
            .unwrap();

        store
            .ingest_documents(&[json!({
                "document_id": "versioned-doc",
                "title": "Versioned evidence",
                "content": "Replacement evidence phrase.",
                "source_tier": "filing",
                "stock_codes": ["600000.SH"]
            })])
            .expect("changed cited document should create a revision");

        let detail = store.thread(thread_id).unwrap();
        assert_eq!(
            detail["answers"][0]["citations"][0]["document_id"],
            "versioned-doc"
        );
        assert_eq!(store.index_status().unwrap()["document_count"], 2);
        let replacement = store
            .query(&json!({"query": "replacement evidence", "stock_code": "600000.SH"}))
            .unwrap();
        assert!(replacement["citations"][0]["document_id"]
            .as_str()
            .unwrap()
            .starts_with("versioned-doc:revision:"));
        drop(store);
        let _ = remove_sqlite_files(&path);
    }

    #[test]
    fn portable_import_preserves_local_history_and_read_state() {
        let previous_path = temporary_database_path("state-previous");
        let staging_path = temporary_database_path("state-staging");
        let previous = ResearchStore::open(&previous_path).unwrap();
        let document = json!({
            "document_id": "shared-doc",
            "title": "Shared evidence",
            "content": "Evidence shared by both databases.",
            "source_tier": "news",
            "stock_codes": ["600000.SH"]
        });
        previous
            .ingest_documents(std::slice::from_ref(&document))
            .unwrap();
        previous
            .mark_read(&json!({"stock_code": "600000.SH"}))
            .unwrap();
        let thread = previous
            .create_thread(&json!({"title": "Preserved thread", "stock_code": "600000.SH"}))
            .unwrap();
        let thread_id = thread["id"].as_str().unwrap().to_string();
        let response = previous
            .query(&json!({"query": "shared evidence", "stock_code": "600000.SH"}))
            .unwrap();
        previous
            .save_answer(&thread_id, "What is shared?", &response)
            .unwrap();

        let staging = ResearchStore::open(&staging_path).unwrap();
        staging
            .ingest_documents(std::slice::from_ref(&document))
            .unwrap();
        preserve_local_research_state(&previous_path, &staging)
            .expect("local state should be preserved");

        assert_eq!(staging.overview(&json!({})).unwrap()["unread_count"], 0);
        let detail = staging.thread(&thread_id).unwrap();
        assert_eq!(detail["answers"].as_array().map(Vec::len), Some(1));
        assert!(
            detail["answers"][0]["citations"][0]["retrieval_score"]
                .as_f64()
                .unwrap_or_default()
                > 0.0
        );
        drop(previous);
        drop(staging);
        let _ = remove_sqlite_files(&previous_path);
        let _ = remove_sqlite_files(&staging_path);
    }

    #[test]
    fn rejects_unknown_or_missing_model_citations() {
        let allowed = vec!["C1".to_string(), "C2".to_string()];
        assert!(validate_model_answer("订单保持增长 [C1]。", &allowed).is_ok());
        assert!(validate_model_answer("订单保持增长 [C9]。", &allowed).is_err());
        assert!(validate_model_answer("订单保持增长。", &allowed).is_err());
    }

    #[test]
    fn exports_real_v2_sqlite_pack_without_chat_or_vectors() {
        let source_path = temporary_database_path("pack-source");
        let pack_path = temporary_database_path("pack-export");
        let imported_path = temporary_database_path("pack-import");
        let source = ResearchStore::open(&source_path).unwrap();
        source
            .ingest_documents(&[json!({
                "document_id": "pack-doc",
                "title": "同步公告",
                "content": "同步包中的公告证据。",
                "source_tier": "filing",
                "stock_codes": ["300750.SZ"]
            })])
            .unwrap();
        source.export_portable_pack(&pack_path).unwrap();

        let bytes = fs::read(&pack_path).unwrap();
        assert!(bytes.starts_with(b"SQLite format 3\0"));
        let pack = Connection::open(&pack_path).unwrap();
        let version: i64 = pack
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        assert!(pack
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='embeddings'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .optional()
            .unwrap()
            .is_none());
        drop(pack);

        let imported = ResearchStore::open(&imported_path).unwrap();
        imported.import_portable_pack(&pack_path).unwrap();
        assert_eq!(imported.index_status().unwrap()["document_count"], 1);
        assert_eq!(imported.index_status().unwrap()["fts_count"], 1);

        drop(source);
        drop(imported);
        for path in [source_path, pack_path, imported_path] {
            let _ = fs::remove_file(path);
        }
    }
}
