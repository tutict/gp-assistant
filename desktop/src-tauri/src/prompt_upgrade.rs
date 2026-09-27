use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::{agent_harness, agent_ledger};

pub(crate) const BASE_PROMPT_VERSION: &str = "rig-agent-runtime-v1";
const MAX_METHOD_CARD_CHARS: usize = 4_000;
const MAX_SYSTEM_PROMPT_CHARS: usize = 12_000;

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
pub(crate) struct ResolvedMethodCard {
    pub body: String,
    pub version: String,
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

fn method_card_digest(body: &str) -> String {
    Sha256::digest(body.trim().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn builtin_method_card_version(profile_id: &str) -> Result<String, String> {
    let contract = profile_contract(profile_id)?;
    let body = builtin_body(contract.mode);
    Ok(format!(
        "{BASE_PROMPT_VERSION}+{profile_id}+sha256-{}",
        method_card_digest(body)
    ))
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

pub(crate) fn resolve_method_card(
    profile_id: &str,
    app: Option<&AppHandle>,
) -> Result<ResolvedMethodCard, String> {
    let contract = profile_contract(profile_id)?;
    let active = app.and_then(|app| read_active_prompt(app, profile_id));
    let active = active.filter(|prompt| evaluate_candidate(profile_id, &prompt.body).is_ok());
    Ok(ResolvedMethodCard {
        body: active
            .as_ref()
            .map(|prompt| prompt.body.clone())
            .unwrap_or_else(|| builtin_body(contract.mode).to_string()),
        version: active
            .map(|prompt| prompt.version)
            .map(Ok)
            .unwrap_or_else(|| builtin_method_card_version(profile_id))?,
    })
}


pub(crate) fn activate_gepa_with_app(
    app: &AppHandle,
    profile_id: &str,
    expected_base_version: &str,
    body: &str,
) -> Result<String, String> {
    let _guard = OVERLAY_LOCK
        .lock()
        .map_err(|_| "prompt overlay lock is poisoned".to_string())?;
    activate_gepa_at(&overlay_path(app)?, profile_id, expected_base_version, body)
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

fn activate_gepa_at(
    overlay_path: &Path,
    profile_id: &str,
    expected_base_version: &str,
    body: &str,
) -> Result<String, String> {
    profile_contract(profile_id)?;
    evaluate_candidate(profile_id, body)?;
    let mut store = load_store(overlay_path);
    let profile = store.profiles.entry(profile_id.to_string()).or_default();
    let current_version = profile
        .active
        .as_ref()
        .filter(|active| evaluate_candidate(profile_id, &active.body).is_ok())
        .map(|active| active.version.clone())
        .unwrap_or(builtin_method_card_version(profile_id)?);
    if current_version != expected_base_version {
        return Err("active prompt version changed; rerun GEPA before applying".to_string());
    }
    let n = profile.next_n.max(1);
    let version = format!("{BASE_PROMPT_VERSION}+{profile_id}+{n}");
    profile.active = Some(ActivePrompt {
        n,
        version: version.clone(),
        body: body.trim().to_string(),
    });
    profile.next_n = n.saturating_add(1);
    profile.consumed_prompt_version = version.clone();
    profile.consumed_run_ids.clear();
    save_store(overlay_path, &store)?;
    Ok(version)
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
                    .unwrap_or_else(|| builtin_method_card_version(profile_id).unwrap_or_else(|_| BASE_PROMPT_VERSION.to_string())),
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

    #[test]
    fn builtin_method_cards_pass_the_contract() {
        evaluate_candidate("hot_money_early_v1", HOT_MONEY_PROMPT).expect("expert card");
        evaluate_candidate("value_compounder_v1", VALUE_COMPOUNDER_PROMPT).expect("research card");
    }

    #[test]
    fn unsafe_or_incomplete_cards_do_not_activate() {
        let dir = TempDir::new();
        let overlay = dir.path().join("prompt-overlays.json");
        let missing = HOT_MONEY_PROMPT.replace("市场环境", "环境");
        assert!(evaluate_candidate("hot_money_early_v1", &missing).is_err());
        let crossed = format!("{HOT_MONEY_PROMPT}\n巴菲特");
        assert!(evaluate_candidate("hot_money_early_v1", &crossed).is_err());
        let trading = format!("{HOT_MONEY_PROMPT}\n建议买入");
        assert!(evaluate_candidate("hot_money_early_v1", &trading).is_err());
        let oversized = format!("{HOT_MONEY_PROMPT}\n{}", "补充".repeat(4_001));
        assert!(evaluate_candidate("hot_money_early_v1", &oversized).is_err());
        assert!(status_at(&overlay).unwrap().iter().all(|status| status.builtin));
    }

    #[test]
    fn gepa_activation_requires_the_unchanged_base_version() {
        let dir = TempDir::new();
        let overlay = dir.path().join("prompt-overlays.json");
        let candidate = format!("{HOT_MONEY_PROMPT}\n\n补充：只描述已核验事实。 ");
        activate_gepa_at(&overlay, "hot_money_early_v1", &builtin_method_card_version("hot_money_early_v1").unwrap(), &candidate)
            .expect("first explicit activation");

        let store_after_first = load_store(&overlay);
        let active = store_after_first
            .profiles
            .get("hot_money_early_v1")
            .and_then(|profile| profile.active.as_ref())
            .expect("activated candidate");
        let first_version = active.version.clone();
        let first_body = active.body.clone();
        let second_candidate = format!("{HOT_MONEY_PROMPT}\n\n补充：进一步标记不确定性。 ");

        assert!(activate_gepa_at(
            &overlay,
            "hot_money_early_v1",
            &builtin_method_card_version("hot_money_early_v1").unwrap(),
            &second_candidate
        )
        .is_err());
        let store_after_stale_attempt = load_store(&overlay);
        let active_after_stale_attempt = store_after_stale_attempt
            .profiles
            .get("hot_money_early_v1")
            .and_then(|profile| profile.active.as_ref())
            .expect("first candidate remains active");
        assert_eq!(active_after_stale_attempt.version, first_version);
        assert_eq!(active_after_stale_attempt.body, first_body);
    }

    #[test]
    fn accepted_card_changes_prompt_until_overlay_is_removed_or_corrupt() {
        let dir = TempDir::new();
        let overlay = dir.path().join("prompt-overlays.json");
        let body = format!("{HOT_MONEY_PROMPT}\n\n补充：只根据已有证据描述不确定性。");
        assert!(activate_gepa_at(&overlay, "hot_money_early_v1", &builtin_method_card_version("hot_money_early_v1").unwrap(), &body).is_ok());
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
