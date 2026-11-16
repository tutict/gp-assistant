use crate::research::storage::documents::bool_field;
use crate::research::storage::documents::string_field;
use crate::research::storage::retrieval::build_fts_match_query;
use crate::research::storage::retrieval::excerpt;
use crate::research::storage::ResearchStore;
use crate::research::time::normalize_research_as_of;
use crate::research::time::normalized_research_timestamp;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

pub(in crate::research) const MAX_CITATIONS: usize = 8;

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
    /// Historical evidence absent or changed in the active document generation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unavailable: bool,
    #[serde(default)]
    pub remote_export_allowed: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ResearchStore {
    pub(crate) fn agent_evidence(&self, request: &Value) -> Result<Value, String> {
        let stock_code = request
            .get("stock_code")
            .and_then(Value::as_str)
            .filter(|code| {
                code.len() == 8
                    && code.ends_with(".HK")
                    && code.as_bytes()[..5].iter().all(u8::is_ascii_digit)
            })
            .ok_or_else(|| {
                "canonical HK stock_code is required (for example 00700.HK)".to_string()
            })?;
        self.agent_evidence_for_scope(request, Some(stock_code))
    }
}

impl ResearchStore {
    pub(crate) fn unscoped_hk_agent_evidence(&self, request: &Value) -> Result<Value, String> {
        self.agent_evidence_for_scope(request, None)
    }
}

