use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

const RAG_PACK_FILE: &str = "rag-pack.json";
const UPSTREAM_PACK_FILE: &str = "rag_pack.sqlite";

static RAG_PACK_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedRagPack>>> = OnceLock::new();

#[derive(Clone)]
struct CachedRagPack {
    bytes: u64,
    modified_at_epoch_ms: Option<u128>,
    manifest: Value,
    chunks: Arc<Vec<IndexedRagChunk>>,
}

#[derive(Clone)]
struct IndexedRagChunk {
    value: Value,
    lower: String,
    terms: HashSet<String>,
}

pub(crate) fn rag_pack_status(app: tauri::AppHandle) -> Result<Value, String> {
    let path = rag_pack_path(&app)?;
    if !path.exists() {
        return Ok(
            json!({"exists": false, "path": path.display().to_string(), "valid": false, "manifest": {}, "notes": ["Tauri/Rust RAG pack has not been built yet."]}),
        );
    }
    let pack = read_json_file(&path)?;
    let manifest = pack.get("manifest").cloned().unwrap_or_else(|| json!({}));
    Ok(json!({
        "exists": true,
        "path": path.display().to_string(),
        "valid": true,
        "manifest": manifest,
        "notes": ["Tauri/Rust lightweight RAG pack is available."]
    }))
}

pub(crate) fn rag_pack_build(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let documents = payload
        .get("documents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if documents.is_empty() {
        return Err("RAG pack build requires at least one document.".to_string());
    }
    let pack_version =
        string_field(&payload, "pack_version").unwrap_or_else(|| "tauri-local".to_string());
    let target_chars = int_field(&payload, "target_chars", 500, 120, 1200) as usize;
    let overlap_chars = int_field(&payload, "overlap_chars", 80, 0, 300) as usize;
    let chunks = build_chunks(&documents, target_chars, overlap_chars);
    write_rag_pack(&app, &pack_version, documents, chunks)
}

pub(crate) fn rag_pack_build_from_news_cache(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let news_path = news_cache_path(&app)?;
    let news_cache = if news_path.exists() {
        read_json_file(&news_path)?
    } else {
        json!({"items": []})
    };
    let mut items = news_cache
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let stock_codes = normalized_codes(payload.get("stock_codes"));
    let source_tiers = string_set(payload.get("source_tiers"));
    let relation_types = string_set(payload.get("relation_types"));
    let limit = int_field(&payload, "limit", 1000, 1, 5000) as usize;
    if !stock_codes.is_empty() {
        items.retain(|item| {
            value_string_array(item.get("stock_codes"))
                .iter()
                .filter_map(|code| normalize_stock_code(code))
                .any(|code| stock_codes.contains(&code))
        });
    }
    if !source_tiers.is_empty() {
        items.retain(|item| {
            item.get("source_tier")
                .and_then(Value::as_str)
                .map(|tier| source_tiers.contains(tier))
                .unwrap_or(false)
        });
    }
    if !relation_types.is_empty() {
        items.retain(|item| {
            value_string_array(item.get("relation_types"))
                .iter()
                .any(|kind| relation_types.contains(kind))
        });
    }
    items.truncate(limit);
    let documents = items.into_iter().enumerate().map(|(index, item)| {
        let title = item.get("title").and_then(Value::as_str).unwrap_or("");
        let summary = item.get("summary").and_then(Value::as_str).unwrap_or("");
        let text = format!("{title}\n{summary}");
        json!({
            "source": item.get("source").and_then(Value::as_str).unwrap_or("local news cache"),
            "source_tier": item.get("source_tier").and_then(Value::as_str).unwrap_or("news"),
            "title": title,
            "text": text,
            "url": item.get("url").and_then(Value::as_str).unwrap_or(""),
            "published_at": item.get("published_at").and_then(Value::as_str).unwrap_or(""),
            "stock_codes": item.get("stock_codes").cloned().unwrap_or_else(|| json!([])),
            "relation_types": item.get("relation_types").cloned().unwrap_or_else(|| json!([])),
            "sentiment": item.get("sentiment").and_then(Value::as_str).unwrap_or("uncertain"),
            "document_id": item.get("id").and_then(Value::as_str).map(ToOwned::to_owned).unwrap_or_else(|| format!("news_doc_{index}"))
        })
    }).collect::<Vec<_>>();
    if documents.is_empty() {
        return Err("No news-cache items matched the RAG pack build filters.".to_string());
    }
    let pack_version =
        string_field(&payload, "pack_version").unwrap_or_else(|| "tauri-news-cache".to_string());
    let target_chars = int_field(&payload, "target_chars", 500, 120, 1200) as usize;
    let overlap_chars = int_field(&payload, "overlap_chars", 80, 0, 300) as usize;
    let chunks = build_chunks(&documents, target_chars, overlap_chars);
    write_rag_pack(&app, &pack_version, documents, chunks)
}

