use crate::research::app_store::ensure_app_database_generation;
use crate::research::app_store::market_from_payload;
use crate::research::app_store::recover_research_files;
use crate::research::app_store::research_database_path;
use crate::research::app_store::with_exclusive_app_database;
use crate::research::app_store::with_model_evidence_database_guard;
use crate::research::app_store::with_shared_app_database;
use crate::research::app_store::APP_RESEARCH_DATABASE_GENERATION;
use crate::research::app_store::APP_RESEARCH_DATABASE_LOCK;
use crate::research::sources::archive_legacy_sources;
use crate::research::sources::ingest_news_cache_file;
use crate::research::sources::ingest_news_cache_file_for_market;
use crate::research::sources::legacy_documents_from_file;
use crate::research::sources::migrate_legacy_sources;
use crate::research::sources::normalize_legacy_document;
use crate::research::storage::documents::sha256_hex;
use crate::research::storage::evidence::validate_model_answer;
use crate::research::storage::packs::preserve_local_research_state;
use crate::research::storage::packs::remove_sqlite_files;
use crate::research::storage::packs::validate_portable_documents;
use crate::research::storage::packs::MAX_PACK_DOCUMENT_CHARS;
use crate::research::storage::retrieval::rrf_fuse;
use crate::research::storage::vectors::cosine_similarity;
use crate::research::storage::vectors::decode_f32_blob;
use crate::research::storage::vectors::RESEARCH_EMBEDDING_DIMENSIONS;
use crate::research::storage::ResearchStore;
use crate::research::time::epoch_millis;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Duration;

fn hk_context_fixture(store: &ResearchStore) -> (Value, Value) {
    let document = json!({"document_id":"permission-hk","title":"腾讯研究资料","content":"原始资料只用于本地回放与已授权模型分析。",
            "source_tier":"research_report","source_name":"用户导入","stock_codes":["00700.HK"],"user_imported":true,"remote_export_allowed":true,
            "metadata":{"market":"hk","metrics":{"eps":{"value":12.639,"currency":"CNY","unit":"currency/share"}}}});
    store.ingest_documents(&[document.clone()]).unwrap();
    let evidence = store
        .agent_evidence(&json!({"query":"腾讯","stock_code":"00700.HK","remote_safe_only":true}))
        .unwrap();
    let context = crate::hk_agent_research::attach_evidence(json!({}), &evidence, "00700.HK")
        ["research_context"]
        .clone();
    (document, context)
}

#[test]
fn saved_hk_context_rechecks_revoked_permission_and_preserves_local_history() {
    let path = temporary_database_path("hk-context-revoked");
    let store = ResearchStore::open(&path).unwrap();
    let (mut document, context) = hk_context_fixture(&store);
    assert!(store.validate_hk_context(&context, true).unwrap());
    store.ingest_documents(&[document.clone()]).unwrap();
    assert!(store.validate_hk_context(&context, true).unwrap());
    document["remote_export_allowed"] = json!(false);
    store.ingest_documents(&[document]).unwrap();
    assert!(
        !store.validate_hk_context(&context, true).unwrap(),
        "old model consent cannot override current permission"
    );
    let current = store
        .agent_evidence(&json!({"query":"腾讯","stock_code":"00700.HK","remote_safe_only":true}))
        .unwrap();
    assert_eq!(current["citations"], json!([]));
    assert!(
        !store.messages(&json!({"stock_code":"00700.HK"})).unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::hk_agent_research::answer_from_context(&context, "继续回放")
            .to_string()
            .contains("原始资料")
    );
}

#[test]
fn frozen_model_tool_evidence_retains_valid_source_identity() {
    let store =
        ResearchStore::open(temporary_database_path("hk-frozen-source-validation")).unwrap();
    let (document, context) = hk_context_fixture(&store);
    let frozen = crate::hk_agent_research::frozen_evidence(&context);
    let current = crate::hk_agent_research::context_from_frozen_evidence(&frozen);
    assert!(store.validate_hk_context(&current, true).unwrap());
    let mut revoked = document;
    revoked["remote_export_allowed"] = json!(false);
    store.ingest_documents(&[revoked]).unwrap();
    assert!(!store.validate_hk_context(&current, true).unwrap());
}

#[test]
fn review_revoked_cited_revision_preserves_history_and_blocks_original_export() {
    let store = ResearchStore::open(temporary_database_path("hk-cited-revocation")).unwrap();
    let (mut document, context) = hk_context_fixture(&store);
    let thread = store
        .create_thread(&json!({"stock_code":"00700.HK","title":"历史研究"}))
        .unwrap();
    let thread_id = thread["id"].as_str().unwrap();
    let response = store
        .query(&json!({"query":"腾讯","stock_code":"00700.HK"}))
        .unwrap();
    store
        .save_answer(thread_id, "腾讯的原始资料", &response)
        .unwrap();
    assert!(
        store
            .connection()
            .unwrap()
            .query_row(
                "SELECT cited_count FROM documents WHERE id='permission-hk'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap()
            > 0
    );
    document["content"] = json!("更新后的资料也只允许在本地阅读。");
    document["remote_export_allowed"] = json!(false);
    store.ingest_documents(&[document]).unwrap();
    assert!(
        !store.validate_hk_context(&context, true).unwrap(),
        "revocation must cover the retained original as well as its new revision"
    );
    let allowed: i64 = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE remote_export_allowed=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(allowed, 0);
    assert!(store
        .thread(thread_id)
        .unwrap()
        .to_string()
        .contains("原始资料"));
}

#[test]
fn review_changed_chunk_semantics_advance_the_sentiment_cutoff_in_both_markets() {
    for (market, code, second_code) in [
        ("hk", "00700.HK", "00005.HK"),
        ("cn", "600519.SH", "600036.SH"),
    ] {
        for (field, value) in [
            ("stock_codes", json!([code, second_code])),
            ("sentiment", json!("positive")),
            ("page_number", json!(2)),
            ("entities", json!(["新增主体"])),
            ("relation_types", json!(["新增关系"])),
        ] {
            let store = ResearchStore::open(temporary_database_path(&format!(
                "chunk-cutoff-{market}-{field}"
            )))
            .unwrap();
            let mut document = json!({"document_id":"known","title":"企业公告","content":"同一份原始公告正文",
                    "source_tier":"filing","source_name":"交易所","stock_codes":[code],"metadata":{"market":market}});
            store.ingest_documents(&[document.clone()]).unwrap();
            store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE documents SET imported_at_epoch_ms=1000,updated_at_epoch_ms=1000",
                    [],
                )
                .unwrap();
            assert_eq!(store.sentiment_documents(code, 2000).unwrap().len(), 1);
            document[field] = value;
            store.ingest_documents(&[document]).unwrap();
            assert!(
                store.sentiment_documents(code, 2000).unwrap().is_empty(),
                "changed {market} {field} cannot enter an earlier evidence snapshot"
            );
        }
    }
}

#[test]
fn unscoped_hk_model_evidence_keeps_current_bindings_and_market_permissions() {
    let store = ResearchStore::open(temporary_database_path("hk-unscoped-permission")).unwrap();
    let (document, _) = hk_context_fixture(&store);
    store.ingest_documents(&[
            json!({"document_id":"macro","title":"港股政策","content":"港股政策原文","source_tier":"news","metadata":{"market":"hk"}}),
            json!({"document_id":"cn","title":"内地资料","content":"内地资料","stock_codes":["600519.SH"],"metadata":{"market":"cn"}}),
            json!({"document_id":"local","title":"私有资料","content":"本地阅读资料","user_imported":true,"remote_export_allowed":false,"metadata":{"market":"hk"}}),
        ]).unwrap();
    let evidence = store
        .unscoped_hk_agent_evidence(&json!({"query":"研究","remote_safe_only":true}))
        .unwrap();
    assert_eq!(evidence["citations"].as_array().unwrap().len(), 2);
    assert!(store
        .validate_hk_model_evidence(&[evidence.clone()])
        .unwrap());
    let mut revoked = document;
    revoked["remote_export_allowed"] = json!(false);
    store.ingest_documents(&[revoked]).unwrap();
    assert!(!store.validate_hk_model_evidence(&[evidence]).unwrap());
}

#[test]
fn model_send_boundary_serializes_revocation_until_the_checked_send_finishes() {
    let path = temporary_database_path("hk-send-permission-barrier");
    let store = ResearchStore::open(&path).unwrap();
    let (mut document, context) = hk_context_fixture(&store);
    document["remote_export_allowed"] = json!(false);
    let writer_store = ResearchStore::open(&path).unwrap();
    let (begin, begin_rx) = std::sync::mpsc::channel();
    let (blocked, blocked_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        begin_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(APP_RESEARCH_DATABASE_LOCK
            .get()
            .unwrap()
            .try_read()
            .is_err());
        blocked.send(()).unwrap();
        with_shared_app_database(|| writer_store.ingest_documents(&[document])).unwrap();
    });
    let frozen = crate::hk_agent_research::frozen_evidence(&context);
    with_model_evidence_database_guard(|| {
        assert!(store.validate_hk_model_evidence(&[frozen.clone()])?);
        let generation = APP_RESEARCH_DATABASE_GENERATION.load(AtomicOrdering::Acquire);
        begin.send(()).unwrap();
        blocked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            APP_RESEARCH_DATABASE_GENERATION.load(AtomicOrdering::Acquire),
            generation
        );
        // This point represents the request being handed to the transport.
        Ok(())
    })
    .unwrap();
    writer.join().unwrap();
    assert!(
        !store.validate_hk_model_evidence(&[frozen]).unwrap(),
        "the next provider turn must reject the committed revocation"
    );
}

#[test]
fn saved_hk_context_rejects_changed_metrics_body_and_removed_documents() {
    for change in ["metric", "body", "delete"] {
        let path = temporary_database_path(&format!("hk-context-change-{change}"));
        let store = ResearchStore::open(&path).unwrap();
        let (mut document, context) = hk_context_fixture(&store);
        match change {
            "metric" => {
                document["metadata"]["metrics"]["eps"]["currency"] = json!("HKD");
                store.ingest_documents(&[document]).unwrap();
            }
            "body" => {
                document["content"] = json!("不同的当前资料，不能沿用原来的引用。");
                store.ingest_documents(&[document]).unwrap();
            }
            _ => {
                store
                    .connection()
                    .unwrap()
                    .execute("DELETE FROM documents WHERE id='permission-hk'", [])
                    .unwrap();
            }
        }
        assert!(
            !store.validate_hk_context(&context, true).unwrap(),
            "changed {change} must invalidate old E bindings"
        );
    }
}

