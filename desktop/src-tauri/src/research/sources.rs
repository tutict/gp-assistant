use crate::research::app_store::with_app_store_for_market;
use crate::research::storage::documents::normalized_string_array;
use crate::research::storage::documents::sha256_hex;
use crate::research::storage::packs::validate_legacy_candidate_size;
use crate::research::storage::packs::validate_legacy_pack_version;
use crate::research::storage::packs::validate_portable_documents;
use crate::research::storage::packs::MAX_PACK_BYTES;
use crate::research::storage::packs::MAX_PACK_DOCUMENTS;
use crate::research::storage::vectors::invalidate_vector_cache;
use crate::research::storage::ResearchStore;
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tauri::Manager;

pub(crate) fn ingest_news_cache(app: &tauri::AppHandle) -> Result<Value, String> {
    ingest_news_cache_for_market(app, "cn")
}

pub(crate) fn ingest_news_cache_for_market(
    app: &tauri::AppHandle,
    market: &str,
) -> Result<Value, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let path = app_data.join("news").join(if market == "hk" {
        "news-cache-hk.json"
    } else {
        "news-cache.json"
    });
    let news_market = if market == "hk" {
        crate::external_data::Market::HongKong
    } else {
        crate::external_data::Market::Global
    };
    with_app_store_for_market(app, market, |store| {
        ingest_news_cache_file_for_market(store, &path, news_market)
    })
}

#[cfg(test)]
pub(in crate::research) fn ingest_news_cache_file(
    store: &ResearchStore,
    path: &Path,
) -> Result<Value, String> {
    ingest_news_cache_file_for_market(store, path, crate::external_data::Market::Global)
}