impl ResearchStore {
    fn agent_evidence_for_scope(
        &self,
        request: &Value,
        stock_code: Option<&str>,
    ) -> Result<Value, String> {
        let query = string_field(request, "query").unwrap_or_default();
        if query.trim().is_empty() {
            return Err("query is required".to_string());
        }
        if request
            .get("market")
            .is_some_and(|market| market.as_str() != Some("hk"))
        {
            return Err("agent evidence supports only the hk market".to_string());
        }
        let top_k = request
            .get("top_k")
            .and_then(Value::as_u64)
            .unwrap_or(16)
            .clamp(1, 16) as usize;
        let remote_safe_only = bool_field(request, "remote_safe_only");
        let as_of = normalize_research_as_of(request.get("as_of"))?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to read agent evidence snapshot: {error}"))?;

        // Recent documents guarantee source coverage even when the question's keywords
        // occur only in news. Keyword hits select the relevant original chunk instead.
        let mut candidates = HashMap::<String, AgentEvidenceCandidate>::new();
        let mut recent = transaction.prepare(
            "WITH scoped AS (
                SELECT documents.id,
                    ROW_NUMBER() OVER (
                        PARTITION BY CASE WHEN documents.source_tier IN ('news', 'news_media') THEN 'news' ELSE documents.source_tier END
                        ORDER BY research_timestamp_utc(documents.published_at) DESC, documents.updated_at_epoch_ms DESC, documents.id
                    ) AS recency
                FROM documents JOIN chunks ON chunks.document_id = documents.id AND chunks.ordinal = 0
                WHERE json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL
                  AND (json_extract(documents.metadata_json, '$.market') IS NULL OR json_extract(documents.metadata_json, '$.market') = 'hk')
                  AND (?2 = 0 OR documents.remote_export_allowed = 1)
                  AND (?3 IS NULL OR research_timestamp_utc(chunks.published_at) <= ?3)
                  AND (?1 IS NULL OR EXISTS (SELECT 1 FROM json_each(chunks.stock_codes_json) WHERE value = ?1))
                  AND NOT EXISTS (SELECT 1 FROM json_each(chunks.stock_codes_json)
                      WHERE type != 'text' OR value NOT GLOB '[0-9][0-9][0-9][0-9][0-9].HK')
            )
            SELECT chunks.id, chunks.document_id, chunks.title, chunks.content, chunks.page_number,
                   chunks.source_tier, chunks.source_name, chunks.published_at, chunks.url,
                   documents.remote_export_allowed, chunks.stock_codes_json, documents.metadata_json, 0.0
                   , documents.content_hash, chunks.content_hash
            FROM scoped JOIN documents ON documents.id = scoped.id
            JOIN chunks ON chunks.document_id = documents.id AND chunks.ordinal = 0
            WHERE scoped.recency <= 48"
        ).map_err(|error| format!("failed to prepare recent agent evidence: {error}"))?;
        let rows = recent
            .query_map(
                params![stock_code, remote_safe_only as i64, as_of],
                agent_evidence_candidate,
            )
            .map_err(|error| format!("failed to read recent agent evidence: {error}"))?;
        for row in rows {
            let candidate =
                row.map_err(|error| format!("failed to decode recent agent evidence: {error}"))?;
            candidates.insert(candidate.document_id.clone(), candidate);
        }
        drop(recent);
        let mut lexical = transaction.prepare(
            "SELECT chunks.id, chunks.document_id, chunks.title, chunks.content, chunks.page_number,
                    chunks.source_tier, chunks.source_name, chunks.published_at, chunks.url,
                    documents.remote_export_allowed, chunks.stock_codes_json, documents.metadata_json,
                    -bm25(chunks_fts, 0.0, 3.0, 2.0, 1.0), documents.content_hash, chunks.content_hash
             FROM chunks_fts JOIN chunks ON chunks.id = chunks_fts.chunk_id
             JOIN documents ON documents.id = chunks.document_id
             WHERE chunks_fts MATCH ?1
               AND json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL
               AND (json_extract(documents.metadata_json, '$.market') IS NULL OR json_extract(documents.metadata_json, '$.market') = 'hk')
               AND (?3 = 0 OR documents.remote_export_allowed = 1)
               AND (?4 IS NULL OR research_timestamp_utc(chunks.published_at) <= ?4)
               AND (?2 IS NULL OR EXISTS (SELECT 1 FROM json_each(chunks.stock_codes_json) WHERE value = ?2))
               AND NOT EXISTS (SELECT 1 FROM json_each(chunks.stock_codes_json)
                   WHERE type != 'text' OR value NOT GLOB '[0-9][0-9][0-9][0-9][0-9].HK')
             ORDER BY bm25(chunks_fts, 0.0, 3.0, 2.0, 1.0), chunks.ordinal, chunks.id
             LIMIT 128"
        ).map_err(|error| format!("failed to prepare lexical agent evidence: {error}"))?;
        let rows = lexical
            .query_map(
                params![
                    build_fts_match_query(&query),
                    stock_code,
                    remote_safe_only as i64,
                    as_of
                ],
                agent_evidence_candidate,
            )
            .map_err(|error| format!("failed to read lexical agent evidence: {error}"))?;
        for row in rows {
            let candidate =
                row.map_err(|error| format!("failed to decode lexical agent evidence: {error}"))?;
            match candidates.entry(candidate.document_id.clone()) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if candidate.lexical_score > entry.get().lexical_score {
                        entry.insert(candidate);
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
            }
        }
        drop(lexical);
        transaction
            .commit()
            .map_err(|error| format!("failed to finish agent evidence snapshot: {error}"))?;