#[test]
fn saved_hk_context_rejects_changes_beyond_excerpt_and_changed_article_summaries() {
    for change in ["tail", "summary", "missing_hash"] {
        let store =
            ResearchStore::open(temporary_database_path(&format!("hk-context-{change}"))).unwrap();
        let (mut document, _) = hk_context_fixture(&store);
        document["content"] = json!(format!("{}原始结尾", "已知资料".repeat(400)));
        document["metadata"]["article_summary"] = json!("这是原始正文摘要，内容涉及企业经营情况。");
        store.ingest_documents(&[document.clone()]).unwrap();
        let evidence = store
            .agent_evidence(&json!({"query":"资料","stock_code":"00700.HK"}))
            .unwrap();
        let mut context =
            crate::hk_agent_research::attach_evidence(json!({}), &evidence, "00700.HK")
                ["research_context"]
                .clone();
        assert!(store.validate_hk_context(&context, true).unwrap());
        match change {
            "tail" => document["content"] = json!(format!("{}不同结尾", "已知资料".repeat(400))),
            "summary" => {
                document["metadata"]["article_summary"] =
                    json!("这是已经变化的正文摘要，讨论另一种财务状况。")
            }
            _ => {
                context["evidence"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("document_content_hash");
            }
        }
        store.ingest_documents(&[document]).unwrap();
        assert!(
            !store.validate_hk_context(&context, true).unwrap(),
            "invalid {change}"
        );
    }
}

#[test]
fn unchanged_hk_ingest_keeps_known_evidence_inside_the_sentiment_cutoff() {
    let path = temporary_database_path("hk-unchanged-cutoff");
    let store = ResearchStore::open(&path).unwrap();
    let docs = [
        json!({"document_id":"known-hk","title":"腾讯公告","content":"已知公告原文",
            "source_tier":"filing","source_name":"HKEXnews","stock_codes":["00700.HK"],
            "published_at":"2026-11-09","metadata":{"market":"hk","report_period":"2026Q3"}}),
    ];
    store.ingest_documents(&docs).unwrap();
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE documents SET imported_at_epoch_ms=1000,updated_at_epoch_ms=1000",
            [],
        )
        .unwrap();
    assert_eq!(
        store.sentiment_documents("00700.HK", 2000).unwrap().len(),
        1
    );
    store.ingest_documents(&docs).unwrap();
    let retained = store.sentiment_documents("00700.HK", 2000).unwrap();
    assert_eq!(
        retained.len(),
        1,
        "reading the same cached content cannot make it newer than the snapshot"
    );
    assert_eq!(retained[0]["updated_at"], 1000);
    let mut changed = docs[0].clone();
    changed["content"] = json!("截止之后真实更新的公告内容");
    store.ingest_documents(&[changed]).unwrap();
    assert!(store
        .sentiment_documents("00700.HK", 2000)
        .unwrap()
        .is_empty());
}
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
fn agent_evidence_covers_sources_with_sparse_keywords_and_preserves_original_summary() {
    let path = temporary_database_path("agent-evidence-sources");
    let store = ResearchStore::open(&path).unwrap();
    store.ingest_documents(&[
            json!({"document_id":"financial","title":"中期业绩","content":"CNY 收入 300；报告期 2026H1；候选值。",
                "stock_codes":["00700.HK"],"source_tier":"financial_snapshot","source_name":"Eastmoney",
                "published_at":"2026-08-13","url":"https://example.com/financial","metadata":{"market":"hk","currency":"CNY"}}),
            json!({"document_id":"filing-old","title":"年度公告","content":"年度公告收入。",
                "stock_codes":["00700.HK"],"source_tier":"filing","published_at":"2025-03-19"}),
            json!({"document_id":"filing-new","title":"季度公告","content":"季度公告净利润。",
                "stock_codes":["00700.HK"],"source_tier":"filing","published_at":"2026-10-30"}),
            json!({"document_id":"news","title":"混元招聘","content":"混元招聘原始标题摘录",
                "stock_codes":["00700.HK"],"source_tier":"news","source_name":"新浪财经",
                "published_at":"2026-11-09","url":"https://example.com/news",
                "metadata":{"market":"hk","article_summary":"正文报道披露招聘六十余个岗位，并列明招聘计划。",
                    "article_summary_status":"available","article_summary_url":"https://example.com/news",
                    "article_summary_fetched_at_epoch_ms":1794116685279i64}}),
        ]).unwrap();
    let response = store
        .agent_evidence(&json!({"query":"混元招聘","stock_code":"00700.HK","top_k":3}))
        .unwrap();
    let citations = response["citations"].as_array().unwrap();
    assert_eq!(
        citations.len(),
        3,
        "sparse query must still cover all available core sources"
    );
    let tiers = citations
        .iter()
        .filter_map(|item| item["source_tier"].as_str())
        .collect::<HashSet<_>>();
    assert_eq!(
        tiers,
        HashSet::from(["filing", "financial_snapshot", "news"])
    );
    assert!(citations
        .iter()
        .any(|item| item["document_id"] == "filing-new"));
    let news = citations
        .iter()
        .find(|item| item["document_id"] == "news")
        .unwrap();
    assert_eq!(news["excerpt"], "混元招聘原始标题摘录");
    assert_eq!(news["stock_codes"], json!(["00700.HK"]));
    assert_eq!(news["metadata"]["market"], "hk");
    assert_eq!(
        news["article_summary"]["text"],
        "正文报道披露招聘六十余个岗位，并列明招聘计划。"
    );
    assert_eq!(
        news["article_summary"]["provenance"],
        "extracted_article_summary"
    );
    assert_eq!(news["article_summary"]["url"], "https://example.com/news");
    assert_eq!(
        news["article_summary"]["fetched_at_epoch_ms"],
        1794116685279i64
    );
    let connection = store.connection().unwrap();
    let original: String = connection
        .query_row(
            "SELECT content FROM documents WHERE id = 'news'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original, "混元招聘原始标题摘录");
    drop(connection);
    drop(store);
    remove_sqlite_files(&path).unwrap();
}

#[test]
fn agent_evidence_rejects_missing_or_noncanonical_hk_identity() {
    let path = temporary_database_path("agent-evidence-identity");
    let store = ResearchStore::open(&path).unwrap();
    for code in [
        Value::Null,
        json!("00700"),
        json!("700.HK"),
        json!("000700.SH"),
        json!("00700.hk"),
    ] {
        assert!(
            store
                .agent_evidence(&json!({"query":"研究","stock_code":code}))
                .is_err(),
            "identity {code} must be rejected"
        );
    }
    assert!(store
        .agent_evidence(&json!({"query":"","stock_code":"00700.HK"}))
        .is_err());
    assert!(store
        .agent_evidence(&json!({"query":"研究","stock_code":"00700.HK","market":"cn"}))
        .is_err());
    drop(store);
    remove_sqlite_files(&path).unwrap();
}

#[test]
fn agent_evidence_filters_stock_market_duplicates_and_remote_permissions() {
    let path = temporary_database_path("agent-evidence-isolation");
    let store = ResearchStore::open(&path).unwrap();
    let mut documents = Vec::new();
    for (id, codes, market, remote) in [
        ("allowed", json!(["00700.HK"]), "hk", true),
        ("private", json!(["00700.HK"]), "hk", false),
        ("other-hk", json!(["00005.HK"]), "hk", true),
        ("cn", json!(["000700.SH"]), "cn", true),
        ("wrong-market", json!(["00700.HK"]), "cn", true),
        ("mixed-market", json!(["00700.HK", "000700.SH"]), "hk", true),
    ] {
        documents.push(json!({"document_id":id,"title":"研究证据","content":"研究原始证据。","stock_codes":codes,
                "source_tier":"news","remote_export_allowed":remote,"metadata":{"market":market}}));
    }
    documents.push(json!({"document_id":"duplicate","title":"研究证据","content":"研究重复证据。","stock_codes":["00700.HK"],
            "source_tier":"news","metadata":{"market":"hk","news_cache_duplicate_of":"allowed"}}));
    store.ingest_documents(&documents).unwrap();
    let request = json!({"query":"研究","stock_code":"00700.HK","top_k":16});
    let local = store.agent_evidence(&request).unwrap();
    let ids = local["citations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["document_id"].as_str())
        .collect::<HashSet<_>>();
    assert_eq!(ids, HashSet::from(["allowed", "private"]));
    assert_eq!(
        local["citations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["document_id"] == "private")
            .unwrap()["remote_export_allowed"],
        false
    );
    let safe = store
        .agent_evidence(
            &json!({"remote_safe_only":true, "query":"研究", "stock_code":"00700.HK","top_k":16}),
        )
        .unwrap();
    assert_eq!(safe["citations"].as_array().unwrap().len(), 1);
    assert_eq!(safe["citations"][0]["document_id"], "allowed");
    assert_eq!(safe["citations"][0]["remote_export_allowed"], true);
    drop(store);
    remove_sqlite_files(&path).unwrap();
}

#[test]
fn agent_evidence_bounds_text_and_prefers_relevant_chunks_without_replacing_them() {
    let path = temporary_database_path("agent-evidence-bounds");
    let store = ResearchStore::open(&path).unwrap();
    let mut documents = Vec::new();
    for index in 0..24 {
        documents.push(json!({"document_id":format!("news-{index:02}"),"title":"新消息标题".repeat(100),
                "content":format!("{}\n专有线索：原始收入证据 {index}。", "普通正文。".repeat(200)),
                "stock_codes":["00700.HK"],"source_tier":"news","published_at":format!("2026-11-{:02}", index + 1),
                "metadata":{"market":"hk","article_summary":"正文提取补充。".repeat(300),"oversized":"长元数据".repeat(10000),
                    "article_summary_status":"状态".repeat(10000),"article_summary_fetched_at_epoch_ms":["未知时间".repeat(10000)]}}));
    }
    store.ingest_documents(&documents).unwrap();
    let result = store
        .agent_evidence(&json!({"query":"专有线索","stock_code":"00700.HK","top_k":99}))
        .unwrap();
    let citations = result["citations"].as_array().unwrap();
    assert_eq!(citations.len(), 16);
    assert_eq!(citations[0]["document_id"], "news-23");
    let mut ids = HashSet::new();
    for item in citations {
        assert!(ids.insert(item["document_id"].clone().to_string()));
        assert!(item["excerpt"].as_str().unwrap().contains("专有线索"));
        assert!(item["excerpt"].as_str().unwrap().chars().count() <= 1200);
        assert!(item["title"].as_str().unwrap().chars().count() <= 256);
        assert!(
            item["article_summary"]["text"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= 1200
        );
        assert!(
            item["article_summary"]["status"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= 64
        );
        assert!(item["article_summary"]["fetched_at_epoch_ms"].is_null());
    }
    assert!(serde_json::to_string(&result).unwrap().chars().count() < 200_000);
    let original: String = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT content FROM chunks WHERE id = ?1",
            params![citations[0]["chunk_id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(citations[0]["excerpt"], original);
    drop(store);
    remove_sqlite_files(&path).unwrap();
}

#[test]
fn research_markets_use_separate_database_paths_and_unknown_markets_fail() {
    let root = temporary_database_path("market-store-paths");
    let research_dir = root.parent().unwrap().join(format!(
        "{}-data",
        root.file_stem().unwrap().to_string_lossy()
    ));
    let cn = research_database_path(&research_dir, "cn").unwrap();
    let hk = research_database_path(&research_dir, "hk").unwrap();
    assert_ne!(cn, hk);
    assert_eq!(cn.file_name().unwrap(), "research.sqlite");
    assert_eq!(hk.file_name().unwrap(), "research-hk.sqlite");
    assert_eq!(market_from_payload(&json!({})).unwrap(), "cn");
    assert_eq!(market_from_payload(&json!({"market":"hk"})).unwrap(), "hk");
    assert!(market_from_payload(&json!({"market":"unknown"})).is_err());
}

fn retrieval_eval_documents(case: &Value) -> Vec<Value> {
    let mut documents = case
        .get("documents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for document in &mut documents {
        let repeat = document
            .get("repeat_content")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 100) as usize;
        if repeat > 1 {
            let content = document
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .repeat(repeat);
            document["content"] = Value::String(content);
        }
    }
    if let Some(generator) = case.get("generated_documents") {
        let count = generator
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            .min(100) as usize;
        let prefix = generator
            .get("document_id_prefix")
            .and_then(Value::as_str)
            .unwrap_or("generated-");
        for index in 0..count {
            documents.push(json!({
                    "document_id": format!("{prefix}{index}"),
                    "title": generator.get("title").cloned().unwrap_or_else(|| json!("Generated evidence")),
                    "content": generator.get("content").cloned().unwrap_or_else(|| json!("generated evidence")),
                    "source_tier": generator.get("source_tier").cloned().unwrap_or_else(|| json!("news")),
                    "stock_codes": generator.get("stock_codes").cloned().unwrap_or_else(|| json!([])),
                }));
        }
    }
    documents
}

fn retrieval_eval_vector(label: &str) -> Vec<f32> {
    let mut vector = vec![0.0; RESEARCH_EMBEDDING_DIMENSIONS as usize];
    match label {
        "query" => vector[0] = 1.0,
        "secondary" => {
            vector[0] = 0.8;
            vector[1] = 0.6;
        }
        _ => vector[1] = 1.0,
    }
    vector
}

#[test]
fn fixed_retrieval_eval_suite_enforces_governance_invariants() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../app/prompts/research_retrieval_eval_cases.json"
    ))
    .expect("retrieval evaluation fixture should be valid JSON");
    assert_eq!(fixture["version"], "research-retrieval-eval-v5");
    assert_eq!(fixture["cases"].as_array().map(Vec::len), Some(40));
    assert_eq!(fixture["dataset"]["split"], "frozen-golden");
    assert_eq!(
        fixture["dataset"]["generator"],
        "agent-authored-synthetic-reviewed-lineage-v1"
    );

    for case in fixture["cases"]
        .as_array()
        .expect("retrieval evaluation cases should be an array")
    {
        let case_id = case["id"]
            .as_str()
            .expect("retrieval evaluation case should have an id");
        let path = temporary_database_path(case_id);
        let store = ResearchStore::open(&path).expect("research store should open");
        let documents = retrieval_eval_documents(case);
        store
            .ingest_documents(&documents)
            .unwrap_or_else(|error| panic!("{case_id}: documents should ingest: {error}"));

        let query_vector = case
            .get("query_vector")
            .and_then(Value::as_str)
            .map(retrieval_eval_vector);
        if query_vector.is_some() {
            let work = store
                .pending_embedding_chunks(500)
                .unwrap_or_else(|error| panic!("{case_id}: embedding work should load: {error}"));
            let embeddings = work
                .into_iter()
                .map(|item| {
                    let label = documents
                        .iter()
                        .find(|document| {
                            document
                                .get("document_id")
                                .and_then(Value::as_str)
                                .is_some_and(|id| item.chunk_id.starts_with(&format!("{id}:")))
                        })
                        .and_then(|document| document.get("embedding"))
                        .and_then(Value::as_str)
                        .unwrap_or("orthogonal");
                    (item, retrieval_eval_vector(label))
                })
                .collect::<Vec<_>>();
            store
                .store_embeddings(&embeddings, "retrieval-eval-v1")
                .unwrap_or_else(|error| panic!("{case_id}: embeddings should store: {error}"));
        }

        let response = store
            .query_with_vector(&case["request"], query_vector.as_deref())
            .unwrap_or_else(|error| panic!("{case_id}: query should succeed: {error}"));
        let citations = response["citations"]
            .as_array()
            .expect("retrieval response should contain citations");
        let mut local_request = case["request"].clone();
        local_request["remote_safe_only"] = Value::Bool(false);
        let local_response = store
            .query_with_vector(&local_request, query_vector.as_deref())
            .unwrap_or_else(|error| panic!("{case_id}: local query should succeed: {error}"));
        let local_citations = local_response["citations"]
            .as_array()
            .expect("local retrieval response should contain citations");
        let expected = &case["expect"];
        let expected_remote_export_allowed = case
            .get("privacy_policy")
            .and_then(|policy| policy.get("expected_remote_export_allowed"))
            .and_then(Value::as_bool);
        // A local-only private document is asserted against the local
        // projection; the remote projection is checked separately below.
        let assertion_citations = if expected_remote_export_allowed == Some(false) {
            local_citations
        } else {
            citations
        };

        if let Some(value) = expected.get("citation_count").and_then(Value::as_u64) {
            assert_eq!(citations.len(), value as usize, "{case_id}: citation count");
        }
        if let Some(value) = expected.get("first_document_id").and_then(Value::as_str) {
            assert_eq!(
                assertion_citations
                    .first()
                    .and_then(|citation| citation["document_id"].as_str()),
                Some(value),
                "{case_id}: first document"
            );
        }
        for required in expected
            .get("required_document_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            assert!(
                assertion_citations
                    .iter()
                    .any(|citation| citation["document_id"] == required),
                "{case_id}: required document {required} should be cited"
            );
        }
        for forbidden in expected
            .get("forbidden_document_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            assert!(
                !assertion_citations
                    .iter()
                    .any(|citation| citation["document_id"] == forbidden),
                "{case_id}: forbidden document {forbidden} must not be cited"
            );
        }
        if let Some(value) = expected.get("remote_safe_only").and_then(Value::as_bool) {
            assert_eq!(
                response["remote_safe_only"], value,
                "{case_id}: remote policy"
            );
            assert!(citations
                .iter()
                .all(|citation| citation["remote_export_allowed"] == true));
        }
        for field in ["community_only", "fact_supported", "retrieval_mode"] {
            if let Some(value) = expected.get(field) {
                assert_eq!(&response[field], value, "{case_id}: {field}");
            }
        }
        if let Some(value) = expected.get("answer_contains").and_then(Value::as_str) {
            assert!(
                response["answer"]
                    .as_str()
                    .is_some_and(|answer| answer.contains(value)),
                "{case_id}: answer should contain {value}"
            );
        }
        if let (Some(document_id), Some(maximum)) = (
            expected.get("document_id").and_then(Value::as_str),
            expected
                .get("document_citation_max")
                .and_then(Value::as_u64),
        ) {
            let count = assertion_citations
                .iter()
                .filter(|citation| citation["document_id"] == document_id)
                .count();
            assert!(
                count <= maximum as usize,
                "{case_id}: per-document citation cap"
            );
        }
        if let Some(document_id) = expected
            .get("vector_scored_document_id")
            .and_then(Value::as_str)
        {
            assert!(
                assertion_citations.iter().any(|citation| {
                    citation["document_id"] == document_id && citation["vector_score"].is_number()
                }),
                "{case_id}: vector candidate should survive hybrid fusion"
            );
        }

        drop(store);
        let _ = remove_sqlite_files(&path);
    }
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
fn hk_as_of_filters_recent_and_lexical_candidates_before_their_limits() {
    let path = temporary_database_path("hk-as-of-filter");
    let store = ResearchStore::open(&path).unwrap();
    let mut documents = (0..140)
        .map(|index| {
            json!({
                "document_id": format!("hk-future-{index:03}"), "title": "Future filing",
                "content": "hkcutoffprobe", "source_tier": "filing",
                "published_at": "2026-08-21T09:00:00+08:00", "stock_codes": ["00700.HK"],
                "metadata": {"market": "hk"}
            })
        })
        .collect::<Vec<_>>();
    documents.extend([
        json!({"document_id":"hk-valid", "title":"Current filing",
                "content":"hkcutoffprobe older supporting details", "source_tier":"filing",
                "published_at":"2026-08-20T08:45:00+08:00", "stock_codes":["00700.HK"]}),
        json!({"document_id":"hk-unknown-date", "title":"Undated filing",
                "content":"hkcutoffprobe", "source_tier":"filing", "stock_codes":["00700.HK"]}),
    ]);
    store.ingest_documents(&documents).unwrap();
    let request = json!({"query":"hkcutoffprobe", "stock_code":"00700.HK", "top_k":1,
            "as_of":"2026-08-20T09:00:00+08:00"});
    for result in [
        store.agent_evidence(&request),
        store.unscoped_hk_agent_evidence(&request),
    ] {
        let result = result.unwrap();
        assert_eq!(result["citations"].as_array().unwrap().len(), 1);
        assert_eq!(result["citations"][0]["document_id"], "hk-valid");
    }
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn as_of_rejects_invalid_dates_and_includes_the_whole_utc_date() {
    let path = temporary_database_path("research-as-of-validation");
    let store = ResearchStore::open(&path).unwrap();
    store
        .ingest_documents(&[json!({"document_id":"late-utc", "title":"Late filing",
            "content":"cutoffvalidation", "source_tier":"filing", "stock_codes":["00700.HK"],
            "published_at":"2026-08-20T23:59:59.999999999Z"})])
        .unwrap();
    for cutoff in [json!("2026-02-30"), json!("tomorrow"), json!(""), json!(42)] {
        let request = json!({"query":"cutoffvalidation", "stock_code":"00700.HK", "as_of":cutoff});
        assert!(store.query(&request).is_err(), "{cutoff}");
        assert!(store.agent_evidence(&request).is_err(), "{cutoff}");
    }
    let request =
        json!({"query":"cutoffvalidation", "stock_code":"00700.HK", "as_of":"2026-08-20"});
    for result in [store.query(&request), store.agent_evidence(&request)] {
        assert_eq!(result.unwrap()["citations"][0]["document_id"], "late-utc");
    }
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn as_of_filters_future_lexical_and_vector_candidates_before_top_k() {
    let path = temporary_database_path("research-as-of-filter");
    let store = ResearchStore::open(&path).expect("research store should open");
    let mut documents = (0..51)
        .map(|index| {
            json!({
                "document_id": format!("future-{index:02}"),
                "title": "Future filing",
                "content": "asofprobe",
                "source_tier": "filing",
                "published_at": "2026-08-21T09:00:00+08:00",
                "stock_codes": ["000001.SZ"]
            })
        })
        .collect::<Vec<_>>();
    documents.push(json!({
        "document_id": "valid-before-cutoff",
        "title": "Current filing",
        "content": "asofprobe older supporting details",
        "source_tier": "filing",
        "published_at": "2026-08-19T09:00:00+08:00",
        "stock_codes": ["000001.SZ"]
    }));
    store
        .ingest_documents(&documents)
        .expect("documents should ingest");
    let work = store
        .pending_embedding_chunks(500)
        .expect("embedding work should load");
    let embeddings = work
        .into_iter()
        .map(|item| {
            let is_future = item.chunk_id.starts_with("future-");
            let mut vector = vec![0.0f32; 512];
            if is_future {
                vector[0] = 1.0;
            } else {
                vector[1] = 1.0;
            }
            (item, vector)
        })
        .collect::<Vec<_>>();
    store
        .store_embeddings(&embeddings, "as-of-test")
        .expect("embeddings should store");
    let mut query_vector = vec![0.0f32; 512];
    query_vector[0] = 1.0;

    let response = store
        .query_with_vector(
            &json!({
                "query": "asofprobe",
                "stock_code": "000001.SZ",
                "top_k": 1,
                "as_of": "2026-08-20T09:00:00+08:00"
            }),
            Some(&query_vector),
        )
        .expect("as-of query should succeed");

    let citations = response["citations"]
        .as_array()
        .expect("citations should be an array");
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0]["document_id"], "valid-before-cutoff");
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn as_of_preserves_subsecond_precision_before_all_candidate_limits() {
    let path = temporary_database_path("research-as-of-subsecond");
    let store = ResearchStore::open(&path).unwrap();
    let mut documents = (0..51)
        .map(|index| {
            json!({
                "document_id":format!("fractional-future-{index:02}"), "title":"Future notice",
                "content":"fractionalprobe", "source_tier":"filing", "stock_codes":["00700.HK"],
                "published_at":"2026-11-10T09:00:00.100000001+08:00"
            })
        })
        .collect::<Vec<_>>();
    documents.push(json!({"document_id":"fractional-valid", "title":"Current notice",
            "content":"fractionalprobe supporting details", "source_tier":"filing", "stock_codes":["00700.HK"],
            "published_at":"2026-11-10T01:00:00.100000000Z"}));
    store.ingest_documents(&documents).unwrap();
    let work = store.pending_embedding_chunks(500).unwrap();
    let embeddings = work
        .into_iter()
        .map(|item| {
            let mut vector = vec![0.0f32; 512];
            let axis = if item.chunk_id.starts_with("fractional-future-") {
                0
            } else {
                1
            };
            vector[axis] = 1.0;
            (item, vector)
        })
        .collect::<Vec<_>>();
    store
        .store_embeddings(&embeddings, "fractional-test")
        .unwrap();
    let mut vector = vec![0.0f32; 512];
    vector[0] = 1.0;
    let request = json!({"query":"fractionalprobe", "stock_code":"00700.HK", "top_k":1,
            "as_of":"2026-11-10T09:00:00.100000000+08:00"});
    for result in [
        store.query(&request),
        store.query_with_vector(&request, Some(&vector)),
        store.agent_evidence(&request),
        store.unscoped_hk_agent_evidence(&request),
    ] {
        let result = result.unwrap();
        assert_eq!(result["citations"].as_array().unwrap().len(), 1);
        assert_eq!(result["citations"][0]["document_id"], "fractional-valid");
    }
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn newer_publication_breaks_equal_source_and_retrieval_score_ties() {
    let path = temporary_database_path("research-recency-tie-break");
    let store = ResearchStore::open(&path).expect("research store should open");
    store
        .ingest_documents(&[
            json!({
                "document_id": "a-old",
                "title": "Certification filing",
                "content": "recencyprobe certification status",
                "source_tier": "filing",
                "published_at": "2026-02-10T09:30:00+08:00"
            }),
            json!({
                "document_id": "z-current",
                "title": "Certification filing",
                "content": "recencyprobe certification status",
                "source_tier": "filing",
                "published_at": "2026-08-20T09:30:00+08:00"
            }),
        ])
        .expect("documents should ingest");

    let response = store
        .query(&json!({"query":"recencyprobe", "top_k":2, "as_of":"2026-08-21T00:00:00+08:00"}))
        .expect("query should succeed");

    let citations = response["citations"]
        .as_array()
        .expect("citations should be an array");
    assert_eq!(citations[0]["document_id"], "z-current");
    assert_eq!(citations[1]["document_id"], "a-old");
    drop(store);
    let _ = remove_sqlite_files(&path);
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
    assert_eq!(store.index_status().unwrap()["schema_version"], 3);
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn deletes_only_the_selected_research_thread_and_cascades_answers() {
    let path = temporary_database_path("delete-research-thread");
    let store = ResearchStore::open(&path).expect("research store should open");
    store
        .ingest_documents(&[json!({
            "document_id": "delete-doc",
            "title": "待删除引用文档",
            "content": "删除会话后引用计数必须回收。",
            "source_tier": "filing"
        })])
        .expect("document should be ingested");
    let first = store
        .create_thread(&json!({"id": "thread-delete", "title": "待删除"}))
        .expect("first thread should be created");
    let second = store
        .create_thread(&json!({"id": "thread-keep", "title": "保留"}))
        .expect("second thread should be created");
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    let evidence = store
        .query(&json!({"query": "引用计数", "top_k": 1}))
        .expect("evidence should be available for the deletion test");
    store
        .save_answer(
            first_id,
            "待删除的问题",
            &json!({
                "answer": "待删除的回答",
                "citations": evidence["citations"].clone()
            }),
        )
        .expect("first answer should be persisted");
    store
        .save_answer(
            second_id,
            "保留的问题",
            &json!({
                "answer": "保留的回答",
                "citations": evidence["citations"].clone()
            }),
        )
        .expect("second answer should be persisted");
    let cited_before: i64 = store
        .connection()
        .expect("research connection should open")
        .query_row(
            "SELECT cited_count FROM documents WHERE id = ?1",
            params!["delete-doc"],
            |row| row.get(0),
        )
        .expect("document citation count should be readable");
    assert_eq!(cited_before, 2);

    assert_eq!(store.delete_thread(first_id).unwrap(), 1);
    assert!(store.thread(first_id).is_err());
    let remaining = store.thread(second_id).expect("other thread should remain");
    assert_eq!(remaining["answers"].as_array().map(Vec::len), Some(1));
    let cited_after: i64 = store
        .connection()
        .expect("research connection should open")
        .query_row(
            "SELECT cited_count FROM documents WHERE id = ?1",
            params!["delete-doc"],
            |row| row.get(0),
        )
        .expect("document citation count should be readable after deletion");
    assert_eq!(cited_after, 1);
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn index_status_reports_retrieval_governance_state() {
    let path = temporary_database_path("index-governance");
    let store = ResearchStore::open(&path).expect("research store should open");
    store
        .ingest_documents(&[json!({
            "document_id": "governance-doc",
            "title": "治理状态",
            "content": "索引完整性需要同时检查 FTS 与向量状态。",
            "source_tier": "filing"
        })])
        .expect("document should be ingested");

    let pending = store
        .index_status()
        .expect("index status should load before embeddings");
    assert_eq!(pending["fts_missing_count"], 0);
    assert_eq!(pending["fts_orphan_count"], 0);
    assert_eq!(pending["embedding_pending_count"], 1);
    assert_eq!(pending["embedding_stale_count"], 0);
    assert_eq!(pending["embedding_orphan_count"], 0);
    assert_eq!(pending["embedding_invalid_count"], 0);
    assert_eq!(pending["hybrid_ready"], false);

    let pending_work = store
        .pending_embedding_chunks(10)
        .expect("pending embeddings should be readable");
    let invalid_work = pending_work
        .iter()
        .cloned()
        .map(|item| (item, vec![0.0; RESEARCH_EMBEDDING_DIMENSIONS as usize]))
        .collect::<Vec<_>>();
    assert!(store.store_embeddings(&invalid_work, "test-model").is_err());

    let work = pending_work
        .into_iter()
        .map(|item| (item, vec![1.0; RESEARCH_EMBEDDING_DIMENSIONS as usize]))
        .collect::<Vec<_>>();
    store
        .store_embeddings(&work, "test-model")
        .expect("embedding should be stored");

    let ready = store
        .index_status()
        .expect("index status should load after embeddings");
    assert_eq!(ready["embedding_pending_count"], 0);
    assert_eq!(ready["embedding_count"], 1);
    assert_eq!(ready["integrity_healthy"], true);
    assert_eq!(ready["hybrid_ready"], false);
    let stored_blob = store
        .connection()
        .expect("research connection should open")
        .query_row("SELECT vector FROM embeddings LIMIT 1", [], |row| {
            row.get::<_, Vec<u8>>(0)
        })
        .expect("stored embedding should be readable");
    let stored_vector = decode_f32_blob(&stored_blob).expect("stored embedding should decode");
    let norm = stored_vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "stored vector norm was {norm}");

    let connection = store.connection().expect("research connection should open");
    connection
        .execute(
            "DELETE FROM chunks_fts WHERE chunk_id = 'governance-doc:0'",
            [],
        )
        .expect("FTS row should be removable for the integrity check");
    connection
        .execute(
            "INSERT INTO chunks_fts (chunk_id, title_terms, entity_terms, body_terms)
                 VALUES ('orphan-chunk', 'orphan', '', 'orphan')",
            [],
        )
        .expect("orphan FTS row should be insertable for the integrity check");
    let mismatched = store
        .index_status()
        .expect("mismatched index status should load");
    assert_eq!(mismatched["chunk_count"], mismatched["fts_count"]);
    assert_eq!(mismatched["healthy"], true);
    assert_eq!(mismatched["fts_healthy"], false);
    assert_eq!(mismatched["integrity_healthy"], false);

    drop(store);
    let _ = remove_sqlite_files(&path);
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
fn unchanged_content_refreshes_search_and_message_metadata() {
    let path = temporary_database_path("incremental-metadata");
    let store = ResearchStore::open(&path).expect("research store should open");
    store
        .ingest_documents(&[json!({
            "document_id": "stable-pdf-page",
            "title": "Old title",
            "content": "The underlying evidence text remains unchanged.",
            "source_tier": "research_report",
            "source_name": "Old source",
            "stock_codes": ["600000.SH"],
            "entities": ["Old entity"],
            "sentiment": "uncertain"
        })])
        .expect("initial document should be ingested");
    let work = store
        .pending_embedding_chunks(10)
        .unwrap()
        .into_iter()
        .map(|item| (item, vec![1.0; 512]))
        .collect::<Vec<_>>();
    store.store_embeddings(&work, "test-model").unwrap();

    let result = store
        .ingest_documents(&[json!({
            "document_id": "stable-pdf-page",
            "title": "Updated title",
            "content": "The underlying evidence text remains unchanged.",
            "source_tier": "filing",
            "source_name": "Updated source",
            "stock_codes": ["000001.SZ"],
            "entities": ["Updated entity"],
            "sentiment": "positive"
        })])
        .expect("metadata-only refresh should succeed");

    assert_eq!(result["chunk_count"], 0);
    assert_eq!(store.index_status().unwrap()["embedding_count"], 1);
    assert_eq!(
        store
            .query(&json!({"query": "updated entity", "stock_code": "000001.SZ"}))
            .unwrap()["citations"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(store
        .query(&json!({"query": "underlying evidence", "stock_code": "600000.SH"}))
        .unwrap()["citations"]
        .as_array()
        .is_some_and(Vec::is_empty));
    let messages = store.messages(&json!({})).unwrap();
    assert_eq!(messages["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(messages["items"][0]["stock_code"], "000001.SZ");
    assert_eq!(messages["items"][0]["title"], "Updated title");
    assert_eq!(messages["items"][0]["sentiment"], "positive");

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
        serde_json::to_vec(
            &json!({"schema_version": 1, "items": [first_item.clone(), second_item.clone()]}),
        )
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
        serde_json::to_vec(&json!({"schema_version": 1, "items": [second_item, first_item]}))
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
fn legacy_document_ids_include_their_source_container() {
    let path = temporary_database_path("legacy-container-collision").with_extension("json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema_version": "upstream-rag-pack-v1",
            "items": [{
                "id": "shared-id",
                "title": "News item",
                "content": "Evidence from the items container."
            }],
            "documents": [{
                "id": "shared-id",
                "title": "Research document",
                "content": "Evidence from the documents container."
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let documents = legacy_documents_from_file(&path).unwrap();
    let ids = documents
        .iter()
        .map(|item| item["document_id"].as_str().unwrap())
        .collect::<HashSet<_>>();

    assert_eq!(documents.len(), 2);
    assert_eq!(ids.len(), 2);
    let _ = fs::remove_file(path);
}

#[test]
fn unique_legacy_document_ids_remain_backward_compatible() {
    let path = temporary_database_path("legacy-id-compatibility").with_extension("json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "items": [{
                "id": "stable-news-id",
                "title": "Stable news",
                "content": "Existing installations already use the original stable ID."
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let path_hash = sha256_hex(path.display().to_string().as_bytes());
    let identity_hash = sha256_hex(b"id:stable-news-id");
    let expected = format!("legacy-{}-{}", &path_hash[..16], &identity_hash[..24]);

    let documents = legacy_documents_from_file(&path).unwrap();

    assert_eq!(documents[0]["document_id"], expected);
    let _ = fs::remove_file(path);
}

#[test]
fn legacy_pack_import_rejects_unknown_versions() {
    let path = temporary_database_path("legacy-unknown-version").with_extension("json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema_version": 7,
            "items": [{"id": "future", "content": "Future schema evidence."}]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = legacy_documents_from_file(&path)
        .expect_err("unknown legacy pack versions must not be guessed");

    assert!(error.contains("expected v1"));
    let _ = fs::remove_file(path);
}

#[test]
fn legacy_pack_import_rejects_oversized_documents() {
    let path = temporary_database_path("legacy-oversized-document").with_extension("json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "items": [{
                "id": "oversized",
                "content": "x".repeat(MAX_PACK_DOCUMENT_CHARS + 1)
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = legacy_documents_from_file(&path)
        .expect_err("oversized legacy documents must fail the whole import");

    assert!(error.contains("oversized document"));
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
fn legacy_sources_archive_after_import_and_resume_sidecars() {
    let app_data = temporary_database_path("legacy-archive").with_extension("appdata");
    let database_path = app_data.join("research").join("research.sqlite");
    let source = app_data.join("news").join("news-cache.json");
    let nested_source = app_data
        .join("upstream_rag_desktop")
        .join("session")
        .join("rag_pack.sqlite");
    fs::create_dir_all(source.parent().unwrap()).expect("news directory should be created");
    fs::create_dir_all(nested_source.parent().unwrap())
        .expect("legacy directory should be created");
    let payload = serde_json::to_vec(&json!({
        "schema_version": 1,
        "items": [{
            "document_id": "legacy-archive-doc",
            "title": "Legacy archive",
            "content": "Legacy content",
            "source_tier": "news"
        }]
    }))
    .expect("legacy payload should serialize");
    fs::write(&source, &payload).expect("news cache should be written");
    fs::write(&nested_source, &payload).expect("legacy sqlite JSON should be written");
    fs::write(
        PathBuf::from(format!("{}-wal", nested_source.display())),
        b"wal",
    )
    .expect("legacy sidecar should be written");

    let store = ResearchStore::open(&database_path).expect("research store should open");
    migrate_legacy_sources(&store, &app_data).expect("legacy migration should complete");

    let archive_root = app_data.join("research").join("legacy-v1");
    assert!(!source.exists());
    assert!(!nested_source.exists());
    assert!(archive_root.join("news").join("news-cache.json").exists());
    assert!(archive_root
        .join("upstream_rag_desktop")
        .join("session")
        .join("rag_pack.sqlite")
        .exists());
    assert!(archive_root
        .join("upstream_rag_desktop")
        .join("session")
        .join("rag_pack.sqlite-wal")
        .exists());
    let archive_marker = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT value FROM research_metadata WHERE key = 'legacy_v1_archive_complete'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(archive_marker, "true");

    // Simulate a process interrupted after moving the primary file but before
    // its SQLite sidecar; the next archive pass must finish the sidecar move.
    let resume_source = app_data.join("rag").join("rag-pack.json");
    let resume_destination = archive_root.join("rag").join("rag-pack.json");
    fs::create_dir_all(resume_source.parent().unwrap()).expect("rag directory should exist");
    fs::create_dir_all(resume_destination.parent().unwrap())
        .expect("archive rag directory should exist");
    fs::write(&resume_source, &payload).expect("resume source should be written");
    fs::write(
        PathBuf::from(format!("{}-wal", resume_source.display())),
        b"wal",
    )
    .expect("resume sidecar should be written");
    fs::rename(&resume_source, &resume_destination)
        .expect("resume primary should be moved before retry");
    archive_legacy_sources(&app_data, &[resume_source.clone()])
        .expect("archive retry should move remaining sidecar");
    assert!(archive_root.join("rag").join("rag-pack.json-wal").exists());

    drop(store);
    let _ = fs::remove_dir_all(&app_data);
}

#[test]
fn overview_reports_latest_refresh_time_and_source_quality_order() {
    let path = temporary_database_path("overview-refresh-and-source-order");
    let store = ResearchStore::open(&path).expect("research store should open");
    store
        .ingest_documents(&[
            json!({
                "document_id": "overview-news",
                "title": "媒体消息",
                "content": "媒体消息内容。",
                "source_tier": "news_media"
            }),
            json!({
                "document_id": "overview-policy",
                "title": "官方政策",
                "content": "官方政策内容。",
                "source_tier": "policy_official"
            }),
            json!({
                "document_id": "overview-community",
                "title": "社区讨论",
                "content": "社区讨论内容。",
                "source_tier": "community"
            }),
        ])
        .expect("documents should be ingested");
    let connection = store.connection().expect("research connection should open");
    connection
        .execute(
            "UPDATE documents SET updated_at_epoch_ms = CASE id
                    WHEN 'overview-news' THEN 100
                    WHEN 'overview-policy' THEN 300
                    WHEN 'overview-community' THEN 200
                    ELSE updated_at_epoch_ms END",
            [],
        )
        .expect("document refresh timestamps should be writable");
    connection
        .execute(
            "INSERT INTO research_metadata (key, value)
                 VALUES ('last_refresh_at_epoch_ms', '400')",
            [],
        )
        .expect("refresh timestamp should be writable");

    let overview = store.overview(&json!({})).expect("overview should load");
    assert_eq!(overview["updated_at_epoch_ms"], 400);
    let tiers = overview["source_counts"]
        .as_array()
        .expect("source counts should be an array")
        .iter()
        .map(|item| item["source_tier"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(tiers, vec!["policy_official", "news_media", "community"]);

    drop(connection);
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn news_cache_ingest_records_refresh_time_even_when_no_documents_exist() {
    let database_path = temporary_database_path("empty-news-cache-refresh");
    let cache_path = database_path.with_extension("json");
    let store = ResearchStore::open(&database_path).expect("research store should open");
    fs::write(
        &cache_path,
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "updated_at_epoch_ms": 1_780_000_000_123_i64,
            "items": []
        }))
        .unwrap(),
    )
    .expect("empty news cache should be written");

    ingest_news_cache_file(&store, &cache_path).expect("news cache should be recorded");

    assert_eq!(
        store.overview(&json!({})).unwrap()["updated_at_epoch_ms"],
        1_780_000_000_123_i64
    );
    drop(store);
    let _ = fs::remove_file(cache_path);
    let _ = remove_sqlite_files(&database_path);
    let migrated_cache_root = database_path.with_extension("");
    if migrated_cache_root.exists() {
        assert_eq!(
            migrated_cache_root.parent(),
            Some(std::env::temp_dir().as_path())
        );
        fs::remove_dir_all(migrated_cache_root).unwrap();
    }
}

#[test]
fn research_ingests_current_hong_kong_news_from_sqlite_when_no_legacy_json_exists() {
    use crate::external_data::{DataDomain, DataStore, Market, StoredRecord};
    let root = temporary_database_path("hk-sqlite-news-ingest").with_extension("dir");
    let cache_path = root.join("news/news-cache-hk.json");
    let mut external = DataStore::open(crate::external_data::store_path(&root)).unwrap();
    let published = "2026-11-06 20:32:00";
    let fetched = "1794154333645";
    let item = json!({"id":"tencent-news-current","title":"腾讯股份购回交割","summary":"公司披露股份购回交割事项。",
            "stock_codes":["00700.HK"],"source":"新浪财经","source_tier":"news","published_at":published,
            "fetched_at_epoch_ms":1794154333645_u64,"url":"https://finance.sina.com.cn/stock/hk/tencent.shtml","remote_export_allowed":false});
    external
        .replace_domain_batch(
            DataDomain::News,
            Market::HongKong,
            &[StoredRecord::new(
                DataDomain::News,
                Market::HongKong,
                "current-news",
                "item",
                "sina_hk",
                "news",
                fetched,
                Some(published),
                None,
                "unverified",
                vec![],
                item,
            )],
        )
        .unwrap();
    external.replace_domain_batch(DataDomain::News, Market::Global, &[StoredRecord::new(DataDomain::News,Market::Global,
            "mainland-news","item","fixture","news",fetched,Some(published),None,"unverified",vec![],
            json!({"id":"cn-only","title":"A股资料不得进入港股库","summary":"A股资料。","stock_codes":["600519.SH"],"source":"fixture","published_at":published}))]).unwrap();
    assert!(!cache_path.exists());
    let store = ResearchStore::open(root.join("research/research-hk.sqlite")).unwrap();
    let imported =
        ingest_news_cache_file_for_market(&store, &cache_path, Market::HongKong).unwrap();
    assert_eq!(imported["document_count"], 1);
    let messages = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(messages["items"][0]["title"], "腾讯股份购回交割");
    assert_eq!(
        messages["items"][0]["url"],
        "https://finance.sina.com.cn/stock/hk/tencent.shtml"
    );
    assert_eq!(
        store.overview(&json!({})).unwrap()["updated_at_epoch_ms"],
        1794154333645_i64
    );
    let result = store
        .query(&json!({"query":"腾讯 股份购回","stock_code":"00700.HK"}))
        .unwrap();
    assert_eq!(result["citations"][0]["source_name"], "新浪财经");
    assert_eq!(result["citations"][0]["remote_export_allowed"], false);
    assert!(!result.to_string().contains("A股资料不得进入港股库"));
    ingest_news_cache_file_for_market(&store, &cache_path, Market::HongKong).unwrap();
    assert_eq!(store.overview(&json!({})).unwrap()["document_count"], 1);
    drop(store);
    drop(external);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn research_sqlite_news_migration_keeps_previously_cited_identity_without_a_legacy_item_id() {
    let root = temporary_database_path("hk-news-original-identity").with_extension("dir");
    let path = root.join("news/news-cache-hk.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"updated_at_epoch_ms":1794154333645_i64,"items":[{
            "title":"腾讯股份购回交割","summary":"腾讯披露股份购回交割事项。","source":"新浪财经","source_tier":"news",
            "published_at":"2026-11-06 20:32:00","stock_codes":["00700.HK"],"url":"https://finance.sina.com.cn/stock/hk/tencent.shtml"
        }]})).unwrap()).unwrap();
    let store = ResearchStore::open(root.join("research/research-hk.sqlite")).unwrap();
    store
        .ingest_documents(&legacy_documents_from_file(&path).unwrap())
        .unwrap();
    let cited = store
        .query(&json!({"query":"腾讯股份购回","stock_code":"00700.HK"}))
        .unwrap()["citations"][0]["document_id"]
        .clone();
    assert!(cited.as_str().is_some());
    ingest_news_cache_file_for_market(&store, &path, crate::external_data::Market::HongKong)
        .unwrap();
    assert_eq!(store.overview(&json!({})).unwrap()["document_count"], 1);
    let current = store
        .query(&json!({"query":"腾讯股份购回","stock_code":"00700.HK"}))
        .unwrap();
    assert_eq!(current["citations"][0]["document_id"], cited);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(windows)]
fn news_cache_ingest_keeps_one_message_across_windows_separator_spellings() {
    let root = temporary_database_path("hk-news-path-spelling").with_extension("dir");
    let first_path = root.join("news/news-cache-hk.json");
    let second_path = root.join("news").join("news-cache-hk.json");
    fs::create_dir_all(second_path.parent().unwrap()).unwrap();
    let item = json!({
        "id":"tencent-path-news", "title":"腾讯股份购回交割",
        "summary":"腾讯披露股份购回交割事项。", "source":"新浪财经",
        "source_tier":"news", "stock_codes":["00700.HK"],
        "published_at":"2026-11-09 11:55:00",
        "url":"https://finance.sina.com.cn/stock/hk/tencent.shtml"
    });
    fs::write(
        &second_path,
        serde_json::to_vec(&json!({"items":[item]})).unwrap(),
    )
    .unwrap();
    let store = ResearchStore::open(root.join("research/research-hk.sqlite")).unwrap();
    ingest_news_cache_file_for_market(&store, &first_path, crate::external_data::Market::HongKong)
        .unwrap();
    let first = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    let original_id = first["items"][0]["document_id"].clone();

    ingest_news_cache_file_for_market(&store, &second_path, crate::external_data::Market::HongKong)
        .unwrap();
    let current = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(current["items"].as_array().unwrap().len(), 1);
    assert_eq!(current["items"][0]["document_id"], original_id);
    assert_eq!(store.overview(&json!({})).unwrap()["document_count"], 1);
    drop(store);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn news_cache_ingest_reconciles_existing_path_duplicates_without_losing_citations() {
    let root = temporary_database_path("hk-news-existing-path-duplicates").with_extension("dir");
    let path = root.join("news").join("news-cache-hk.json");
    let old_path = root.join("news").join(".").join("news-cache-hk.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let item = json!({
        "id":"tencent-existing-news", "title":"腾讯股份购回交割",
        "summary":"腾讯披露股份购回交割事项。", "source":"新浪财经",
        "source_tier":"news", "stock_codes":["00700.HK"],
        "published_at":"2026-11-09 11:55:00",
        "url":"https://finance.sina.com.cn/stock/hk/tencent.shtml",
        "remote_export_allowed":false
    });
    fs::write(
        &path,
        serde_json::to_vec(&json!({"items":[item.clone()]})).unwrap(),
    )
    .unwrap();
    let store = ResearchStore::open(root.join("research/research-hk.sqlite")).unwrap();
    // Reproduce documents written by the previous two importer paths.
    let mut old = normalize_legacy_document(item.clone(), &old_path, &[]).unwrap();
    old["remote_export_allowed"] = json!(false);
    let mut current = normalize_legacy_document(item, &path, &[]).unwrap();
    current["remote_export_allowed"] = json!(false);
    store.ingest_documents(&[old, current]).unwrap();
    let before = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(before["items"].as_array().unwrap().len(), 2);
    store
        .mark_read(&json!({"message_ids":[before["items"][1]["id"].clone()]}))
        .unwrap();
    let response = store
        .query(&json!({"query":"腾讯股份购回", "stock_code":"00700.HK"}))
        .unwrap();
    assert_eq!(response["citations"].as_array().unwrap().len(), 2);
    let vector = vec![1.0_f32; 512];
    let embeddings = store
        .pending_embedding_chunks(10)
        .unwrap()
        .into_iter()
        .map(|work| (work, vector.clone()))
        .collect::<Vec<_>>();
    store
        .store_embeddings(&embeddings, "news-identity-fixture")
        .unwrap();
    let vector_query = json!({"query":"no-lexical-match-xyz", "stock_code":"00700.HK"});
    assert_eq!(
        store
            .query_with_vector(&vector_query, Some(&vector))
            .unwrap()["citations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let thread = store
        .create_thread(&json!({"title":"腾讯查询", "stock_code":"00700.HK"}))
        .unwrap();
    let thread_id = thread["id"].as_str().unwrap();
    store
        .save_answer(thread_id, "腾讯股份购回", &response)
        .unwrap();

    ingest_news_cache_file_for_market(&store, &path, crate::external_data::Market::HongKong)
        .unwrap();
    let messages = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(messages["items"].as_array().unwrap().len(), 1);
    assert_eq!(messages["items"][0]["unread"], false);
    let overview = store.overview(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(overview["document_count"], 1);
    assert_eq!(overview["chunk_count"], 1);
    assert_eq!(overview["unread_count"], 0);
    assert!(overview["unread_by_stock"].get("00700.HK").is_none());
    assert_eq!(overview["source_counts"][0]["count"], 1);
    assert_eq!(
        store
            .messages(&json!({"stock_code":"00700.HK", "offset":1}))
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let query = store
        .query(&json!({"query":"腾讯股份购回", "stock_code":"00700.HK"}))
        .unwrap();
    assert_eq!(query["citations"].as_array().unwrap().len(), 1);
    assert_eq!(query["citations"][0]["remote_export_allowed"], false);
    let hybrid = store
        .query_with_vector(&vector_query, Some(&vector))
        .unwrap();
    assert_eq!(hybrid["retrieval_mode"], "hybrid_rrf");
    assert_eq!(hybrid["citations"].as_array().unwrap().len(), 1);
    let history = store.thread(thread_id).unwrap();
    assert_eq!(history["answers"][0]["citations"], response["citations"]);
    assert_eq!(
        store.document_statuses().unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .sentiment_documents("00700.HK", epoch_millis() + 60_000)
            .unwrap()
            .len(),
        1
    );

    ingest_news_cache_file_for_market(&store, &old_path, crate::external_data::Market::HongKong)
        .unwrap();
    assert_eq!(
        store.messages(&json!({"stock_code":"00700.HK"})).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.thread(thread_id).unwrap()["answers"][0]["citations"],
        response["citations"]
    );
    drop(store);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn news_cache_ingest_keeps_identical_item_ids_in_distinct_source_containers() {
    let root = temporary_database_path("hk-news-distinct-containers").with_extension("dir");
    let path = root.join("news").join("news-cache-hk.json");
    let other_path = root.join("archive").join("news-cache-hk.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let item = json!({
        "id":"tencent-shared-id", "title":"腾讯股份购回交割",
        "summary":"腾讯披露股份购回交割事项。", "source":"新浪财经",
        "source_tier":"news", "stock_codes":["00700.HK"],
        "published_at":"2026-11-09 11:55:00",
        "url":"https://finance.sina.com.cn/stock/hk/tencent.shtml"
    });
    fs::write(
        &path,
        serde_json::to_vec(&json!({"items":[item.clone()]})).unwrap(),
    )
    .unwrap();
    let store = ResearchStore::open(root.join("research/research-hk.sqlite")).unwrap();
    store
        .ingest_documents(&[normalize_legacy_document(item, &other_path, &[]).unwrap()])
        .unwrap();
    ingest_news_cache_file_for_market(&store, &path, crate::external_data::Market::HongKong)
        .unwrap();
    assert_eq!(
        store.messages(&json!({"stock_code":"00700.HK"})).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(store.overview(&json!({})).unwrap()["document_count"], 2);
    drop(store);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn news_cache_ingest_survives_retention_of_duplicate_identities() {
    for (label, protect_old) in [("old-canonical", true), ("new-alias", false)] {
        let root = temporary_database_path(label).with_extension("dir");
        let path = root.join("news").join("news-cache-hk.json");
        let old_path = root.join("news").join(".").join("news-cache-hk.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let item = json!({
            "id":"tencent-old-news", "title":"腾讯股份购回交割",
            "summary":"腾讯披露股份购回交割事项。", "source":"新浪财经",
            "source_tier":"news", "stock_codes":["00700.HK"],
            "published_at":"2025-01-01 11:55:00", "user_imported":false,
            "url":"https://finance.sina.com.cn/stock/hk/tencent-old.shtml"
        });
        fs::write(
            &path,
            serde_json::to_vec(&json!({"items":[item.clone()]})).unwrap(),
        )
        .unwrap();
        let store = ResearchStore::open(root.join("research/research-hk.sqlite")).unwrap();
        let mut old = normalize_legacy_document(item.clone(), &old_path, &[]).unwrap();
        let mut current = normalize_legacy_document(item, &path, &[]).unwrap();
        // Seed old records protected until the test establishes a citation.
        for document in [&mut old, &mut current] {
            document["pinned"] = json!(true);
            document["user_imported"] = json!(false);
        }
        let old_id = old["document_id"].as_str().unwrap().to_string();
        let current_id = current["document_id"].as_str().unwrap().to_string();
        store.ingest_documents(&[old, current]).unwrap();
        let protected_id = if protect_old { &old_id } else { &current_id };
        let mut response = store
            .query(&json!({"query":"腾讯股份购回","stock_code":"00700.HK"}))
            .unwrap();
        response["citations"]
            .as_array_mut()
            .unwrap()
            .retain(|citation| citation["document_id"].as_str() == Some(protected_id.as_str()));
        assert_eq!(response["citations"].as_array().unwrap().len(), 1);
        let thread = store
            .create_thread(&json!({"title":"保留历史引用"}))
            .unwrap();
        let thread_id = thread["id"].as_str().unwrap();
        store
            .save_answer(thread_id, "腾讯股份购回", &response)
            .unwrap();
        let connection = store.connection().unwrap();
        connection
            .execute(
                "UPDATE documents SET pinned=0,imported_at_epoch_ms=1 WHERE id=?1",
                params![old_id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE documents SET pinned=0,imported_at_epoch_ms=2 WHERE id=?1",
                params![current_id],
            )
            .unwrap();
        if !protect_old {
            connection.execute("UPDATE documents SET metadata_json=json_set(metadata_json,'$.news_cache_duplicate_of',?2) WHERE id=?1",params![current_id,old_id]).unwrap();
        }
        drop(connection);

        ingest_news_cache_file_for_market(&store, &path, crate::external_data::Market::HongKong)
            .expect("retention may remove a duplicate; cache ingestion must remain available");
        let messages = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
        assert_eq!(messages["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            messages["items"][0]["document_id"].as_str(),
            Some(protected_id.as_str())
        );
        assert_eq!(store.overview(&json!({})).unwrap()["document_count"], 1);
        assert_eq!(
            store
                .query(&json!({"query":"腾讯股份购回","stock_code":"00700.HK"}))
                .unwrap()["citations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.thread(thread_id).unwrap()["answers"][0]["citations"],
            response["citations"]
        );
        drop(store);
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn article_summary_supplements_messages_without_changing_cited_document() {
    let database = temporary_database_path("article-summary-metadata");
    let source = database.with_extension("news-cache-hk.json");
    let store = ResearchStore::open(&database).unwrap();
    let mut item = json!({"id":"sina-recruitment","title":"腾讯招聘","summary":"腾讯招聘",
            "source":"新浪财经","source_tier":"news","stock_codes":["00700.HK"],
            "url":"https://t.cj.sina.cn/articles/view/1","published_at":"2026-11-09"});
    let old = normalize_legacy_document(item.clone(), &source, &[]).unwrap();
    store.ingest_documents(&[old.clone()]).unwrap();
    let result = store
        .query(&json!({"query":"腾讯招聘","stock_code":"00700.HK"}))
        .unwrap();
    let thread = store.create_thread(&json!({"title":"旧新闻引用"})).unwrap();
    let thread_id = thread["id"].as_str().unwrap();
    store.save_answer(thread_id, "腾讯招聘", &result).unwrap();
    store.mark_read(&json!({"stock_code":"00700.HK"})).unwrap();
    item["article_summary"] =
        json!("腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。");
    item["article_summary_url"] = item["url"].clone();
    item["article_summary_status"] = json!("available");
    let updated = normalize_legacy_document(item, &source, &[]).unwrap();
    assert_eq!(updated["content"], old["content"]);
    store.ingest_documents(&[updated]).unwrap();
    let messages = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    assert_eq!(
        messages["items"][0]["summary"],
        "腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。"
    );
    assert_eq!(messages["items"][0]["unread"], false);
    assert_eq!(messages["items"][0]["document_id"], old["document_id"]);
    assert_eq!(store.index_status().unwrap()["document_count"], 1);
    assert_eq!(
        store.thread(thread_id).unwrap()["answers"][0]["citations"],
        result["citations"]
    );
    drop(store);
    let _ = remove_sqlite_files(&database);
}

#[test]
fn message_summary_read_recovers_legacy_cache_without_changing_history() {
    let path = temporary_database_path("legacy-message-summary");
    let store = ResearchStore::open(&path).unwrap();
    let headline = "腾讯健康与灵北中国达成战略合作";
    let long_headline = "腾讯披露人工智能业务、产品及研发投入的新进展，".repeat(20);
    let body = "公司披露季度收入增长 15%，研发投入增加。";
    let documents = [
        json!({"document_id":"headline-only-news","title":headline,
            "content":format!("{headline}\n{headline}"),"source_tier":"news","source_name":"新浪财经",
            "stock_codes":["00700.HK"],"url":"https://example.com/tencent/news","published_at":"2026-11-09"}),
        json!({"document_id":"long-headline-news","title":long_headline,
            "content":format!("{long_headline}\n{body}"),"source_tier":"news","source_name":"新浪财经",
            "stock_codes":["00700.HK"],"published_at":"2026-11-09"}),
    ];
    store.ingest_documents(&documents).unwrap();
    let response = store
        .query(&json!({"query":"腾讯","stock_code":"00700.HK"}))
        .unwrap();
    let thread = store
        .create_thread(&json!({"title":"历史新闻查询"}))
        .unwrap();
    let thread_id = thread["id"].as_str().unwrap();
    store.save_answer(thread_id, "腾讯新闻", &response).unwrap();
    store
        .mark_read(&json!({"message_ids":["message:headline-only-news:00700.HK"]}))
        .unwrap();
    // Simulate persisted summaries made by the old 180-character excerpt.
    let connection = store.connection().unwrap();
    connection
        .execute(
            "UPDATE research_messages SET summary=?2 WHERE document_id=?1",
            params!["headline-only-news", format!("{headline}\n{headline}")],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE research_messages SET summary=?2 WHERE document_id=?1",
            params![
                "long-headline-news",
                format!("{}…", long_headline.chars().take(180).collect::<String>())
            ],
        )
        .unwrap();
    drop(connection);
    let result = store.messages(&json!({"stock_code":"00700.HK"})).unwrap();
    let items = result["items"].as_array().unwrap();
    let only = items
        .iter()
        .find(|item| item["document_id"] == "headline-only-news")
        .unwrap();
    let detailed = items
        .iter()
        .find(|item| item["document_id"] == "long-headline-news")
        .unwrap();
    assert_eq!(only["summary"], "");
    assert_eq!(only["unread"], false);
    assert_eq!(only["url"], "https://example.com/tencent/news");
    assert_eq!(detailed["summary"], body);
    store.ingest_documents(&documents).unwrap();
    assert_eq!(store.index_status().unwrap()["document_count"], 2);
    assert_eq!(
        store.thread(thread_id).unwrap()["answers"][0]["citations"],
        response["citations"]
    );
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn message_summary_generation_keeps_only_additional_content() {
    let path = temporary_database_path("generated-message-summary");
    let store = ResearchStore::open(&path).unwrap();
    let headline = "腾讯发布业务进展";
    let body = "公司披露季度收入增长 15%。";
    let mut documents = [
        json!({"document_id":"only","title":headline,"content":format!("{headline}\n{headline}")}),
        json!({"document_id":"detailed","title":headline,"content":format!("{headline}\n{body}")}),
        json!({"document_id":"issuer","title":"腾讯","content":"腾讯控股披露季度收入增长 15%。"}),
    ];
    store.ingest_documents(&documents).unwrap();
    for (id, expected) in [
        ("only", ""),
        ("detailed", body),
        ("issuer", "腾讯控股披露季度收入增长 15%。"),
    ] {
        let summary: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT summary FROM research_messages WHERE document_id=?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(summary, expected);
    }
    store
        .mark_read(&json!({"message_ids":["message:detailed:all"]}))
        .unwrap();
    documents[1]["entities"] = json!(["腾讯"]);
    store.ingest_documents(&documents[1..2]).unwrap();
    let result = store.messages(&json!({})).unwrap();
    let detailed = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["document_id"] == "detailed")
        .unwrap();
    assert_eq!(detailed["summary"], body);
    assert_eq!(detailed["unread"], false);
    let summary: String = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT summary FROM research_messages WHERE document_id='detailed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(summary, body);
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn messages_include_document_source_name_and_policy_metadata() {
    let path = temporary_database_path("message-source-metadata");
    let store = ResearchStore::open(&path).unwrap();
    store
        .ingest_documents(&[json!({
            "document_id":"policy-doc",
            "title":"政策原文",
            "content":"区域政策正文",
            "source_tier":"policy_official",
            "source_name":"中国政府网",
            "stock_codes":["300750.SZ"],
            "scope_type":"region",
            "scope_tags":["宁德"],
            "mapped_stock_codes":["300750.SZ"]
        })])
        .unwrap();
    let item = store.messages(&json!({"stock_code":"300750.SZ"})).unwrap()["items"][0].clone();
    assert_eq!(item["source_name"], "中国政府网");
    assert_eq!(item["scope_type"], "region");
    assert_eq!(item["scope_tags"], json!(["宁德"]));
}

#[test]
fn cn_news_messages_preserve_publication_fetch_and_summary_provenance() {
    let database = temporary_database_path("cn-news-provenance");
    let source = database.with_extension("news-cache.json");
    let store = ResearchStore::open(&database).unwrap();
    let item = json!({"id":"cn-source","title":"华安证券发布业务进展","summary":"华安证券发布业务进展",
            "source":"新浪财经","source_id":"sina_finance","source_tier":"news","stock_codes":["600909.SH"],
            "url":"https://finance.sina.com.cn/stock/s/2026-11-12/doc-cn-fixture.shtml",
            "published_at":"2026-11-12 09:00:00","fetched_at_epoch_ms":1794448800000_i64,
            "article_summary":"证券经纪和资产管理业务持续发展。","article_summary_status":"available",
            "article_summary_fetched_at_epoch_ms":1794448920000_i64});
    let document = normalize_legacy_document(item, &source, &[]).unwrap();
    store.ingest_documents(&[document.clone()]).unwrap();
    store.mark_read(&json!({"stock_code":"600909.SH"})).unwrap();
    let projected = store.messages(&json!({"stock_code":"600909.SH"})).unwrap()["items"][0].clone();
    assert_eq!(projected["published_at"], "2026-11-12 09:00:00");
    assert_eq!(projected["source_name"], "新浪财经");
    assert_eq!(projected["fetched_at_epoch_ms"], 1794448800000_i64);
    assert_eq!(
        projected["article_summary_fetched_at_epoch_ms"],
        1794448920000_i64
    );
    assert_eq!(projected["summary_kind"], "article_summary");
    assert_eq!(projected["summary"], "证券经纪和资产管理业务持续发展。");
    assert_eq!(projected["document_id"], document["document_id"]);
    assert_eq!(projected["unread"], false);
    drop(store);
    remove_sqlite_files(&database).unwrap();
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
fn protected_documents_cannot_silently_exceed_chunk_limit() {
    let path = temporary_database_path("protected-overflow");
    let store = ResearchStore::open(&path).unwrap();
    store
        .ingest_documents(&[
            json!({
                "document_id": "protected-a",
                "title": "Protected A",
                "content": "First protected evidence.",
                "source_tier": "filing",
                "pinned": true
            }),
            json!({
                "document_id": "protected-b",
                "title": "Protected B",
                "content": "Second protected evidence.",
                "source_tier": "filing",
                "pinned": true
            }),
        ])
        .unwrap();

    let error = store
        .prune_retention_to_limit(1)
        .expect_err("protected overflow must be rejected");

    assert!(error.contains("chunk limit"));
    assert_eq!(store.index_status().unwrap()["chunk_count"], 2);
    drop(store);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn exclusive_database_switch_waits_for_shared_operations() {
    let (shared_started_tx, shared_started_rx) = std::sync::mpsc::channel();
    let (release_shared_tx, release_shared_rx) = std::sync::mpsc::channel();
    let shared = std::thread::spawn(move || {
        with_shared_app_database(|| {
            shared_started_tx.send(()).unwrap();
            release_shared_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
    });
    shared_started_rx.recv().unwrap();

    let lock = APP_RESEARCH_DATABASE_LOCK.get().unwrap();
    assert!(lock.try_write().is_err());

    release_shared_tx.send(()).unwrap();
    shared.join().unwrap();
    assert!(lock.try_write().is_ok());
}

#[test]
fn exclusive_database_switch_invalidates_in_flight_answers() {
    let generation = APP_RESEARCH_DATABASE_GENERATION.load(AtomicOrdering::Acquire);
    with_exclusive_app_database(|| Ok(())).unwrap();

    let error = ensure_app_database_generation(generation)
        .expect_err("an answer from the previous database generation must be rejected");

    assert!(error.contains("database changed"));
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
fn state_copy_replaces_stale_threads_and_keeps_latest_answers_and_unread() {
    let old_path = temporary_database_path("d3-stale");
    let current_path = temporary_database_path("d3-current");
    let old = ResearchStore::open(&old_path).unwrap();
    let current = ResearchStore::open(&current_path).unwrap();
    let doc = json!({"document_id":"d3-doc", "title":"Evidence", "content":"shared evidence", "source_tier":"news"});
    old.ingest_documents(&[doc.clone()]).unwrap();
    current.ingest_documents(&[doc]).unwrap();
    let deleted = old
        .create_thread(&json!({"title":"Deleted later"}))
        .unwrap();
    let fresh = current
        .create_thread(&json!({"title":"Created after import"}))
        .unwrap();
    let fresh_id = fresh["id"].as_str().unwrap();
    current
        .save_answer(
            fresh_id,
            "new question",
            &json!({"answer":"latest answer", "citations":[]}),
        )
        .unwrap();
    current
        .connection()
        .unwrap()
        .execute("UPDATE research_messages SET unread=0", [])
        .unwrap();
    preserve_local_research_state(&current_path, &old).unwrap();
    assert!(
        old.thread(deleted["id"].as_str().unwrap()).is_err(),
        "deleted threads must not return"
    );
    assert_eq!(
        old.thread(fresh_id).unwrap()["answers"][0]["answer"],
        "latest answer"
    );
    assert_eq!(old.overview(&json!({})).unwrap()["unread_count"], 0);
    remove_sqlite_files(&old_path).unwrap();
    remove_sqlite_files(&current_path).unwrap();
}

#[test]
fn state_copy_does_not_rebind_reused_chunk_ids() {
    let old_path = temporary_database_path("d3-citation-old");
    let new_path = temporary_database_path("d3-citation-new");
    let old = ResearchStore::open(&old_path).unwrap();
    let new = ResearchStore::open(&new_path).unwrap();
    old.ingest_documents(&[json!({"document_id":"same-id", "title":"Evidence", "content":"original evidence", "source_tier":"news"})]).unwrap();
    new.ingest_documents(&[json!({"document_id":"same-id", "title":"Evidence", "content":"different evidence", "source_tier":"news"})]).unwrap();
    let thread = old.create_thread(&json!({"title":"History"})).unwrap();
    let id = thread["id"].as_str().unwrap();
    let response = old.query(&json!({"query":"original evidence"})).unwrap();
    assert!(!response["citations"].as_array().unwrap().is_empty());
    old.save_answer(id, "question", &response).unwrap();
    preserve_local_research_state(&old_path, &new).unwrap();
    let history = new.thread(id).unwrap();
    assert_eq!(
        history["answers"][0]["citations"][0]["excerpt"],
        response["citations"][0]["excerpt"]
    );
    assert_eq!(history["answers"][0]["citations"][0]["unavailable"], true);
    remove_sqlite_files(&old_path).unwrap();
    remove_sqlite_files(&new_path).unwrap();
}

#[test]
fn state_copy_defaults_absent_historical_messages_to_unread() {
    let old_path = temporary_database_path("d3-stale-unread");
    let current_path = temporary_database_path("d3-empty-current");
    let old = ResearchStore::open(&old_path).unwrap();
    ResearchStore::open(&current_path).unwrap();
    old.ingest_documents(&[json!({"document_id":"old-only", "content":"old evidence"})])
        .unwrap();
    old.connection()
        .unwrap()
        .execute("UPDATE research_messages SET unread=0", [])
        .unwrap();
    preserve_local_research_state(&current_path, &old).unwrap();
    assert_eq!(old.overview(&json!({})).unwrap()["unread_count"], 1);
    remove_sqlite_files(&old_path).unwrap();
    remove_sqlite_files(&current_path).unwrap();
}

#[test]
fn unavailable_citation_cannot_be_saved_as_an_active_reference() {
    let path = temporary_database_path("d3-save-unavailable");
    let store = ResearchStore::open(&path).unwrap();
    store
        .ingest_documents(&[
            json!({"document_id":"evidence", "title":"Evidence", "content":"research evidence"}),
        ])
        .unwrap();
    let id = store.create_thread(&json!({"title":"History"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut response = store.query(&json!({"query":"research evidence"})).unwrap();
    assert!(!response["citations"].as_array().unwrap().is_empty());
    response["citations"][0]["unavailable"] = json!(true);
    assert!(store
        .save_answer(&id, "question", &response)
        .unwrap_err()
        .contains("unavailable"));
    assert!(store.thread(&id).unwrap()["answers"]
        .as_array()
        .unwrap()
        .is_empty());
    remove_sqlite_files(&path).unwrap();
}

#[test]
fn recovery_never_promotes_an_uncommitted_orphan_staging_database() {
    let root = temporary_database_path("d3-orphan");
    fs::create_dir_all(&root).unwrap();
    ResearchStore::open(root.join("research.sqlite.importing")).unwrap();
    assert!(recover_research_files(&root).is_err());
    assert!(!root.join("research.sqlite").exists());
    assert!(root.join("research.sqlite.importing").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_or_empty_existing_main_is_never_initialized() {
    for contents in [b"not sqlite".as_slice(), b"".as_slice()] {
        let root = temporary_database_path("d3-corrupt");
        fs::create_dir_all(&root).unwrap();
        let main = root.join("research.sqlite");
        fs::write(&main, contents).unwrap();
        assert!(recover_research_files(&root).is_err());
        assert!(ResearchStore::open(&main).is_err());
        assert_eq!(fs::read(&main).unwrap(), contents);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn rejects_unknown_or_missing_model_citations() {
    let citations = vec![
        json!({"citation_id": "C1", "source_tier": "filing"}),
        json!({"citation_id": "C2", "source_tier": "community"}),
    ];
    assert!(validate_model_answer("订单保持增长 [C1]。", &citations).is_ok());
    assert!(validate_model_answer("收入同比增长 3.5% [C1]。", &citations).is_ok());
    assert!(validate_model_answer("订单保持增长 [C9]。", &citations).is_err());
    assert!(validate_model_answer("订单保持增长。", &citations).is_err());
    assert!(validate_model_answer("订单保持增长 [C1]。利润同步改善。", &citations).is_err());
    assert!(validate_model_answer("公司已经获得大额订单 [C2]。", &citations).is_err());
    assert!(validate_model_answer(
        "社区传闻称可能获得订单 [C2]，该说法仍待核查 [C2]。",
        &citations
    )
    .is_ok());
    assert!(validate_model_answer(
        "公告显示收入增长 [C1]，社区称已经获得订单 [C2]。",
        &citations
    )
    .is_err());
    assert!(validate_model_answer("收入增长 [C1]，利润下降。", &citations).is_err());
    assert!(validate_model_answer(
        "社区传闻称可能获得订单 [C2]，公司已经签约 [C2]。",
        &citations
    )
    .is_err());
}

#[test]
fn rejects_documents_that_cannot_fit_in_a_portable_pack() {
    let oversized = "证".repeat(MAX_PACK_DOCUMENT_CHARS + 1);
    let error = validate_portable_documents(&[json!({
        "document_id": "oversized-pack-doc",
        "content": oversized
    })])
    .expect_err("oversized portable documents must be rejected before export");

    assert!(error.contains("oversized document"));
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
#[test]
fn schema_v2_migration_keeps_user_imports_private_and_public_docs_exportable() {
    let path = temporary_database_path("schema-v2-privacy-migration");
    let connection = Connection::open(&path).expect("legacy database should open");
    connection.execute_batch(
            "CREATE TABLE documents (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, content TEXT NOT NULL,
                source_tier TEXT NOT NULL, source_name TEXT NOT NULL, url TEXT,
                published_at TEXT, imported_at_epoch_ms INTEGER NOT NULL,
                updated_at_epoch_ms INTEGER NOT NULL, content_hash TEXT NOT NULL,
                user_imported INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0,
                cited_count INTEGER NOT NULL DEFAULT 0, metadata_json TEXT NOT NULL DEFAULT '{}'
             );
             INSERT INTO documents VALUES
                ('legacy-public','Public','public','filing','source',NULL,NULL,1,1,'a',0,0,0,'{}'),
                ('legacy-private','Private','private','research_report','user',NULL,NULL,1,1,'b',1,1,0,'{}');
             PRAGMA user_version=2;"
        ).unwrap();
    drop(connection);

    let _store = ResearchStore::open(&path).expect("legacy schema should migrate");
    let connection = Connection::open(&path).unwrap();
    let public_permission: i64 = connection
        .query_row(
            "SELECT remote_export_allowed FROM documents WHERE id='legacy-public'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let private_permission: i64 = connection
        .query_row(
            "SELECT remote_export_allowed FROM documents WHERE id='legacy-private'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(public_permission, 1);
    assert_eq!(private_permission, 0);
    assert_eq!(version, 3);
    drop(connection);
    let _ = remove_sqlite_files(&path);
}

#[test]
fn remote_safe_queries_exclude_local_imports_by_default() {
    let path = temporary_database_path("remote-safe-policy");
    let store = ResearchStore::open(&path).expect("research store should open");
    store
        .ingest_documents(&[
            json!({
                "document_id": "public-policy",
                "title": "Public policy",
                "content": "remote policy signal",
                "source_tier": "policy_official",
                "source_name": "Public source",
                "stock_codes": ["000001.SZ"]
            }),
            json!({
                "document_id": "local-research",
                "title": "Private research",
                "content": "remote policy signal",
                "source_tier": "research_report",
                "source_name": "User import",
                "user_imported": true,
                "stock_codes": ["000001.SZ"]
            }),
        ])
        .expect("documents should ingest");

    let local = store
        .query(&json!({
            "query": "remote policy signal",
            "stock_code": "000001.SZ",
            "top_k": 8
        }))
        .expect("local query should succeed");
    assert_eq!(local["citations"].as_array().unwrap().len(), 2);

    let remote_safe = store
        .query(&json!({
            "query": "remote policy signal",
            "stock_code": "000001.SZ",
            "remote_safe_only": true,
            "top_k": 8
        }))
        .expect("remote-safe query should succeed");
    let citations = remote_safe["citations"].as_array().unwrap();
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0]["document_id"], "public-policy");
    assert_eq!(citations[0]["remote_export_allowed"], true);

    store
        .ingest_documents(&[json!({
            "document_id": "local-research",
            "title": "Private research",
            "content": "remote policy signal",
            "source_tier": "research_report",
            "source_name": "User import",
            "user_imported": true,
            "remote_export_allowed": true,
            "stock_codes": ["000001.SZ"]
        })])
        .expect("explicit per-document consent should update the imported record");
    let consented = store
        .query(&json!({
            "query": "remote policy signal",
            "stock_code": "000001.SZ",
            "remote_safe_only": true,
            "top_k": 8
        }))
        .expect("consented query should succeed");
    assert_eq!(consented["citations"].as_array().unwrap().len(), 2);
    assert!(consented["citations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["document_id"] == "local-research"));

    let _ = remove_sqlite_files(&path);
}