pub(crate) fn rag_pack_query(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let path = rag_pack_path(&app)?;
    if !path.exists() {
        return Err("Tauri/Rust RAG pack has not been built yet.".to_string());
    }
    let pack = load_rag_pack_cache(&path)?;
    let query = string_field(&payload, "query").unwrap_or_default();
    if query.trim().is_empty() {
        return Err("query is required.".to_string());
    }
    let query_terms = tokenize(&query);
    let stock_codes = normalized_codes(payload.get("stock_codes"));
    let source_tiers = string_set(payload.get("source_tiers"));
    let relation_types = string_set(payload.get("relation_types"));
    let published_after = string_field(&payload, "published_after");
    let top_k = int_field(&payload, "top_k", 8, 1, 50) as usize;
    let mut scored = pack
        .chunks
        .iter()
        .enumerate()
        .filter(|(_, indexed)| {
            let chunk = &indexed.value;
            if !stock_codes.is_empty()
                && !value_string_array(chunk.get("stock_codes"))
                    .iter()
                    .filter_map(|code| normalize_stock_code(code))
                    .any(|code| stock_codes.contains(&code))
            {
                return false;
            }
            if !source_tiers.is_empty()
                && !chunk
                    .get("source_tier")
                    .and_then(Value::as_str)
                    .map(|tier| source_tiers.contains(tier))
                    .unwrap_or(false)
            {
                return false;
            }
            if !relation_types.is_empty()
                && !value_string_array(chunk.get("relation_types"))
                    .iter()
                    .any(|kind| relation_types.contains(kind))
            {
                return false;
            }
            if let Some(after) = published_after.as_deref() {
                if chunk
                    .get("published_at")
                    .and_then(Value::as_str)
                    .map(|date| date < after)
                    .unwrap_or(false)
                {
                    return false;
                }
            }
            true
        })
        .filter_map(|(index, indexed)| {
            let score = lexical_score_prepared(&query_terms, &indexed.lower, &indexed.terms);
            (score > 0.0).then_some((score, index))
        })
        .collect::<Vec<_>>();
    retain_top_scored(&mut scored, top_k);
    let hits = scored
        .into_iter()
        .map(|(score, index)| {
            let mut chunk = pack.chunks[index].value.clone();
            if let Value::Object(ref mut map) = chunk {
                map.insert("score".to_string(), json!(score));
            }
            chunk
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "hits": hits,
        "manifest": pack.manifest.clone(),
        "notes": ["Queried cached Tauri/Rust local RAG index with lexical scoring."]
    }))
}

pub(crate) fn upstream_rag_status(app: tauri::AppHandle) -> Result<Value, String> {
    let root = upstream_desktop_root(&app)?;
    let manifest_path = root.join("latest_manifest.json");
    if !manifest_path.exists() {
        return Ok(
            json!({"exists": false, "manifest": {}, "transfer": active_transfer(&app).unwrap_or_else(|| json!({"active": false})), "notes": ["Tauri/Rust upstream RAG pack has not been built yet."]}),
        );
    }
    let manifest = read_json_file(&manifest_path)?;
    Ok(
        json!({"exists": true, "manifest": manifest, "transfer": active_transfer(&app).unwrap_or_else(|| json!({"active": false})), "notes": ["Tauri/Rust upstream RAG pack is available."]}),
    )
}

