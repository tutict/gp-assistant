use super::*;

#[test]
fn adaptive_screen_request_accepts_nested_and_legacy_contracts() {
    let nested = adaptive_screen_request_from_payload(json!({
        "criteria": { "min_roe": 8.0, "limit": 10 },
        "mode": "range",
        "horizon": "swing_10_30d",
        "primary_limit": 8,
        "exploration_limit": 6,
        "run_id": "run-1"
    }))
    .expect("nested adaptive request should parse");
    assert_eq!(nested.mode, "range");
    assert_eq!(nested.primary_limit, 8);
    assert_eq!(nested.criteria.min_roe, Some(8.0));

    let legacy = adaptive_screen_request_from_payload(json!({
        "min_roe": 6.0,
        "limit": 10,
        "score_profile": "balanced"
    }))
    .expect("legacy flat request should map to adaptive auto mode");
    assert_eq!(legacy.mode, "auto");
    assert_eq!(legacy.horizon, "swing_10_30d");
    assert_eq!(legacy.criteria.min_roe, Some(6.0));
}

#[test]
fn legacy_adapter_uses_primary_limit_for_nested_requests_and_preserves_flat_limit() {
    let nested = legacy_screen_criteria_from_payload(json!({
        "criteria": { "limit": 80, "min_roe": 8.0 },
        "primary_limit": 10,
        "exploration_limit": 10
    }))
    .expect("nested request should map to legacy criteria");
    assert_eq!(nested.limit, 10);
    assert_eq!(nested.min_roe, Some(8.0));

    let flat = legacy_screen_criteria_from_payload(json!({ "limit": 7, "min_roe": 6.0 }))
        .expect("flat legacy request should preserve its own limit");
    assert_eq!(flat.limit, 7);
}

#[test]
fn adaptive_exposure_sqlite_deduplicates_same_day_and_keeps_five_trade_dates() {
    let mut connection = Connection::open_in_memory().expect("in-memory sqlite should open");
    initialize_adaptive_exposure_db(&connection).expect("schema should initialize");
    for day in 1..=6 {
        let date = format!("202607{day:02}");
        let result = json!({
            "algorithm_version": "adaptive_swing_v1",
            "market_regime": { "effective": "range" },
            "groups": [
                {
                    "key": "primary",
                    "items": [{ "stock": { "code": format!("60000{day}.SH") } }]
                }
            ]
        });
        adaptive_exposure_record_rows(&mut connection, &result, &date)
            .expect("exposure should persist");
        adaptive_exposure_record_rows(&mut connection, &result, &date)
            .expect("same-day rerun should upsert");
    }
    let rows = adaptive_exposure_recent_rows(&connection, None).expect("recent rows should load");
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| row.trade_date.as_str() >= "20260702"));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM adaptive_screen_exposure", [], |row| {
            row.get(0)
        })
        .expect("count should query");
    assert_eq!(count, 6);

    let prior_rows = adaptive_exposure_recent_rows(&connection, Some("20260706"))
        .expect("same-day rows should be excluded from novelty history");
    assert_eq!(prior_rows.len(), 5);
    assert!(prior_rows.iter().all(|row| row.trade_date != "20260706"));

    let historical_rows = adaptive_exposure_recent_rows(&connection, Some("20260704"))
        .expect("historical novelty history should use only prior trade dates");
    assert_eq!(historical_rows.len(), 3);
    assert!(
        historical_rows
            .iter()
            .all(|row| row.trade_date.as_str() < "20260704"),
        "future exposure must not affect a historical as-of rerun"
    );
}

#[test]
fn adaptive_cache_requires_sixty_rows_and_the_target_trade_date() {
    let rows = (0..60)
        .map(|index| gp_core::HistoryBar {
            date: format!("{:08}", 20_260_101 + index),
            open: Some(10.0),
            high: Some(10.2),
            low: Some(9.8),
            close: 10.0,
            volume: Some(1_000.0),
            capital: None,
        })
        .collect::<Vec<_>>();
    let data = gp_core::CoreDataSet {
        histories: HashMap::from([("600000.SH".to_string(), rows)]),
        ..gp_core::CoreDataSet::default()
    };
    assert!(adaptive_history_cache_is_usable(
        &data,
        "600000.SH",
        Some("20260160"),
        60,
    ));
    assert!(!adaptive_history_cache_is_usable(
        &data,
        "600000.SH",
        Some("20260161"),
        60,
    ));
    assert!(!adaptive_history_cache_is_usable(
        &data,
        "000001.SZ",
        Some("20260160"),
        60,
    ));
}

#[test]
fn adaptive_runtime_limits_match_the_release_contract() {
    assert_eq!(ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT, 80);
    assert_eq!(ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS, 20);
}

#[test]
fn adaptive_operational_evidence_requires_the_default_release_screen_spec() {
    let mut request = gp_core::AdaptiveScreenRequest::default();
    assert!(adaptive_release_screen_request_qualified(&request));

    request.mode = "range".to_string();
    assert!(!adaptive_release_screen_request_qualified(&request));
    request.mode = "auto".to_string();
    request.criteria.max_pe = Some(20.0);
    assert!(!adaptive_release_screen_request_qualified(&request));
    request.criteria.max_pe = None;
    request.primary_limit = 9;
    assert!(!adaptive_release_screen_request_qualified(&request));
    request.primary_limit = 10;
    request.exploration_limit = 9;
    assert!(!adaptive_release_screen_request_qualified(&request));
    request.exploration_limit = 10;
    request.horizon = "other".to_string();
    assert!(!adaptive_release_screen_request_qualified(&request));
}

#[test]
fn adaptive_prefetch_prepares_eighty_candidates_and_three_cached_benchmarks_offline() {
    let candidates = (1..=80)
        .map(|index| format!("{index:06}.SZ"))
        .collect::<Vec<_>>();
    let required = adaptive_required_history_codes(&candidates);
    assert_eq!(required.len(), 83);
    assert_eq!(&required[..80], candidates.as_slice());
    assert!(adaptive_benchmark_codes()
        .iter()
        .all(|code| required.contains(&code.to_string())));

    let rows = (0..60)
        .map(|index| gp_core::HistoryBar {
            date: format!("{:08}", 20_260_101 + index),
            open: Some(10.0),
            high: Some(10.2),
            low: Some(9.8),
            close: 10.0,
            volume: Some(1_000.0),
            capital: None,
        })
        .collect::<Vec<_>>();
    let mut data = gp_core::CoreDataSet {
        histories: required
            .iter()
            .map(|code| (code.clone(), rows.clone()))
            .collect(),
        ..gp_core::CoreDataSet::default()
    };
    assert!(
        adaptive_missing_history_codes(&data, &required, Some("20260160")).is_empty(),
        "complete local candidate and index histories must avoid network fetches"
    );

    data.histories.remove(&candidates[79]);
    assert_eq!(
        adaptive_missing_history_codes(&data, &required, Some("20260160")),
        vec![candidates[79].clone()]
    );
}

#[test]
fn adaptive_timeout_and_progress_payloads_enforce_the_runtime_contract() {
    let timed_out = tauri::async_runtime::block_on(adaptive_screen_with_timeout(
        Duration::from_millis(1),
        std::future::pending::<Result<(), String>>(),
    ))
    .expect_err("a pending adaptive run must respect the timeout wrapper");
    assert!(timed_out.contains("20"));

    let current =
        adaptive_screen_progress_payload(Some("run-current"), "history_fetch", 24, "补齐日线");
    let stale = adaptive_screen_progress_payload(Some("run-stale"), "complete", 100, "完成");
    assert_eq!(current["run_id"], "run-current");
    assert_eq!(current["stage"], "history_fetch");
    assert_eq!(stale["run_id"], "run-stale");
    assert_ne!(current["run_id"], stale["run_id"]);
}

