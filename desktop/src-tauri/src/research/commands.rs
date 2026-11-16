use crate::research::app_store::market_from_payload;
#[cfg(target_os = "windows")]
use crate::research::background::rebuild_research_embeddings_for_market;
#[cfg(target_os = "windows")]
use crate::research::background::research_embedding_model_dir;
use crate::research::background::schedule_research_embeddings_for_market;
use crate::research::storage::vectors::finalize_research_index_status;
use serde_json::json;
use serde_json::Value;

pub(crate) async fn api_research_overview(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    crate::runtime::run_io_bound("api_research_overview", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            let mut overview = store.overview(&payload)?;
            #[cfg(target_os = "windows")]
            {
                let index = store.index_status()?;
                let embedding_count = index
                    .get("embedding_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let mut vector =
                    crate::research_embeddings::model_status(&research_embedding_model_dir(&app));
                let model_ready = vector
                    .get("ready")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                vector["ready"] = Value::Bool(model_ready && embedding_count > 0);
                vector["embedding_count"] = json!(embedding_count);
                overview["retrieval"]["vector"] = vector;
            }
            Ok(overview)
        })
    })
    .await?
}

pub(crate) async fn api_research_messages(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    crate::runtime::run_io_bound("api_research_messages", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            store.messages(&payload)
        })
    })
    .await?
}

pub(crate) async fn api_research_mark_read(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    crate::runtime::run_io_bound("api_research_mark_read", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            store.mark_read(&payload)
        })
    })
    .await?
}

pub(crate) async fn api_research_query(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    let retrieval_app = app.clone();
    let mut retrieval_payload = payload.clone();
    let retrieval_market = market.clone();
    let remote_model_requested = payload.get("llm").is_some();
    if remote_model_requested {
        retrieval_payload["remote_safe_only"] = Value::Bool(true);
    }
    let (database_generation, mut response) =
        crate::runtime::run_cpu_bound("api_research_query", move || {
            crate::research::app_store::with_app_store_snapshot_for_market(
                &retrieval_app,
                &retrieval_market,
                |store| {
                    let response = {
                        #[cfg(target_os = "windows")]
                        {
                            let query_vector_result = retrieval_payload
                                .get("query")
                                .and_then(Value::as_str)
                                .map(|query| {
                                    crate::research_embeddings::embed(
                                        &research_embedding_model_dir(&retrieval_app),
                                        &[query.to_string()],
                                    )
                                })
                                .transpose();
                            match query_vector_result {
                                Ok(vectors) => {
                                    let query_vector =
                                        vectors.and_then(|items| items.into_iter().next());
                                    store.query_with_vector(
                                        &retrieval_payload,
                                        query_vector.as_deref(),
                                    )?
                                }
                                Err(error) => {
                                    let mut fallback = store.query(&retrieval_payload)?;
                                    fallback["vector_warning"] = Value::String(error);
                                    fallback
                                }
                            }
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            store.query(&retrieval_payload)?
                        }
                    };
                    Ok::<Value, String>(response)
                },
            )
        })
        .await??;

    if remote_model_requested {
        response["remote_evidence_policy"] = json!({
            "remote_safe_only": true,
            "private_evidence_egress_allowed": false,
            "warning": "用户导入材料默认不会发送给远程模型"
        });
    }
    let citations = response
        .get("citations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let community_only = response
        .get("community_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if community_only {
        response["model_warning"] =
            Value::String("仅命中社区信息，已保持证据模式，不调用模型生成事实结论。".to_string());
    } else if payload.get("llm").is_some() && !citations.is_empty() {
        let question = payload
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let market_question = if market == "hk" {
            let stock_code = payload
                .get("stock_code")
                .and_then(Value::as_str)
                .unwrap_or("港股知识库");
            format!(
                "市场：港股；目标证券：{stock_code}。仅基于港股知识库证据回答。问题：{question}"
            )
        } else {
            question.to_string()
        };
        match crate::runtime::with_heavy_network_permit(
            "api_research_answer",
            crate::news_rag::synthesize_research_answer(
                payload.get("llm"),
                &market_question,
                &citations,
            ),
        )
        .await
        {
            Ok(Some(answer)) => {
                match crate::research::storage::evidence::validate_model_answer(&answer, &citations)
                {
                    Ok(()) => {
                        response["answer"] = Value::String(answer);
                        response["mode"] = Value::String("model".to_string());
                    }
                    Err(error) => {
                        response["model_warning"] = Value::String(error);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                response["model_warning"] = Value::String(error);
            }
        }
    }

    let thread_id = payload
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let question = payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let saved_response = response.clone();
    crate::runtime::run_io_bound("api_research_save_answer", move || {
        crate::research::app_store::with_app_store_at_generation_for_market(
            &app,
            database_generation,
            &market,
            |store| {
                if let Some(thread_id) = thread_id.as_deref() {
                    store.save_answer(thread_id, &question, &saved_response)?;
                }
                Ok(())
            },
        )
    })
    .await??;
    Ok(response)
}

pub(crate) async fn api_research_refresh(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    let news = crate::runtime::with_heavy_network_permit(
        "api_research_refresh",
        crate::news_rag::api_news_rag_impl(app.clone(), payload),
    )
    .await?;
    let imported = {
        let app = app.clone();
        let ingest_market = market.clone();
        crate::runtime::run_io_bound("api_research_ingest_news", move || {
            crate::research::sources::ingest_news_cache_for_market(&app, &ingest_market)
        })
        .await??
    };
    schedule_research_embeddings_for_market(app, market);
    Ok(json!({"news": news, "imported": imported}))
}

pub(crate) async fn api_research_threads(
    app: tauri::AppHandle,
    market: Option<String>,
) -> Result<Value, String> {
    let market = market.as_deref().unwrap_or("cn").to_string();
    if !matches!(market.as_str(), "cn" | "hk") {
        return Err(format!("unsupported research market: {market}"));
    }
    crate::runtime::run_io_bound("api_research_threads", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            store.threads()
        })
    })
    .await?
}