pub(in crate::research) fn ingest_news_cache_file_for_market(
    store: &ResearchStore,
    path: &Path,
    market: crate::external_data::Market,
) -> Result<Value, String> {
    // Follow the news cache's authoritative storage, including its SQLite
    // migration, instead of rereading the obsolete JSON recovery copy.
    // Components normalize Windows slash spellings even when the JSON recovery
    // file no longer exists, so filesystem canonicalization cannot be used.
    let path = path.components().collect::<PathBuf>();
    let items = crate::news_rag::read_news_cache_items_for_market(&path, market)?;
    let fetched_at = items
        .iter()
        .filter_map(|item| item.get("fetched_at_epoch_ms").and_then(Value::as_i64))
        .max();
    let mut documents = Vec::new();
    let mut duplicate_groups = Vec::new();
    let connection = store.connection()?;
    let mut existing = HashMap::new();
    // Keep the raw spellings: PathBuf equality already considers the two slash
    // forms equal, but the previous importer hashed their different strings.
    let mut paths = HashSet::from([path.display().to_string()]);
    {
        let mut statement = connection
            .prepare(
                "SELECT id, imported_at_epoch_ms, json_extract(metadata_json, '$.legacy_path')
             FROM documents WHERE json_type(metadata_json, '$.legacy_path') = 'text'",
            )
            .map_err(|error| format!("failed to inspect news cache identities: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| format!("failed to query news cache identities: {error}"))?;
        for row in rows {
            let (id, imported_at, old_path) =
                row.map_err(|error| format!("failed to read news cache identity: {error}"))?;
            if Path::new(&old_path).components().collect::<PathBuf>() == path {
                existing.insert(id, imported_at);
                paths.insert(old_path);
            }
        }
    }
    for item in items {
        validate_legacy_candidate_size(&item)?;
        let permissions = item
            .as_object()
            .map(|item| {
                ["user_imported", "remote_export_allowed"]
                    .into_iter()
                    .filter_map(|key| item.get(key).map(|value| (key, value.clone())))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(mut document) = normalize_legacy_document(item.clone(), &path, &[]) {
            // Match both the former path spellings and news migrated from JSON
            // without an item id. The oldest identity keeps its historical links.
            let mut old_item = item.clone();
            if let Some(fields) = old_item.as_object_mut() {
                for key in ["document_id", "chunk_id", "id"] {
                    fields.remove(key);
                }
            }
            let mut matches = Vec::new();
            for old_path in &paths {
                for candidate in [&item, &old_item] {
                    if let Some(candidate) =
                        normalize_legacy_document(candidate.clone(), Path::new(old_path), &[])
                    {
                        let id = candidate["document_id"].as_str().unwrap();
                        if let Some(imported_at) = existing.get(id) {
                            matches.push((*imported_at, id.to_string(), old_path.clone()));
                        }
                    }
                }
            }
            matches.sort();
            matches.dedup_by(|left, right| left.1 == right.1);
            if let Some((_, id, original_path)) = matches.first() {
                document["document_id"] = json!(id);
                document["metadata"]["legacy_path"] = json!(original_path);
            }
            if matches.len() > 1 {
                duplicate_groups.push(matches.into_iter().map(|(_, id, _)| id).collect::<Vec<_>>());
            }
            for (key, value) in permissions {
                document[key] = value;
            }
            documents.push(document);
        }
    }
    drop(connection);
    validate_portable_documents(&documents)?;
    let result = store.ingest_documents(&documents)?;
    reconcile_news_cache_duplicates(store, &duplicate_groups)?;
    let legacy_refresh_epoch_ms = if path.exists() {
        serde_json::from_slice::<Value>(
            &fs::read(&path)
                .map_err(|error| format!("failed to read news cache refresh metadata: {error}"))?,
        )
        .ok()
        .and_then(|value| value.get("updated_at_epoch_ms").and_then(Value::as_i64))
    } else {
        None
    };
    let refresh_epoch_ms = fetched_at.into_iter().chain(legacy_refresh_epoch_ms).max();
    if let Some(refresh_epoch_ms) = refresh_epoch_ms {
        store
            .connection()?
            .execute(
                "INSERT INTO research_metadata (key, value) VALUES ('last_refresh_at_epoch_ms', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![refresh_epoch_ms.to_string()],
            )
            .map_err(|error| format!("failed to record news refresh time: {error}"))?;
    }
    Ok(result)
}

fn reconcile_news_cache_duplicates(
    store: &ResearchStore,
    groups: &[Vec<String>],
) -> Result<(), String> {
    if groups.is_empty() {
        return Ok(());
    }
    let mut connection = store.connection()?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| format!("failed to start news identity reconciliation: {error}"))?;
    let mut changed = false;
    for group in groups {
        // The preceding import runs retention. Re-read live candidates after
        // taking the write lock, since some pre-import identities may be gone.
        let mut survivors = Vec::new();
        for document_id in group {
            let document = transaction
                .query_row(
                    "SELECT imported_at_epoch_ms, content_hash, metadata_json
                     FROM documents WHERE id = ?1",
                    params![document_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("failed to read surviving news document: {error}"))?;
            if let Some((imported_at, content_hash, metadata)) = document {
                survivors.push((imported_at, document_id.clone(), content_hash, metadata));
            }
        }
        survivors.sort();
        let Some((_, canonical_id, canonical_hash, metadata)) = survivors.first() else {
            continue;
        };
        let mut metadata: Value = serde_json::from_str(metadata)
            .map_err(|error| format!("failed to decode canonical news metadata: {error}"))?;
        if metadata
            .as_object_mut()
            .and_then(|fields| fields.remove("news_cache_duplicate_of"))
            .is_some()
        {
            transaction
                .execute(
                    "UPDATE documents SET metadata_json=?2 WHERE id=?1",
                    params![canonical_id, metadata.to_string()],
                )
                .map_err(|error| format!("failed to restore surviving news identity: {error}"))?;
            changed = true;
        }
        for (_, duplicate_id, content_hash, metadata) in survivors.iter().skip(1) {
            // Different cited content versions remain separate evidence.
            if content_hash != canonical_hash {
                continue;
            }
            let mut metadata: Value = serde_json::from_str(metadata)
                .map_err(|error| format!("failed to decode duplicate news metadata: {error}"))?;
            if metadata
                .get("news_cache_duplicate_of")
                .and_then(Value::as_str)
                != Some(canonical_id.as_str())
            {
                metadata["news_cache_duplicate_of"] = json!(canonical_id);
                transaction
                    .execute(
                        "UPDATE documents SET metadata_json = ?2 WHERE id = ?1",
                        params![duplicate_id, metadata.to_string()],
                    )
                    .map_err(|error| {
                        format!("failed to reconcile duplicate news identity: {error}")
                    })?;
                changed = true;
            }
            transaction
                .execute(
                    "UPDATE research_messages AS current
                 SET unread = MIN(unread, COALESCE((
                     SELECT MIN(previous.unread) FROM research_messages previous
                     WHERE previous.document_id = ?2 AND previous.stock_code IS current.stock_code
                 ), unread)) WHERE current.document_id = ?1",
                    params![canonical_id, duplicate_id],
                )
                .map_err(|error| {
                    format!("failed to preserve reconciled news read state: {error}")
                })?;
        }
    }
    // Alias documents and chunks are retained so saved citation ids still resolve.
    transaction
        .commit()
        .map_err(|error| format!("failed to commit news identity reconciliation: {error}"))?;
    if changed {
        invalidate_vector_cache(store.database_path());
    }
    Ok(())
}

pub(in crate::research) fn migrate_legacy_sources(
    store: &ResearchStore,
    app_data: &Path,
) -> Result<(), String> {
    let connection = store.connection()?;
    let imported = connection
        .query_row(
            "SELECT value FROM research_metadata WHERE key = 'legacy_v1_import_complete'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("failed to read migration state: {error}"))?
        .as_deref()
        == Some("true");
    let archived = connection
        .query_row(
            "SELECT value FROM research_metadata WHERE key = 'legacy_v1_archive_complete'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("failed to read legacy archive state: {error}"))?
        .as_deref()
        == Some("true");

    let paths = legacy_source_paths(app_data);
    if !imported {
        let mut documents = Vec::new();
        for path in &paths {
            if !path.exists() {
                continue;
            }
            documents.extend(legacy_documents_from_file(path)?);
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
    }
    if !archived {
        archive_legacy_sources(app_data, &paths)?;
        connection
            .execute(
                "INSERT INTO research_metadata (key, value) VALUES ('legacy_v1_archive_complete', 'true')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .map_err(|error| format!("failed to record legacy archive state: {error}"))?;
    }
    Ok(())
}

fn legacy_source_paths(app_data: &Path) -> Vec<PathBuf> {
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
    paths
}

pub(in crate::research) fn archive_legacy_sources(
    app_data: &Path,
    paths: &[PathBuf],
) -> Result<(), String> {
    let archive_root = app_data.join("research").join("legacy-v1");
    for source in paths {
        let relative = source.strip_prefix(app_data).map_err(|error| {
            format!(
                "failed to resolve legacy archive path {}: {error}",
                source.display()
            )
        })?;
        let destination = archive_root.join(relative);
        let sidecars = ["-wal", "-shm"]
            .iter()
            .map(|suffix| {
                (
                    PathBuf::from(format!("{}{}", source.display(), suffix)),
                    PathBuf::from(format!("{}{}", destination.display(), suffix)),
                )
            })
            .filter(|(sidecar, _)| sidecar.exists())
            .collect::<Vec<_>>();
        if !source.exists() && sidecars.is_empty() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create legacy archive directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        if source.exists() {
            if destination.exists() {
                return Err(format!(
                    "legacy archive destination already exists while source remains: {}",
                    destination.display()
                ));
            }
            fs::rename(source, &destination).map_err(|error| {
                format!(
                    "failed to archive legacy source {}: {error}",
                    source.display()
                )
            })?;
        }

        for (sidecar, sidecar_destination) in sidecars {
            if sidecar_destination.exists() {
                return Err(format!(
                    "legacy archive sidecar destination already exists while source remains: {}",
                    sidecar_destination.display()
                ));
            }
            fs::rename(&sidecar, &sidecar_destination).map_err(|error| {
                format!(
                    "failed to archive legacy SQLite sidecar {}: {error}",
                    sidecar.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(in crate::research) fn legacy_documents_from_file(path: &Path) -> Result<Vec<Value>, String> {
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
    validate_legacy_pack_version(&value)?;
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
            candidates.extend(
                items
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(ordinal, item)| (key.to_string(), ordinal, item)),
            );
        }
    }
    if candidates.len() > MAX_PACK_DOCUMENTS {
        return Err("legacy research source contains too many documents".to_string());
    }
    let path_hash = sha256_hex(path.display().to_string().as_bytes());
    let mut seen_ids = HashSet::new();
    let mut documents = Vec::new();
    for (container, ordinal, item) in candidates {
        validate_legacy_candidate_size(&item)?;
        let Some(mut document) = normalize_legacy_document(item, path, &inherited_codes) else {
            continue;
        };
        let document_id = document
            .get("document_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "normalized legacy document is missing document_id".to_string())?;
        if !seen_ids.insert(document_id.to_string()) {
            let collision_hash = sha256_hex(
                format!("{document_id}|container:{container}|ordinal:{ordinal}").as_bytes(),
            );
            let collision_id = format!("legacy-{}-{}", &path_hash[..16], &collision_hash[..24]);
            document["document_id"] = Value::String(collision_id.clone());
            seen_ids.insert(collision_id);
        }
        documents.push(document);
    }
    validate_portable_documents(&documents)?;
    Ok(documents)
}

pub(in crate::research) fn normalize_legacy_document(
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
            let mapped = normalized_string_array(object.get("mapped_stock_codes"));
            if mapped.is_empty() {
                inherited_codes.to_vec()
            } else {
                mapped
            }
        } else {
            own
        }
    };
    let mut metadata = json!({"legacy_path": path.display().to_string()});
    if let Some(metadata_object) = metadata.as_object_mut() {
        for key in [
            "scope_type",
            "scope_tags",
            "mapped_stock_codes",
            "fetched_at_epoch_ms",
            "article_summary",
            "article_summary_status",
            "article_summary_url",
            "article_summary_fetched_at_epoch_ms",
        ] {
            if let Some(value) = object.get(key) {
                metadata_object.insert(key.to_string(), value.clone());
            }
        }
    }
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
        "metadata": metadata
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