#[test]
fn adaptive_release_gate_uses_persisted_report_and_real_run_evidence() {
    let connection = Connection::open_in_memory().expect("in-memory sqlite should open");
    initialize_adaptive_exposure_db(&connection).expect("schema should initialize");
    assert!(adaptive_release_gate_load_rows(&connection)
        .expect("empty gate query should work")
        .is_none());

    for run in 0..5 {
        let items = (0..4)
            .map(|index| json!({ "stock": { "code": format!("{:06}.SZ", run * 4 + index + 1) } }))
            .collect::<Vec<_>>();
        let result = json!({
            "algorithm_version": "adaptive_swing_v1",
            "groups": [{ "key": "primary", "items": items }]
        });
        adaptive_release_run_record_rows(
            &connection,
            Some(&format!("run-{run}")),
            &result,
            "20260729",
            if run == 0 { 19_000 } else { 1_900 },
            run != 0,
            true,
        )
        .expect("run evidence should persist");
    }
    connection
        .execute(
            "INSERT INTO adaptive_screen_runs_v3
               (run_id, implementation_fingerprint, release_evidence_qualified, trade_date, selected_codes_json, elapsed_millis, cache_hit, algorithm_version, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "old-implementation-run",
                "old-implementation-fingerprint",
                1_i64,
                "20260729",
                "[]",
                99_000_i64,
                1_i64,
                "adaptive_swing_v1",
                epoch_millis().min(i64::MAX as u128) as i64,
            ],
        )
        .expect("old implementation evidence should persist independently");
    adaptive_release_run_record_rows(
        &connection,
        Some("manual-mode-slow-run"),
        &json!({
            "algorithm_version": "adaptive_swing_v1",
            "groups": [{
                "key": "primary",
                "items": [{ "stock": { "code": "999999.SZ" } }]
            }]
        }),
        "20260729",
        99_000,
        true,
        false,
    )
    .expect("non-release screen runs may persist without affecting gate evidence");
    let evidence = adaptive_release_operational_evidence_rows(&connection)
        .expect("operational evidence should aggregate");
    assert_eq!(evidence, (Some(20), Some(19_000), Some(1_900)));

    let input = gp_core::AdaptiveReleaseGateInput {
        release_configuration_qualified: Some(true),
        legacy_annualized_return: Some(0.10),
        adaptive_annualized_return: Some(0.10),
        legacy_max_drawdown: Some(-0.12),
        adaptive_max_drawdown: Some(-0.12),
        legacy_precision_at_10: Some(0.55),
        adaptive_precision_at_10: Some(0.55),
        max_primary_industry_count: Some(3),
        average_adjacent_jaccard: Some(0.70),
        five_run_unique_coverage: evidence.0,
        first_run_millis: evidence.1,
        cached_run_millis: evidence.2,
    };
    let qualification = json!({
        "implementation_fingerprint": adaptive_release_implementation_fingerprint(),
        "qualified": true,
        "mode": "auto",
        "full_universe": true,
        "oos_fold_count": 60
    });
    let report = gp_core::evaluate_adaptive_release_gate(&input);
    assert!(report.passed);
    adaptive_release_gate_store_rows(&connection, &input, &report, &qualification)
        .expect("passing gate should persist");
    assert!(
        adaptive_release_gate_load_rows(&connection)
            .expect("stored gate should load")
            .expect("gate should exist")
            .passed
    );

    adaptive_release_run_record_rows(
        &connection,
        Some("run-slow-cache"),
        &json!({
            "algorithm_version": "adaptive_swing_v1",
            "groups": [{
                "key": "primary",
                "items": (1..=4)
                    .map(|code| json!({ "stock": { "code": format!("{code:06}.SZ") } }))
                    .collect::<Vec<_>>()
            }]
        }),
        "20260729",
        2_001,
        true,
        true,
    )
    .expect("later operational evidence should recompute the persisted gate");
    assert!(
        !adaptive_release_gate_load_rows(&connection)
            .expect("updated gate should load")
            .expect("gate should exist")
            .passed
    );

    adaptive_release_gate_store_rows(&connection, &input, &report, &qualification)
        .expect("passing gate should be restaged for expiry verification");
    let expired_at =
        (epoch_millis().min(i64::MAX as u128) as i64).saturating_sub(31 * 24 * 60 * 60 * 1_000);
    connection
        .execute(
            "UPDATE adaptive_screen_runs_v3
             SET recorded_at = ?1
             WHERE implementation_fingerprint = ?2",
            params![expired_at, adaptive_release_implementation_fingerprint()],
        )
        .expect("current implementation evidence should expire");
    assert!(
        !adaptive_release_gate_refresh_and_load_rows(&connection)
            .expect("route-time gate refresh should work")
            .expect("gate should still exist")
            .passed,
        "expired operational evidence must disable adaptive routing before the next run"
    );
}

#[test]
fn adaptive_release_uses_the_worst_of_every_consecutive_five_run_window() {
    let connection = Connection::open_in_memory().expect("in-memory sqlite should open");
    initialize_adaptive_exposure_db(&connection).expect("schema should initialize");
    for run in 0..6 {
        let codes = if run < 5 {
            (1..=4).collect::<Vec<_>>()
        } else {
            (100..120).collect::<Vec<_>>()
        };
        let items = codes
            .into_iter()
            .map(|code| json!({ "stock": { "code": format!("{code:06}.SZ") } }))
            .collect::<Vec<_>>();
        adaptive_release_run_record_rows(
            &connection,
            Some(&format!("window-run-{run}")),
            &json!({
                "algorithm_version": "adaptive_swing_v1",
                "groups": [{ "key": "primary", "items": items }]
            }),
            "20260729",
            if run == 0 { 10_000 } else { 1_000 },
            run != 0,
            true,
        )
        .expect("window evidence should persist");
    }
    let evidence = adaptive_release_operational_evidence_rows(&connection)
        .expect("all windows should aggregate");
    assert_eq!(evidence.0, Some(4));
}

#[test]
fn adaptive_release_qualification_requires_auto_full_universe_long_oos_configuration() {
    let mut request = gp_core::BacktestRequest {
        criteria: gp_core::ScreenCriteria::default(),
        source: "criteria".to_string(),
        strategy_mode: "adaptive_swing_v1:auto".to_string(),
        stock_codes: Vec::new(),
        start_date: "20200101".to_string(),
        end_date: "20260729".to_string(),
        top_n: 10,
        initial_cash: 1_000_000.0,
        rebalance_frequency: "monthly".to_string(),
        transaction_cost_bps: 10.0,
        benchmark: "candidate_equal_weight".to_string(),
    };
    let qualified = adaptive_release_backtest_qualification(&request, 60);
    assert_eq!(qualified["qualified"], true);
    assert_eq!(
        qualified["implementation_fingerprint"],
        adaptive_release_implementation_fingerprint()
    );
    assert_eq!(
        adaptive_release_backtest_qualification(&request, 59)["qualified"],
        false
    );
    request.strategy_mode = "adaptive_swing_v1:range".to_string();
    assert_eq!(
        adaptive_release_backtest_qualification(&request, 60)["qualified"],
        false
    );
    request.strategy_mode = "adaptive_swing_v1:auto".to_string();
    request.criteria.max_pe = Some(20.0);
    assert_eq!(
        adaptive_release_backtest_qualification(&request, 60)["qualified"],
        false
    );
}

#[test]
fn adaptive_data_date_uses_only_participating_histories_and_the_common_minimum() {
    let bar = |date: &str| gp_core::HistoryBar {
        date: date.to_string(),
        open: Some(10.0),
        high: Some(10.2),
        low: Some(9.8),
        close: 10.0,
        volume: Some(1_000.0),
        capital: None,
    };
    let data = gp_core::CoreDataSet {
        histories: HashMap::from([
            ("600000.SH".to_string(), vec![bar("20260728")]),
            ("000001.SH".to_string(), vec![bar("20260729")]),
            ("999999.SH".to_string(), vec![bar("20991231")]),
        ]),
        ..gp_core::CoreDataSet::default()
    };
    assert_eq!(
        latest_adaptive_data_date(
            &data,
            &HashMap::new(),
            &["600000.SH".to_string(), "000001.SH".to_string()],
        ),
        Some("20260728".to_string()),
    );
}

#[test]
fn market_refresh_preserves_cached_adaptive_benchmark_histories() {
    let filtered = filter_seed_histories(
        &json!({
            "histories": {
                "600000.SH": [{ "date": "20260729", "close": 10.0 }],
                "000001.SH": [{ "date": "20260729", "close": 3800.0 }],
                "399001.SZ": [{ "date": "20260729", "close": 12000.0 }],
                "399006.SZ": [{ "date": "20260729", "close": 2500.0 }],
                "900001.SH": [{ "date": "20260729", "close": 1.0 }]
            }
        }),
        &HashSet::from(["600000.SH".to_string()]),
    );
    assert!(filtered.get("600000.SH").is_some());
    for code in adaptive_benchmark_codes() {
        assert!(filtered.get(code).is_some(), "{code} should remain cached");
    }
    assert!(filtered.get("900001.SH").is_none());
}