        let mut ranked = candidates.into_values().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            (right.lexical_score > 0.0)
                .cmp(&(left.lexical_score > 0.0))
                .then_with(|| right.published_at_utc.cmp(&left.published_at_utc))
                .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
                .then_with(|| left.document_id.cmp(&right.document_id))
        });
        let mut selected = Vec::new();
        let mut selected_ids = HashSet::new();
        for group in ["filing", "financial_snapshot", "news"] {
            if selected.len() >= top_k {
                break;
            }
            if let Some(candidate) = ranked.iter().find(|candidate| {
                agent_source_group(
                    candidate.citation["source_tier"]
                        .as_str()
                        .unwrap_or_default(),
                ) == group
            }) {
                selected_ids.insert(candidate.document_id.clone());
                selected.push(candidate.citation.clone());
            }
        }
        for candidate in ranked {
            if selected.len() >= top_k {
                break;
            }
            if selected_ids.insert(candidate.document_id) {
                selected.push(candidate.citation);
            }
        }
        for (index, citation) in selected.iter_mut().enumerate() {
            citation["citation_id"] = json!(format!("C{}", index + 1));
        }
        let community_only = !selected.is_empty()
            && selected
                .iter()
                .all(|item| item["source_tier"] == "community");
        let answer = if selected.is_empty() {
            "未找到足以支撑结论的本地证据。".to_string()
        } else if community_only {
            "当前只命中社区信息，不能单独作为事实结论；请补充公告、财务快照或可信新闻核验。"
                .to_string()
        } else {
            selected
                .iter()
                .map(|item| {
                    format!(
                        "[{}] {}",
                        item["citation_id"].as_str().unwrap_or_default(),
                        item["excerpt"].as_str().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(
            json!({"mode":"evidence_only","query":query.chars().take(2000).collect::<String>(),"stock_code":stock_code,"market":"hk",
            "answer":answer,"citations":selected,"community_only":community_only,"fact_supported":!community_only && !selected.is_empty(),
            "retrieval_mode":"scoped_sources_bm25_recency","remote_safe_only":remote_safe_only}),
        )
    }
}

impl ResearchStore {
    pub(crate) fn validate_hk_context(
        &self,
        context: &Value,
        remote_safe_only: bool,
    ) -> Result<bool, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        self.validate_hk_context_in_connection(&transaction, context, remote_safe_only)
    }
}

impl ResearchStore {
    fn validate_hk_context_in_connection(
        &self,
        connection: &Connection,
        context: &Value,
        remote_safe_only: bool,
    ) -> Result<bool, String> {
        let Some(code) = context
            .get("stock_code")
            .and_then(Value::as_str)
            .filter(|code| {
                code.len() == 8
                    && code.ends_with(".HK")
                    && code.as_bytes()[..5].iter().all(u8::is_ascii_digit)
            })
        else {
            return Ok(false);
        };
        if context.get("market").and_then(Value::as_str) != Some("hk") {
            return Ok(false);
        }
        let Some(evidence) = context
            .get("evidence")
            .and_then(Value::as_array)
            .filter(|evidence| {
                !evidence.is_empty()
                    && evidence.len() <= crate::hk_agent_research::MAX_EVIDENCE_ITEMS
            })
        else {
            return Ok(false);
        };
        self.validate_hk_catalog(connection, evidence, Some(code), remote_safe_only)
    }
}

impl ResearchStore {
    fn validate_hk_catalog(
        &self,
        connection: &Connection,
        evidence: &[Value],
        code: Option<&str>,
        remote_safe_only: bool,
    ) -> Result<bool, String> {
        let mut statement=connection.prepare(
            "SELECT c.id,c.document_id,c.title,c.content,c.page_number,c.source_tier,c.source_name,c.published_at,c.url,
                    d.remote_export_allowed,c.stock_codes_json,d.metadata_json,0.0,d.content_hash,c.content_hash
             FROM chunks c JOIN documents d ON d.id=c.document_id
             WHERE c.id=?1 AND d.id=?2
               AND json_extract(d.metadata_json,'$.news_cache_duplicate_of') IS NULL
               AND (json_extract(d.metadata_json,'$.market') IS NULL OR json_extract(d.metadata_json,'$.market')='hk')
               AND (?3=0 OR d.remote_export_allowed=1)
               AND (?4 IS NULL OR EXISTS(SELECT 1 FROM json_each(c.stock_codes_json) WHERE value=?4))
               AND NOT EXISTS(SELECT 1 FROM json_each(c.stock_codes_json)
                    WHERE type!='text' OR value NOT GLOB '[0-9][0-9][0-9][0-9][0-9].HK')"
        ).map_err(|error|error.to_string())?;
        let mut current = Vec::new();
        for item in evidence {
            let Some(document) = item.get("document_id").and_then(Value::as_str) else {
                return Ok(false);
            };
            let Some(chunk) = item.get("chunk_id").and_then(Value::as_str) else {
                return Ok(false);
            };
            if item
                .get("document_content_hash")
                .and_then(Value::as_str)
                .is_none()
                || item
                    .get("chunk_content_hash")
                    .and_then(Value::as_str)
                    .is_none()
            {
                return Ok(false);
            }
            let Some(mut candidate) = statement
                .query_row(
                    params![chunk, document, remote_safe_only as i64, code],
                    agent_evidence_candidate,
                )
                .optional()
                .map_err(|error| error.to_string())?
            else {
                return Ok(false);
            };
            candidate.citation["citation_id"] = item
                .get("citation_id")
                .cloned()
                .unwrap_or_else(|| json!(""));
            current.push(candidate.citation);
        }
        drop(statement);
        let source = json!({"citations":current});
        let canonical = match code {
            Some(code) => crate::hk_agent_research::evidence_catalog(&source, code),
            None => crate::hk_agent_research::unscoped_evidence_catalog(&source),
        };
        Ok(canonical == evidence)
    }
}

impl ResearchStore {
    pub(crate) fn validate_hk_model_evidence(&self, snapshots: &[Value]) -> Result<bool, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        for snapshot in snapshots {
            if snapshot.get("frozen_snapshot").and_then(Value::as_bool) == Some(true) {
                let context = crate::hk_agent_research::context_from_frozen_evidence(snapshot);
                if !self.validate_hk_context_in_connection(&transaction, &context, true)? {
                    return Ok(false);
                }
            } else {
                let Some(citations) = snapshot.get("citations").and_then(Value::as_array) else {
                    return Ok(false);
                };
                let catalog = crate::hk_agent_research::unscoped_evidence_catalog(snapshot);
                if catalog.len() != citations.len()
                    || !self.validate_hk_catalog(&transaction, &catalog, None, true)?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Debug)]
struct AgentEvidenceCandidate {
    document_id: String,
    published_at_utc: Option<String>,
    lexical_score: f64,
    citation: Value,
}

fn agent_source_group(tier: &str) -> &str {
    match tier {
        "news_media" | "news" => "news",
        other => other,
    }
}

fn bounded_agent_metadata(value: &Value, budget: &mut usize, depth: usize) -> Option<Value> {
    if depth > 6 || *budget < 4 {
        return None;
    }
    match value {
        Value::Object(object) => {
            *budget -= 2;
            let mut result = serde_json::Map::new();
            let preferred = [
                "market",
                "report_period",
                "raw_report_period",
                "report_date",
                "period_end",
                "disclosed_at",
                "currency",
                "unit",
                "verification_status",
                "audit_status",
                "metrics",
                "official_values",
                "verification_conflicts",
                "official_source_url",
                "official_source_name",
                "fetched_at",
                "source_kind",
                "article_summary_status",
                "article_summary_url",
                "article_summary_fetched_at_epoch_ms",
            ];
            let keys = preferred
                .iter()
                .filter_map(|key| object.get_key_value(*key))
                .chain(
                    object
                        .iter()
                        .filter(|(key, _)| !preferred.contains(&key.as_str())),
                )
                .take(48);
            for (key, item) in keys {
                if key.chars().count() > 128 {
                    continue;
                }
                let key_cost = serde_json::to_string(key).ok()?.chars().count() + 2;
                if *budget < key_cost + 4 {
                    break;
                }
                *budget -= key_cost;
                if let Some(item) = bounded_agent_metadata(item, budget, depth + 1) {
                    result.insert(key.clone(), item);
                }
            }
            Some(Value::Object(result))
        }
        Value::Array(items) => {
            *budget -= 2;
            let mut result = Vec::new();
            for item in items.iter().take(32) {
                if *budget < 5 {
                    break;
                }
                *budget -= 1;
                if let Some(item) = bounded_agent_metadata(item, budget, depth + 1) {
                    result.push(item);
                }
            }
            Some(Value::Array(result))
        }
        Value::String(text) => {
            let mut text = text.chars().take(1200).collect::<String>();
            while serde_json::to_string(&text).ok()?.chars().count() > *budget {
                text.pop()?;
            }
            *budget -= serde_json::to_string(&text).ok()?.chars().count();
            Some(Value::String(text))
        }
        _ => {
            let cost = value.to_string().chars().count();
            if *budget < cost {
                None
            } else {
                *budget -= cost;
                Some(value.clone())
            }
        }
    }
}

fn agent_evidence_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentEvidenceCandidate> {
    let document_id: String = row.get(1)?;
    let title: String = row.get(2)?;
    let content: String = row.get(3)?;
    let published_at: Option<String> = row.get(7)?;
    let source_name: String = row.get(6)?;
    let url: Option<String> = row.get(8)?;
    let raw_metadata: String = row.get(11)?;
    let metadata = serde_json::from_str::<Value>(&raw_metadata).unwrap_or_else(|_| json!({}));
    let lexical_score: f64 = row.get(12)?;
    let stock_codes: String = row.get(10)?;
    let bounded_metadata =
        bounded_agent_metadata(&metadata, &mut 6000, 0).unwrap_or_else(|| json!({}));
    let metadata_truncated = bounded_metadata != metadata;
    let mut citation = json!({"citation_id":"","document_id":document_id,"chunk_id":row.get::<_, String>(0)?,
        "title":title.chars().take(256).collect::<String>(),"excerpt":content.chars().take(1200).collect::<String>(),
        "source_tier":row.get::<_, String>(5)?,"source_name":source_name.chars().take(128).collect::<String>(),
        "published_at":published_at,"url":url.as_ref().map(|url| url.chars().take(2048).collect::<String>()),
        "page_number":row.get::<_, Option<u32>>(4)?,"lexical_score":lexical_score,"vector_score":Value::Null,
        "retrieval_score":lexical_score,"remote_export_allowed":row.get::<_, i64>(9)? != 0,
        "document_content_hash":row.get::<_,String>(13)?,"chunk_content_hash":row.get::<_,String>(14)?,
        "stock_codes":serde_json::from_str::<Value>(&stock_codes).unwrap_or_else(|_| json!([])),
        "metadata":bounded_metadata});
    if metadata_truncated {
        citation["metadata_truncated"] = json!(true);
    }
    if let Some(summary) = metadata
        .get("article_summary")
        .and_then(Value::as_str)
        .filter(|text| text.chars().count() >= 15 && text.trim() != title.trim())
    {
        citation["article_summary"] = json!({"text":summary.chars().take(1200).collect::<String>(),
            "provenance":"extracted_article_summary", "verification_status":"unverified_media_summary",
            "status":metadata.get("article_summary_status").and_then(Value::as_str).unwrap_or("available").chars().take(64).collect::<String>(),
            "url":metadata.get("article_summary_url").and_then(Value::as_str).or(url.as_deref()).map(|url|url.chars().take(2048).collect::<String>()),
            "fetched_at_epoch_ms":metadata.get("article_summary_fetched_at_epoch_ms").and_then(Value::as_i64)});
    }
    let published_at_utc = published_at
        .as_deref()
        .and_then(|value| normalized_research_timestamp(value, false));
    Ok(AgentEvidenceCandidate {
        document_id,
        published_at_utc,
        lexical_score,
        citation,
    })
}

pub(crate) fn validate_model_answer(answer: &str, citations: &[Value]) -> Result<(), String> {
    let allowed = citations
        .iter()
        .filter_map(|citation| {
            let citation_id = citation.get("citation_id")?.as_str()?;
            let source_tier = citation
                .get("source_tier")
                .and_then(Value::as_str)
                .unwrap_or("community");
            Some((citation_id.to_string(), source_tier != "community"))
        })
        .collect::<HashMap<_, _>>();
    if allowed.is_empty() {
        return Err("model answer cannot be validated without citations".to_string());
    }

    let mut checked_segments = 0usize;
    for segment in answer_segments(answer) {
        let (plain_text, segment_citations) = inspect_answer_segment(segment, &allowed)?;
        let substantive = plain_text.chars().any(|character| {
            character.is_alphanumeric() || ('\u{3400}'..='\u{9fff}').contains(&character)
        });
        if !substantive {
            continue;
        }
        checked_segments += 1;
        if segment_citations.is_empty() {
            return Err(format!(
                "model answer contains an uncited statement: {}",
                excerpt(plain_text.trim(), 80)
            ));
        }
        if !segment_citations
            .iter()
            .any(|can_support_fact| *can_support_fact)
            && !is_explicitly_unverified(&plain_text)
        {
            return Err(format!(
                "community citation cannot support a factual statement: {}",
                excerpt(plain_text.trim(), 80)
            ));
        }
    }
    if checked_segments == 0 {
        return Err("model answer did not contain a cited statement".to_string());
    }
    Ok(())
}

fn answer_segments(answer: &str) -> Vec<&str> {
    let characters = answer.char_indices().collect::<Vec<_>>();
    let mut segments = Vec::new();
    let mut start = 0usize;
    for (position, (index, character)) in characters.iter().copied().enumerate() {
        let decimal_point = character == '.'
            && position > 0
            && position + 1 < characters.len()
            && characters[position - 1].1.is_ascii_digit()
            && characters[position + 1].1.is_ascii_digit();
        let numeric_separator = matches!(character, ',' | ':' | '，' | '：')
            && position > 0
            && position + 1 < characters.len()
            && characters[position - 1].1.is_ascii_digit()
            && characters[position + 1].1.is_ascii_digit();
        let boundary = matches!(
            character,
            '。' | '！' | '？' | '!' | '?' | '；' | ';' | '，' | ',' | '：' | ':' | '\n'
        ) && !numeric_separator
            || (character == '.' && !decimal_point);
        if !boundary {
            continue;
        }
        let end = index + character.len_utf8();
        segments.push(&answer[start..end]);
        start = end;
    }
    if start < answer.len() {
        segments.push(&answer[start..]);
    }
    segments
}

fn inspect_answer_segment(
    segment: &str,
    allowed: &HashMap<String, bool>,
) -> Result<(String, Vec<bool>), String> {
    let mut plain_text = String::new();
    let mut citation_support = Vec::new();
    let mut remaining = segment;
    while let Some(start) = remaining.find("[C") {
        plain_text.push_str(&remaining[..start]);
        let after_prefix = &remaining[start + 2..];
        let digit_count = after_prefix
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        if digit_count == 0 || after_prefix.as_bytes().get(digit_count) != Some(&b']') {
            plain_text.push_str("[C");
            remaining = after_prefix;
            continue;
        }
        let citation_id = format!("C{}", &after_prefix[..digit_count]);
        let can_support_fact = allowed
            .get(&citation_id)
            .copied()
            .ok_or_else(|| format!("model returned unknown citation [{citation_id}]"))?;
        citation_support.push(can_support_fact);
        remaining = &after_prefix[digit_count + 1..];
    }
    plain_text.push_str(remaining);
    Ok((plain_text, citation_support))
}

fn is_explicitly_unverified(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    [
        "待核查",
        "传闻",
        "未经证实",
        "不确定",
        "可能",
        "据称",
        "社区讨论",
        "rumor",
        "unverified",
        "uncertain",
        "pending verification",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}
