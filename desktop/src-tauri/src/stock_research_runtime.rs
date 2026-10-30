use chrono::NaiveDate;
use serde_json::{json, Value};
use std::path::Path;
use stock_optimizer_core as gp_core;
use tauri::Manager;

fn registry_exists(root: &Path) -> bool {
    root.join("stock-research")
        .join("research.sqlite")
        .is_file()
}

fn optional_text<'a>(payload: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    payload
        .get(key)
        .map(|value| {
            value
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| format!("研究元数据 {key} 必须是非空字符串"))
        })
        .transpose()
}

fn validate_metadata(payload: &Value) -> Result<(), String> {
    if payload
        .get("scoring_parameters")
        .is_some_and(|value| !value.is_null())
    {
        return Err("普通研究请求只能选择已验证的参数版本，不能直接提交权重对象".into());
    }
    if let Some(version) = payload.get("metric_contract_version") {
        if version.as_u64() != Some(gp_core::METRIC_CONTRACT_VERSION as u64) {
            return Err("指标单位契约版本不匹配".into());
        }
    }
    if let Some(revision) = payload.get("scoring_revision") {
        if revision.as_str() != Some(gp_core::SCORING_REVISION) {
            return Err("评分修订不匹配，请重新选股".into());
        }
    }
    if let Some(fingerprint) = payload.get("implementation_fingerprint") {
        if fingerprint.as_str() != Some(gp_core::scoring_implementation_fingerprint()) {
            return Err("选股实现已更新，请重新选股后回测".into());
        }
    }
    for key in [
        "algorithm_version",
        "parameter_set_id",
        "dataset_id",
        "data_fingerprint",
        "research_fingerprint",
        "parameter_fingerprint",
    ] {
        optional_text(payload, key)?;
    }
    if optional_text(payload, "research_fingerprint")?
        .is_some_and(|value| value != gp_research::research_implementation_fingerprint())
    {
        return Err("研究实现已更新，请重新选股后回测".into());
    }
    Ok(())
}

fn requested_parameters(
    root: &Path,
    payload: &Value,
) -> Result<gp_research::ResolvedParameters, String> {
    validate_metadata(payload)?;
    let algorithm = optional_text(payload, "algorithm_version")?;
    let id = optional_text(payload, "parameter_set_id")?;
    if algorithm.is_some_and(|algorithm| {
        algorithm != "adaptive_swing_v1" && algorithm != "adaptive_swing_v2"
    }) {
        return Err("未知自适应算法版本".into());
    }
    if algorithm == Some("adaptive_swing_v1") || id == Some("fixed-v1") {
        if algorithm == Some("adaptive_swing_v2") || id.is_some_and(|id| id != "fixed-v1") {
            return Err("算法与参数版本不匹配".into());
        }
        return Ok(gp_research::ResolvedParameters {
            parameters: gp_core::fixed_scoring_parameters(),
            dataset_id: None,
        });
    }
    if algorithm == Some("adaptive_swing_v2") && id.is_none() {
        return Err("校准策略需要明确的已冻结参数版本".into());
    }
    let resolved = if registry_exists(root) {
        gp_research::ResearchStore::open_readonly(root)
            .map_err(|error| error.to_string())?
            .resolve_parameters(id)
            .map_err(|error| error.to_string())?
    } else if id.is_none() {
        gp_research::ResolvedParameters {
            parameters: gp_core::fixed_scoring_parameters(),
            dataset_id: None,
        }
    } else {
        return Err("未知或尚未验证的参数版本".into());
    };
    if algorithm.is_some_and(|algorithm| algorithm != resolved.parameters.algorithm_version) {
        return Err("算法与参数版本不匹配".into());
    }
    Ok(resolved)
}