#[test]
fn agent_harness_builds_an_expert_prompt_from_history_and_tool_evidence() {
    let preview = agent_harness::prompt_preview(
        &json!({
            "message": "比较自选股里的主线机会",
            "mode": "expert",
            "history": [
                {"role": "user", "content": "先看市场环境"},
                {"role": "assistant", "content": "需要结合本地证据"}
            ]
        }),
        &json!({
            "reply": "已完成本地趋势筛选。",
            "action": "trend_screen",
            "evidence_summary": [{"title": "趋势筛选", "source": "本地K线", "level": "primary"}]
        }),
    );

    assert_eq!(preview["profile_id"], "hot_money_early_v1");
    assert!(preview["system_prompt"]
        .as_str()
        .unwrap_or_default()
        .contains("仅供选股研究"));
    assert!(preview["system_prompt"]
        .as_str()
        .unwrap_or_default()
        .contains("情绪周期"));
    assert_eq!(
        preview["user_payload"]["history"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        preview["user_payload"]["tool_result"]["action"],
        "trend_screen"
    );
    assert_eq!(preview["user_payload"]["tool_result_truncated"], false);
}

#[test]
fn agent_harness_marks_compacted_tool_context() {
    let preview = agent_harness::prompt_preview(
        &json!({"message": "比较候选", "mode": "expert"}),
        &json!({"items": (0..20).map(|index| json!({"index": index})).collect::<Vec<_>>() }),
    );
    assert_eq!(preview["user_payload"]["tool_result_truncated"], true);
    assert_eq!(
        preview["user_payload"]["tool_result"]["items"]
            .as_array()
            .map(Vec::len),
        Some(16)
    );
}

#[test]
fn agent_harness_builds_a_value_compounder_prompt_for_research_mode() {
    let preview = agent_harness::prompt_preview(
        &json!({
            "message": "从长期持有角度研究这家公司",
            "mode": "research"
        }),
        &json!({
            "reply": "已读取公司财务与公告证据。",
            "action": "observe_stock",
            "evidence_summary": [{"title": "年度报告", "source": "交易所公告", "level": "primary"}]
        }),
    );

    let prompt = preview["system_prompt"].as_str().unwrap_or_default();
    assert_eq!(preview["profile_id"], "value_compounder_v1");
    for required in [
        "格雷厄姆",
        "费雪",
        "巴菲特",
        "芒格",
        "护城河",
        "所有者收益",
        "资本配置",
        "安全边际",
    ] {
        assert!(
            prompt.contains(required),
            "research prompt should contain {required}"
        );
    }
    assert!(!prompt.contains("打板"));
    assert!(!prompt.contains("二板"));
    assert!(prompt.contains("不构成投资建议"));
}

#[test]
fn agent_harness_prompt_profiles_pass_contrast_evaluation() {
    let suite: Value = serde_json::from_str(include_str!(
        "../../../app/prompts/agent_harness_eval_cases.json"
    ))
    .expect("agent harness evaluation fixture should be valid JSON");
    let tool_result = json!({
        "reply": "本地工具已完成取证。",
        "action": "observe_stock",
        "data": {"code": "000001.SZ", "returned": 1},
        "evidence_summary": [{"title": "财报与行情", "source": "本地工具", "level": "primary"}]
    });

    for case in suite["cases"].as_array().expect("evaluation cases") {
        let question = case["question"].as_str().expect("case question");
        for mode in ["expert", "research"] {
            let contract = &suite["profiles"][mode];
            let preview = agent_harness::prompt_preview(
                &json!({"message": question, "mode": mode}),
                &tool_result,
            );
            let prompt = preview["system_prompt"].as_str().unwrap_or_default();
            assert_eq!(preview["profile_id"], contract["profile_id"]);
            assert_eq!(preview["user_payload"]["question"], question);
            for term in contract["required_prompt_terms"]
                .as_array()
                .expect("required terms")
            {
                let term = term.as_str().unwrap();
                assert!(prompt.contains(term), "{mode} prompt should contain {term}");
            }
            for term in contract["forbidden_prompt_terms"]
                .as_array()
                .expect("forbidden terms")
            {
                let term = term.as_str().unwrap();
                assert!(
                    !prompt.contains(term),
                    "{mode} prompt should not contain {term}"
                );
            }
            let merged = agent_harness::merge_model_response(
                tool_result.clone(),
                &case[format!("{mode}_output")],
                contract["profile_id"].as_str().unwrap(),
                Some("eval-model"),
            )
            .expect("safe evaluation sample should pass output gates");
            assert_eq!(merged["action"], tool_result["action"]);
            assert_eq!(merged["data"], tool_result["data"]);
            let rendered = merged.to_string();
            for term in case[format!("{mode}_required_output_terms")]
                .as_array()
                .expect("required output terms")
            {
                let term = term.as_str().unwrap();
                assert!(
                    rendered.contains(term),
                    "{mode} output should contain {term}"
                );
            }
            for term in contract["forbidden_prompt_terms"]
                .as_array()
                .expect("cross-profile terms")
            {
                let term = term.as_str().unwrap();
                assert!(
                    !rendered.contains(term),
                    "{mode} output should not contain {term}"
                );
            }
        }
    }

    for unsafe_output in suite["unsafe_outputs"].as_array().expect("unsafe outputs") {
        for profile_id in ["hot_money_early_v1", "value_compounder_v1"] {
            assert!(agent_harness::merge_model_response(
                tool_result.clone(),
                unsafe_output,
                profile_id,
                Some("eval-model"),
            )
            .is_err());
        }
    }
}

#[test]
fn agent_harness_merges_model_explanation_without_overwriting_tool_facts() {
    let tool_response = json!({
        "reply": "本地工具结论",
        "action": "trend_screen",
        "data": {"returned": 3, "items": [{"code": "000001.SZ"}]},
        "evidence_summary": [{"title": "趋势筛选", "source": "本地K线", "level": "primary", "summary": "返回 3 个候选"}],
        "answer_sections": [{"title": "本地工具事实", "bullets": ["返回 3 个候选。"]}],
        "warnings": ["工具提示：部分行情数据缺失。"],
        "next_actions": ["复核本地行情"]
    });
    let merged = agent_harness::merge_model_response(
        tool_response.clone(),
        &json!({
            "reply": "模型基于本地证据完成解释。[E1]",
            "action": "backtest",
            "data": {"returned": 999},
            "answer_sections": [{"title": "市场环境", "bullets": ["当前证据偏强，但仍需复核。[E1]"]}],
            "warnings": ["模型结论存在不确定性。"],
            "next_actions": ["核验公告原文"]
        }),
        "hot_money_early_v1",
        Some("test-model"),
    )
    .expect("safe model output should merge");

    assert_eq!(merged["action"], tool_response["action"]);
    assert_eq!(merged["data"], tool_response["data"]);
    assert!(merged["reply"].as_str().unwrap().contains("本地工具结论"));
    assert!(merged["reply"]
        .as_str()
        .unwrap()
        .contains("模型基于本地证据完成解释"));
    assert!(merged["answer_sections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|section| { section["title"].as_str() == Some("本地工具事实") }));
    assert!(merged["model_answer_sections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|section| {
            section["title"].as_str() == Some("市场环境")
                && section["provenance"].as_str() == Some("model_inference")
        }));
    assert!(merged["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| { action.as_str() == Some("复核本地行情") }));
    assert!(merged["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some("仅供选股研究，不构成投资建议。")));
    assert!(merged["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item
            .as_str()
            .unwrap_or_default()
            .contains("部分行情数据缺失")));
    assert_eq!(merged["harness"]["model_used"], true);
    assert_eq!(merged["harness"]["profile_id"], "hot_money_early_v1");
}

#[test]
fn agent_harness_rejects_direct_trading_or_manipulation_in_model_output() {
    for unsafe_reply in [
        "建议立即买入并满仓持有。",
        "可以通过虚假申报和拉抬股价吸引跟风盘。",
    ] {
        let result = agent_harness::merge_model_response(
            json!({"reply": "工具结论", "action": "screen", "data": {"returned": 1}}),
            &json!({"reply": unsafe_reply}),
            "hot_money_early_v1",
            Some("test-model"),
        );
        assert!(
            result.is_err(),
            "unsafe output should be rejected: {unsafe_reply}"
        );
    }
}

#[test]
fn agent_harness_rejects_missing_or_unknown_model_evidence_references() {
    let tool_response = json!({
        "reply": "工具结论",
        "action": "screen",
        "data": {"returned": 1},
        "evidence_summary": [{"title": "筛选结果", "source": "本地股票池", "level": "primary", "summary": "返回 1 项"}]
    });
    for reply in ["模型结论没有引用。", "模型错误引用。[E2]"] {
        let result = agent_harness::merge_model_response(
            tool_response.clone(),
            &json!({"reply": reply}),
            "hot_money_early_v1",
            Some("test-model"),
        );
        assert!(
            result.is_err(),
            "invalid evidence should be rejected: {reply}"
        );
    }
}

#[test]
fn agent_harness_calls_an_openai_compatible_model_endpoint() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock model endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept model request");
        let mut request = vec![0_u8; 65_536];
        let read = stream.read(&mut request).expect("read model request");
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.contains("hot_money_early_v1"));

        let content = json!({
            "reply": "模型链路已接通。",
            "answer_sections": [{"title": "市场环境", "bullets": ["依据本地证据解释。"]}],
            "warnings": ["仅供选股研究，不构成投资建议。"],
            "next_actions": ["核验原始数据"]
        })
        .to_string();
        let body = json!({"choices": [{"message": {"content": content}}]}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        ).expect("write model response");
    });

    let preview = agent_harness::prompt_preview(
        &json!({"message": "分析市场主线", "mode": "expert"}),
        &json!({"reply": "工具结果", "action": "trend_screen", "data": {"returned": 2}}),
    );
    let result = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({
            "base_url": format!("http://{address}/v1"),
            "model": "mock-model",
            "timeout_seconds": 5,
            "json_mode": true
        })),
        &preview,
    ))
    .expect("model request should succeed")
    .expect("model should be configured");

    assert_eq!(result["reply"], "模型链路已接通。");
    server.join().expect("mock model server should finish");
}

#[test]
fn agent_harness_treats_incomplete_model_settings_as_unconfigured() {
    let preview = agent_harness::prompt_preview(
        &json!({"message": "分析市场主线", "mode": "expert"}),
        &json!({"reply": "工具结果", "action": "trend_screen"}),
    );
    let result = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({"temperature": 0.7, "timeout_seconds": 1})),
        &preview,
    ))
    .expect("incomplete settings should not trigger a request");
    assert!(result.is_none());
}

#[test]
fn agent_harness_only_retries_when_an_endpoint_rejects_json_mode() {
    assert!(agent_harness::should_retry_without_json_mode(
        "Agent LLM HTTP 400 Bad Request: response_format json_object is unsupported"
    ));
    for error in [
        "Agent LLM HTTP 401 Unauthorized",
        "Agent LLM HTTP 500 Internal Server Error",
        "Agent LLM request failed: timed out",
        "parse Agent model JSON failed",
    ] {
        assert!(!agent_harness::should_retry_without_json_mode(error));
    }
}

#[test]
fn agent_harness_rejects_invalid_model_endpoints_before_network_io() {
    let preview = agent_harness::prompt_preview(
        &json!({"message": "分析市场主线", "mode": "expert"}),
        &json!({"reply": "工具结果", "action": "trend_screen"}),
    );
    let error = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({"base_url": "file:///tmp/model", "model": "mock"})),
        &preview,
    ))
    .expect_err("non-HTTP model endpoints must be rejected");
    assert!(error.contains("http or https"));
    let error = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({"base_url": "http://models.example.test/v1", "model": "mock"})),
        &preview,
    ))
    .expect_err("remote plaintext model endpoints must be rejected");
    assert!(error.contains("must use https"));
}

