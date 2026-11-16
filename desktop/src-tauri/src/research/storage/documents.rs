use crate::research::schema::RESEARCH_SCHEMA_VERSION;
use crate::research::storage::conversations::message_summary_with_metadata;
use crate::research::storage::evidence::MAX_CITATIONS;
use crate::research::storage::retrieval::tokenize_for_fts;
use crate::research::storage::vectors::invalidate_vector_cache;
use crate::research::storage::vectors::vector_backend_status;
use crate::research::storage::vectors::RESEARCH_EMBEDDING_BLOB_BYTES;
use crate::research::storage::vectors::RESEARCH_EMBEDDING_DIMENSIONS;
use crate::research::storage::ResearchStore;
use crate::research::time::epoch_millis;
use crate::research::time::unique_suffix;
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::fs;

const DEFAULT_CHUNK_CHARS: usize = 520;

const DEFAULT_CHUNK_OVERLAP: usize = 80;

const MAX_RESEARCH_CHUNKS: i64 = 50_000;

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

impl ResearchStore {
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
            let user_imported = bool_field(document, "user_imported");
            let remote_export_allowed = document
                .get("remote_export_allowed")
                .and_then(Value::as_bool)
                .unwrap_or(!user_imported);
            let stock_codes = normalized_string_array(document.get("stock_codes"));
            let entities = normalized_string_array(document.get("entities"));
            let relation_types = normalized_string_array(document.get("relation_types"));
            let sentiment =
                string_field(document, "sentiment").unwrap_or_else(|| "uncertain".to_string());
            let page_number = document
                .get("page_number")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok());
            let stock_codes_json = serde_json::to_string(&stock_codes)
                .map_err(|error| format!("failed to encode stock codes: {error}"))?;
            let entities_json = serde_json::to_string(&entities)
                .map_err(|error| format!("failed to encode entities: {error}"))?;
            let relation_types_json = serde_json::to_string(&relation_types)
                .map_err(|error| format!("failed to encode relation types: {error}"))?;
            let metadata_json = document_metadata(document).to_string();
            let content_hash = sha256_hex(content.as_bytes());
            // Revocation applies to the logical source, including versions retained
            // for saved citations. Public cache refreshes still respect user consent.
            if !remote_export_allowed {
                transaction
                    .execute(
                        "UPDATE documents SET remote_export_allowed=0, updated_at_epoch_ms=?3
                     WHERE (id=?1 OR substr(id,1,length(?1)+10)=?1 || ':revision:')
                       AND (?2=1 OR user_imported=0) AND remote_export_allowed<>0",
                        params![document_id, user_imported, imported_at],
                    )
                    .map_err(|error| {
                        format!("failed to revoke retained source permission: {error}")
                    })?;
            }
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
            let derived_metadata_unchanged = content_unchanged
                && transaction
                    .query_row(
                        "SELECT title, page_number, stock_codes_json, entities_json,
                                relation_types_json, sentiment, source_tier, source_name,
                                url, published_at
                         FROM chunks WHERE document_id = ?1 ORDER BY ordinal LIMIT 1",
                        params![document_id],
                        |row| {
                            Ok(row.get::<_, String>(0)? == title
                                && row.get::<_, Option<u32>>(1)? == page_number
                                && row.get::<_, String>(2)? == stock_codes_json
                                && row.get::<_, String>(3)? == entities_json
                                && row.get::<_, String>(4)? == relation_types_json
                                && row.get::<_, String>(5)? == sentiment
                                && row.get::<_, String>(6)? == source_tier
                                && row.get::<_, String>(7)? == source_name
                                && row.get::<_, Option<String>>(8)? == url
                                && row.get::<_, Option<String>>(9)? == published_at
                                && transaction
                                    .query_row(
                                        "SELECT metadata_json FROM documents WHERE id = ?1",
                                        params![document_id],
                                        |metadata_row| metadata_row.get::<_, String>(0),
                                    )
                                    .optional()?
                                    .as_deref()
                                    == Some(metadata_json.as_str()))
                        },
                    )
                    .optional()
                    .map_err(|error| {
                        format!("failed to inspect indexed document metadata: {error}")
                    })?
                    .unwrap_or(false);

            transaction
                .execute(
                    "INSERT INTO documents (
                        id, title, content, source_tier, source_name, url, published_at,
                        imported_at_epoch_ms, updated_at_epoch_ms, content_hash, user_imported,
                        remote_export_allowed, pinned, cited_count, metadata_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, ?11, ?12, 0, ?13)
                    ON CONFLICT(id) DO UPDATE SET
                        title=excluded.title, content=excluded.content,
                        source_tier=excluded.source_tier, source_name=excluded.source_name,
                        url=excluded.url, published_at=excluded.published_at,
                        updated_at_epoch_ms=CASE WHEN documents.title IS excluded.title
                            AND documents.content_hash IS excluded.content_hash
                            AND documents.content IS excluded.content
                            AND documents.source_tier IS excluded.source_tier
                            AND documents.source_name IS excluded.source_name
                            AND documents.url IS excluded.url
                            AND documents.published_at IS excluded.published_at
                            AND documents.metadata_json IS excluded.metadata_json
                            AND ?14=1
                            AND documents.user_imported=MAX(documents.user_imported,excluded.user_imported)
                            AND documents.remote_export_allowed=CASE WHEN excluded.user_imported=1 THEN excluded.remote_export_allowed
                                WHEN documents.user_imported=1 THEN documents.remote_export_allowed ELSE excluded.remote_export_allowed END
                            THEN documents.updated_at_epoch_ms ELSE excluded.updated_at_epoch_ms END,
                        content_hash=excluded.content_hash,
                        user_imported=MAX(documents.user_imported, excluded.user_imported),
                        remote_export_allowed=CASE WHEN excluded.user_imported = 1 THEN excluded.remote_export_allowed WHEN documents.user_imported = 1 THEN documents.remote_export_allowed ELSE excluded.remote_export_allowed END,
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
                        user_imported,
                        remote_export_allowed,
                        bool_field(document, "pinned"),
                        metadata_json,
                        derived_metadata_unchanged
                    ],
                )
                .map_err(|error| format!("failed to store research document: {error}"))?;

            if derived_metadata_unchanged {
                continue;
            }

            if content_unchanged {
                refresh_unchanged_document_metadata(
                    &transaction,
                    &document_id,
                    &title,
                    page_number,
                    &stock_codes_json,
                    &entities_json,
                    &relation_types_json,
                    &sentiment,
                    &source_tier,
                    &source_name,
                    url.as_deref(),
                    published_at.as_deref(),
                    &content,
                    &stock_codes,
                    &metadata_json,
                    imported_at,
                )?;
                invalidate_vector_cache(&self.path);
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
                            message_summary_with_metadata(&title, &content, &metadata_json),
                            sentiment,
                            source_tier,
                            published_at,
                            imported_at
                        ],
                    )
                    .map_err(|error| format!("failed to create research message: {error}"))?;
            }
        }

        let retention = prune_retention_transaction(&transaction, MAX_RESEARCH_CHUNKS)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit research import: {error}"))?;
        if !documents.is_empty() || retention["removed_chunks"].as_u64().unwrap_or(0) > 0 {
            invalidate_vector_cache(&self.path);
        }
        Ok(json!({
            "document_count": documents.len(),
            "chunk_count": chunk_count,
            "schema_version": RESEARCH_SCHEMA_VERSION,
            "retention": retention
        }))
    }
}