fn requested_dataset(
    payload: &Value,
    resolved: &gp_research::ResolvedParameters,
) -> Result<Option<String>, String> {
    let explicit = optional_text(payload, "dataset_id")?;
    if resolved.parameters.algorithm_version == "adaptive_swing_v2" {
        if let Some(id) = explicit {
            if resolved.dataset_id.as_deref() != Some(id) {
                return Err("数据集与已验证参数的冻结证据不匹配".into());
            }
        }
    }
    Ok(explicit
        .map(str::to_string)
        .or_else(|| resolved.dataset_id.clone()))
}

fn compact_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .map_err(|_| format!("无效的研究日期：{value}"))
}

pub(crate) fn status_at(root: &Path) -> Result<Value, String> {
    if registry_exists(root) {
        let store =
            gp_research::ResearchStore::open_readonly(root).map_err(|error| error.to_string())?;
        let datasets = store.datasets().map_err(|error| error.to_string())?;
        let mut runs = store.reports().map_err(|error| error.to_string())?;
        runs.truncate(25);
        let active = store
            .resolve_parameters(None)
            .map_err(|error| error.to_string())?;
        return Ok(
            json!({"available":true,"app_data_dir":root,"datasets":datasets,"runs":runs,
            "active_parameter_set":active.parameters,"active_dataset_id":active.dataset_id}),
        );
    }
    Ok(json!({"available": false, "app_data_dir": root,
        "datasets": [], "runs": [], "active_parameter_set": gp_core::fixed_scoring_parameters()}))
}

pub(crate) fn report_at(root: &Path, run_id: &str) -> Result<Value, String> {
    if run_id.trim().is_empty() || run_id.len() > 200 {
        return Err("研究报告标识无效".to_string());
    }
    let store =
        gp_research::ResearchStore::open_readonly(root).map_err(|error| error.to_string())?;
    let mut report = store.report(run_id).map_err(|error| error.to_string())?;
    let object = report
        .as_object_mut()
        .ok_or_else(|| "研究报告格式无效".to_string())?;
    let verified = store
        .qualification_verified(run_id)
        .map_err(|error| error.to_string())?;
    object.insert("qualification_verified".into(), json!(verified));
    Ok(report)
}

pub(crate) async fn screen_if_selected(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Option<Value>, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    crate::runtime::run_cpu_bound("pit_screen", move || screen_at(&root, &payload)).await?
}

fn screen_at(root: &Path, payload: &Value) -> Result<Option<Value>, String> {
    let resolved = requested_parameters(root, payload)?;
    let Some(dataset_id) = requested_dataset(payload, &resolved)? else {
        return Ok(None);
    };
    let store =
        gp_research::ResearchStore::open_readonly(root).map_err(|error| error.to_string())?;
    let dataset = store
        .dataset(&dataset_id)
        .map_err(|error| error.to_string())?;
    let mut request: gp_core::AdaptiveScreenRequest =
        serde_json::from_value(payload.clone()).map_err(|error| error.to_string())?;
    let date = match request.as_of_date.as_deref() {
        Some(date) => compact_date(date)?,
        None => *dataset
            .calendar()
            .last()
            .ok_or_else(|| "PIT 数据集没有交易日".to_string())?,
    };
    request.as_of_date = Some(date.format("%Y%m%d").to_string());
    let screened = dataset
        .screen_on(date, &request, &resolved.parameters)
        .map_err(|error| error.to_string())?;
    let mut value = serde_json::to_value(screened).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "选股结果格式无效".to_string())?;
    object.insert("dataset_id".into(), json!(dataset_id));
    object.insert(
        "data_fingerprint".into(),
        json!(dataset.record().data_fingerprint),
    );
    object.insert(
        "research_fingerprint".into(),
        json!(gp_research::research_implementation_fingerprint()),
    );
    object.insert("real_data".into(), json!(dataset.record().real_data));
    let notes = object.entry("notes").or_insert_with(|| json!([]));
    if let Some(notes) = notes.as_array_mut() {
        notes.push(json!(format!(
            "使用冻结 PIT 数据，截至 {}；新增交易日需要补齐 PIT 数据。",
            date
        )));
        if !dataset.record().real_data {
            notes.push(json!("合成样例仅用于软件验证，不提供真实收益证据。"));
        }
    }
    Ok(Some(value))
}

