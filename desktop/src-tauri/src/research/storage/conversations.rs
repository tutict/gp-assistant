use crate::research::storage::documents::bool_field;
use crate::research::storage::documents::metadata_epoch_millis;
use crate::research::storage::documents::metadata_string;
use crate::research::storage::documents::metadata_strings;
use crate::research::storage::documents::normalized_string_array;
use crate::research::storage::documents::string_field;
use crate::research::storage::evidence::ResearchCitation;
use crate::research::storage::retrieval::excerpt;
use crate::research::storage::ResearchStore;
use crate::research::time::epoch_millis;
use crate::research::time::unique_suffix;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;

pub(crate) const MAX_RESEARCH_THREAD_ID_BYTES: usize = 256;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResearchMessage {
    pub id: String,
    pub document_id: String,
    pub stock_code: Option<String>,
    pub title: String,
    pub summary: String,
    pub sentiment: String,
    pub source_tier: String,
    pub source_name: String,
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at_epoch_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub article_summary_fetched_at_epoch_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub unread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapped_stock_codes: Option<Vec<String>>,
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

impl ResearchStore {
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
                "SELECT messages.id, messages.document_id, messages.stock_code, messages.title,
                        messages.summary, messages.sentiment, messages.source_tier,
                        documents.source_name, messages.published_at, messages.unread,
                        documents.metadata_json, documents.url, documents.content
                 FROM research_messages messages
                 JOIN documents ON documents.id = messages.document_id
                 WHERE (?1 IS NULL OR messages.stock_code = ?1)
                   AND json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL
                   AND (?2 = 0 OR messages.unread = 1)
                 ORDER BY COALESCE(messages.published_at, '') DESC, messages.created_at_epoch_ms DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|error| format!("failed to prepare research messages: {error}"))?;
        let items = statement
            .query_map(params![stock_code, unread_only, limit, offset], |row| {
                let title: String = row.get(3)?;
                let content: String = row.get(12)?;
                let metadata_json: String = row.get(10)?;
                let metadata: Value = serde_json::from_str(&metadata_json).unwrap_or(Value::Null);
                // Derive from full content so an old truncated headline does not
                // hide the actual summary. Documents and saved citations stay intact.
                let summary = message_summary_with_metadata(&title, &content, &metadata_json);
                let summary_kind = if summary.is_empty() {
                    "unavailable"
                } else if metadata
                    .get("article_summary")
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.chars().count() >= 15 && text.trim() != title.trim())
                {
                    "article_summary"
                } else {
                    "excerpt"
                };
                Ok(ResearchMessage {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    stock_code: row.get(2)?,
                    title,
                    summary,
                    sentiment: row.get(5)?,
                    source_tier: row.get(6)?,
                    source_name: row.get(7)?,
                    published_at: row.get(8)?,
                    fetched_at_epoch_ms: metadata_epoch_millis(&metadata, "fetched_at_epoch_ms"),
                    article_summary_fetched_at_epoch_ms: metadata_epoch_millis(
                        &metadata,
                        "article_summary_fetched_at_epoch_ms",
                    ),
                    summary_kind: Some(summary_kind.to_owned()),
                    url: row.get(11)?,
                    unread: row.get::<_, i64>(9)? != 0,
                    scope_type: metadata_string(row.get::<_, String>(10)?, "scope_type"),
                    scope_tags: metadata_strings(row.get::<_, String>(10)?, "scope_tags"),
                    mapped_stock_codes: metadata_strings(
                        row.get::<_, String>(10)?,
                        "mapped_stock_codes",
                    ),
                })
            })
            .map_err(|error| format!("failed to query research messages: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read research messages: {error}"))?;
        Ok(json!({"items": items, "limit": limit, "offset": offset}))
    }
}

impl ResearchStore {
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
}

impl ResearchStore {
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
}

impl ResearchStore {
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
}

impl ResearchStore {
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
}