impl ResearchStore {
    pub(crate) fn prune_retention(&self) -> Result<Value, String> {
        self.prune_retention_to_limit(MAX_RESEARCH_CHUNKS)
    }
}

impl ResearchStore {
    pub(in crate::research) fn prune_retention_to_limit(
        &self,
        chunk_limit: i64,
    ) -> Result<Value, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start research retention cleanup: {error}"))?;
        let retention = prune_retention_transaction(&transaction, chunk_limit)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit retention cleanup: {error}"))?;
        if retention["removed_chunks"].as_u64().unwrap_or(0) > 0 {
            invalidate_vector_cache(&self.path);
        }
        Ok(retention)
    }
}

impl ResearchStore {
    pub(crate) fn overview(&self, request: &Value) -> Result<Value, String> {
        let connection = self.connection()?;
        let stock_code = string_field(request, "stock_code");
        let unread_count: i64 = if let Some(code) = stock_code.as_deref() {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM research_messages messages
                     JOIN documents ON documents.id = messages.document_id
                     WHERE unread = 1 AND stock_code = ?1
                       AND json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL",
                    params![code],
                    |row| row.get(0),
                )
                .map_err(|error| format!("failed to count unread research messages: {error}"))?
        } else {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM research_messages messages
                     JOIN documents ON documents.id = messages.document_id
                     WHERE unread = 1
                       AND json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| format!("failed to count unread research messages: {error}"))?
        };
        let mut unread_by_stock = HashMap::new();
        let mut unread_statement = connection
            .prepare(
                "SELECT stock_code, COUNT(*) FROM research_messages messages
                 JOIN documents ON documents.id = messages.document_id
                 WHERE unread = 1 AND stock_code IS NOT NULL AND stock_code != ''
                   AND json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL
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
            .query_row("SELECT COUNT(*) FROM documents WHERE json_extract(metadata_json, '$.news_cache_duplicate_of') IS NULL", [], |row| row.get(0))
            .map_err(|error| format!("failed to count research documents: {error}"))?;
        let chunk_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks JOIN documents ON documents.id = chunks.document_id
                        WHERE json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL", [], |row| row.get(0))
            .map_err(|error| format!("failed to count research chunks: {error}"))?;
        let updated_at_epoch_ms: Option<i64> = connection
            .query_row(
                "SELECT COALESCE(
                    (SELECT CAST(value AS INTEGER) FROM research_metadata WHERE key = 'last_refresh_at_epoch_ms'),
                    (SELECT MAX(updated_at_epoch_ms) FROM documents)
                )",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to read research refresh time: {error}"))?;
        let mut source_statement = connection
            .prepare(
                "SELECT source_tier, COUNT(*) FROM documents
                 WHERE json_extract(metadata_json, '$.news_cache_duplicate_of') IS NULL
                 GROUP BY source_tier
                 ORDER BY CASE source_tier
                    WHEN 'policy_official' THEN 1 WHEN 'filing' THEN 2
                    WHEN 'financial_snapshot' THEN 3 WHEN 'news_media' THEN 4
                    WHEN 'news' THEN 5 WHEN 'research_report' THEN 6
                    WHEN 'community' THEN 7 ELSE 8 END",
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
            "updated_at_epoch_ms": updated_at_epoch_ms,
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
}