pub(crate) async fn backtest_if_selected(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Option<Value>, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    crate::runtime::run_cpu_bound("pit_backtest", move || backtest_at(&root, &payload)).await?
}

fn backtest_at(root: &Path, payload: &Value) -> Result<Option<Value>, String> {
    let mut identity = payload.clone();
    if !identity.is_object() {
        return Err("回测请求必须是对象".into());
    }
    if let Some(spec) = payload.get("adaptive_screen_spec") {
        if !spec.is_object() {
            return Err("冻结选股请求格式无效".into());
        }
        validate_metadata(spec)?;
        for key in [
            "algorithm_version",
            "parameter_set_id",
            "scoring_parameters",
            "metric_contract_version",
            "scoring_revision",
            "implementation_fingerprint",
            "dataset_id",
            "data_fingerprint",
            "research_fingerprint",
            "parameter_fingerprint",
        ] {
            if let Some(value) = spec.get(key) {
                if identity.get(key).is_some_and(|outer| outer != value) {
                    return Err(format!("回测与冻结选股元数据 {key} 不匹配"));
                }
                identity[key] = value.clone();
            }
        }
    }
    validate_metadata(&identity)?;
    let strategy = optional_text(&identity, "strategy_mode")?
        .unwrap_or("candidate_snapshot")
        .trim()
        .to_ascii_lowercase();
    identity["strategy_mode"] = json!(strategy);
    let (version, mode) = strategy.split_once(':').unwrap_or((&strategy, "auto"));
    let adaptive = version.starts_with("adaptive_swing_v");
    if !adaptive && payload.get("adaptive_screen_spec").is_some() {
        return Err("冻结选股请求需要明确的自适应策略版本与模式".into());
    }
    if adaptive {
        if !matches!(version, "adaptive_swing_v1" | "adaptive_swing_v2") {
            return Err("未知自适应策略版本".into());
        }
        if !matches!(mode, "auto" | "range" | "trend" | "defensive") {
            return Err("未知自适应策略模式".into());
        }
        if optional_text(&identity, "algorithm_version")?
            .is_some_and(|algorithm| algorithm != version)
        {
            return Err("算法与策略版本不匹配".into());
        }
        identity["algorithm_version"] = json!(version);
        if let Some(spec_mode) = payload.pointer("/adaptive_screen_spec/mode") {
            if spec_mode
                .as_str()
                .map(|value| value.trim().to_ascii_lowercase())
                .as_deref()
                != Some(mode)
            {
                return Err("回测模式与冻结选股模式不匹配".into());
            }
        }
    } else if matches!(version, "candidate_snapshot" | "snapshot")
        && (identity.get("algorithm_version").is_some()
            || identity.get("parameter_set_id").is_some()
            || identity.get("dataset_id").is_some())
    {
        return Err("算法与候选快照策略不匹配".into());
    }
    if !adaptive
        && identity.get("algorithm_version").is_none()
        && (identity.get("parameter_set_id").is_some() || identity.get("dataset_id").is_some())
    {
        return Err("冻结参数或 PIT 数据集需要明确的自适应算法版本".into());
    }
    let selected = adaptive
        || ["algorithm_version", "parameter_set_id", "dataset_id"]
            .iter()
            .any(|key| identity.get(key).is_some());
    if !selected {
        if identity.get("data_fingerprint").is_some()
            || identity.get("parameter_fingerprint").is_some()
        {
            return Err("冻结指纹缺少数据集或参数身份".into());
        }
        return Ok(None);
    }
    let resolved = requested_parameters(root, &identity)?;
    if optional_text(&identity, "parameter_fingerprint")?.is_some_and(|fingerprint| {
        use sha2::Digest;
        let bytes =
            serde_json::to_vec(&resolved.parameters).expect("serializable scoring parameters");
        let digest = sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        fingerprint != digest
    }) {
        return Err("冻结参数指纹不匹配".into());
    }
    let Some(dataset_id) = requested_dataset(&identity, &resolved)? else {
        if identity.get("data_fingerprint").is_some() {
            return Err("冻结数据指纹缺少 PIT 数据集".into());
        }
        return Ok(None);
    };
    let request: gp_core::BacktestRequest =
        serde_json::from_value(identity.clone()).map_err(|error| error.to_string())?;
    if request.benchmark != "candidate_equal_weight" {
        return Err("严格 PIT 回测需要当期候选池等权基准；无基准历史组合请使用候选快照。".into());
    }
    if request.source != "criteria" || !request.stock_codes.is_empty() {
        return Err("PIT 自适应回测使用条件候选池；固定自选标的请使用候选快照。".into());
    }
    let store =
        gp_research::ResearchStore::open_readonly(root).map_err(|error| error.to_string())?;
    let dataset = store
        .dataset(&dataset_id)
        .map_err(|error| error.to_string())?;
    if optional_text(&identity, "data_fingerprint")?
        .is_some_and(|fingerprint| fingerprint != dataset.record().data_fingerprint)
    {
        return Err("冻结数据指纹不匹配".into());
    }
    let config = gp_research::EvaluationConfig {
        start_date: compact_date(&request.start_date)?,
        end_date: compact_date(&request.end_date)?,
        initial_cash: request.initial_cash,
        transaction_cost_bps: request.transaction_cost_bps,
        top_n: request.top_n,
        mode: mode.to_string(),
        rebalance_frequency: request.rebalance_frequency,
        criteria: request.criteria,
        ..gp_research::EvaluationConfig::default()
    };
    let report = gp_research::evaluate(&dataset, &resolved.parameters, &config)
        .map_err(|error| error.to_string())?;
    Ok(Some(backtest_view(&report)?))
}