impl ResearchStore {
    pub(crate) fn delete_thread(&self, thread_id: &str) -> Result<usize, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start research thread deletion: {error}"))?;
        let affected_document_ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT DISTINCT citations.document_id
                     FROM research_answer_citations citations
                     JOIN research_answers answers ON answers.id = citations.answer_id
                     WHERE answers.thread_id = ?1",
                )
                .map_err(|error| {
                    format!("failed to prepare cited documents for deletion: {error}")
                })?;
            let rows = statement
                .query_map(params![thread_id], |row| row.get::<_, String>(0))
                .map_err(|error| format!("failed to query cited documents for deletion: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to read cited documents for deletion: {error}"))?;
            rows
        };
        let deleted = transaction
            .execute(
                "DELETE FROM research_threads WHERE id = ?1",
                params![thread_id],
            )
            .map_err(|error| format!("failed to delete research thread: {error}"))?;
        if deleted == 0 {
            return Err("research thread not found".to_string());
        }
        for document_id in affected_document_ids {
            transaction
                .execute(
                    "UPDATE documents
                     SET cited_count = (
                         SELECT COUNT(*) FROM research_answer_citations
                         WHERE document_id = ?1
                     )
                     WHERE id = ?1",
                    params![document_id],
                )
                .map_err(|error| format!("failed to reconcile cited document count: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit research thread deletion: {error}"))?;
        Ok(deleted)
    }
}

impl ResearchStore {
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
            if citation.unavailable {
                return Err(
                    "unavailable historical evidence cannot be saved as an active citation"
                        .to_string(),
                );
            }
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
}

impl ResearchStore {
    pub(in crate::research) fn answer_citations(
        &self,
        connection: &Connection,
        answer_id: &str,
    ) -> Result<Vec<ResearchCitation>, String> {
        let mut statement = connection
            .prepare(
                "SELECT ac.citation_id, c.document_id, c.id, c.title, c.content,
                        c.source_tier, c.source_name, c.published_at, c.url, c.page_number,
                        ac.lexical_score, ac.vector_score, ac.retrieval_score, d.remote_export_allowed
                 FROM research_answer_citations ac
                 JOIN chunks c ON c.id = ac.chunk_id
                 JOIN documents d ON d.id = c.document_id
                 WHERE ac.answer_id = ?1 ORDER BY ac.ordinal",
            )
            .map_err(|error| format!("failed to prepare stored citations: {error}"))?;
        let mut citations = statement
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
                    unavailable: false,
                    remote_export_allowed: row.get::<_, i64>(13)? != 0,
                })
            })
            .map_err(|error| format!("failed to query stored citations: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read stored citations: {error}"))?;
        let mut snapshots = connection.prepare(
            "SELECT payload_json FROM research_unavailable_citations WHERE answer_id=?1 ORDER BY ordinal"
        ).map_err(|error| error.to_string())?;
        let payloads = snapshots
            .query_map([answer_id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        for payload in payloads {
            let mut citation: ResearchCitation =
                serde_json::from_str(&payload.map_err(|error| error.to_string())?)
                    .map_err(|error| format!("invalid historical citation snapshot: {error}"))?;
            citation.unavailable = true;
            citations.push(citation);
        }
        let mut ordinals = connection.prepare(
            "SELECT citation_id,ordinal FROM research_answer_citations WHERE answer_id=?1
             UNION ALL SELECT citation_id,ordinal FROM research_unavailable_citations WHERE answer_id=?1"
        ).map_err(|error| error.to_string())?;
        let order = ordinals
            .query_map([answer_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(|error| error.to_string())?;
        citations.sort_by_key(|citation| {
            order
                .get(&citation.citation_id)
                .copied()
                .unwrap_or(i64::MAX)
        });
        // Citation labels preserve answer identity; unavailable snapshots never join active chunks.
        Ok(citations)
    }
}

pub(in crate::research) fn message_summary_with_metadata(
    title: &str,
    content: &str,
    metadata_json: &str,
) -> String {
    if let Ok(metadata) = serde_json::from_str::<Value>(metadata_json) {
        if let Some(summary) = metadata
            .get("article_summary")
            .and_then(Value::as_str)
            .filter(|text| text.chars().count() >= 15 && text.trim() != title.trim())
        {
            return excerpt(summary.trim(), 240);
        }
    }
    message_summary(title, content)
}

fn message_summary(title: &str, content: &str) -> String {
    let heading = title.trim();
    let mut body = content.trim();
    if !heading.is_empty() {
        if body.replace(heading, "").trim().is_empty() {
            return String::new();
        }
        while let Some(rest) = body.strip_prefix(heading) {
            if rest
                .chars()
                .next()
                .is_some_and(|character| !character.is_whitespace())
            {
                break;
            }
            body = rest.trim_start();
        }
    }
    excerpt(body, 180)
}
