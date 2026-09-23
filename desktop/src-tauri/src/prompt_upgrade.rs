use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::{agent_harness, agent_ledger};

pub(crate) const BASE_PROMPT_VERSION: &str = "rig-agent-runtime-v1";
const MAX_METHOD_CARD_CHARS: usize = 4_000;
const MAX_SYSTEM_PROMPT_CHARS: usize = 12_000;
const REJECTION_THRESHOLD: usize = 5;
const MAX_SAMPLE_QUESTION_CHARS: usize = 500;
const MAX_SAMPLE_ERROR_CHARS: usize = 300;

const HOT_MONEY_PROMPT: &str = include_str!("../../../app/prompts/hot_money_early_v1.md");
const VALUE_COMPOUNDER_PROMPT: &str = include_str!("../../../app/prompts/value_compounder_v1.md");
const STOCK_SOUL_PROMPT: &str = include_str!("../../../app/prompts/stock_soul.md");
const EVAL_CASES: &str = include_str!("../../../app/prompts/agent_harness_eval_cases.json");

static OVERLAY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
pub(crate) struct ResolvedPrompt {
    pub text: String,
    pub version: String,
}

#[derive(Clone, Debug)]
pub(crate) struct DraftSample {
    pub question: String,
    pub error: String,
}

#[derive(Clone, Debug)]
pub(crate) struct UpgradeClaim {
    pub profile_id: String,
    #[allow(dead_code)]
    pub mode: String,
    #[allow(dead_code)]
    pub prompt_version: String,
    pub current_body: String,
    pub required_terms: Vec<String>,
    pub forbidden_terms: Vec<String>,
    pub samples: Vec<DraftSample>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PromptOverlayStatus {
    pub profile_id: String,
    pub label: String,
    pub prompt_version: String,
    pub builtin: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OverlayStore {
    #[serde(default)]
    profiles: BTreeMap<String, ProfileOverlay>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfileOverlay {
    #[serde(default)]
    active: Option<ActivePrompt>,
    #[serde(default)]
    next_n: u32,
    #[serde(default)]
    consumed_prompt_version: String,
    #[serde(default)]
    consumed_run_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ActivePrompt {
    n: u32,
    version: String,
    body: String,
}

struct ProfileContract {
    mode: &'static str,
    required_terms: Vec<String>,
    forbidden_terms: Vec<String>,
}

pub(crate) fn rejected_profile(response: &Value) -> Option<String> {
    let harness = response.get("harness")?;
    if harness.get("model_outcome").and_then(Value::as_str) != Some("policy_rejected") {
        return None;
    }
    let profile_id = harness.get("profile_id").and_then(Value::as_str)?;
    matches!(
        profile_id,
        "hot_money_early_v1" | "value_compounder_v1"
    )
    .then(|| profile_id.to_string())
}

pub(crate) fn resolve_prompt(mode: &str, app: Option<&AppHandle>) -> ResolvedPrompt {
    if !is_upgradeable_mode(mode) {
        return compose(mode, builtin_body(mode), BASE_PROMPT_VERSION);
    }
    let profile_id = profile_id_for_mode(mode);
    if let Some(app) = app {
        if let Some(active) = read_active_prompt(app, profile_id) {
            if evaluate_candidate(profile_id, &active.body).is_ok() {
                return compose(mode, &active.body, &active.version);
            }
        }
    }
    compose(mode, builtin_body(mode), BASE_PROMPT_VERSION)
}

pub(crate) fn claim_with_app(
    app: &AppHandle,
    profile_id: &str,
) -> Result<Option<UpgradeClaim>, String> {
    let _guard = OVERLAY_LOCK
        .lock()
        .map_err(|_| "prompt overlay lock is poisoned".to_string())?;
    claim_at(&ledger_path(app)?, &overlay_path(app)?, profile_id)
}

pub(crate) fn activate_with_app(
    app: &AppHandle,
    profile_id: &str,
    body: &str,
) -> Result<bool, String> {
    let _guard = OVERLAY_LOCK
        .lock()
        .map_err(|_| "prompt overlay lock is poisoned".to_string())?;
    activate_at(&overlay_path(app)?, profile_id, body)
}

pub(crate) fn status_with_app(app: &AppHandle) -> Result<Vec<PromptOverlayStatus>, String> {
    let _guard = OVERLAY_LOCK
        .lock()
        .map_err(|_| "prompt overlay lock is poisoned".to_string())?;
    status_at(&overlay_path(app)?)
}

pub(crate) fn revert_with_app(app: &AppHandle, profile_id: &str) -> Result<(), String> {
    let _guard = OVERLAY_LOCK
        .lock()
        .map_err(|_| "prompt overlay lock is poisoned".to_string())?;
    revert_at(&ledger_path(app)?, &overlay_path(app)?, profile_id)
}

pub(crate) fn draft_request(claim: &UpgradeClaim) -> Value {
    json!({
        "profile_id": claim.profile_id,
        "required_terms": claim.required_terms,
        "forbidden_terms": claim.forbidden_terms,
        "current_method_card": claim.current_body,
        "rejections": claim.samples.iter().map(|sample| json!({
            "question": sample.question,
            "error": sample.error,
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn evaluate_candidate(profile_id: &str, body: &str) -> Result<(), String> {
    let contract = profile_contract(profile_id)?;
    let body = body.trim();
    if body.is_empty() {
        return Err("method card is empty".to_string());
    }
    if body.chars().count() > MAX_METHOD_CARD_CHARS {
        return Err(format!(
            "method card exceeds {MAX_METHOD_CARD_CHARS} characters"
        ));
    }
    let prompt = compose(contract.mode, body, BASE_PROMPT_VERSION).text;
    if !prompt.contains("不构成投资建议") {
        return Err("method card removed the research-only disclaimer".to_string());
    }
    for term in &contract.required_terms {
        if !prompt.contains(term) {
            return Err(format!("method card is missing required term: {term}"));
        }
    }
    for term in &contract.forbidden_terms {
        if prompt.contains(term) {
            return Err(format!("method card contains a forbidden term: {term}"));
        }
    }
    if agent_harness::contains_prohibited_instruction_text(&prompt) {
        return Err(
            "method card contains a trading, position, or return promise".to_string(),
        );
    }
    Ok(())
}
fn claim_at(
    ledger_path: &Path,
    overlay_path: &Path,
    profile_id: &str,
) -> Result<Option<UpgradeClaim>, String> {
    let contract = profile_contract(profile_id)?;
    let mut store = load_store(overlay_path);
    let mut claim = None;
    {
        let profile = store.profiles.entry(profile_id.to_string()).or_default();
        let current_version = profile
            .active
            .as_ref()
            .filter(|active| evaluate_candidate(profile_id, &active.body).is_ok())
            .map(|active| active.version.clone())
            .unwrap_or_else(|| BASE_PROMPT_VERSION.to_string());
        if profile.consumed_prompt_version != current_version {
            profile.consumed_run_ids.clear();
            profile.consumed_prompt_version = current_version.clone();
        }
        let current_body = profile
            .active
            .as_ref()
            .filter(|active| {
                active.version == current_version
                    && evaluate_candidate(profile_id, &active.body).is_ok()
            })
            .map(|active| active.body.clone())
            .unwrap_or_else(|| builtin_body(contract.mode).to_string());
        let ledger = agent_ledger::AgentRunStore::open(ledger_path)?;
        let rejections = ledger.list_policy_rejections(profile_id, &current_version)?;
        let consumed: BTreeSet<&str> = profile.consumed_run_ids.iter().map(String::as_str).collect();
        let eligible: Vec<_> = rejections
            .into_iter()
            .filter(|sample| !consumed.contains(sample.run_id.as_str()))
            .collect();
        if eligible.len() >= REJECTION_THRESHOLD {
            let samples = eligible
                .iter()
                .take(REJECTION_THRESHOLD)
                .map(|sample| DraftSample {
                    question: sanitize_sample(&sample.question, MAX_SAMPLE_QUESTION_CHARS),
                    error: sanitize_sample(&sample.error, MAX_SAMPLE_ERROR_CHARS),
                })
                .collect::<Vec<_>>();
            profile
                .consumed_run_ids
                .extend(eligible.iter().map(|sample| sample.run_id.clone()));
            profile.consumed_run_ids.sort();
            profile.consumed_run_ids.dedup();
            if profile.consumed_run_ids.len() > 4_000 {
                let extra = profile.consumed_run_ids.len() - 4_000;
                profile.consumed_run_ids.drain(0..extra);
            }
            claim = Some(UpgradeClaim {
                profile_id: profile_id.to_string(),
                mode: contract.mode.to_string(),
                prompt_version: current_version,
                current_body,
                required_terms: contract.required_terms.clone(),
                forbidden_terms: contract.forbidden_terms.clone(),
                samples,
            });
        }
    }
    save_store(overlay_path, &store)?;
    Ok(claim)
}

fn activate_at(overlay_path: &Path, profile_id: &str, body: &str) -> Result<bool, String> {
    if evaluate_candidate(profile_id, body).is_err() {
        return Ok(false);
    }
    let mut store = load_store(overlay_path);
    let profile = store.profiles.entry(profile_id.to_string()).or_default();
    let n = profile.next_n.max(1);
    let version = format!("{BASE_PROMPT_VERSION}+{profile_id}+{n}");
    profile.active = Some(ActivePrompt {
        n,
        version: version.clone(),
        body: body.trim().to_string(),
    });
    profile.next_n = n.saturating_add(1);
    profile.consumed_prompt_version = version;
    profile.consumed_run_ids.clear();
    save_store(overlay_path, &store)?;
    Ok(true)
}

fn status_at(overlay_path: &Path) -> Result<Vec<PromptOverlayStatus>, String> {
    let store = load_store(overlay_path);
    Ok(["hot_money_early_v1", "value_compounder_v1"]
        .into_iter()
        .map(|profile_id| {
            let active = store
                .profiles
                .get(profile_id)
                .and_then(|profile| profile.active.as_ref())
                .filter(|active| evaluate_candidate(profile_id, &active.body).is_ok());
            PromptOverlayStatus {
                profile_id: profile_id.to_string(),
                label: profile_label(profile_id).to_string(),
                prompt_version: active
                    .map(|item| item.version.clone())
                    .unwrap_or_else(|| BASE_PROMPT_VERSION.to_string()),
                builtin: active.is_none(),
            }
        })
        .collect())
}

fn revert_at(
    ledger_path: &Path,
    overlay_path: &Path,
    profile_id: &str,
) -> Result<(), String> {
    profile_contract(profile_id)?;
    let mut store = load_store(overlay_path);
    let consumed = agent_ledger::AgentRunStore::open(ledger_path)?
        .list_policy_rejections(profile_id, BASE_PROMPT_VERSION)?
        .into_iter()
        .map(|sample| sample.run_id)
        .collect::<Vec<_>>();
    let profile = store.profiles.entry(profile_id.to_string()).or_default();
    profile.active = None;
    profile.consumed_prompt_version = BASE_PROMPT_VERSION.to_string();
    profile.consumed_run_ids = consumed;
    save_store(overlay_path, &store)
}

fn read_active_prompt(app: &AppHandle, profile_id: &str) -> Option<ActivePrompt> {
    let path = overlay_path(app).ok()?;
    let _guard = OVERLAY_LOCK.lock().ok()?;
    load_store(&path)
        .profiles
        .get(profile_id)
        .and_then(|profile| profile.active.clone())
}

fn compose(mode: &str, body: &str, version: &str) -> ResolvedPrompt {
    let text = format!(
        "{}\n\n当前模式：{mode}。只使用只读工具和本地证据，不能覆盖或虚构工具事实，不提供买卖建议或收益承诺。最终输出必须符合 JSON schema；reply 和事实 bullet 必须邻近引用有效证据编号。",
        body.trim()
    );
    ResolvedPrompt {
        text: text.chars().take(MAX_SYSTEM_PROMPT_CHARS).collect(),
        version: version.to_string(),
    }
}

fn builtin_body(mode: &str) -> &'static str {
    match mode {
        "expert" => HOT_MONEY_PROMPT,
        "research" => VALUE_COMPOUNDER_PROMPT,
        _ => STOCK_SOUL_PROMPT,
    }
}

fn is_upgradeable_mode(mode: &str) -> bool {
    matches!(mode, "expert" | "research")
}

fn profile_id_for_mode(mode: &str) -> &'static str {
    match mode {
        "expert" => "hot_money_early_v1",
        "research" => "value_compounder_v1",
        _ => "",
    }
}

fn profile_label(profile_id: &str) -> &'static str {
    match profile_id {
        "hot_money_early_v1" => "专家模式",
        "value_compounder_v1" => "研报模式",
        _ => "研究模式",
    }
}

fn profile_contract(profile_id: &str) -> Result<ProfileContract, String> {
    let mode = match profile_id {
        "hot_money_early_v1" => "expert",
        "value_compounder_v1" => "research",
        _ => {
            return Err("only expert and research method cards can be upgraded".to_string());
        }
    };
    let suite: Value = serde_json::from_str(EVAL_CASES)
        .map_err(|error| format!("invalid prompt contract: {error}"))?;
    let contract = suite
        .get("profiles")
        .and_then(|profiles| profiles.get(mode))
        .ok_or_else(|| "prompt contract is missing the research profile".to_string())?;
    Ok(ProfileContract {
        mode,
        required_terms: string_list(contract.get("required_prompt_terms")),
        forbidden_terms: string_list(contract.get("forbidden_prompt_terms")),
    })
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn sanitize_sample(value: &str, max_chars: usize) -> String {
    let redacted = agent_harness::redact_persisted_question(value, None);
    redact_secret_tokens(&redacted)
        .chars()
        .take(max_chars)
        .collect()
}

fn redact_secret_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while !rest.is_empty() {
        let lower = rest.to_ascii_lowercase();
        let index = ["sk-", "api_key", "api-key", "bearer"]
            .iter()
            .filter_map(|marker| lower.find(marker))
            .min();
        let Some(index) = index else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..index]);
        let tail = &rest[index..];
        let mut end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        if tail[..end].to_ascii_lowercase().starts_with("bearer") {
            let after = &tail[end..];
            let whitespace = after
                .find(|character: char| !character.is_whitespace())
                .unwrap_or(after.len());
            let next = &after[whitespace..];
            end += whitespace + next.find(char::is_whitespace).unwrap_or(next.len());
        }
        output.push_str("[redacted]");
        rest = &tail[end..];
    }
    output
}

fn overlay_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve prompt overlay directory: {error}"))?;
    path.push("agent");
    path.push("prompt-overlays.json");
    Ok(path)
}

fn ledger_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve agent ledger directory: {error}"))?;
    path.push("agent");
    path.push("agent-runs.sqlite");
    Ok(path)
}

fn load_store(path: &Path) -> OverlayStore {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            eprintln!("prompt overlay is unreadable and will be ignored: {error}");
            OverlayStore::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OverlayStore::default(),
        Err(error) => {
            eprintln!("prompt overlay could not be read and will be ignored: {error}");
            OverlayStore::default()
        }
    }
}

fn save_store(path: &Path, store: &OverlayStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create prompt overlay directory: {error}"))?;
    }
    let text = serde_json::to_string_pretty(store)
        .map_err(|error| format!("failed to encode prompt overlay: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, &text)
        .map_err(|error| format!("failed to write prompt overlay: {error}"))?;
    if fs::rename(&temporary, path).is_err() {
        fs::write(path, text)
            .map_err(|error| format!("failed to replace prompt overlay: {error}"))?;
        let _ = fs::remove_file(temporary);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(1);
            let path = std::env::temp_dir().join(format!(
                "gp-prompt-upgrade-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rejection(outcome: &str, profile_id: &str, prompt_version: &str, warning: &str) -> Value {
        json!({
            "reply": "本地工具结果",
            "warnings": [warning],
            "harness": {
                "prompt_version": prompt_version,
                "policy_version": "agent-policy-v1",
                "profile_id": profile_id,
                "model_used": false,
                "model_outcome": outcome,
                "api_format": "openai_chat"
            }
        })
    }

    fn seed(
        ledger: &Path,
        count: usize,
        outcome: &str,
        profile_id: &str,
        prompt_version: &str,
        warning: &str,
    ) {
        static IDS: AtomicU64 = AtomicU64::new(1);
        let store = agent_ledger::AgentRunStore::open(ledger).expect("ledger");
        let mode = if profile_id == "value_compounder_v1" {
            "research"
        } else {
            "expert"
        };
        for index in 0..count {
            let id = IDS.fetch_add(1, Ordering::Relaxed);
            let run_id = format!("run-{id}");
            let started = 1_000 + id as i64;
            store
                .start_run(
                    &json!({
                        "run_id": run_id,
                        "conversation_id": "conversation",
                        "message": format!("问题 {index} https://secret.example/v1?api_key=hidden sk-live-secret"),
                        "mode": mode,
                    }),
                    started,
                )
                .expect("start");
            store
                .complete_run(
                    &run_id,
                    &[],
                    &rejection(outcome, profile_id, prompt_version, warning),
                    started + 10,
                )
                .expect("complete");
        }
    }

    #[test]
    fn builtin_method_cards_pass_the_contract() {
        evaluate_candidate("hot_money_early_v1", HOT_MONEY_PROMPT).expect("expert card");
        evaluate_candidate("value_compounder_v1", VALUE_COMPOUNDER_PROMPT).expect("research card");
    }

    #[test]
    fn four_rejections_do_not_claim_and_request_failures_do_not_count() {
        let dir = TempDir::new();
        let ledger = dir.path().join("agent-runs.sqlite");
        let overlay = dir.path().join("prompt-overlays.json");
        seed(
            &ledger,
            4,
            "policy_rejected",
            "hot_money_early_v1",
            BASE_PROMPT_VERSION,
            "模型输出未通过安全校验",
        );
        seed(
            &ledger,
            5,
            "request_failed",
            "hot_money_early_v1",
            BASE_PROMPT_VERSION,
            "模型执行失败",
        );
        assert!(claim_at(&ledger, &overlay, "hot_money_early_v1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn five_rejections_claim_once_and_five_new_rejections_are_required_after_failure() {
        let dir = TempDir::new();
        let ledger = dir.path().join("agent-runs.sqlite");
        let overlay = dir.path().join("prompt-overlays.json");
        seed(
            &ledger,
            5,
            "policy_rejected",
            "hot_money_early_v1",
            BASE_PROMPT_VERSION,
            "模型输出未通过安全校验",
        );
        let first = claim_at(&ledger, &overlay, "hot_money_early_v1")
            .unwrap()
            .expect("first claim");
        assert_eq!(first.samples.len(), 5);
        assert!(claim_at(&ledger, &overlay, "hot_money_early_v1")
            .unwrap()
            .is_none());
        seed(
            &ledger,
            4,
            "policy_rejected",
            "hot_money_early_v1",
            BASE_PROMPT_VERSION,
            "再次拒绝",
        );
        assert!(claim_at(&ledger, &overlay, "hot_money_early_v1")
            .unwrap()
            .is_none());
        seed(
            &ledger,
            1,
            "policy_rejected",
            "hot_money_early_v1",
            BASE_PROMPT_VERSION,
            "第五次新拒绝",
        );
        assert!(claim_at(&ledger, &overlay, "hot_money_early_v1")
            .unwrap()
            .is_some());
    }

    #[test]
    fn unsafe_or_incomplete_cards_do_not_activate() {
        let dir = TempDir::new();
        let overlay = dir.path().join("prompt-overlays.json");
        let missing = HOT_MONEY_PROMPT.replace("市场环境", "环境");
        assert!(evaluate_candidate("hot_money_early_v1", &missing).is_err());
        assert!(!activate_at(&overlay, "hot_money_early_v1", &missing).unwrap());
        let crossed = format!("{HOT_MONEY_PROMPT}\n巴菲特");
        assert!(evaluate_candidate("hot_money_early_v1", &crossed).is_err());
        let trading = format!("{HOT_MONEY_PROMPT}\n建议买入");
        assert!(evaluate_candidate("hot_money_early_v1", &trading).is_err());
        let oversized = format!("{HOT_MONEY_PROMPT}\n{}", "补充".repeat(4_001));
        assert!(evaluate_candidate("hot_money_early_v1", &oversized).is_err());
        assert!(status_at(&overlay).unwrap().iter().all(|status| status.builtin));
    }

    #[test]
    fn accepted_card_changes_prompt_until_overlay_is_removed_or_corrupt() {
        let dir = TempDir::new();
        let overlay = dir.path().join("prompt-overlays.json");
        let body = format!("{HOT_MONEY_PROMPT}\n\n补充：只根据已有证据描述不确定性。");
        assert!(activate_at(&overlay, "hot_money_early_v1", &body).unwrap());
        let resolved = resolve_from_path("expert", &overlay);
        assert_eq!(
            resolved.version,
            "rig-agent-runtime-v1+hot_money_early_v1+1"
        );
        assert!(resolved.text.contains("只根据已有证据描述不确定性"));
        assert!(resolved.text.contains("不提供买卖建议或收益承诺"));
        fs::write(&overlay, "{").unwrap();
        let fallback = resolve_from_path("expert", &overlay);
        assert_eq!(fallback.version, BASE_PROMPT_VERSION);
        assert!(!fallback.text.contains("只根据已有证据描述不确定性"));
        let _ = fs::remove_file(&overlay);
        assert_eq!(
            resolve_from_path("expert", &overlay).version,
            BASE_PROMPT_VERSION
        );
        assert_eq!(
            resolve_from_path("quick", &overlay).version,
            BASE_PROMPT_VERSION
        );
    }

    #[test]
    fn draft_request_excludes_secrets_urls_and_tool_payloads() {
        let claim = UpgradeClaim {
            profile_id: "hot_money_early_v1".to_string(),
            mode: "expert".to_string(),
            prompt_version: BASE_PROMPT_VERSION.to_string(),
            current_body: HOT_MONEY_PROMPT.to_string(),
            required_terms: vec!["市场环境".to_string()],
            forbidden_terms: vec!["巴菲特".to_string()],
            samples: vec![DraftSample {
                question: sanitize_sample(
                    "问题 https://secret.example/v1?api_key=hidden sk-live-secret",
                    500,
                ),
                error: sanitize_sample("失败 bearer secret-token", 300),
            }],
        };
        let request = draft_request(&claim).to_string();
        assert!(!request.contains("https://"));
        assert!(!request.contains("sk-"));
        assert!(!request.contains("api_key"));
        assert!(!request.contains("secret-token"));
        assert!(!request.contains("events"));
    }

    #[test]
    fn revert_consumes_existing_builtin_rejections() {
        let dir = TempDir::new();
        let ledger = dir.path().join("agent-runs.sqlite");
        let overlay = dir.path().join("prompt-overlays.json");
        seed(
            &ledger,
            5,
            "policy_rejected",
            "value_compounder_v1",
            BASE_PROMPT_VERSION,
            "模型输出未通过安全校验",
        );
        assert!(claim_at(&ledger, &overlay, "value_compounder_v1")
            .unwrap()
            .is_some());
        let body = format!("{VALUE_COMPOUNDER_PROMPT}\n\n补充：估值只展示假设。");
        assert!(activate_at(&overlay, "value_compounder_v1", &body).unwrap());
        revert_at(&ledger, &overlay, "value_compounder_v1").unwrap();
        assert!(claim_at(&ledger, &overlay, "value_compounder_v1")
            .unwrap()
            .is_none());
        assert!(status_at(&overlay).unwrap().into_iter().any(|status| {
            status.profile_id == "value_compounder_v1" && status.builtin
        }));
    }

    fn resolve_from_path(mode: &str, overlay: &Path) -> ResolvedPrompt {
        if !is_upgradeable_mode(mode) {
            return compose(mode, builtin_body(mode), BASE_PROMPT_VERSION);
        }
        let profile_id = profile_id_for_mode(mode);
        let store = load_store(overlay);
        if let Some(active) = store
            .profiles
            .get(profile_id)
            .and_then(|profile| profile.active.as_ref())
        {
            if evaluate_candidate(profile_id, &active.body).is_ok() {
                return compose(mode, &active.body, &active.version);
            }
        }
        compose(mode, builtin_body(mode), BASE_PROMPT_VERSION)
    }
}