fn backtest_view(report: &gp_research::EvaluationReport) -> Result<Value, String> {
    let symbols = report
        .monthly_folds
        .iter()
        .flat_map(|fold| fold.selected_symbols.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let complete = report
        .monthly_folds
        .iter()
        .filter(|fold| fold.complete)
        .count();
    let folds = report.monthly_folds.iter().map(|fold| {
        let evaluated = if fold.precision_at_n.is_some() { fold.selected_symbols.len() } else { 0 };
        let hits = fold.precision_at_n.map(|precision| (precision * evaluated as f64).round() as usize).unwrap_or(0);
        json!({"signal_date":fold.signal_date,"selection_date":fold.selection_date,"evaluation_end_date":fold.evaluation_end_date,
            "selected_symbols":fold.selected_symbols,"eligible_symbol_count":fold.eligible_symbol_count,
            "evaluated_selection_count":evaluated,"hit_count":hits,"precision_at_n":fold.precision_at_n,
            "average_forward_return":null,"benchmark_forward_return":null,"average_excess_return":null})
    }).collect::<Vec<_>>();
    let benchmark_symbols = report
        .exposures
        .iter()
        .filter(|entry| entry.benchmark)
        .map(|entry| entry.code.clone())
        .collect::<std::collections::BTreeSet<_>>();
    Ok(json!({
        "metrics": {"num_stocks":symbols.len(),"total_return":report.metrics.total_return,
            "annualized_return":report.metrics.annualized_return,"max_drawdown":report.metrics.max_drawdown.map(|value| -value.abs()),
            "annualized_turnover":report.metrics.annualized_turnover,"total_transaction_cost":report.metrics.total_transaction_cost,
            "total_turnover":report.metrics.annualized_turnover * report.equity_curve.len() as f64 / 252.0,
            "benchmark_total_return":report.benchmark_metrics.total_return,"benchmark_annualized_return":report.benchmark_metrics.annualized_return,
            "benchmark_max_drawdown":report.benchmark_metrics.max_drawdown.map(|value| -value.abs()),
            "excess_return":report.metrics.total_return-report.benchmark_metrics.total_return,
            "rebalance_count":report.monthly_folds.len(),"oos_fold_count":complete,"strategy_mode":report.parameters.algorithm_version},
        "equity_curve":report.equity_curve,"benchmark_curve":report.benchmark_curve,"symbols":symbols,"benchmark_symbols":benchmark_symbols,
        "rebalance_dates":report.monthly_folds.iter().map(|fold| fold.selection_date).collect::<Vec<_>>(),"walk_forward_folds":folds,
        "volatility_snapshots":[],"volatility_message":"PIT 回测保留实际交易与公司行为记录；波动率快照请通过个股观察查看。",
        "strategy_mode":report.parameters.algorithm_version,"notes":report.notes,
        "algorithm_version":report.parameters.algorithm_version,"parameter_set_id":report.parameters.parameter_set_id,
        "scoring_revision":gp_core::SCORING_REVISION,"metric_contract_version":gp_core::METRIC_CONTRACT_VERSION,
        "implementation_fingerprint":report.implementation_fingerprint,"dataset_id":report.dataset_id,
        "data_fingerprint":report.data_fingerprint,"research_fingerprint":report.research_fingerprint,"pit_evaluation_report":report
    }))
}

#[tauri::command]
pub(crate) async fn api_stock_research_status(app: tauri::AppHandle) -> Result<Value, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    crate::runtime::run_io_bound("stock_research_status", move || status_at(&root)).await?
}