#[test]
fn agent_harness_executes_local_tools_then_synthesizes_the_final_result() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock model endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept model request");
        let mut request = vec![0_u8; 65_536];
        let _ = stream.read(&mut request).expect("read model request");
        let content = json!({
            "reply": "已按专家模式解释本地自选股。[E1]",
            "answer_sections": [{"title": "市场环境", "bullets": ["当前只有自选股事实，情绪位置待验证。[E1]"]}],
            "warnings": ["仅供选股研究，不构成投资建议。"],
            "next_actions": ["补充行情与公告证据"]
        }).to_string();
        let body = json!({"choices": [{"message": {"content": content}}]}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        ).unwrap();
    });

    let streamed = Arc::new(Mutex::new(Vec::new()));
    let streamed_sink = Arc::clone(&streamed);
    let outcome = tauri::async_runtime::block_on(agent_harness::execute_with_event_sink(
        json!({
            "message": "查看自选股",
            "run_id": "harness-run",
            "mode": "expert",
            "llm": {
                "base_url": format!("http://{address}/v1"),
                "model": "mock-model",
                "timeout_seconds": 5
            },
            "context": {"watchlist": [{"code": "000001.SZ", "name": "平安银行"}]}
        }),
        json!({}),
        move |event| streamed_sink.lock().unwrap().push(event),
    ))
    .expect("harness should execute");

    assert_eq!(outcome.response["action"], "watchlist_action");
    assert_eq!(outcome.response["harness"]["model_used"], true);
    assert_eq!(
        outcome.response["harness"]["profile_id"],
        "hot_money_early_v1"
    );
    assert!(outcome.events.iter().any(|event| event["type"] == "result"));
    let streamed = streamed.lock().unwrap();
    assert_eq!(
        streamed.first().and_then(|event| event["stage"].as_str()),
        Some("tools")
    );
    let model_index = streamed
        .iter()
        .position(|event| event["stage"] == "model")
        .unwrap();
    let result_index = streamed
        .iter()
        .position(|event| event["type"] == "result")
        .unwrap();
    assert!(model_index < result_index);
    server.join().unwrap();
}

#[test]
fn agent_harness_keeps_quick_mode_deterministic_when_a_model_is_configured() {
    let outcome = tauri::async_runtime::block_on(agent_harness::execute(
        json!({
            "message": "查看自选股",
            "run_id": "quick-run",
            "mode": "quick",
            "llm": {
                "base_url": "http://127.0.0.1:9/v1",
                "model": "must-not-be-called",
                "timeout_seconds": 1
            },
            "context": {"watchlist": [{"code": "000001.SZ", "name": "平安银行"}]}
        }),
        json!({}),
    ))
    .expect("quick harness should execute without a model request");

    assert_eq!(
        outcome.response["harness"]["profile_id"],
        "deterministic_v1"
    );
    assert_eq!(outcome.response["harness"]["model_used"], false);
    assert!(!outcome.response["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap_or_default()
            .contains("模型调用失败")));
}

#[cfg(target_os = "windows")]
#[test]
fn research_embedding_job_gate_replays_requests_arriving_during_a_run() {
    let gate = ResearchEmbeddingJobGate::new();
    assert!(gate.request());
    gate.begin_cycle();
    assert!(!gate.request());
    assert!(gate.finish_cycle());
    gate.begin_cycle();
    assert!(!gate.finish_cycle());
    assert!(gate.request());
}

#[test]
fn llm_models_endpoint_appends_models_to_provider_base_path() {
    assert_eq!(
        llm_models_endpoint("https://api.openai.com/v1/")
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("https://gateway.example/v1/chat/completions?trace=1")
            .unwrap()
            .as_str(),
        "https://gateway.example/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("http://127.0.0.1:11434/v1/models")
            .unwrap()
            .as_str(),
        "http://127.0.0.1:11434/v1/models"
    );
    assert!(llm_models_endpoint("file:///tmp/models").is_err());
}

#[test]
fn parse_llm_model_options_supports_openai_and_ollama_shapes() {
    let openai = parse_llm_model_options(&json!({
        "data": [
            {"id": "gpt-5.4", "owned_by": "openai"},
            {"id": "gpt-5.4", "owned_by": "duplicate"},
            {"id": "gpt-5.4-mini", "display_name": "GPT 5.4 Mini"}
        ]
    }));
    assert_eq!(openai.len(), 2);
    assert_eq!(openai[0]["id"].as_str(), Some("gpt-5.4"));
    assert_eq!(openai[1]["name"].as_str(), Some("GPT 5.4 Mini"));

    let ollama = parse_llm_model_options(&json!({
        "models": [
            {"name": "qwen2.5:7b", "model": "qwen2.5:7b"},
            "deepseek-r1:8b"
        ]
    }));
    assert_eq!(ollama.len(), 2);
    assert_eq!(ollama[0]["id"].as_str(), Some("qwen2.5:7b"));
    assert_eq!(ollama[1]["id"].as_str(), Some("deepseek-r1:8b"));
}

#[test]
fn llm_models_http_error_explains_auth_and_version_failures() {
    assert!(llm_models_http_error(401, b"{}", "sk-test").contains("API 密钥"));
    assert!(llm_models_http_error(404, b"{}", "sk-test").contains("/v1"));
    assert!(
        llm_models_http_error(429, br#"{"error":{"message":"rate limited"}}"#, "sk-test")
            .contains("rate limited")
    );
    let redacted = llm_models_http_error(
        500,
        br#"{"error":{"message":"upstream echoed sk-secret-token"}}"#,
        "sk-secret-token",
    );
    assert!(!redacted.contains("sk-secret-token"));
    assert!(redacted.contains("[已隐藏密钥]"));
}

#[test]
fn append_preserved_seed_stocks_keeps_seed_rows_and_fills_missing_industry() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "industry": "", "price": 10.0},
            {"code": "600000.SH", "name": "SPD Bank", "industry": "Banking", "price": 8.0}
        ]
    });
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let mut stocks = Vec::new();
    let mut seen = HashSet::new();

    let preserved = append_preserved_seed_stocks(&seed_codes, &seed_stocks, &mut stocks, &mut seen);

    assert_eq!(preserved, 2);
    assert_eq!(stocks.len(), 2);
    let sz_stock = stocks
        .iter()
        .find(|stock| stock.get("code").and_then(Value::as_str) == Some("000001.SZ"))
        .expect("SZ stock should be preserved");
    assert_eq!(
        sz_stock.get("industry").and_then(Value::as_str),
        Some("\u{6df1}\u{5e02}A\u{80a1}")
    );
    let sh_stock = stocks
        .iter()
        .find(|stock| stock.get("code").and_then(Value::as_str) == Some("600000.SH"))
        .expect("SH stock should be preserved");
    assert_eq!(
        sh_stock.get("industry").and_then(Value::as_str),
        Some("Banking")
    );
}

#[test]
fn append_preserved_seed_stocks_skips_already_fetched_rows() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0},
            {"code": "600000.SH", "name": "SPD Bank", "price": 8.0}
        ]
    });
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let mut stocks = vec![json!({"code": "000001.SZ", "name": "Live Quote"})];
    let mut seen = HashSet::from(["000001.SZ".to_string()]);

    let preserved = append_preserved_seed_stocks(&seed_codes, &seed_stocks, &mut stocks, &mut seen);

    assert_eq!(preserved, 1);
    assert_eq!(stocks.len(), 2);
    assert!(stocks
        .iter()
        .any(|stock| stock.get("code").and_then(Value::as_str) == Some("600000.SH")));
}

#[test]
fn tencent_candidates_are_interleaved_across_markets() {
    let mut codes = Vec::new();

    append_tencent_candidate_codes(&mut codes);

    assert_eq!(
        &codes[..5],
        &[
            "000001.SZ".to_string(),
            "600000.SH".to_string(),
            "300000.SZ".to_string(),
            "688000.SH".to_string(),
            "920000.BJ".to_string(),
        ]
    );
    assert!(codes.iter().position(|code| code == "600010.SH").unwrap() < 64);
    assert!(codes.iter().position(|code| code == "300010.SZ").unwrap() < 64);
    assert!(codes.iter().position(|code| code == "688010.SH").unwrap() < 64);
    assert!(codes.iter().position(|code| code == "920010.BJ").unwrap() < 64);
}

#[test]
fn candidate_batch_window_clamps_to_available_batches() {
    assert_eq!(candidate_batch_window(0, 0, Some(4)), (0, 0, 0));
    assert_eq!(candidate_batch_window(600, 0, Some(2)), (0, 2, 5));
    assert_eq!(candidate_batch_window(600, 4, Some(4)), (4, 5, 5));
    assert_eq!(candidate_batch_window(600, 9, Some(1)), (5, 5, 5));
}

#[test]
fn tencent_batch_size_keeps_full_refresh_round_trips_reasonable() {
    assert_eq!(15_000usize.div_ceil(TENCENT_BATCH_SIZE), 125);
}

#[test]
fn tencent_quote_parser_extracts_market_cap_and_share_structure() {
    // Preserve the complete field shape of a public Tencent quote sample so
    // index drift around the market-cap/share-count tail is caught by tests.
    let raw = r#"v_sh600941="1~中国移动~600941~93.01~92.80~92.79~50872~23976~26896~93.01~3~93.00~33~92.99~67~92.98~51~92.97~2~93.02~7~93.03~44~93.04~22~93.05~7~93.06~88~~20260716120558~0.21~0.23~93.39~92.46~93.01/50872/472965240~50872~47297~0.56~14.85~~93.39~92.46~1.00~839.66~20168.01~1.42~102.08~83.52~1.03~-12~92.97~17.18~14.71~~~0.17~47296.5240~0.0000~0~   A~GP-A~-5.91~4.85~5.05~9.55~6.31~108.58~85.38~5.39~0.03~1.91~902767867~21683696323~-3.70~-11.90~902767867~~~-14.08~0.00~~CNY~0~___D__F__N~93.09~-170~";"#;

    let stocks = parse_tencent_quotes(raw, &HashMap::new(), false);
    assert_eq!(stocks.len(), 1);
    let stock = &stocks[0];
    assert_eq!(
        stock
            .get("circulating_market_cap_billion")
            .and_then(Value::as_f64),
        Some(839.66)
    );
    assert_eq!(
        stock.get("market_cap_billion").and_then(Value::as_f64),
        Some(20168.01)
    );
    assert_eq!(
        stock.get("circulating_shares").and_then(Value::as_f64),
        Some(902_767_867.0)
    );
    assert_eq!(
        stock.get("total_shares").and_then(Value::as_f64),
        Some(21_683_696_323.0)
    );
}