impl ResearchStore {
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
        let missing_fts_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chunks c
                 LEFT JOIN chunks_fts f ON f.chunk_id = c.id
                 WHERE f.chunk_id IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count missing FTS rows: {error}"))?;
        let orphan_fts_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts f
                 LEFT JOIN chunks c ON c.id = f.chunk_id
                 WHERE c.id IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count orphan FTS rows: {error}"))?;
        let pending_embedding_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chunks c
                 LEFT JOIN embeddings e ON e.chunk_id = c.id
                 WHERE e.chunk_id IS NULL OR e.content_hash <> c.content_hash",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count pending embeddings: {error}"))?;
        let stale_embedding_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chunks c
                 JOIN embeddings e ON e.chunk_id = c.id
                 WHERE e.content_hash <> c.content_hash",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count stale embeddings: {error}"))?;
        let orphan_embedding_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM embeddings e
                 LEFT JOIN chunks c ON c.id = e.chunk_id
                 WHERE c.id IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count orphan embeddings: {error}"))?;
        let invalid_embedding_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM embeddings
                 WHERE dimensions <> ?1 OR length(vector) <> ?2 OR normalized <> 1",
                params![RESEARCH_EMBEDDING_DIMENSIONS, RESEARCH_EMBEDDING_BLOB_BYTES],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count invalid embeddings: {error}"))?;
        let database_bytes = fs::metadata(&self.path)
            .map(|value| value.len())
            .unwrap_or(0);
        let fts_healthy = missing_fts_count == 0 && orphan_fts_count == 0;
        let vector_status = vector_backend_status();
        let vector_supported = vector_status
            .get("supported")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let model_ready = vector_status
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let embeddings_healthy = orphan_embedding_count == 0
            && invalid_embedding_count == 0
            && (!vector_supported || pending_embedding_count == 0);
        Ok(json!({
            "schema_version": RESEARCH_SCHEMA_VERSION,
            "document_count": document_count,
            "chunk_count": chunk_count,
            "fts_count": fts_count,
            "embedding_count": embedding_count,
            "database_bytes": database_bytes,
            "healthy": chunk_count == fts_count,
            "fts_healthy": fts_healthy,
            "integrity_healthy": fts_healthy && embeddings_healthy,
            "hybrid_ready": fts_healthy && model_ready && embedding_count > 0 && embeddings_healthy,
            "fts_missing_count": missing_fts_count,
            "fts_orphan_count": orphan_fts_count,
            "embedding_pending_count": pending_embedding_count,
            "embedding_stale_count": stale_embedding_count,
            "embedding_orphan_count": orphan_embedding_count,
            "embedding_invalid_count": invalid_embedding_count,
            "vector": vector_status
        }))
    }
}

