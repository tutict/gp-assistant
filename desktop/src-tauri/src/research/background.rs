use serde_json::json;
#[cfg(target_os = "windows")]
use serde_json::Value;
#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Duration;
use tauri::Emitter;
#[cfg(target_os = "windows")]
use tauri::Manager;

#[cfg(target_os = "windows")]
pub(crate) static RESEARCH_EMBEDDING_JOB_GATES: ResearchEmbeddingJobGates =
    ResearchEmbeddingJobGates::new();

#[cfg(target_os = "windows")]
pub(crate) struct ResearchEmbeddingJobGate {
    pub(crate) running: AtomicBool,
    pub(crate) pending: AtomicBool,
}

#[cfg(target_os = "windows")]
impl ResearchEmbeddingJobGate {
    pub(crate) const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            pending: AtomicBool::new(false),
        }
    }

    pub(crate) fn request(&self) -> bool {
        self.pending.store(true, AtomicOrdering::Release);
        self.running
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
    }

    pub(crate) fn begin_cycle(&self) {
        self.pending.store(false, AtomicOrdering::Release);
    }

    pub(crate) fn finish_cycle(&self) -> bool {
        if self.pending.load(AtomicOrdering::Acquire) {
            return true;
        }
        self.running.store(false, AtomicOrdering::Release);
        if !self.pending.swap(false, AtomicOrdering::AcqRel) {
            return false;
        }
        self.running
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct ResearchEmbeddingJobGates {
    cn: ResearchEmbeddingJobGate,
    hk: ResearchEmbeddingJobGate,
}

#[cfg(target_os = "windows")]
impl ResearchEmbeddingJobGates {
    pub(crate) const fn new() -> Self {
        Self {
            cn: ResearchEmbeddingJobGate::new(),
            hk: ResearchEmbeddingJobGate::new(),
        }
    }

    pub(crate) fn for_market(&self, market: &str) -> &ResearchEmbeddingJobGate {
        if market == "hk" {
            &self.hk
        } else {
            &self.cn
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn rebuild_research_embeddings(app: &tauri::AppHandle) -> Result<Value, String> {
    rebuild_research_embeddings_for_market(app, "cn")
}

#[cfg(target_os = "windows")]
pub(crate) fn rebuild_research_embeddings_for_market(
    app: &tauri::AppHandle,
    market: &str,
) -> Result<Value, String> {
    crate::research::app_store::with_app_store_for_market(app, market, |store| {
        let model_dir = research_embedding_model_dir(app);
        let mut stored = 0usize;
        loop {
            let pending = store.pending_embedding_chunks(256)?;
            if pending.is_empty() {
                break;
            }
            let texts = pending
                .iter()
                .map(|item| item.text.clone())
                .collect::<Vec<_>>();
            let vectors = crate::research_embeddings::embed(&model_dir, &texts)?;
            if vectors.len() != pending.len() {
                return Err("embedding service returned an unexpected batch size".to_string());
            }
            let items = pending.into_iter().zip(vectors).collect::<Vec<_>>();
            stored += items.len();
            store.store_embeddings(&items, crate::research_embeddings::MODEL_ID)?;
        }
        Ok(json!({
            "stored": stored,
            "model_id": crate::research_embeddings::MODEL_ID,
            "notes": if stored == 0 {
                vec!["所有分块均已有当前内容哈希对应的向量。"]
            } else {
                Vec::<&str>::new()
            }
        }))
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn schedule_research_embeddings(app: tauri::AppHandle) {
    schedule_research_embeddings_for_market(app, "cn".to_string());
}

#[cfg(target_os = "windows")]
pub(crate) fn schedule_research_embeddings_for_market(app: tauri::AppHandle, market: String) {
    let gate = RESEARCH_EMBEDDING_JOB_GATES.for_market(&market);
    if !gate.request() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            gate.begin_cycle();
            let worker_app = app.clone();
            let worker_market = market.clone();
            let result =
                crate::runtime::run_cpu_bound("background_research_embeddings", move || {
                    rebuild_research_embeddings_for_market(&worker_app, &worker_market)
                })
                .await
                .and_then(|result| result);
            let event = match result {
                Ok(status) => json!({"ok": true, "status": status}),
                Err(error) => json!({"ok": false, "error": error}),
            };
            let _ = app.emit("research-embedding-status", event);
            if !gate.finish_cycle() {
                break;
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn schedule_research_embeddings(_app: tauri::AppHandle) {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn schedule_research_embeddings_for_market(_app: tauri::AppHandle, _market: String) {}

#[cfg(target_os = "windows")]
pub(crate) fn research_embedding_model_dir(app: &tauri::AppHandle) -> PathBuf {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|root| root.join("models").join("bge-small-zh-v1.5-int8"));
    if let Some(path) = bundled.filter(|path| path.join("model_quantized.onnx").exists()) {
        return path;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/bge-small-zh-v1.5-int8")
}

pub(crate) fn schedule_research_maintenance(app: tauri::AppHandle) {
    // Keep the ONNX embedding runtime lazy. Loading it during every desktop
    // startup adds hundreds of megabytes before the user opens research; the
    // import, refresh, index-status, and explicit rebuild paths schedule it
    // when vector retrieval is actually needed.
    tauri::async_runtime::spawn(async move {
        loop {
            let worker_app = app.clone();
            let result = crate::runtime::run_io_bound("background_research_retention", move || {
                crate::research::app_store::with_app_store(&worker_app, |store| {
                    store.prune_retention()
                })
            })
            .await
            .and_then(|result| result);
            let event = match result {
                Ok(status) => json!({"ok": true, "status": status}),
                Err(error) => json!({"ok": false, "error": error}),
            };
            let _ = app.emit("research-retention-status", event);
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}