#[tauri::command]
pub(crate) async fn api_stock_research_report(
    app: tauri::AppHandle,
    payload: Value,
) -> Result<Value, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let run_id = payload
        .get("run_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    crate::runtime::run_io_bound("stock_research_report", move || report_at(&root, &run_id)).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn repair_backtest_authority_precedes_legacy_fallback() {
        let root = TestRoot::new();
        let mut raw_v2 = gp_core::fixed_scoring_parameters();
        raw_v2.algorithm_version = "adaptive_swing_v2".into();
        raw_v2.parameter_set_id = "never-registered".into();
        for strategy in [
            "walk_forward",
            "candidate_snapshot",
            " ADAPTIVE_SWING_V2:range ",
            "adaptive_swing_v2:range",
        ] {
            for metadata in [
                json!({"scoring_parameters":gp_core::fixed_scoring_parameters()}),
                json!({"algorithm_version":"adaptive_swing_v2","parameter_set_id":"never-registered","scoring_parameters":raw_v2}),
                json!({"algorithm_version":"adaptive_swing_v2","parameter_set_id":"never-registered"}),
                json!({"algorithm_version":"adaptive_swing_v999"}),
                json!({"parameter_set_id":42}),
                json!({"dataset_id":"pit-without-algorithm"}),
                json!({"metric_contract_version":1}),
                json!({"implementation_fingerprint":"obsolete"}),
                json!({"adaptive_screen_spec":{"scoring_parameters":{}}}),
                json!({"adaptive_screen_spec":{"implementation_fingerprint":"obsolete"}}),
            ] {
                let mut payload = metadata;
                payload["strategy_mode"] = json!(strategy);
                assert!(
                    backtest_at(&root.0, &payload).is_err(),
                    "accepted {payload}"
                );
            }
        }
        assert!(!root.0.exists());
    }

    #[test]
    fn repair_backtest_rejects_exact_strategy_and_nested_identity_conflicts() {
        let root = TestRoot::new();
        for strategy in [
            "adaptive_swing_v2:defensive",
            "adaptive_swing_v999:defensive",
            "adaptive_swing_v1:unknown",
        ] {
            let payload = json!({"strategy_mode":strategy,"algorithm_version":"adaptive_swing_v1","parameter_set_id":"fixed-v1"});
            assert!(
                backtest_at(&root.0, &payload).is_err(),
                "accepted {payload}"
            );
        }
        let payload = json!({"strategy_mode":"adaptive_swing_v1:defensive","algorithm_version":"adaptive_swing_v1","parameter_set_id":"fixed-v1",
            "adaptive_screen_spec":{"algorithm_version":"adaptive_swing_v2","parameter_set_id":"never-registered"}});
        assert!(backtest_at(&root.0, &payload).is_err());
        for strategy in ["candidate_snapshot", "walk_forward"] {
            assert!(backtest_at(&root.0, &json!({"strategy_mode":strategy}))
                .unwrap()
                .is_none());
        }
        assert!(backtest_at(&root.0, &json!({"strategy_mode":" ADAPTIVE_SWING_V1:defensive ","algorithm_version":"adaptive_swing_v1","parameter_set_id":"fixed-v1"})).unwrap().is_none());
        assert!(!root.0.exists());
    }

    struct TestRoot(std::path::PathBuf);
    impl TestRoot {
        fn new() -> Self {
            let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("research-runtime-tests")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            Self(root)
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("research-runtime-tests");
            if self.0.starts_with(base) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn absent_registry_status_never_creates_files_or_a_database() {
        let root = TestRoot::new();
        assert!(!root.0.exists());
        let status = status_at(&root.0).unwrap();
        assert_eq!(status["available"], false);
        assert_eq!(
            status["active_parameter_set"]["parameter_set_id"],
            "fixed-v1"
        );
        assert!(!root.0.exists());
        assert!(report_at(&root.0, "not-found").is_err());
        assert!(!root.0.exists());
    }

    #[test]
    fn unknown_or_unqualified_parameters_fail_before_data_access() {
        let root = TestRoot::new();
        assert!(
            requested_parameters(&root.0, &json!({"algorithm_version":"adaptive_swing_v2"}))
                .is_err()
        );
        assert!(requested_parameters(&root.0, &json!({"parameter_set_id":"unknown"})).is_err());
        assert!(requested_parameters(
            &root.0,
            &json!({"algorithm_version":"adaptive_swing_v1","parameter_set_id":"unknown"})
        )
        .is_err());
        assert!(
            requested_parameters(&root.0, &json!({"implementation_fingerprint":"obsolete"}))
                .is_err()
        );
        assert!(requested_parameters(&root.0, &json!({"scoring_parameters":{}})).is_err());
        assert!(!root.0.exists());
    }

    #[test]
    fn status_and_reports_read_committed_cli_outputs_without_new_writes() {
        let root = TestRoot::new();
        let writer = gp_research::ResearchStore::open(&root.0).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../native/gp-research/tests/fixtures/tiny");
        let dataset = writer.import_bundle(&fixture).unwrap();
        let report = json!({"run_id":"native-readonly-report", "dataset_id":dataset.dataset_id, "parameters":gp_core::fixed_scoring_parameters(),
            "data_quality":{"passed":true,"real_data":false}, "gate":{"passed":true,"checks":[]}, "metrics":{"total_return":0.1}});
        writer.save_report("evaluation", &report).unwrap();
        let status = status_at(&root.0).unwrap();
        assert_eq!(status["datasets"].as_array().unwrap().len(), 1);
        assert_eq!(status["datasets"][0]["real_data"], false);
        let view = report_at(&root.0, "native-readonly-report").unwrap();
        assert_eq!(view["run_id"], "native-readonly-report");
        assert_eq!(view["qualification_verified"], false);
        assert!(!root
            .0
            .join("stock-research")
            .join("parameters.json")
            .exists());
    }

    #[test]
    fn pit_screen_and_backtest_keep_dataset_identity_and_compatible_results() {
        let root = TestRoot::new();
        let writer = gp_research::ResearchStore::open(&root.0).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../native/gp-research/tests/fixtures/tiny");
        let record = writer.import_bundle(&fixture).unwrap();
        let dataset = writer.dataset(&record.dataset_id).unwrap();
        let last = *dataset.calendar().last().unwrap();
        let screened = screen_at(&root.0, &json!({"algorithm_version":"adaptive_swing_v1", "parameter_set_id":"fixed-v1",
            "dataset_id":record.dataset_id,"mode":"defensive","primary_limit":10,"exploration_limit":0,"as_of_date":last.to_string()})).unwrap().unwrap();
        assert_eq!(screened["dataset_id"], record.dataset_id);
        assert_eq!(screened["parameter_set_id"], "fixed-v1");
        assert_eq!(screened["real_data"], false);
        let start = dataset.calendar()[dataset.calendar().len() - 20];
        let mut payload = json!({"strategy_mode":"adaptive_swing_v1:defensive","algorithm_version":"adaptive_swing_v1","parameter_set_id":"fixed-v1",
            "dataset_id":record.dataset_id,"data_fingerprint":record.data_fingerprint,"research_fingerprint":gp_research::research_implementation_fingerprint(),
            "implementation_fingerprint":gp_core::scoring_implementation_fingerprint(),"source":"criteria","start_date":start.to_string(),"end_date":last.to_string(),
            "benchmark":"candidate_equal_weight"});
        for strategy in [
            "adaptive_swing_v1:defensive",
            " ADAPTIVE_SWING_V1:DEFENSIVE ",
        ] {
            payload["strategy_mode"] = json!(strategy);
            let actual = backtest_at(&root.0, &payload).unwrap().unwrap();
            assert_eq!(actual["algorithm_version"], "adaptive_swing_v1");
            assert_eq!(actual["dataset_id"], record.dataset_id);
        }
        payload["strategy_mode"] = json!("adaptive_swing_v1:defensive");
        payload["adaptive_screen_spec"] = json!({"mode":"defensive","algorithm_version":"adaptive_swing_v1","parameter_set_id":"fixed-v1",
            "dataset_id":record.dataset_id,"data_fingerprint":record.data_fingerprint,"implementation_fingerprint":gp_core::scoring_implementation_fingerprint()});
        assert!(backtest_at(&root.0, &payload).unwrap().is_some());
        for (pointer, value) in [
            ("/data_fingerprint", json!("wrong")),
            ("/research_fingerprint", json!("wrong")),
            (
                "/adaptive_screen_spec/implementation_fingerprint",
                json!("wrong"),
            ),
            ("/adaptive_screen_spec/dataset_id", json!("wrong")),
            ("/adaptive_screen_spec/mode", json!("trend")),
        ] {
            let mut invalid = payload.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(backtest_at(&root.0, &invalid).is_err());
        }
        let mut unqualified = gp_core::fixed_scoring_parameters();
        unqualified.algorithm_version = "adaptive_swing_v2".into();
        unqualified.parameter_set_id = "registered-unqualified".into();
        writer
            .register_parameters(&unqualified, &record.dataset_id)
            .unwrap();
        for strategy in ["walk_forward", " ADAPTIVE_SWING_V2:DEFENSIVE "] {
            assert!(backtest_at(&root.0, &json!({"strategy_mode":strategy,"algorithm_version":"adaptive_swing_v2","parameter_set_id":unqualified.parameter_set_id})).is_err());
        }
        let report = gp_research::evaluate(
            &dataset,
            &gp_core::fixed_scoring_parameters(),
            &gp_research::EvaluationConfig {
                start_date: start,
                end_date: last,
                mode: "defensive".to_string(),
                ..gp_research::EvaluationConfig::default()
            },
        )
        .unwrap();
        let view = backtest_view(&report).unwrap();
        let typed: gp_core::BacktestResult = serde_json::from_value(view.clone()).unwrap();
        assert_eq!(view["dataset_id"], record.dataset_id);
        assert_eq!(view["parameter_set_id"], "fixed-v1");
        assert!(typed.metrics.total_return.is_finite());
        assert!(!typed.symbols.is_empty());
        assert_eq!(typed.equity_curve.len(), report.equity_curve.len());
        assert_eq!(
            view["pit_evaluation_report"]["data_quality"]["real_data"],
            false
        );
        let error=backtest_at(&root.0,&json!({"strategy_mode":"adaptive_swing_v1:defensive","algorithm_version":"adaptive_swing_v1",
            "dataset_id":record.dataset_id,"source":"criteria","start_date":start.to_string(),"end_date":last.to_string(),"benchmark":"none"})).unwrap_err();
        assert!(error.contains("基准"));
    }
}