pub(crate) fn upstream_rag_build(
    app: tauri::AppHandle,
    payload: Value,
    market_data: Value,
) -> Result<Value, String> {
    let code = string_field(&payload, "code")
        .and_then(|value| normalize_stock_code(&value))
        .ok_or_else(|| "code is required.".to_string())?;
    let data_until = string_field(&payload, "data_until").unwrap_or_else(|| current_stamp());
    let stock = find_stock(&market_data, &code)
        .unwrap_or_else(|| json!({"code": code, "name": code, "industry": ""}));
    let stock_name = stock
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&code)
        .to_string();
    let mut docs = upstream_documents_from_market_data(&market_data, &code, &stock_name);
    for (index, url) in value_string_array(payload.get("manual_urls"))
        .into_iter()
        .take(12)
        .enumerate()
    {
        docs.push(json!({"document_id": format!("manual_{index}"), "source_tier": "news", "source_name": "manual_url", "title": url, "text": url, "source_url": url, "published_at": data_until}));
    }
    if docs.is_empty() {
        docs.push(json!({"document_id": "snapshot", "source_tier": "financial_snapshot", "source_name": "Tauri/Rust market cache", "title": format!("{stock_name} local market snapshot"), "text": format!("{stock_name} {code} local market snapshot and financial cache."), "source_url": "local://market-cache", "published_at": data_until}));
    }
    let evidence_chunks = build_upstream_chunks(&docs);
    let relation_edges =
        upstream_edges_from_market_data(&market_data, &code, &stock_name, &evidence_chunks);
    let valid = !evidence_chunks.is_empty();
    let quality = json!({
        "valid": valid,
        "errors": if valid { json!([]) } else { json!(["No evidence chunks generated."]) },
        "warnings": if relation_edges.is_empty() { json!(["No explicit relation edges found in local market cache; generated evidence-only pack."]) } else { json!([]) },
        "source_tier_counts": source_tier_counts(&docs),
        "fact_evidence_count": evidence_chunks.iter().filter(|item| item.get("source_tier").and_then(Value::as_str) != Some("community")).count(),
    });
    let root = upstream_desktop_root(&app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("create upstream RAG dir failed: {error}"))?;
    let pack_payload = json!({"schema_version": "upstream-rag-pack-tauri-v1", "source_documents": docs, "evidence_chunks": evidence_chunks, "relation_edges": relation_edges});
    let pack_bytes = serde_json::to_vec_pretty(&pack_payload)
        .map_err(|error| format!("serialize upstream pack failed: {error}"))?;
    let sha256 = sha256_hex(&pack_bytes);
    let version = format!("{}_{}_{}", code, data_until.replace('-', ""), &sha256[..8]);
    let version_dir = root.join(sanitize_path_part(&version));
    fs::create_dir_all(&version_dir)
        .map_err(|error| format!("create upstream version dir failed: {error}"))?;
    let pack_path = version_dir.join(UPSTREAM_PACK_FILE);
    fs::write(&pack_path, &pack_bytes)
        .map_err(|error| format!("write upstream pack failed: {error}"))?;
    let manifest = json!({
        "schema_version": "upstream-rag-pack-v1",
        "pack_version": version,
        "target_stock_code": code,
        "target_stock_name": stock_name,
        "target_stock_industry": stock.get("industry").and_then(Value::as_str).unwrap_or(""),
        "data_until": data_until,
        "built_at": current_stamp(),
        "document_count": pack_payload.get("source_documents").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0),
        "evidence_count": pack_payload.get("evidence_chunks").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0),
        "relation_edge_count": pack_payload.get("relation_edges").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0),
        "source_documents": pack_payload.get("source_documents").cloned().unwrap_or_else(|| json!([])),
        "evidence_chunks": pack_payload.get("evidence_chunks").cloned().unwrap_or_else(|| json!([])),
        "relation_edges": pack_payload.get("relation_edges").cloned().unwrap_or_else(|| json!([])),
        "quality": quality,
        "valid": valid,
        "sha256": sha256,
        "file_size": pack_bytes.len(),
        "files": {"pack": UPSTREAM_PACK_FILE, "manifest": "manifest.json"}
    });
    let manifest_path = version_dir.join("manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("serialize manifest failed: {error}"))?,
    )
    .map_err(|error| format!("write manifest failed: {error}"))?;
    fs::write(
        root.join("latest_manifest.json"),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("serialize latest manifest failed: {error}"))?,
    )
    .map_err(|error| format!("write latest manifest failed: {error}"))?;
    fs::write(
        root.join("latest_pack_path.txt"),
        pack_path.display().to_string(),
    )
    .map_err(|error| format!("write latest pack pointer failed: {error}"))?;
    Ok(
        json!({"pack_path": pack_path.display().to_string(), "manifest_path": manifest_path.display().to_string(), "manifest": manifest, "quality": quality, "notes": ["Built Tauri/Rust upstream RAG pack without Python."]}),
    )
}