impl ResearchStore {
    pub(crate) fn document_statuses(&self) -> Result<Value, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT d.id, d.title, d.source_tier, d.updated_at_epoch_ms,
                        COUNT(DISTINCT c.id), COUNT(DISTINCT e.chunk_id)
                 FROM documents d
                 LEFT JOIN chunks c ON c.document_id = d.id
                 LEFT JOIN embeddings e ON e.chunk_id = c.id
                 WHERE json_extract(d.metadata_json, '$.news_cache_duplicate_of') IS NULL
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
}

impl ResearchStore {
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
}

impl ResearchStore {
    pub(crate) fn sentiment_documents(
        &self,
        stock_code: &str,
        cutoff: i64,
    ) -> Result<Vec<Value>, String> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.id,d.title,d.content,d.source_tier,d.source_name,d.url,d.published_at,
                    d.imported_at_epoch_ms,d.updated_at_epoch_ms,d.metadata_json,c.sentiment,c.stock_codes_json
             FROM documents d JOIN chunks c ON c.id=(SELECT id FROM chunks WHERE document_id=d.id ORDER BY ordinal LIMIT 1)
             WHERE d.imported_at_epoch_ms<=?2 AND d.updated_at_epoch_ms<=?2
               AND json_extract(d.metadata_json, '$.news_cache_duplicate_of') IS NULL
               AND (EXISTS(SELECT 1 FROM chunks sc,json_each(sc.stock_codes_json) code WHERE sc.document_id=d.id AND code.value=?1)
                    OR EXISTS(SELECT 1 FROM json_each(d.metadata_json,'$.mapped_stock_codes') code WHERE code.value=?1))
             ORDER BY d.published_at DESC,d.id LIMIT 10000"
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map(params![stock_code, cutoff], |row| {
            let metadata: String = row.get(9)?;
            let codes: String = row.get(11)?;
            Ok(json!({
                "document_id":row.get::<_,String>(0)?, "title":row.get::<_,String>(1)?,
                "content":row.get::<_,String>(2)?, "source_tier":row.get::<_,String>(3)?,
                "source_name":row.get::<_,String>(4)?, "url":row.get::<_,Option<String>>(5)?,
                "published_at":row.get::<_,Option<String>>(6)?, "first_seen_at":row.get::<_,i64>(7)?,
                "updated_at":row.get::<_,i64>(8)?, "metadata":serde_json::from_str::<Value>(&metadata).unwrap_or(json!({})),
                "sentiment":row.get::<_,String>(10)?, "stock_codes":serde_json::from_str::<Value>(&codes).unwrap_or(json!([]))
            }))
        }).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
}

