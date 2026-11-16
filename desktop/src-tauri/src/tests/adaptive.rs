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
fn adaptive_backtest_requirements_fail_before_history_fetch_without_point_in_time_factors() {
    let data = gp_core::CoreDataSet {
        stocks: vec![gp_core::StockItem {
            code: "111111.SZ".to_string(),
            ..gp_core::StockItem::default()
        }],
        ..gp_core::CoreDataSet::default()
    };
    let request = gp_core::BacktestRequest {
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
        ..gp_core::BacktestRequest::default()
    };

    let error = backtest_history_requirements(&data, None, &request)
        .expect_err("adaptive requirements must reject missing point-in-time factors");

    assert!(error.contains("factor_snapshots"));
    assert!(error.contains("111111.SZ"));
}

#[test]
fn candidate_snapshot_watchlist_requirements_keep_saved_codes() {
    let data = gp_core::CoreDataSet {
        stocks: vec![gp_core::StockItem {
            code: "111111.SZ".to_string(),
            ..gp_core::StockItem::default()
        }],
        ..gp_core::CoreDataSet::default()
    };
    let request = gp_core::BacktestRequest {
        criteria: gp_core::ScreenCriteria::default(),
        source: "watchlist".to_string(),
        strategy_mode: "candidate_snapshot".to_string(),
        stock_codes: vec!["111111.SZ".to_string()],
        start_date: "20200101".to_string(),
        end_date: "20200103".to_string(),
        top_n: 10,
        initial_cash: 1_000.0,
        rebalance_frequency: "monthly".to_string(),
        transaction_cost_bps: 10.0,
        benchmark: "candidate_equal_weight".to_string(),
        ..gp_core::BacktestRequest::default()
    };

    let symbols = backtest_history_requirements(&data, None, &request)
        .expect("candidate snapshot requirements should keep watchlist codes");

    assert_eq!(symbols, vec!["111111.SZ"]);
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
fn screen_routing_defaults_to_adaptive_and_keeps_explicit_legacy_compatibility() {
    assert!(
        !legacy_screen_requested(&json!({})),
        "ordinary screen requests must use adaptive_swing_v1"
    );
    assert!(
        !legacy_screen_requested(&json!({ "internal_algorithm": "adaptive_swing_v1" })),
        "an explicit adaptive request must use adaptive_swing_v1"
    );
    assert!(
        legacy_screen_requested(&json!({ "internal_algorithm": "legacy_balanced" })),
        "legacy_balanced remains available only through an explicit compatibility request"
    );
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
fn adaptive_history_window_is_invariant_to_bars_after_the_common_as_of_date() {
    let rows = (0..62)
        .map(|index| gp_core::HistoryBar {
            date: format!("{:08}", 20_260_101 + index),
            open: Some(10.0 + index as f64),
            high: Some(10.2 + index as f64),
            low: Some(9.8 + index as f64),
            close: 10.0 + index as f64,
            volume: Some(1_000.0),
            capital: None,
        })
        .collect::<Vec<_>>();
    let before_future = adaptive_history_window(&rows[..60], Some("20260160"));
    let with_future = adaptive_history_window(&rows, Some("20260160"));
    let signature = |bars: &[gp_core::HistoryBar]| {
        bars.iter()
            .map(|bar| (bar.date.clone(), bar.close))
            .collect::<Vec<_>>()
    };
    assert_eq!(signature(&before_future), signature(&with_future));
    assert_eq!(
        with_future.last().map(|bar| bar.date.as_str()),
        Some("20260160")
    );

    let without_exact_date = rows
        .iter()
        .filter(|bar| bar.date != "20260160")
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        adaptive_history_window(&without_exact_date, Some("20260160")).is_empty(),
        "a candidate without an exact as-of bar must count as missing coverage"
    );
}

#[test]
fn adaptive_universe_drops_quote_fields_from_after_the_common_as_of_date() {
    let stock = gp_core::StockItem {
        code: "600000.SH".to_string(),
        price: 99.0,
        change_pct: Some(0.15),
        volume: Some(9_999.0),
        amount: Some(8_000_000_000.0),
        turnover_rate: Some(0.25),
        volume_ratio: Some(8.0),
        quote_time: Some("2026-07-30 15:00:00".to_string()),
        ..gp_core::StockItem::default()
    };
    let histories = HashMap::from([(
        stock.code.clone(),
        vec![
            gp_core::HistoryBar {
                date: "20260728".to_string(),
                open: Some(10.0),
                high: Some(10.0),
                low: Some(10.0),
                close: 10.0,
                volume: Some(1_000.0),
                capital: None,
            },
            gp_core::HistoryBar {
                date: "20260729".to_string(),
                open: Some(10.0),
                high: Some(10.5),
                low: Some(10.0),
                close: 10.5,
                volume: Some(1_500.0),
                capital: None,
            },
        ],
    )]);

    let normalized = adaptive_point_in_time_universe(&[stock], &histories, Some("20260729"));
    let item = &normalized[0];
    assert_eq!(item.price, 10.5);
    assert!((item.change_pct.unwrap_or_default() - 0.05).abs() < f64::EPSILON * 4.0);
    assert_eq!(item.volume, Some(1_500.0));
    assert_eq!(item.amount, None);
    assert_eq!(item.turnover_rate, None);
    assert_eq!(item.volume_ratio, None);
    assert_eq!(item.quote_time.as_deref(), Some("20260729"));
}

#[test]
fn adaptive_universe_keeps_same_day_market_breadth_without_prefetched_history() {
    let stock = gp_core::StockItem {
        code: "600000.SH".to_string(),
        price: 99.0,
        change_pct: Some(0.012),
        volume: Some(9_999.0),
        amount: Some(800_000_000.0),
        turnover_rate: Some(0.025),
        volume_ratio: Some(1.2),
        quote_time: Some("2026-07-29 15:00:00".to_string()),
        ..gp_core::StockItem::default()
    };
    let histories = HashMap::<String, Vec<gp_core::HistoryBar>>::new();

    let normalized = adaptive_point_in_time_universe(&[stock], &histories, Some("20260729"));
    let item = &normalized[0];
    assert_eq!(item.price, 99.0);
    assert_eq!(item.change_pct, Some(0.012));
    assert_eq!(item.volume, Some(9_999.0));
    assert_eq!(item.amount, Some(800_000_000.0));
    assert_eq!(item.turnover_rate, Some(0.025));
    assert_eq!(item.volume_ratio, Some(1.2));
}

#[test]
fn adaptive_runtime_limits_match_the_release_contract() {
    assert_eq!(ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT, 80);
    assert_eq!(ADAPTIVE_SCREEN_TOTAL_TIMEOUT_SECS, 120);
    assert_eq!(ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS, 90);
}

#[test]
fn backtest_history_timeout_is_extended_only_for_internal_release_validation() {
    let mut payload = json!({});
    assert_eq!(backtest_history_timeout_secs(&payload), 30);
    payload[stringify!(internal_release_validation)] = json!(true);
    assert_eq!(backtest_history_timeout_secs(&payload), 180);
    payload[stringify!(internal_release_validation)] = json!(false);
    assert_eq!(backtest_history_timeout_secs(&payload), 30);
}

#[test]
fn adaptive_release_cold_start_flag_is_explicit() {
    let mut payload = json!({});
    assert_eq!(
        adaptive_release_validation_force_cold_start(&payload),
        false
    );
    payload[stringify!(internal_release_validation_cold_start)] = json!(true);
    assert_eq!(adaptive_release_validation_force_cold_start(&payload), true);
}

#[test]
fn adaptive_release_cold_start_adds_first_code_without_dropping_real_missing_codes() {
    let required_codes = vec!["600000.SH".to_string(), "000001.SZ".to_string()];
    let mut missing = vec!["000001.SZ".to_string()];
    adaptive_release_force_cold_start_code(&mut missing, &required_codes);
    assert_eq!(
        missing,
        vec!["000001.SZ".to_string(), "600000.SH".to_string()]
    );
    adaptive_release_force_cold_start_code(&mut missing, &required_codes);
    assert_eq!(
        missing,
        vec!["000001.SZ".to_string(), "600000.SH".to_string()]
    );
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
fn adaptive_history_progress_is_incremental_and_bounded() {
    assert_eq!(adaptive_history_progress_percent(0, 83), 24);
    assert_eq!(adaptive_history_progress_percent(1, 83), 25);
    assert!(adaptive_history_progress_percent(40, 83) > 25);
    assert_eq!(adaptive_history_progress_percent(83, 83), 72);
    assert_eq!(adaptive_history_progress_percent(100, 83), 72);
    assert_eq!(adaptive_history_progress_percent(0, 0), 72);
}

#[test]
fn adaptive_history_timeout_keeps_completed_results_and_reports_progress() {
    let progress = Arc::new(Mutex::new(Vec::new()));
    let progress_sink = Arc::clone(&progress);
    let outcome = tauri::async_runtime::block_on(collect_adaptive_history_results(
        vec!["fast".to_string(), "slow".to_string()],
        "20260101".to_string(),
        Duration::from_millis(50),
        2,
        |code, _start_date| {
            let slow = code == "slow";
            async move {
                if slow {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Ok(Vec::<Value>::new())
            }
        },
        move |completed, total| {
            progress_sink
                .lock()
                .expect("progress mutex should lock")
                .push((completed, total));
        },
    ));

    assert!(outcome.timed_out);
    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        progress
            .lock()
            .expect("progress mutex should lock")
            .as_slice(),
        &[(1, 2)]
    );
}

#[test]
fn adaptive_timeout_and_progress_payloads_enforce_the_runtime_contract() {
    let timed_out = tauri::async_runtime::block_on(adaptive_screen_with_timeout(
        Duration::from_millis(1),
        std::future::pending::<Result<(), String>>(),
    ))
    .expect_err("a pending adaptive run must respect the timeout wrapper");
    assert!(timed_out.contains("120"));

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
        ..gp_core::BacktestRequest::default()
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
fn adaptive_release_industry_count_uses_each_folds_point_in_time_snapshots() {
    let fold = gp_core::WalkForwardFold {
        signal_date: Some("2026-01-31".to_string()),
        selection_date: "2026-02-01".to_string(),
        evaluation_end_date: None,
        selected_symbols: vec!["111111.SZ".to_string(), "222222.SZ".to_string()],
        eligible_symbol_count: 2,
        evaluated_selection_count: 0,
        hit_count: 0,
        precision_at_n: None,
        average_forward_return: None,
        benchmark_forward_return: None,
        average_excess_return: None,
    };
    let snapshot = |industry: &str, available_date: &str| gp_core::StockFactorSnapshot {
        date: "2025-12-31".to_string(),
        available_date: Some(available_date.to_string()),
        industry: Some(industry.to_string()),
        ..gp_core::StockFactorSnapshot::default()
    };
    let data = gp_core::CoreDataSet {
        factor_snapshots: HashMap::from([
            (
                "111111.SZ".to_string(),
                vec![
                    snapshot("历史行业甲", "20260110"),
                    snapshot("未来行业", "20260210"),
                ],
            ),
            (
                "222222.SZ".to_string(),
                vec![snapshot("历史行业乙", "20260110")],
            ),
        ]),
        ..gp_core::CoreDataSet::default()
    };
    assert_eq!(
        adaptive_backtest_fold_max_industry_count(&fold, &data),
        Some(1),
        "historical-only stocks in different PIT industries must not collapse into unknown"
    );

    let missing = gp_core::CoreDataSet::default();
    assert_eq!(
        adaptive_backtest_fold_max_industry_count(&fold, &missing),
        None,
        "missing historical industry evidence must fail the release metric closed"
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
fn adaptive_screen_uses_quote_date_for_market_breadth_when_history_is_older() {
    assert_eq!(
        adaptive_screen_as_of_date(
            None,
            Some("20261009".to_string()),
            Some("20260930".to_string()),
        ),
        Some("20261009".to_string()),
    );
    assert_eq!(
        adaptive_screen_as_of_date(
            Some("20260729".to_string()),
            Some("20261009".to_string()),
            Some("20260930".to_string()),
        ),
        Some("20260729".to_string()),
    );
}

#[test]
fn trend_refresh_accepts_partial_history_and_leaves_short_ipo_candidates_to_core() {
    assert!(trend_screen_refresh_has_usable_history(&[
        json!({"code":"600000.SH", "sufficient": true}),
        json!({"code":"920181.BJ", "sufficient": false}),
    ]));
    assert!(!trend_screen_refresh_has_usable_history(&[
        json!({"code":"920181.BJ", "sufficient": false}),
        json!({"code":"920012.BJ", "sufficient": false}),
    ]));
}

#[test]
#[ignore = "requires GP_REAL_MARKET_CACHE and live Tencent daily history"]
fn real_cached_adaptive_screen_smoke_uses_tencent_history() {
    let cache_path = std::env::var("GP_REAL_MARKET_CACHE")
        .expect("set GP_REAL_MARKET_CACHE to mobile-market-data.json");
    let raw = std::fs::read_to_string(&cache_path)
        .unwrap_or_else(|error| panic!("read real market cache failed at {cache_path}: {error}"));
    let data_value = serde_json::from_str::<Value>(&raw)
        .unwrap_or_else(|error| panic!("real market cache is not valid JSON: {error}"));
    let (data, summary) =
        parse_mobile_market_data_snapshot(&data_value, "real cached mobile market data")
            .unwrap_or_else(|error| panic!("real market cache parse failed: {error}"));

    let mut request = gp_core::AdaptiveScreenRequest::default();
    request.criteria.limit = ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT;
    request.primary_limit = 10;
    request.exploration_limit = 10;

    let candidates = gp_core::adaptive_candidate_codes(
        data.stocks.as_slice(),
        &request.criteria,
        ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT,
    );
    assert_eq!(
        candidates.len(),
        ADAPTIVE_SCREEN_HISTORY_PREFETCH_LIMIT,
        "real cache should produce the full adaptive prefetch pool; summary={summary}"
    );

    let required_codes = adaptive_required_history_codes(&candidates);
    let target_history_date = adaptive_quote_target_date(data.stocks.as_slice(), &candidates)
        .or_else(|| expected_market_quote_date_from_epoch_ms(epoch_millis()));
    let missing = adaptive_missing_history_codes(
        data.as_ref(),
        &required_codes,
        target_history_date.as_deref(),
    );

    let history_start_date = adaptive_history_start_date();
    let fetch_outcome = tauri::async_runtime::block_on(collect_adaptive_history_results(
        missing.clone(),
        history_start_date,
        Duration::from_secs(ADAPTIVE_SCREEN_HISTORY_PREFETCH_TIMEOUT_SECS),
        ADAPTIVE_SCREEN_HISTORY_CONCURRENCY,
        |code, start_date| {
            let code = code.to_string();
            let start_date = start_date.to_string();
            async move { fetch_observe_daily_history(&code, &start_date, "20501231").await }
        },
        |_, _| {},
    ));

    let mut history_override = HashMap::new();
    let mut failed = Vec::new();
    for (code, result) in fetch_outcome.results {
        match result {
            Ok(rows) if !rows.is_empty() => {
                let rows = rows
                    .into_iter()
                    .rev()
                    .take(120)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>();
                let typed = serde_json::from_value::<Vec<gp_core::HistoryBar>>(Value::Array(rows))
                    .unwrap_or_else(|error| panic!("history parse failed for {code}: {error}"));
                history_override.insert(code, typed);
            }
            Ok(_) => failed.push(format!("{code}: empty history")),
            Err(error) => failed.push(format!("{code}: {error}")),
        }
    }

    let fetched_count = history_override.len();
    request.as_of_date =
        latest_adaptive_data_date(data.as_ref(), &history_override, &required_codes)
            .or_else(|| target_history_date.clone());
    let calculation_as_of = request.as_of_date.as_deref();
    let mut histories = candidates
        .iter()
        .map(String::as_str)
        .chain(adaptive_benchmark_codes())
        .filter_map(|code| {
            data.histories.get(code).map(|rows| {
                (
                    code.to_string(),
                    adaptive_history_window(rows, calculation_as_of),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    histories.extend(
        history_override
            .into_iter()
            .map(|(code, rows)| (code, adaptive_history_window(&rows, calculation_as_of))),
    );
    let benchmark_histories = adaptive_benchmark_codes()
        .into_iter()
        .filter_map(|code| {
            histories
                .get(code)
                .cloned()
                .map(|rows| (code.to_string(), rows))
        })
        .collect::<HashMap<_, _>>();
    let point_in_time_universe =
        adaptive_point_in_time_universe(data.stocks.as_slice(), &histories, calculation_as_of);

    println!(
        "real adaptive smoke pre-core: cache={}, candidates={}, required={}, missing={}, fetched={}, failed={}, timed_out={}, target={:?}, as_of={:?}",
        cache_path,
        candidates.len(),
        required_codes.len(),
        missing.len(),
        fetched_count,
        failed.len(),
        fetch_outcome.timed_out,
        target_history_date,
        request.as_of_date
    );
    if !failed.is_empty() {
        println!(
            "failed history samples: {:?}",
            &failed[..failed.len().min(5)]
        );
    }

    let result = gp_core::adaptive_screen_stocks(
        &point_in_time_universe,
        &histories,
        &benchmark_histories,
        &[],
        &request,
    )
    .unwrap_or_else(|error| {
        panic!(
            "real adaptive smoke failed: candidates={}, required={}, missing={}, fetched={}, failed={}, timed_out={}, as_of={:?}, error={}",
            candidates.len(),
            required_codes.len(),
            missing.len(),
            fetched_count,
            failed.len(),
            fetch_outcome.timed_out,
            request.as_of_date,
            error
        )
    });
    let primary_count = result
        .groups
        .iter()
        .find(|group| group.key == "primary")
        .map(|group| group.items.len())
        .unwrap_or(0);
    let exploration_count = result
        .groups
        .iter()
        .find(|group| group.key == "exploration")
        .map(|group| group.items.len())
        .unwrap_or(0);
    println!(
        "real adaptive smoke ok: cache={}, candidates={}, required={}, missing={}, fetched={}, failed={}, timed_out={}, as_of={:?}, total={}, returned={}, primary={}, exploration={}",
        cache_path,
        candidates.len(),
        required_codes.len(),
        missing.len(),
        fetched_count,
        failed.len(),
        fetch_outcome.timed_out,
        request.as_of_date,
        result.total,
        result.returned,
        primary_count,
        exploration_count
    );

    assert_eq!(primary_count, request.primary_limit);
    assert!(
        exploration_count > 0,
        "exploration group should not be empty"
    );
}