pub(crate) fn upstream_rag_transfer_start(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let root = upstream_desktop_root(&app)?;
    let manifest_path = root.join("latest_manifest.json");
    let pack_pointer = root.join("latest_pack_path.txt");
    if !manifest_path.exists() || !pack_pointer.exists() {
        return Err("Build an upstream RAG pack before starting transfer.".to_string());
    }
    let manifest = read_json_file(&manifest_path)?;
    let pack_path = PathBuf::from(
        fs::read_to_string(&pack_pointer)
            .map_err(|error| format!("read latest pack pointer failed: {error}"))?
            .trim(),
    );
    let pack_bytes = fs::read(&pack_path)
        .map_err(|error| format!("read latest upstream pack failed: {error}"))?;
    let ttl = int_field(&payload, "ttl_minutes", 15, 1, 60);
    let descriptor =
        json!({"manifest": manifest, "pack_base64": general_purpose::STANDARD.encode(&pack_bytes)});
    let descriptor_text = descriptor.to_string();
    let transfer = json!({
        "active": true,
        "manifest_url": "tauri://inline-upstream-rag-manifest",
        "pack_url": "tauri://inline-upstream-rag-pack",
        "token": format!("tauri-inline-{}", epoch_millis()),
        "expires_at": format!("{}+{}m", current_stamp(), ttl),
        "descriptor_json": descriptor_text,
        "notes": ["Tauri/Rust replacement uses inline descriptor JSON instead of a Python LAN HTTP server. Paste descriptor_json into the mobile import field if QR transfer is unavailable."]
    });
    fs::write(
        root.join("latest_transfer.json"),
        serde_json::to_vec_pretty(&transfer)
            .map_err(|error| format!("serialize transfer failed: {error}"))?,
    )
    .map_err(|error| format!("write transfer state failed: {error}"))?;
    Ok(transfer)
}

fn write_rag_pack(
    app: &tauri::AppHandle,
    pack_version: &str,
    documents: Vec<Value>,
    chunks: Vec<Value>,
) -> Result<Value, String> {
    let path = rag_pack_path(app)?;
    let root = path
        .parent()
        .ok_or_else(|| "RAG pack path has no parent".to_string())?;
    fs::create_dir_all(root).map_err(|error| format!("create RAG pack dir failed: {error}"))?;
    let content_hash = sha256_hex(
        &serde_json::to_vec(&chunks)
            .map_err(|error| format!("serialize chunks failed: {error}"))?,
    );
    let manifest = json!({"schema_version": "rag-pack-tauri-v1", "pack_version": pack_version, "document_count": documents.len(), "chunk_count": chunks.len(), "content_hash": content_hash, "embedding_model": "tauri-lexical", "embedding_backend": "tauri-rust", "embedding_quantization": "none", "embedding_dim": 0, "built_at": current_stamp()});
    let pack = json!({"manifest": manifest, "documents": documents, "chunks": chunks});
    fs::write(
        &path,
        serde_json::to_vec_pretty(&pack)
            .map_err(|error| format!("serialize RAG pack failed: {error}"))?,
    )
    .map_err(|error| format!("write RAG pack failed: {error}"))?;
    forget_rag_pack_cache(&path);
    Ok(
        json!({"path": path.display().to_string(), "document_count": manifest["document_count"], "chunk_count": manifest["chunk_count"], "content_hash": content_hash, "embedding_model": "tauri-lexical", "embedding_backend": "tauri-rust", "embedding_quantization": "none", "embedding_dim": 0, "notes": ["Built Tauri/Rust lightweight RAG pack without Python/ONNX."]}),
    )
}

