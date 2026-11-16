use crate::research::storage::documents::source_rank;
use crate::research::storage::documents::string_field;
use crate::research::storage::evidence::ResearchCitation;
use crate::research::storage::evidence::MAX_CITATIONS;
use crate::research::storage::ResearchStore;
use crate::research::time::normalize_research_as_of;
use jieba_rs::Jieba;
use rusqlite::params;
use rusqlite::Connection;
use serde_json::json;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;

impl ResearchStore {
    pub(crate) fn query(&self, request: &Value) -> Result<Value, String> {
        self.query_with_vector(request, None)
    }
}

impl ResearchStore {
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
        let remote_safe_only = request
            .get("remote_safe_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let connection = self.connection()?;
        let as_of = normalize_research_as_of(request.get("as_of"))?;
        let match_query = build_fts_match_query(&query);
        let mut statement = connection
            .prepare(
                "SELECT chunks.id, chunks.document_id, chunks.title, chunks.content, chunks.page_number,
                        chunks.source_tier, chunks.source_name, chunks.published_at, chunks.url,
                        research_timestamp_utc(chunks.published_at) AS published_at_utc,
                        documents.remote_export_allowed,
                        -bm25(chunks_fts, 0.0, 3.0, 2.0, 1.0) AS lexical_score
                 FROM chunks_fts
                 JOIN chunks ON chunks.id = chunks_fts.chunk_id
                 JOIN documents ON documents.id = chunks.document_id
                 WHERE chunks_fts MATCH ?1
                   AND json_extract(documents.metadata_json, '$.news_cache_duplicate_of') IS NULL
                   AND (?3 = 0 OR documents.remote_export_allowed = 1)
                   AND (
                     ?2 IS NULL OR EXISTS (
                       SELECT 1 FROM json_each(chunks.stock_codes_json)
                       WHERE value = ?2 COLLATE NOCASE
                     )
                   )
                   AND (
                     ?4 IS NULL OR (
                       chunks.published_at IS NOT NULL
                       AND research_timestamp_utc(chunks.published_at) <= ?4
                     )
                   )
                 ORDER BY bm25(chunks_fts, 0.0, 3.0, 2.0, 1.0)
                 LIMIT 50",
            )
            .map_err(|error| format!("failed to prepare research query: {error}"))?;
        let rows = statement
            .query_map(
                params![match_query, stock_code, remote_safe_only as i64, as_of],
                |row| {
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
                            published_at_utc: row.get(9)?,
                            remote_export_allowed: row.get::<_, i64>(10)? != 0,
                        },
                        row.get::<_, f64>(11)?,
                    ))
                },
            )
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
            .map(|vector| {
                self.vector_candidates(
                    &connection,
                    vector,
                    stock_code.as_deref(),
                    remote_safe_only,
                    as_of.as_deref(),
                )
            })
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
                let source_order = source_rank(&right_candidate.source_tier)
                    .cmp(&source_rank(&left_candidate.source_tier));
                let recency_order = right_candidate
                    .published_at_utc
                    .cmp(&left_candidate.published_at_utc);
                let score_order = right_score
                    .partial_cmp(left_score)
                    .unwrap_or(Ordering::Equal);
                if as_of.is_some() {
                    source_order
                        .then(recency_order)
                        .then(score_order)
                        .then_with(|| left_candidate.chunk_id.cmp(&right_candidate.chunk_id))
                } else {
                    source_order
                        .then(score_order)
                        .then(recency_order)
                        .then_with(|| left_candidate.chunk_id.cmp(&right_candidate.chunk_id))
                }
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
                unavailable: false,
                remote_export_allowed: candidate.remote_export_allowed,
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
            "retrieval_mode": if used_vector { "hybrid_rrf" } else { "bm25" },
            "remote_safe_only": remote_safe_only
        }))
    }
}

#[derive(Clone, Debug)]
pub(in crate::research) struct Candidate {
    chunk_id: String,
    document_id: String,
    title: String,
    content: String,
    page_number: Option<u32>,
    source_tier: String,
    source_name: String,
    published_at: Option<String>,
    published_at_utc: Option<String>,
    url: Option<String>,
    remote_export_allowed: bool,
}

pub(in crate::research) fn load_candidate(
    connection: &Connection,
    chunk_id: &str,
) -> Result<Candidate, String> {
    connection
        .query_row(
            "SELECT c.id, c.document_id, c.title, c.content, c.page_number,
                    c.source_tier, c.source_name, c.published_at, c.url,
                    research_timestamp_utc(c.published_at), d.remote_export_allowed
             FROM chunks c JOIN documents d ON d.id = c.document_id
             WHERE c.id = ?1",
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
                    published_at_utc: row.get(9)?,
                    remote_export_allowed: row.get::<_, i64>(10)? != 0,
                })
            },
        )
        .map_err(|error| format!("failed to load vector candidate {chunk_id}: {error}"))
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

pub(in crate::research) fn tokenize_for_fts(value: &str) -> String {
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

pub(in crate::research) fn build_fts_match_query(value: &str) -> String {
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

pub(in crate::research) fn excerpt(value: &str, max_chars: usize) -> String {
    let mut result = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        result.push('…');
    }
    result
}