#[test]
fn observe_result_preserves_share_structure_from_quote_data() {
    let core_payload = json!({
        "data": {
            "stocks": [{
                "code": "600941.SH",
                "market_cap_billion": 20168.01,
                "circulating_market_cap_billion": 839.66,
                "total_shares": 21683696323.0,
                "circulating_shares": 902767867.0
            }]
        }
    });
    let mut result = json!({"stock": {"code": "600941.SH"}});

    enrich_observe_stock_quote_fields(&mut result, &core_payload);

    assert_eq!(
        result["stock"]["total_shares"].as_f64(),
        Some(21_683_696_323.0)
    );
    assert_eq!(
        result["stock"]["circulating_shares"].as_f64(),
        Some(902_767_867.0)
    );
}

#[test]
fn build_candidate_codes_is_stable_as_seed_grows() {
    // A full rebuild paginates with `batch_start`; the candidate list must
    // not drift as freshly fetched codes accumulate into the in-memory seed.
    let base = build_candidate_codes(&[], true, 100_000);
    assert_eq!(base.first().map(String::as_str), Some("000001.SZ"));

    // Seed codes already inside the scan ranges must not reorder the list.
    let seed: Vec<String> = vec![
        "600000.SH".to_string(),
        "000001.SZ".to_string(),
        "300500.SZ".to_string(),
    ];
    let with_seed = build_candidate_codes(&seed, true, 100_000);
    assert_eq!(
        base, with_seed,
        "pagination windows drifted when the seed grew"
    );

    // Seed-only codes (outside the scan ranges) are appended at the end so
    // the scan prefix — and therefore every batch window over it — is fixed.
    let extra_seed: Vec<String> = vec!["830001.BJ".to_string()];
    let with_extra = build_candidate_codes(&extra_seed, true, 100_000);
    assert_eq!(&with_extra[..base.len()], &base[..]);
    assert_eq!(with_extra.last().map(String::as_str), Some("830001.BJ"));
}

#[test]
fn build_candidate_codes_uses_seed_only_without_scan() {
    let seed: Vec<String> = vec!["600000.SH".to_string(), "000001.SZ".to_string()];
    let codes = build_candidate_codes(&seed, false, 100_000);
    assert_eq!(codes, seed);
}

#[test]
fn paginated_rebuild_covers_every_candidate_without_drift() {
    // Simulate the real full rebuild: the frontend walks `batch_start` 0..N
    // while the in-memory seed grows each invocation. The candidate list is
    // rebuilt from that growing seed every batch, so coverage is only correct
    // if the windows do not drift. (Worst case for drift: 100% hit rate, so
    // the seed grows as fast as possible.)
    use std::collections::BTreeSet;
    let max = 1_000usize;
    let stable = build_candidate_codes(&[], true, max);
    let total_batches = stable.len().div_ceil(TENCENT_BATCH_SIZE);
    let mut fetched: BTreeSet<String> = BTreeSet::new();
    for batch_start in 0..total_batches {
        let seed: Vec<String> = fetched.iter().cloned().collect();
        let candidates = build_candidate_codes(&seed, true, max);
        let (start, end, _) = candidate_batch_window(candidates.len(), batch_start, Some(1));
        for code in candidates
            .chunks(TENCENT_BATCH_SIZE)
            .skip(start)
            .take(end - start)
            .flatten()
        {
            fetched.insert(code.clone());
        }
    }
    let expected: BTreeSet<String> = stable.into_iter().collect();
    assert_eq!(fetched, expected, "rebuild missed or duplicated candidates");
}

#[test]
fn payload_usize_field_accepts_query_string_values() {
    let payload = json!({"limit": "3"});
    assert_eq!(payload_usize_field(&payload, "limit", 8, 1, 20), 3);

    let numeric_payload = json!({"limit": 25});
    assert_eq!(payload_usize_field(&numeric_payload, "limit", 8, 1, 20), 20);

    let invalid_payload = json!({"limit": "bad"});
    assert_eq!(payload_usize_field(&invalid_payload, "limit", 8, 1, 20), 8);
}

#[test]
fn market_data_payload_present_detects_any_cached_section() {
    assert!(market_data_payload_present(
        &json!({"stocks": [{"code": "000001.SZ"}]})
    ));
    assert!(market_data_payload_present(
        &json!({"relations": [{"source_code": "000001.SZ"}]})
    ));
    assert!(market_data_payload_present(
        &json!({"histories": {"000001.SZ": []}})
    ));
    assert!(!market_data_payload_present(&json!({})));
}

#[test]
fn market_cache_record_can_omit_data_payload() {
    let payload = json!({
        "generated_at_epoch_ms": 123,
        "notes": ["cached note"],
        "stocks": [{"code": "000001.SZ"}],
        "relations": [],
        "histories": {}
    });
    let summary = json!({"stock_count": 1, "warnings": []});
    let record = market_cache_record(
        Path::new("cache.json"),
        42,
        Some(456),
        summary,
        &payload,
        false,
        "cache written",
    );

    assert!(record.get("data").is_none());
    assert_eq!(record.get("stock_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        record.get("generated_at").and_then(Value::as_u64),
        Some(123)
    );
    assert_eq!(
        record
            .get("data_notes")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(1)
    );
}

#[test]
fn retry_with_attempts_retries_transient_failures() {
    let mut attempts = 0usize;
    let result = retry_with_attempts(3, 0, |_| {
        attempts += 1;
        if attempts < 2 {
            Err("transient write failure".to_string())
        } else {
            Ok("ok")
        }
    })
    .expect("retry helper should recover after a transient failure");

    assert_eq!(result, "ok");
    assert_eq!(attempts, 2);
}

#[test]
fn cache_epoch_ms_accepts_iso_quote_dates() {
    let epoch = cache_epoch_ms(Some(&json!("2026-06-26T09:35:00+08:00")))
        .expect("ISO quote time should parse");
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(epoch).as_deref(),
        Some("20260626")
    );
    let utc_epoch =
        cache_epoch_ms(Some(&json!("2026-06-26T01:35:00Z"))).expect("UTC quote time should parse");
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(utc_epoch).as_deref(),
        Some("20260626")
    );
}

#[test]
fn expected_market_quote_date_uses_previous_session_before_open_and_weekends() {
    let friday_morning = cache_epoch_ms(Some(&json!("2026-06-26T08:00:00+08:00")))
        .expect("Friday morning should parse");
    assert_eq!(
        expected_market_quote_date_from_epoch_ms(friday_morning).as_deref(),
        Some("20260625")
    );
    let saturday =
        cache_epoch_ms(Some(&json!("2026-06-27T12:00:00+08:00"))).expect("Saturday should parse");
    assert_eq!(
        expected_market_quote_date_from_epoch_ms(saturday).as_deref(),
        Some("20260626")
    );
}

#[test]
fn market_quote_cache_stale_uses_quote_generation_date() {
    let june_25_quote = 1_782_381_094_692u128;
    let june_26_now = june_25_quote + 86_400_000;

    assert!(market_quote_cache_stale(Some(june_25_quote), june_26_now));
    assert!(!market_quote_cache_stale(Some(june_26_now), june_26_now));
    assert!(market_quote_cache_stale(None, june_26_now));
    assert_eq!(
        local_yyyymmdd_from_epoch_ms(june_25_quote).as_deref(),
        Some("20260625")
    );
}

#[test]
fn financial_snapshot_enriches_seed_without_preserving_snapshot_only_rows() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "stocks": [
            {
                "code": "000001.SZ",
                "deducted_net_profit_billion": 1.2,
                "deducted_net_profit_growth_rate": 12.0,
                "latest_eps": 0.45,
                "latest_bps": 12.3,
                "period": "2026Q1",
                "source": "data/cache/tdx_fundamentals.csv",
                "notes": ["季度 EPS 明细来自缓存"],
                "quarterly_eps": [{"period": "2026Q1", "value": 0.45}]
            },
            {"code": "300001.SZ", "deducted_net_profit_billion": 0.8, "deducted_net_profit_growth_rate": 15.0, "latest_eps": 0.2}
        ]
    });
    let (seed_stocks, _) = seed_stock_maps(&seed);
    let enriched = enriched_stock_maps(&seed_stocks, &snapshot);

    assert_eq!(
        enriched
            .get("000001.SZ")
            .and_then(|stock| object_f64(stock, "deducted_net_profit_billion")),
        Some(1.2)
    );
    assert!(enriched.contains_key("300001.SZ"));

    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &enriched, &mut stocks, &mut seen);

    assert_eq!(preserved, 1);
    assert_eq!(stocks.len(), 1);
    assert_eq!(
        stocks[0].get("code").and_then(Value::as_str),
        Some("000001.SZ")
    );
    assert_eq!(
        stocks[0]
            .get("deducted_net_profit_growth_rate")
            .and_then(Value::as_f64),
        Some(12.0)
    );
    assert_eq!(
        stocks[0].get("latest_eps").and_then(Value::as_f64),
        Some(0.45)
    );

    let valid_codes = HashSet::from(["000001.SZ".to_string()]);
    let financials = filtered_financial_snapshot_map(&seed, &snapshot, &valid_codes);
    let item = financials
        .get("000001.SZ")
        .and_then(Value::as_object)
        .expect("financial snapshot should be preserved for stock universe rows");
    assert_eq!(item.get("latest_eps").and_then(Value::as_f64), Some(0.45));
    assert_eq!(item.get("latest_bps").and_then(Value::as_f64), Some(12.3));
    assert_eq!(item.get("period").and_then(Value::as_str), Some("2026Q1"));
    assert_eq!(
        item.get("notes")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("季度 EPS 明细来自缓存")
    );
    assert_eq!(
        item.get("quarterly_eps")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("value"))
            .and_then(Value::as_f64),
        Some(0.45)
    );
    assert!(financials.get("300001.SZ").is_none());
}