fn build_chunks(documents: &[Value], target_chars: usize, overlap_chars: usize) -> Vec<Value> {
    let mut chunks = Vec::new();
    for (doc_index, doc) in documents.iter().enumerate() {
        let document_id =
            string_field(doc, "document_id").unwrap_or_else(|| format!("doc_{doc_index}"));
        let text = string_field(doc, "text")
            .unwrap_or_else(|| string_field(doc, "summary").unwrap_or_default());
        let title = string_field(doc, "title").unwrap_or_default();
        let parts = chunk_text(&text, target_chars, overlap_chars);
        for (chunk_index, part) in parts.into_iter().enumerate() {
            chunks.push(json!({
                "chunk_id": format!("{document_id}_chunk_{chunk_index}"),
                "document_id": document_id,
                "score": 0.0,
                "title": title,
                "text": part,
                "source": doc.get("source").or_else(|| doc.get("source_name")).and_then(Value::as_str).unwrap_or(""),
                "source_tier": doc.get("source_tier").and_then(Value::as_str).unwrap_or("news"),
                "published_at": doc.get("published_at").and_then(Value::as_str).unwrap_or(""),
                "url": doc.get("url").or_else(|| doc.get("source_url")).and_then(Value::as_str).unwrap_or(""),
                "stock_codes": doc.get("stock_codes").cloned().unwrap_or_else(|| json!([])),
                "relation_types": doc.get("relation_types").cloned().unwrap_or_else(|| json!([])),
                "sentiment": doc.get("sentiment").and_then(Value::as_str).unwrap_or("uncertain")
            }));
        }
    }
    chunks
}

fn chunk_text(text: &str, target_chars: usize, overlap_chars: usize) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![String::new()];
    }
    let mut result = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + target_chars).min(chars.len());
        result.push(chars[start..end].iter().collect());
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(overlap_chars.min(target_chars.saturating_sub(1)));
    }
    result
}

fn build_upstream_chunks(docs: &[Value]) -> Vec<Value> {
    docs.iter().enumerate().map(|(index, doc)| json!({
        "chunk_id": format!("chunk_{index}"),
        "document_id": doc.get("document_id").and_then(Value::as_str).unwrap_or("doc"),
        "title": doc.get("title").and_then(Value::as_str).unwrap_or(""),
        "evidence_text": doc.get("text").and_then(Value::as_str).unwrap_or_else(|| doc.get("title").and_then(Value::as_str).unwrap_or("")),
        "source_tier": doc.get("source_tier").and_then(Value::as_str).unwrap_or("news"),
        "source_name": doc.get("source_name").and_then(Value::as_str).unwrap_or(""),
        "source_url": doc.get("source_url").and_then(Value::as_str).unwrap_or(""),
        "published_at": doc.get("published_at").and_then(Value::as_str).unwrap_or("")
    })).collect()
}

fn upstream_documents_from_market_data(data: &Value, code: &str, stock_name: &str) -> Vec<Value> {
    let mut docs = Vec::new();
    if let Some(stock) = find_stock(data, code) {
        docs.push(json!({"document_id": "stock_snapshot", "source_tier": "financial_snapshot", "source_name": "Tauri/Rust market cache", "title": format!("{stock_name} local stock snapshot"), "text": format!("{} {} industry {} price {:?} PE {:?} PB {:?} ROE {:?}", stock_name, code, stock.get("industry").and_then(Value::as_str).unwrap_or(""), stock.get("price"), stock.get("pe"), stock.get("pb"), stock.get("roe")), "source_url": "local://market-cache", "published_at": current_stamp()}));
    }
    if let Some(financial) = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|items| items.get(code))
    {
        docs.push(json!({"document_id": "financial_snapshot", "source_tier": "financial_snapshot", "source_name": "Tauri/Rust financial cache", "title": format!("{stock_name} financial snapshot"), "text": financial.to_string(), "source_url": "local://financial-cache", "published_at": current_stamp()}));
    }
    docs
}