pub(crate) async fn api_research_thread_create(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    crate::runtime::run_io_bound("api_research_thread_create", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            store.create_thread(&payload)
        })
    })
    .await?
}

pub(crate) async fn api_research_thread_detail(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    crate::runtime::run_io_bound("api_research_thread_detail", move || {
        let thread_id = payload
            .get("thread_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "thread_id is required".to_string())?;
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            store.thread(thread_id)
        })
    })
    .await?
}

pub(crate) async fn api_research_thread_delete(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    let thread_id = payload
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "thread_id is required".to_string())?
        .to_string();
    if thread_id.len() > crate::research::storage::conversations::MAX_RESEARCH_THREAD_ID_BYTES {
        return Err(format!(
            "thread_id exceeds {} bytes",
            crate::research::storage::conversations::MAX_RESEARCH_THREAD_ID_BYTES
        ));
    }
    crate::runtime::run_io_bound("api_research_thread_delete", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            let deleted = store.delete_thread(&thread_id)?;
            Ok(json!({"deleted": deleted}))
        })
    })
    .await?
}

pub(crate) async fn api_research_index_status(
    app: tauri::AppHandle,
    market: Option<String>,
) -> Result<Value, String> {
    let market = market.as_deref().unwrap_or("cn").to_string();
    if !matches!(market.as_str(), "cn" | "hk") {
        return Err(format!("unsupported research market: {market}"));
    }
    let status_app = app.clone();
    let status_market = market.clone();
    let status = crate::runtime::run_io_bound("api_research_index_status", move || {
        crate::research::app_store::with_app_store_for_market(
            &status_app,
            &status_market,
            |store| {
                let mut status = store.index_status()?;
                status["documents"] = store.document_statuses()?["items"].clone();
                #[cfg(target_os = "windows")]
                let vector_status = crate::research_embeddings::model_status(
                    &research_embedding_model_dir(&status_app),
                );
                #[cfg(not(target_os = "windows"))]
                let vector_status = status["vector"].clone();
                Ok(finalize_research_index_status(status, vector_status))
            },
        )
    })
    .await??;
    #[cfg(target_os = "windows")]
    if status
        .get("embedding_pending_count")
        .and_then(Value::as_i64)
        .is_some_and(|count| count > 0)
    {
        schedule_research_embeddings_for_market(app, market);
    }
    Ok(status)
}

pub(crate) async fn api_research_rebuild_index(
    app: tauri::AppHandle,
    payload: Option<Value>,
) -> Result<Value, String> {
    let market = payload
        .as_ref()
        .map(market_from_payload)
        .transpose()?
        .unwrap_or("cn")
        .to_string();
    crate::runtime::run_cpu_bound("api_research_rebuild_index", move || {
        crate::research::app_store::with_app_store_for_market(&app, &market, |store| {
            store.rebuild_fts()
        })
    })
    .await?
}

pub(crate) async fn api_research_rebuild_embeddings(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    #[cfg(target_os = "windows")]
    {
        crate::runtime::run_cpu_bound("api_research_rebuild_embeddings", move || {
            rebuild_research_embeddings_for_market(&app, &market)
        })
        .await?
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, market);
        Err("Android uses BM25 only and does not load the Windows embedding model".to_string())
    }
}

pub(crate) async fn api_research_pack_export(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    crate::runtime::run_io_bound("api_research_pack_export", move || {
        crate::research::storage::packs::export_app_pack_for_market(&app, &payload, &market)
    })
    .await?
}

pub(crate) async fn api_research_pack_import(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let market = market_from_payload(&payload)?.to_string();
    let worker_app = app.clone();
    let worker_market = market.clone();
    let result = crate::runtime::run_io_bound("api_research_pack_import", move || {
        if worker_market == "hk" {
            crate::research::storage::packs::import_app_pack_for_market(
                &worker_app,
                &payload,
                &worker_market,
            )
        } else {
            crate::research::storage::packs::import_app_pack(&worker_app, &payload)
        }
    })
    .await??;
    schedule_research_embeddings_for_market(app, market);
    Ok(result)
}

pub(crate) async fn api_research_pack_rollback(
    app: tauri::AppHandle,
    payload: Option<Value>,
) -> Result<Value, String> {
    let market = payload
        .as_ref()
        .map(market_from_payload)
        .transpose()?
        .unwrap_or("cn");
    if market == "hk" {
        return Err("HK research pack rollback is unavailable; HK workspace documents are stored separately".to_string());
    }
    crate::runtime::run_io_bound("api_research_pack_rollback", move || {
        crate::research::storage::packs::rollback_app_pack(&app)
    })
    .await?
}