impl ResearchStore {
    pub(crate) fn sentiment_generation(&self) -> Result<String, String> {
        let connection = self.connection()?;
        connection.execute("INSERT OR IGNORE INTO research_metadata(key,value) VALUES('sentiment_generation',?1)",
            [format!("{}-{}", epoch_millis(), unique_suffix())]).map_err(|error| error.to_string())?;
        connection
            .query_row(
                "SELECT value FROM research_metadata WHERE key='sentiment_generation'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }
}

impl ResearchStore {
    pub(in crate::research) fn reset_sentiment_generation(&self) -> Result<(), String> {
        self.connection()?.execute("INSERT OR REPLACE INTO research_metadata(key,value) VALUES('sentiment_generation',?1)",
            [format!("{}-{}", epoch_millis(), unique_suffix())]).map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn prune_retention_transaction(
    transaction: &rusqlite::Transaction<'_>,
    chunk_limit: i64,
) -> Result<Value, String> {
    if chunk_limit < 0 {
        return Err("research chunk limit cannot be negative".to_string());
    }
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
        removed_chunks += delete_document(transaction, &document_id)?;
        removed_documents += 1;
    }
    let mut chunk_count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .map_err(|error| format!("failed to count retained chunks: {error}"))?;
    if chunk_count > chunk_limit {
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
            if chunk_count <= chunk_limit {
                break;
            }
            let deleted = delete_document(transaction, &document_id)?;
            chunk_count -= deleted as i64;
            removed_chunks += deleted;
            removed_documents += 1;
        }
    }
    if chunk_count > chunk_limit {
        return Err(format!(
            "research chunk limit {chunk_limit} cannot be satisfied: {chunk_count} protected chunks remain"
        ));
    }
    Ok(json!({
        "removed_documents": removed_documents,
        "removed_chunks": removed_chunks,
        "chunk_count": chunk_count,
        "chunk_limit": chunk_limit,
        "news_community_days": 365
    }))
}

#[allow(clippy::too_many_arguments)]
fn refresh_unchanged_document_metadata(
    transaction: &rusqlite::Transaction<'_>,
    document_id: &str,
    title: &str,
    page_number: Option<u32>,
    stock_codes_json: &str,
    entities_json: &str,
    relation_types_json: &str,
    sentiment: &str,
    source_tier: &str,
    source_name: &str,
    url: Option<&str>,
    published_at: Option<&str>,
    content: &str,
    stock_codes: &[String],
    metadata_json: &str,
    imported_at: i64,
) -> Result<(), String> {
    let existing_messages = {
        let mut statement = transaction
            .prepare(
                "SELECT id, unread, created_at_epoch_ms
                 FROM research_messages WHERE document_id = ?1",
            )
            .map_err(|error| format!("failed to prepare existing research messages: {error}"))?;
        let rows = statement
            .query_map(params![document_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, bool>(1)?, row.get::<_, i64>(2)?),
                ))
            })
            .map_err(|error| format!("failed to query existing research messages: {error}"))?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(|error| format!("failed to read existing research messages: {error}"))?;
        rows
    };
    let chunk_rows = {
        let mut statement = transaction
            .prepare("SELECT id, content FROM chunks WHERE document_id = ?1 ORDER BY ordinal")
            .map_err(|error| format!("failed to prepare unchanged research chunks: {error}"))?;
        let rows = statement
            .query_map(params![document_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("failed to query unchanged research chunks: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read unchanged research chunks: {error}"))?;
        rows
    };

    transaction
        .execute(
            "DELETE FROM chunks_fts WHERE chunk_id IN (
                SELECT id FROM chunks WHERE document_id = ?1
             )",
            params![document_id],
        )
        .map_err(|error| format!("failed to clear unchanged research FTS rows: {error}"))?;
    transaction
        .execute(
            "UPDATE chunks SET
                title = ?2, page_number = ?3, stock_codes_json = ?4,
                entities_json = ?5, relation_types_json = ?6, sentiment = ?7,
                source_tier = ?8, source_name = ?9, url = ?10, published_at = ?11
             WHERE document_id = ?1",
            params![
                document_id,
                title,
                page_number,
                stock_codes_json,
                entities_json,
                relation_types_json,
                sentiment,
                source_tier,
                source_name,
                url,
                published_at
            ],
        )
        .map_err(|error| format!("failed to refresh unchanged research chunks: {error}"))?;
    for (chunk_id, chunk_content) in chunk_rows {
        transaction
            .execute(
                "INSERT INTO chunks_fts (chunk_id, title_terms, entity_terms, body_terms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    chunk_id,
                    tokenize_for_fts(title),
                    tokenize_for_fts(entities_json),
                    tokenize_for_fts(&chunk_content)
                ],
            )
            .map_err(|error| format!("failed to refresh unchanged research FTS row: {error}"))?;
    }

    transaction
        .execute(
            "DELETE FROM research_messages WHERE document_id = ?1",
            params![document_id],
        )
        .map_err(|error| format!("failed to replace unchanged research messages: {error}"))?;
    let message_stocks = if stock_codes.is_empty() {
        vec![None]
    } else {
        stock_codes.iter().map(String::as_str).map(Some).collect()
    };
    for message_stock in message_stocks {
        let message_scope = message_stock.unwrap_or("all");
        let message_id = format!("message:{document_id}:{message_scope}");
        let (unread, created_at) = existing_messages
            .get(&message_id)
            .map(|(unread, created_at)| (*unread, *created_at))
            .unwrap_or((true, imported_at));
        transaction
            .execute(
                "INSERT INTO research_messages (
                    id, document_id, stock_code, title, summary, sentiment, source_tier,
                    published_at, unread, created_at_epoch_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    message_id,
                    document_id,
                    message_stock,
                    title,
                    message_summary_with_metadata(title, content, metadata_json),
                    sentiment,
                    source_tier,
                    published_at,
                    unread,
                    created_at
                ],
            )
            .map_err(|error| format!("failed to refresh unchanged research message: {error}"))?;
    }
    Ok(())
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