fn upstream_edges_from_market_data(
    data: &Value,
    code: &str,
    stock_name: &str,
    chunks: &[Value],
) -> Vec<Value> {
    let mut edges = Vec::new();
    if let Some(relations) = data.get("relations").and_then(Value::as_array) {
        for (index, relation) in relations.iter().enumerate() {
            let source = relation
                .get("source_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code);
            let target = relation
                .get("target_code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code);
            if source.as_deref() != Some(code) && target.as_deref() != Some(code) {
                continue;
            }
            let other = if source.as_deref() == Some(code) {
                target.unwrap_or_else(|| code.to_string())
            } else {
                source.unwrap_or_else(|| code.to_string())
            };
            edges.push(json!({
                "edge_id": format!("edge_{index}"),
                "source_entity": {"entity_name": stock_name, "stock_code": code},
                "target_entity": {"entity_name": other, "stock_code": other},
                "relation_type": relation.get("relation_type").and_then(Value::as_str).unwrap_or("related"),
                "status": "supported",
                "confidence": relation.get("weight").and_then(Value::as_f64).unwrap_or(0.5),
                "evidence_text": relation.get("description").and_then(Value::as_str).unwrap_or("Local relation from market cache."),
                "source_ref": chunks.first().and_then(|item| item.get("chunk_id")).and_then(Value::as_str).unwrap_or("chunk_0")
            }));
        }
    }
    edges
}

fn find_stock(data: &Value, code: &str) -> Option<Value> {
    data.get("stocks")?
        .as_array()?
        .iter()
        .find(|stock| {
            stock
                .get("code")
                .and_then(Value::as_str)
                .and_then(normalize_stock_code)
                .as_deref()
                == Some(code)
        })
        .cloned()
}

fn active_transfer(app: &tauri::AppHandle) -> Option<Value> {
    let path = upstream_desktop_root(app)
        .ok()?
        .join("latest_transfer.json");
    read_json_file(&path).ok()
}

fn source_tier_counts(docs: &[Value]) -> Value {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for doc in docs {
        *counts
            .entry(
                doc.get("source_tier")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
            )
            .or_default() += 1;
    }
    json!(counts)
}

fn rag_pack_cache() -> &'static Mutex<HashMap<PathBuf, CachedRagPack>> {
    RAG_PACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rag_file_signature(path: &Path) -> (u64, Option<u128>) {
    let metadata = fs::metadata(path).ok();
    let bytes = metadata.as_ref().map(|value| value.len()).unwrap_or(0);
    let modified_at_epoch_ms = metadata
        .and_then(|value| value.modified().ok())
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis());
    (bytes, modified_at_epoch_ms)
}

fn load_rag_pack_cache(path: &Path) -> Result<CachedRagPack, String> {
    let (bytes, modified_at_epoch_ms) = rag_file_signature(path);
    if let Ok(slot) = rag_pack_cache().lock() {
        if let Some(entry) = slot.get(path) {
            if entry.bytes == bytes && entry.modified_at_epoch_ms == modified_at_epoch_ms {
                return Ok(entry.clone());
            }
        }
    }
    let pack = read_json_file(path)?;
    let chunks = pack
        .get("chunks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|chunk| {
            let text = format!(
                "{} {}",
                chunk.get("title").and_then(Value::as_str).unwrap_or(""),
                chunk.get("text").and_then(Value::as_str).unwrap_or("")
            );
            IndexedRagChunk {
                value: chunk.clone(),
                lower: text.to_lowercase(),
                terms: tokenize(&text).into_iter().collect::<HashSet<_>>(),
            }
        })
        .collect::<Vec<_>>();
    let entry = CachedRagPack {
        bytes,
        modified_at_epoch_ms,
        manifest: pack.get("manifest").cloned().unwrap_or_else(|| json!({})),
        chunks: Arc::new(chunks),
    };
    if let Ok(mut slot) = rag_pack_cache().lock() {
        slot.insert(path.to_path_buf(), entry.clone());
    }
    Ok(entry)
}

fn forget_rag_pack_cache(path: &Path) {
    if let Ok(mut slot) = rag_pack_cache().lock() {
        slot.remove(path);
    }
}

fn retain_top_scored(scored: &mut Vec<(f64, usize)>, top_k: usize) {
    let compare = |left: &(f64, usize), right: &(f64, usize)| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    };
    if scored.len() > top_k {
        scored.select_nth_unstable_by(top_k, compare);
        scored.truncate(top_k);
    }
    scored.sort_by(compare);
}