#[test]
fn observe_financial_snapshot_merges_inferred_prior_year_eps() {
    let mut data = json!({
        "financials": {
            "000100.SZ": {
                "period": "2026Q1",
                "latest_eps": 0.0692,
                "quarterly_eps": [{"period": "2026Q1", "value": 0.0692, "source": "cache"}]
            }
        }
    });
    let snapshot = json!({
        "financials": {
            "000100.SZ": {
                "period": "2026Q1",
                "latest_eps": 0.0692,
                "quarterly_eps": [
                    {"period": "2026Q1", "value": 0.0692, "source": "snapshot"},
                    {"period": "2025Q1", "value": 0.0545, "source": "snapshot / inferred EPS YoY", "inferred": true, "note": "inferred_from_eps_yoy"}
                ]
            }
        }
    });

    assert!(merge_observe_financial_snapshot(
        &mut data,
        "000100.SZ",
        &snapshot
    ));
    assert_eq!(financial_quarterly_eps_count(&data, "000100.SZ"), 2);
    let rows = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|financials| financials.get("000100.SZ"))
        .and_then(|entry| entry.get("quarterly_eps"))
        .and_then(Value::as_array)
        .expect("quarterly EPS rows should exist");
    let prior = rows
        .iter()
        .find(|row| row.get("period").and_then(Value::as_str) == Some("2025Q1"))
        .expect("inferred 2025Q1 EPS should be preserved");
    assert_eq!(prior.get("value").and_then(Value::as_f64), Some(0.0545));
    assert_eq!(prior.get("inferred").and_then(Value::as_bool), Some(true));
}

#[test]
fn observe_financial_snapshot_merges_detailed_fundamental_metrics() {
    let mut data = json!({"financials": {}});
    let snapshot = json!({
        "financials": {
            "000100.SZ": {
                "period": "2026Q1",
                "operating_revenue_billion": 434.778212,
                "operating_revenue_yoy": 8.370839,
                "parent_net_profit_billion": 15.564526,
                "parent_net_profit_yoy": 53.712048,
                "gross_margin": 12.497484,
                "net_margin": 1.399071,
                "roe": 2.47,
                "asset_liability_ratio": 65.027185,
                "goodwill_period": "2026一季报",
                "pledged_share_period": "2026-07-10",
                "dividend_period": "2025-12-31"
            }
        }
    });

    assert!(merge_observe_financial_snapshot(
        &mut data,
        "000100.SZ",
        &snapshot
    ));
    let entry = data["financials"]["000100.SZ"]
        .as_object()
        .expect("financial entry should be merged");
    assert_eq!(
        finite_object_number(entry, "operating_revenue_billion"),
        Some(434.778212)
    );
    assert_eq!(finite_object_number(entry, "gross_margin"), Some(12.497484));
    assert_eq!(finite_object_number(entry, "roe"), Some(2.47));
    assert_eq!(
        finite_object_number(entry, "asset_liability_ratio"),
        Some(65.027185)
    );
    assert_eq!(
        entry.get("goodwill_period").and_then(Value::as_str),
        Some("2026一季报")
    );
    assert_eq!(
        entry.get("pledged_share_period").and_then(Value::as_str),
        Some("2026-07-10")
    );
    assert_eq!(
        entry.get("dividend_period").and_then(Value::as_str),
        Some("2025-12-31")
    );
}

#[test]
fn eastmoney_fundamental_parsers_calculate_specialized_metrics() {
    let balance = json!({
        "result": {"data": [{
            "REPORT_DATE_NAME": "2026一季报",
            "GOODWILL": 11_436_177_181.0,
            "TOTAL_PARENT_EQUITY": 63_684_356_235.0
        }]}
    });
    let pledge = json!({
        "result": {"data": [{"TRADE_DATE": "2026-07-10 00:00:00", "PLEDGE_RATIO": 0.74}]}
    });
    let dividend = json!({
        "result": {"data": [{
            "REPORT_DATE": "2025-12-31 00:00:00",
            "PRETAX_BONUS_RMB": 0.9,
            "BASIC_EPS": 0.2333,
            "DIVIDENT_RATIO": 0.019271948608,
            "EX_DIVIDEND_DATE": "2026-06-11 00:00:00"
        }]}
    });

    let goodwill = parse_goodwill_to_net_assets(&balance).expect("goodwill ratio");
    assert!((goodwill - 17.9575925).abs() < 0.00001);
    assert_eq!(parse_latest_pledged_share_ratio(&pledge), Some(0.74));
    let (dividend_yield, payout_ratio) = parse_latest_dividend_metrics(&dividend, Some(4.95));
    assert!((dividend_yield.expect("dividend yield") - 1.8181818).abs() < 0.00001);
    assert!((payout_ratio.expect("payout ratio") - 38.5769396).abs() < 0.00001);
    assert_eq!(
        eastmoney_metric_period(
            eastmoney_result_rows(&balance)
                .first()
                .and_then(Value::as_object),
            &["REPORT_DATE_NAME", "REPORT_DATE"]
        ),
        Some("2026一季报".to_string())
    );
    assert_eq!(
        eastmoney_metric_period(
            eastmoney_result_rows(&pledge)
                .first()
                .and_then(Value::as_object),
            &["TRADE_DATE"]
        ),
        Some("2026-07-10".to_string())
    );
    assert_eq!(
        eastmoney_metric_period(latest_dividend_row(&dividend), &["REPORT_DATE"]),
        Some("2025-12-31".to_string())
    );
}

#[test]
fn eastmoney_empty_result_is_only_normalized_for_optional_datasets() {
    let empty = json!({
        "success": false,
        "code": 9201,
        "message": "返回数据为空",
        "result": null
    });

    let normalized = normalize_eastmoney_public_json(empty.clone(), "optional", true)
        .expect("optional empty response should be accepted");
    assert!(eastmoney_result_rows(&normalized).is_empty());
    assert_eq!(
        normalized.get("success").and_then(Value::as_bool),
        Some(true)
    );
    assert!(normalize_eastmoney_public_json(empty, "required", false).is_err());
}

#[test]
fn eastmoney_fetch_retries_direct_after_preferred_proxy_failure() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local Eastmoney fixture");
    let address = listener.local_addr().expect("local fixture address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept direct retry");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).expect("read fixture request");
        let body = r#"{"result":{"data":[{"PLEDGE_RATIO":0.69}]},"success":true,"code":0}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write fixture response");
    });
    let preferred_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("bad proxy URL"))
        .build()
        .expect("preferred client");
    let direct_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .expect("direct client");

    let result = tauri::async_runtime::block_on(fetch_eastmoney_public_json_with_direct_retry(
        &preferred_client,
        &direct_client,
        &format!("http://{address}"),
        "Eastmoney fixture",
        false,
    ))
    .expect("direct retry must recover the public metric");
    assert_eq!(parse_latest_pledged_share_ratio(&result), Some(0.69));
    server.join().expect("local Eastmoney fixture");
}

#[test]
fn eastmoney_malformed_metric_rows_do_not_become_false_zeroes() {
    let malformed_pledge = json!({"result": {"data": [{}]}});
    let malformed_dividend = json!({"result": {"data": ["invalid"]}});

    assert_eq!(parse_latest_pledged_share_ratio(&malformed_pledge), None);
    assert_eq!(
        parse_latest_dividend_metrics(&malformed_dividend, Some(4.95)),
        (None, None)
    );
}

#[test]
fn observe_fundamental_supplement_records_freshness_and_refreshes_stale_values() {
    let mut data = json!({"financials": {"000100.SZ": {}}});
    let fields = json!({
        "goodwill_to_net_assets": 17.96,
        "goodwill_period": "2026一季报",
        "pledged_share_ratio": 0.74,
        "pledged_share_period": "2026-07-10",
        "dividend_yield": 1.82,
        "dividend_payout_ratio": 38.58,
        "dividend_period": "2025-12-31"
    })
    .as_object()
    .expect("supplement fields")
    .clone();

    assert!(observe_needs_fundamental_supplement(&data, "000100.SZ"));
    assert!(merge_observe_fundamental_supplement(
        &mut data,
        "000100.SZ",
        fields
    ));
    assert!(!observe_needs_fundamental_supplement(&data, "000100.SZ"));
    assert_eq!(
        data["financials"]["000100.SZ"]["goodwill_period"].as_str(),
        Some("2026一季报")
    );
    assert_eq!(
        data["financials"]["000100.SZ"]["pledged_share_period"].as_str(),
        Some("2026-07-10")
    );
    assert_eq!(
        data["financials"]["000100.SZ"]["dividend_period"].as_str(),
        Some("2025-12-31")
    );
    assert!(cache_epoch_ms(Some(
        &data["financials"]["000100.SZ"]["supplement_updated_at_epoch_ms"]
    ))
    .is_some());

    data["financials"]["000100.SZ"]["supplement_updated_at_epoch_ms"] = json!("1");
    assert!(observe_needs_fundamental_supplement(&data, "000100.SZ"));
}

#[test]
fn observe_quote_snapshot_fills_exact_share_structure() {
    let mut data = json!({
        "stocks": [{"code": "000100.SZ", "name": "TCL", "price": 4.95}]
    });
    assert!(observe_needs_exact_share_refresh(&data, "000100.SZ"));
    let quote = json!({
        "total_shares": 20_800_862_447.0,
        "circulating_shares": 20_118_326_408.0,
        "market_cap_billion": 1029.64,
        "circulating_market_cap_billion": 995.86,
        "quote_time": "20260716150000"
    });
    assert!(merge_observe_quote_snapshot(
        &mut data,
        "000100.SZ",
        quote.as_object().expect("quote object")
    ));
    assert!(!observe_needs_exact_share_refresh(&data, "000100.SZ"));
    assert_eq!(
        data["stocks"][0]["total_shares"].as_f64(),
        Some(20_800_862_447.0)
    );
    assert_eq!(
        data["stocks"][0]["circulating_shares"].as_f64(),
        Some(20_118_326_408.0)
    );
}