pub(in crate::research) fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(in crate::research) fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn document_metadata(document: &Value) -> Value {
    let mut metadata = document
        .get("metadata")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(object) = metadata.as_object_mut() {
        for key in ["scope_type", "scope_tags", "mapped_stock_codes"] {
            if let Some(value) = document.get(key) {
                object.insert(key.to_string(), value.clone());
            }
        }
    }
    metadata
}

pub(in crate::research) fn metadata_epoch_millis(metadata: &Value, key: &str) -> Option<i64> {
    metadata
        .get(key)
        .and_then(Value::as_i64)
        .filter(|time| *time > 0 && *time <= 8_640_000_000_000_000)
}

pub(in crate::research) fn metadata_string(raw: String, key: &str) -> Option<String> {
    serde_json::from_str::<Value>(&raw).ok().and_then(|value| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

pub(in crate::research) fn metadata_strings(raw: String, key: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Value>(&raw).ok().and_then(|value| {
        value.get(key).and_then(Value::as_array).map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
    })
}

pub(in crate::research) fn normalized_string_array(value: Option<&Value>) -> Vec<String> {
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
        "policy_official" | "official_policy" | "policy" => "policy_official",
        "news_media" | "media" | "traditional_media" => "news_media",
        "filing" | "official" | "announcement" => "filing",
        "financial" | "financial_snapshot" | "finance" => "financial_snapshot",
        "research" | "research_report" | "report" => "research_report",
        "community" | "social" => "community",
        _ => "news",
    }
    .to_string()
}

pub(in crate::research) fn source_rank(value: &str) -> u8 {
    match normalize_source_tier(value).as_str() {
        "policy_official" => 7,
        "filing" => 6,
        "financial_snapshot" => 5,
        "news_media" => 4,
        "news" => 3,
        "research_report" => 2,
        "community" => 1,
        _ => 0,
    }
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

pub(in crate::research) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