fn lexical_score_prepared(
    query_terms: &[String],
    lower: &str,
    text_terms: &HashSet<String>,
) -> f64 {
    let mut score = 0.0;
    for term in query_terms {
        if term.len() >= 2 && lower.contains(term) {
            score += 2.0;
        }
        if text_terms.contains(term) {
            score += 1.0;
        }
    }
    score
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '.'))
        .filter(|part| part.chars().count() >= 2)
        .map(ToOwned::to_owned)
        .collect()
}

fn rag_pack_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("rag");
    root.push(RAG_PACK_FILE);
    Ok(root)
}

fn upstream_desktop_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("upstream_rag_desktop");
    Ok(root)
}

fn news_cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("get app data dir failed: {error}"))?;
    root.push("news");
    root.push("news-cache.json");
    Ok(root)
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("read JSON failed: {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse JSON failed: {}: {error}", path.display()))
}

fn normalized_codes(value: Option<&Value>) -> HashSet<String> {
    value_string_array(value)
        .into_iter()
        .filter_map(|code| normalize_stock_code(&code))
        .collect()
}
fn string_set(value: Option<&Value>) -> HashSet<String> {
    value_string_array(value).into_iter().collect()
}
fn value_string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}
fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}
fn int_field(value: &Value, key: &str, default: i64, min: i64, max: i64) -> i64 {
    value
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(default)
        .clamp(min, max)
}
fn normalize_stock_code(value: &str) -> Option<String> {
    let raw = value.trim().to_ascii_uppercase();
    if raw.is_empty() {
        return None;
    }
    if let Some((digits, market)) = raw.split_once('.') {
        if valid_digits(digits) && matches!(market, "SH" | "SZ" | "BJ") {
            return Some(format!("{digits}.{market}"));
        }
    }
    let digits: String = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(6)
        .collect();
    if !valid_digits(&digits) {
        return None;
    }
    let market = if digits.starts_with('6') || digits.starts_with('9') || digits.starts_with('5') {
        "SH"
    } else if digits.starts_with('4') || digits.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    };
    Some(format!("{digits}.{market}"))
}
fn valid_digits(value: &str) -> bool {
    value.len() == 6 && value.chars().all(|ch| ch.is_ascii_digit())
}
fn sanitize_path_part(value: &str) -> String {
    let mut part: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(120)
        .collect();
    if part.is_empty() {
        part.push_str("unknown");
    }
    part
}
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
fn current_stamp() -> String {
    epoch_millis().to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_lexical_score_preserves_scoring_rules() {
        let text = "Alpha beta alpha";
        let query_terms = tokenize("alpha beta gamma");
        let lower = text.to_lowercase();
        let text_terms = tokenize(text).into_iter().collect::<HashSet<_>>();

        assert_eq!(
            lexical_score_prepared(&query_terms, &lower, &text_terms),
            6.0
        );
    }

    #[test]
    fn top_k_ranking_is_stable_for_equal_scores() {
        let mut scored = vec![(3.0, 2), (2.0, 3), (3.0, 0), (3.0, 1)];
        retain_top_scored(&mut scored, 2);
        assert_eq!(scored, vec![(3.0, 0), (3.0, 1)]);
    }

    #[test]
    fn rag_pack_cache_reuses_unchanged_index() {
        let path = std::env::temp_dir().join(format!(
            "gp-rag-pack-cache-{}-{}.json",
            std::process::id(),
            epoch_millis()
        ));
        let pack = json!({
            "manifest": {"version": "test"},
            "chunks": [{"title": "Alpha", "text": "beta gamma"}]
        });
        fs::write(
            &path,
            serde_json::to_vec(&pack).expect("serialize test pack"),
        )
        .expect("write test pack");
        forget_rag_pack_cache(&path);

        let first = load_rag_pack_cache(&path).expect("load first index");
        let second = load_rag_pack_cache(&path).expect("load cached index");
        assert!(Arc::ptr_eq(&first.chunks, &second.chunks));

        forget_rag_pack_cache(&path);
        let _ = fs::remove_file(path);
    }
}