#[test]
fn eastmoney_guba_parser_extracts_hottest_sentiment_items() {
    let rows = (0..12)
            .map(|idx| {
                format!(
                    r#"{{"post_id":{},"post_title":"利好突破 看多上涨 {}","stockbar_code":"000725","post_click_count":{},"post_comment_count":{},"post_publish_time":"2026-06-25 12:{:02}:50","bullish_bearish":1}}"#,
                    100 + idx,
                    idx,
                    idx * 10,
                    idx,
                    idx
                )
            })
            .collect::<Vec<_>>()
            .join(",");
    let html = format!(r#"<script>var article_list={{"re":[{}]}};</script>"#, rows);
    let items = parse_eastmoney_guba_items(
        &html,
        "000725.SZ",
        "https://guba.eastmoney.com/list,000725.html",
    );

    assert_eq!(items.len(), 10);
    let first = items.first().unwrap();
    assert_eq!(
        first.get("category").and_then(Value::as_str),
        Some("community_sentiment")
    );
    assert_eq!(
        first.get("source").and_then(Value::as_str),
        Some("东方财富股吧")
    );
    assert!(first
        .get("title")
        .and_then(Value::as_str)
        .unwrap()
        .contains("11"));
    assert_eq!(
        first
            .get("metrics")
            .and_then(Value::as_object)
            .and_then(|metrics| metrics.get("评论数"))
            .and_then(Value::as_str),
        Some("11.00")
    );
    assert!(first.get("score").and_then(Value::as_f64).unwrap() > 60.0);
}

#[test]
fn eastmoney_lhb_parser_extracts_matching_stock() {
    let raw = r#"{"result":{"data":[{"SECURITY_CODE":"000725","SECURITY_NAME_ABBR":"京东方A","TRADE_DATE":"2026-06-17 00:00:00","BUY_AMT":147874970.48,"SELL_AMT":274892234.61,"NET_BUY_AMT":-127017264.13,"RATIO":-1.0467,"BUY_TIMES":0,"SELL_TIMES":1,"EXPLANATION":"日涨幅偏离值达到7%的前5只证券"}]}}"#;
    let item = parse_eastmoney_lhb_item(raw, "000725.SZ", "2026-06-01", "2026-06-25").unwrap();
    assert_eq!(
        item.get("category").and_then(Value::as_str),
        Some("institution_lhb")
    );
    assert_eq!(
        item.get("source").and_then(Value::as_str),
        Some("东方财富龙虎榜机构统计")
    );
    assert!(item.get("score").and_then(Value::as_f64).unwrap() < 50.0);
    let metrics = item.get("metrics").and_then(Value::as_object).unwrap();
    assert!(metrics
        .get("机构净买额")
        .and_then(Value::as_str)
        .unwrap()
        .contains("亿"));
    assert_eq!(
        metrics.get("机构买卖比").and_then(Value::as_str),
        Some("0.538")
    );
}

#[test]
fn institution_buy_sell_ratio_handles_zero_sell_amount() {
    assert_eq!(institution_buy_sell_ratio(100.0, 0.0), "∞");
    assert_eq!(institution_buy_sell_ratio(0.0, 0.0), "-");
    assert_eq!(institution_buy_sell_ratio(150.0, 100.0), "1.500");
}

#[test]
fn eastmoney_lhb_parser_returns_no_hit_status() {
    let raw =
        r#"{"result":{"data":[{"SECURITY_CODE":"600000","TRADE_DATE":"2026-06-17 00:00:00"}]}}"#;
    let item = parse_eastmoney_lhb_item(raw, "000725.SZ", "2026-06-01", "2026-06-25").unwrap();
    assert_eq!(
        item.get("category").and_then(Value::as_str),
        Some("institution_lhb_status")
    );
}

#[test]
fn eastmoney_lhb_seats_merge_buy_and_sell_without_inventing_one_sided_net() {
    let buy_raw = r#"{"result":{"data":[
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"A","OPERATEDEPT_NAME":"机构专用","BUY":100000000.0,"TOTAL_BUYRIO":12.5,"CHANGE_RATE":9.91,"EXPLANATION":"日涨幅偏离值达到7%"},
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"B","OPERATEDEPT_NAME":"某证券上海营业部","BUY":50000000.0,"CHANGE_RATE":9.91}
    ]}}"#;
    let sell_raw = r#"{"result":{"data":[
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"A","OPERATEDEPT_NAME":"机构专用","SELL":30000000.0,"TOTAL_SELLRIO":4.2},
        {"SECURITY_CODE":"600360","TRADE_DATE":"2026-07-14","OPERATEDEPT_CODE":"C","OPERATEDEPT_NAME":"某证券深圳营业部","SELL":80000000.0,"CHANGE_RATE":9.91}
    ]}}"#;

    let buy = parse_eastmoney_lhb_seat_side(buy_raw, "600360", true).unwrap();
    let sell = parse_eastmoney_lhb_seat_side(sell_raw, "600360", false).unwrap();
    let seats = merge_eastmoney_lhb_seats(buy, sell);

    assert_eq!(seats.len(), 3);
    let institution = seats
        .iter()
        .find(|seat| seat.get("seat_code").and_then(Value::as_str) == Some("A"))
        .expect("institution seat");
    assert_eq!(
        institution.get("direction").and_then(Value::as_str),
        Some("both")
    );
    assert_eq!(
        institution.get("net_amount").and_then(Value::as_f64),
        Some(70_000_000.0)
    );

    let buy_only = seats
        .iter()
        .find(|seat| seat.get("seat_code").and_then(Value::as_str) == Some("B"))
        .expect("buy-only seat");
    assert_eq!(
        buy_only.get("direction").and_then(Value::as_str),
        Some("buy")
    );
    assert!(buy_only.get("net_amount").is_some_and(Value::is_null));
}

#[test]
fn eastmoney_main_fund_flow_parser_uses_latest_requested_trade_date() {
    let raw = r#"{"data":{"klines":["2026-07-15,762761376.0,-551448.0,-762209936.0,582441216.0,180320160.0,8.55,-0.01,-8.54,6.53,2.02,1251.06,2.98","2026-07-16,-79273120.0,-267954.0,79541072.0,81928864.0,-161201984.0,-1.32,-0.00,1.33,1.37,-2.69,1258.99,0.63","2026-07-17,-854126672.0,-610634.0,854737312.0,-71324912.0,-782801760.0,-11.66,-0.01,11.67,-0.97,-10.69,1253.00,-0.48"]}}"#;

    let item = parse_eastmoney_main_fund_flow_item(raw, "600519.SH", "2026-07-16").unwrap();

    assert_eq!(
        item.get("category").and_then(Value::as_str),
        Some("fund_flow")
    );
    assert_eq!(item.get("date").and_then(Value::as_str), Some("2026-07-16"));
    assert_eq!(
        item.get("sentiment").and_then(Value::as_str),
        Some("uncertain")
    );
    let metrics = item.get("metrics").and_then(Value::as_object).unwrap();
    assert_eq!(
        metrics.get("主力净占比").and_then(Value::as_str),
        Some("-1.32%")
    );
    assert_eq!(
        metrics.get("主力介入度").and_then(Value::as_str),
        Some("低（1.32%）")
    );
    assert!(metrics
        .get("通俗结论")
        .and_then(Value::as_str)
        .unwrap()
        .contains("净卖出"));
}

#[test]
fn main_fund_flow_conclusion_explains_high_outflow_is_not_positive() {
    assert_eq!(main_fund_involvement(-11.66), "高");
    let conclusion = main_fund_flow_plain_conclusion(-11.66);
    assert!(conclusion.contains("每 100 元成交约有 11.66 元"));
    assert!(conclusion.contains("净卖出"));
    assert!(conclusion.contains("影响较大"));
}

#[test]
fn merging_real_main_fund_flow_preserves_local_proxy() {
    let code = "000001.SZ";
    let mut data = json!({
        "capital_evidence": {
            "000001.SZ": {
                "items": [
                    {
                        "category": "fund_flow",
                        "title": "本地量价资金代理",
                        "source": "Tauri/Rust 日线量价",
                        "metrics": {"证据类型": "本地日线量价代理"}
                    },
                    {
                        "category": "fund_flow",
                        "title": "旧主力资金流",
                        "date": "2026-07-16",
                        "metrics": {"主力净占比": "1.00%"}
                    }
                ]
            }
        }
    });
    let refreshed = json!({
        "category": "fund_flow",
        "title": "当日主力资金流",
        "date": "2026-07-17",
        "metrics": {"主力净占比": "-2.00%"}
    });

    assert!(merge_capital_evidence_items(
        &mut data,
        code,
        vec![refreshed],
        "2026-07-17",
    ));

    let items = data["capital_evidence"][code]["items"]
        .as_array()
        .expect("merged capital evidence items");
    let fund_flow_items = items
        .iter()
        .filter(|item| item.get("category").and_then(Value::as_str) == Some("fund_flow"))
        .collect::<Vec<_>>();
    assert_eq!(fund_flow_items.len(), 2);
    assert_eq!(
        fund_flow_items
            .iter()
            .filter(|item| is_local_fund_flow_proxy_value(item))
            .count(),
        1
    );
    let real_item = fund_flow_items
        .iter()
        .find(|item| !is_local_fund_flow_proxy_value(item))
        .expect("real fund flow item");
    assert_eq!(
        real_item.get("date").and_then(Value::as_str),
        Some("2026-07-17")
    );
}

#[test]
fn ths_quarterly_eps_parser_extracts_metric_value() {
    let raw = json!({
        "data": {
            "data": [
                {
                    "date": "2026-06-30",
                    "index_list": {
                        "基本每股收益": {"value": "0.88"},
                        "每股净资产": {"value": "5.2"}
                    }
                }
            ]
        }
    })
    .to_string();

    let rows = parse_ths_quarterly_eps_json(&raw).expect("ths parser");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("period").and_then(Value::as_str),
        Some("2026Q2")
    );
    assert_eq!(rows[0].get("value").and_then(Value::as_f64), Some(0.88));
}

