use crate::research::storage::retrieval::load_candidate;
use crate::research::storage::retrieval::Candidate;
use crate::research::storage::ResearchStore;
use crate::research::time::epoch_millis;
use rusqlite::params;
use rusqlite::Connection;
use serde_json::json;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

pub(in crate::research) const RESEARCH_EMBEDDING_DIMENSIONS: i64 = 512;

pub(in crate::research) const RESEARCH_EMBEDDING_BLOB_BYTES: i64 =
    RESEARCH_EMBEDDING_DIMENSIONS * 4;

impl ResearchStore {
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
}

impl ResearchStore {
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
            let vector = normalize_embedding(vector).map_err(|error| {
                format!("embedding {} cannot be normalized: {error}", item.chunk_id)
            })?;
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
                        encode_f32_blob(&vector),
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
}

impl ResearchStore {
    pub(in crate::research) fn vector_candidates(
        &self,
        connection: &Connection,
        query_vector: &[f32],
        stock_code: Option<&str>,
        remote_safe_only: bool,
        as_of: Option<&str>,
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
                (!remote_safe_only || row.remote_export_allowed)
                    && as_of
                        .map(|cutoff| {
                            row.published_at_utc
                                .as_deref()
                                .is_some_and(|published| published <= cutoff)
                        })
                        .unwrap_or(true)
                    && stock_code
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
    remote_export_allowed: bool,
    published_at_utc: Option<String>,
}

#[derive(Clone)]
struct VectorCacheEntry {
    count: i64,
    newest_epoch_ms: i64,
    newest_document_updated_at_epoch_ms: i64,
    rows: Arc<Vec<CachedVector>>,
}

static VECTOR_CACHE: OnceLock<Mutex<HashMap<PathBuf, VectorCacheEntry>>> = OnceLock::new();

fn load_vector_cache(
    connection: &Connection,
    path: &Path,
) -> Result<Arc<Vec<CachedVector>>, String> {
    let (count, newest_epoch_ms, newest_document_updated_at_epoch_ms) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(created_at_epoch_ms), 0),
                    COALESCE((SELECT MAX(updated_at_epoch_ms) FROM documents), 0)
             FROM embeddings",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(|error| format!("failed to inspect embedding cache version: {error}"))?;
    let cache = VECTOR_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache
            .lock()
            .map_err(|_| "vector cache lock is poisoned".to_string())?;
        if let Some(entry) = guard.get(path) {
            if entry.count == count
                && entry.newest_epoch_ms == newest_epoch_ms
                && entry.newest_document_updated_at_epoch_ms == newest_document_updated_at_epoch_ms
            {
                return Ok(Arc::clone(&entry.rows));
            }
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT e.chunk_id, c.stock_codes_json, e.vector, e.dimensions,
                    d.remote_export_allowed, research_timestamp_utc(c.published_at)
             FROM embeddings e JOIN chunks c ON c.id = e.chunk_id
             JOIN documents d ON d.id = c.document_id
             WHERE json_extract(d.metadata_json, '$.news_cache_duplicate_of') IS NULL",
        )
        .map_err(|error| format!("failed to prepare embedding cache: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            let chunk_id: String = row.get(0)?;
            let stock_codes_json: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            let dimensions: i64 = row.get(3)?;
            let remote_export_allowed: i64 = row.get(4)?;
            let published_at_utc: Option<String> = row.get(5)?;
            Ok((
                chunk_id,
                stock_codes_json,
                blob,
                dimensions,
                remote_export_allowed,
                published_at_utc,
            ))
        })
        .map_err(|error| format!("failed to query embedding cache: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read embedding cache: {error}"))?;
    let mut decoded = Vec::with_capacity(rows.len());
    for (chunk_id, stock_codes_json, blob, dimensions, remote_export_allowed, published_at_utc) in
        rows
    {
        let vector = decode_f32_blob(&blob)?;
        if vector.len() != dimensions.max(0) as usize || vector.len() != 512 {
            continue;
        }
        decoded.push(CachedVector {
            chunk_id,
            stock_codes: serde_json::from_str(&stock_codes_json).unwrap_or_default(),
            vector,
            remote_export_allowed: remote_export_allowed != 0,
            published_at_utc,
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
                newest_document_updated_at_epoch_ms,
                rows: Arc::clone(&rows),
            },
        );
    Ok(rows)
}

pub(in crate::research) fn invalidate_vector_cache(path: &Path) {
    if let Some(cache) = VECTOR_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.remove(path);
        }
    }
}

fn encode_f32_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn normalize_embedding(vector: &[f32]) -> Result<Vec<f32>, String> {
    if vector.iter().any(|value| !value.is_finite()) {
        return Err("vector contains a non-finite value".to_string());
    }
    let norm = vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return Err("vector norm is zero".to_string());
    }
    Ok(vector
        .iter()
        .map(|value| f64::from(*value) / norm)
        .map(|value| value as f32)
        .collect())
}

pub(in crate::research) fn decode_f32_blob(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err("embedding BLOB length is not aligned to f32".to_string());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
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

pub(in crate::research) fn vector_backend_status() -> Value {
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

pub(crate) fn finalize_research_index_status(mut status: Value, vector_status: Value) -> Value {
    let count = |field: &str| status.get(field).and_then(Value::as_i64).unwrap_or(0);
    let model_ready = vector_status
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let hybrid_ready = status
        .get("fts_healthy")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && model_ready
        && count("embedding_count") > 0
        && count("embedding_pending_count") == 0
        && count("embedding_stale_count") == 0
        && count("embedding_orphan_count") == 0
        && count("embedding_invalid_count") == 0;
    status["vector"] = vector_status;
    status["hybrid_ready"] = Value::Bool(hybrid_ready);
    status
}