#[test]
fn sina_quarterly_eps_parser_extracts_table_cells() {
    let html = r#"
            <table>
              <tr><th>指标</th><th>2026-03-31</th><th>2025-03-31</th></tr>
              <tr><td>基本每股收益</td><td>0.42</td><td>0.36</td></tr>
            </table>
        "#;

    let rows = parse_sina_quarterly_eps_html(html);

    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("period").and_then(Value::as_str),
        Some("2026Q1")
    );
    assert_eq!(
        rows[1].get("period").and_then(Value::as_str),
        Some("2025Q1")
    );
}

#[test]
fn basic_financial_merge_adds_tdx_latest_eps_as_quarter() {
    let mut data = json!({
        "stocks": [
            {
                "code": "000001.SZ",
                "latest_eps": 0.42,
                "latest_bps": 12.5,
                "period": "2026Q1"
            }
        ],
        "financials": {}
    });

    assert!(merge_basic_financial_from_stock(&mut data, "000001.SZ"));
    assert_eq!(financial_quarterly_eps_count(&data, "000001.SZ"), 1);
    let entry = data
        .get("financials")
        .and_then(Value::as_object)
        .and_then(|items| items.get("000001.SZ"))
        .and_then(Value::as_object)
        .expect("financial entry");
    assert_eq!(entry.get("latest_eps").and_then(Value::as_f64), Some(0.42));
    assert!(entry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("")
        .contains("通达信基础财务"));
}
#[test]
fn append_all_preserved_seed_stocks_keeps_rows_outside_current_batch() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0},
            {"code": "600000.SH", "name": "SPD Bank", "price": 8.0},
            {"code": "688001.SH", "name": "STAR", "price": 20.0}
        ]
    });
    let (seed_stocks, _) = seed_stock_maps(&seed);
    let mut stocks = vec![json!({"code": "000001.SZ", "name": "Live Quote"})];
    let mut seen = HashSet::from(["000001.SZ".to_string()]);

    let preserved =
        append_all_preserved_seed_stocks(&seed_stocks, &seed_stocks, &mut stocks, &mut seen);

    assert_eq!(preserved, 2);
    assert_eq!(stocks.len(), 3);
    assert!(stocks
        .iter()
        .any(|stock| stock.get("code").and_then(Value::as_str) == Some("600000.SH")));
    assert!(stocks
        .iter()
        .any(|stock| stock.get("code").and_then(Value::as_str) == Some("688001.SH")));
}
#[test]
fn financial_snapshot_financials_object_enriches_screen_stock_rows() {
    let seed = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "financials": {
            "000001.SZ": {"latest_eps": 0.45, "latest_bps": 12.3}
        }
    });
    let (seed_stocks, seed_codes) = seed_stock_maps(&seed);
    let enriched = enriched_stock_maps(&seed_stocks, &snapshot);
    let mut stocks = Vec::new();
    let mut seen = HashSet::new();
    append_preserved_seed_stocks(&seed_codes, &enriched, &mut stocks, &mut seen);

    assert_eq!(stocks.len(), 1);
    assert_eq!(
        stocks[0].get("latest_eps").and_then(Value::as_f64),
        Some(0.45)
    );
    assert_eq!(
        stocks[0].get("latest_bps").and_then(Value::as_f64),
        Some(12.3)
    );
}

#[test]
fn core_payload_strips_financial_snapshot_after_merging_screen_rows() {
    let mut data = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "Ping An Bank", "price": 10.0}
        ]
    });
    let snapshot = json!({
        "financials": {
            "000001.SZ": {"latest_eps": 0.45}
        }
    });
    merge_screen_financial_snapshot_into_data(&mut data, &snapshot);
    let stripped = strip_core_side_payload_fields(json!({
        "financial_snapshot": snapshot,
        "limit": 10
    }));

    assert_eq!(
        data["stocks"][0].get("latest_eps").and_then(Value::as_f64),
        Some(0.45)
    );
    assert!(stripped.get("financial_snapshot").is_none());
    assert_eq!(stripped.get("limit").and_then(Value::as_i64), Some(10));
}

#[test]
fn mobile_market_cache_reuses_matching_metadata_and_invalidates_on_size() {
    let path = PathBuf::from("test-mobile-market-cache.json");
    let data = json!({
        "stocks": [{"code": "000001.SZ", "name": "Ping An Bank", "industry": "Banking", "price": 10.0}],
        "relations": []
    });
    let (typed, summary) =
        parse_mobile_market_data_snapshot(&data, "test market data").expect("valid summary");

    forget_mobile_market_data_cache(&path);
    remember_mobile_market_data_cache(&path, 100, Some(1), &data, typed, &summary);

    let reused = cached_mobile_market_data_entry(&path, 100, Some(1)).expect("cache hit");
    assert_eq!(reused.bytes, 100);
    assert_eq!(reused.data["stocks"][0]["code"].as_str(), Some("000001.SZ"));
    assert!(cached_mobile_market_data_entry(&path, 101, Some(1)).is_none());
    assert!(cached_mobile_market_data_entry(&path, 100, Some(2)).is_none());

    forget_mobile_market_data_cache(&path);
    assert!(cached_mobile_market_data_entry(&path, 100, Some(1)).is_none());
}

#[test]
fn trend_history_prefetch_codes_normalizes_and_deduplicates_candidates() {
    let result = json!({
        "items": [
            {"stock": {"code": "000001"}},
            {"stock": {"code": "000001.SZ"}},
            {"stock": {"code": "600000.SH"}},
            {"stock": {"code": "invalid"}}
        ]
    });

    assert_eq!(
        trend_history_prefetch_codes(&result, 10),
        vec!["000001.SZ".to_string(), "600000.SH".to_string()]
    );
}

#[test]
fn trend_history_cache_requires_enough_bars_inside_requested_window() {
    let data = json!({
        "histories": {
            "000001.SZ": [
                {"date": "2026-03-04", "close": 9.8},
                {"date": "2026-03-05", "close": 9.9},
                {"date": "2026-03-06", "close": 10.0}
            ]
        }
    });

    assert!(history_cache_has_bars(
        &data,
        "000001.SZ",
        "20260305",
        "20260306",
        2
    ));
    assert!(!history_cache_has_bars(
        &data,
        "000001.SZ",
        "20260305",
        "20260306",
        3
    ));
}

#[test]
fn backtest_prefetch_matches_core_watchlist_top_n_slots() {
    let universe = ["000001.SZ", "600000.SH", "002432.SZ"]
        .into_iter()
        .map(|code| gp_core::StockItem {
            code: code.to_string(),
            ..gp_core::StockItem::default()
        })
        .collect::<Vec<_>>();
    let request = gp_core::BacktestRequest {
        criteria: gp_core::ScreenCriteria::default(),
        source: "watchlist".to_string(),
        strategy_mode: "candidate_snapshot".to_string(),
        stock_codes: vec![
            "600000".to_string(),
            "unknown".to_string(),
            "600000.SH".to_string(),
            "000001.SZ".to_string(),
            "002432.SZ".to_string(),
        ],
        start_date: "20200101".to_string(),
        end_date: "20260717".to_string(),
        top_n: 2,
        initial_cash: 1000.0,
        rebalance_frequency: "monthly".to_string(),
        transaction_cost_bps: 10.0,
        benchmark: "none".to_string(),
    };

    assert_eq!(
        backtest_history_prefetch_codes(&universe, &request),
        vec!["600000.SH".to_string()]
    );
}

#[test]
fn backtest_prefetch_rejects_single_history_row() {
    assert!(!backtest_history_rows_are_usable(&[json!({
        "date": "2026-07-17",
        "close": 10.0
    })]));
    assert!(backtest_history_rows_are_usable(&[
        json!({ "date": "2026-07-16", "close": 9.8 }),
        json!({ "date": "2026-07-17", "close": 10.0 }),
    ]));
    assert!(!backtest_history_rows_are_usable(&[
        json!({ "date": "2026-07-17", "close": 9.8 }),
        json!({ "date": "2026-07-17", "close": 10.0 }),
    ]));
    assert!(!backtest_history_rows_are_usable(&[
        json!({ "date": "2026-07-16", "close": 0.0 }),
        json!({ "date": "2026-07-17", "close": 10.0 }),
    ]));
}

#[test]
fn market_data_patch_updates_only_requested_stock() {
    let source = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "A", "price": 10.5},
            {"code": "600000.SH", "name": "B", "price": 8.0}
        ],
        "histories": {
            "000001.SZ": [{"date": "2026-07-09", "close": 10.5}],
            "600000.SH": [{"date": "2026-07-09", "close": 8.0}]
        },
        "financials": {
            "000001.SZ": {"latest_eps": 1.2},
            "600000.SH": {"latest_eps": 0.8}
        }
    });
    let patch = market_data_patch_for_codes(&source, &["000001.SZ".to_string()]);
    assert!(patch["histories"].get("000001.SZ").is_some());
    assert!(patch["histories"].get("600000.SH").is_none());

    let mut target = json!({
        "stocks": [
            {"code": "000001.SZ", "name": "A", "price": 9.5},
            {"code": "600000.SH", "name": "B", "price": 8.0}
        ],
        "histories": {
            "600000.SH": [{"date": "2026-07-09", "close": 8.0}]
        },
        "financials": {
            "600000.SH": {"latest_eps": 0.8}
        }
    });
    apply_market_data_patch(&mut target, &patch);

    assert_eq!(target["stocks"][0]["price"].as_f64(), Some(10.5));
    assert_eq!(target["stocks"][1]["price"].as_f64(), Some(8.0));
    assert_eq!(
        target["financials"]["000001.SZ"]["latest_eps"].as_f64(),
        Some(1.2)
    );
    assert_eq!(
        target["financials"]["600000.SH"]["latest_eps"].as_f64(),
        Some(0.8)
    );
}
