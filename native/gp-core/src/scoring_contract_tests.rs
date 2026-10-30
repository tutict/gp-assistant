use super::*;

fn factors(value: f64, overheat: f64) -> BTreeMap<String, f64> {
    [
        "quality",
        "valuation",
        "range_setup",
        "trend",
        "relative_strength",
        "liquidity",
        "risk",
        "stability",
    ]
    .into_iter()
    .map(|key| (key.to_string(), value))
    .chain(std::iter::once(("overheat_penalty".into(), overheat)))
    .collect()
}

fn calibrated() -> ScoringParameters {
    let mut p = fixed_scoring_parameters();
    p.algorithm_version = "adaptive_swing_v2".into();
    p.parameter_set_id = "test-calibrated-v1".into();
    p
}

#[test]
fn rate_unit_conversions_are_explicit_finite_and_equivalent() {
    let pp = RateRatio::from_percentage_points(0.804352).unwrap();
    let ratio = RateRatio::from_ratio(0.00804352).unwrap();
    assert!((pp.as_ratio() - ratio.as_ratio()).abs() < 1e-15);
    assert!((ratio.as_percentage_points().unwrap() - 0.804352).abs() < 1e-12);
    assert_eq!(
        RateRatio::from_ratio(1.2)
            .unwrap()
            .as_percentage_points()
            .unwrap(),
        120.0
    );
    assert_eq!(
        serde_json::to_string(&RateUnit::PercentagePoints).unwrap(),
        "\"percentage_points\""
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(RateRatio::from_ratio(invalid).is_err());
        assert!(RateRatio::from_percentage_points(invalid).is_err());
    }
    assert!(RateRatio::from_ratio(f64::MAX)
        .unwrap()
        .as_percentage_points()
        .is_err());
    assert!(serde_json::from_str::<RateRatio>("1e999").is_err());
}

#[test]
fn fixed_parameters_export_exact_metadata_and_mode_supports() {
    let p = fixed_scoring_parameters();
    validate_scoring_parameters(&p).unwrap();
    assert_eq!(p.metric_contract_version, 2);
    assert_eq!(p.scoring_revision, "unit-rank-fix/v1");
    assert_eq!(p.parameter_set_id, "fixed-v1");
    assert_eq!(p.algorithm_version, "adaptive_swing_v1");
    assert_eq!(
        p.implementation_fingerprint,
        scoring_implementation_fingerprint()
    );
    assert_eq!(p.implementation_fingerprint.len(), 64);
    assert_eq!(
        p.modes.keys().map(String::as_str).collect::<Vec<_>>(),
        ["defensive", "range", "trend"]
    );
    for mode in p.modes.values() {
        assert_eq!(mode.weights.len(), 8);
        assert!((mode.weights.values().sum::<f64>() - 1.0).abs() < 1e-12);
    }
    let json = serde_json::to_string(&p).unwrap();
    validate_scoring_parameters(&serde_json::from_str(&json).unwrap()).unwrap();
}

#[test]
fn calibrated_parameters_reject_missing_unknown_nonfinite_and_out_of_bounds_values() {
    validate_scoring_parameters(&calibrated()).unwrap();
    let mut cases = Vec::new();
    let mut p = calibrated();
    p.metric_contract_version = 1;
    cases.push(p);
    let mut p = calibrated();
    p.scoring_revision = "old".into();
    cases.push(p);
    let mut p = calibrated();
    p.implementation_fingerprint = "invalid".into();
    cases.push(p);
    let mut p = calibrated();
    p.algorithm_version = "unknown".into();
    cases.push(p);
    let mut p = calibrated();
    p.parameter_set_id.clear();
    cases.push(p);
    let mut p = calibrated();
    p.parameter_set_id = "fixed-v1".into();
    cases.push(p);
    let mut p = calibrated();
    p.modes.remove("trend");
    cases.push(p);
    let mut p = calibrated();
    p.modes
        .insert("transition".into(), p.modes["range"].clone());
    cases.push(p);
    let mut p = calibrated();
    p.modes.get_mut("range").unwrap().weights.remove("risk");
    cases.push(p);
    let mut p = calibrated();
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("unknown".into(), 0.0);
    cases.push(p);
    for invalid in [f64::NAN, f64::INFINITY, -0.01, 0.31] {
        let mut p = calibrated();
        p.modes
            .get_mut("range")
            .unwrap()
            .weights
            .insert("quality".into(), invalid);
        cases.push(p);
    }
    let mut p = calibrated();
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("trend".into(), 0.01);
    cases.push(p);
    let mut p = calibrated();
    p.modes.get_mut("range").unwrap().overheat_penalty = 0.121;
    cases.push(p);
    let mut p = calibrated();
    p.modes.get_mut("range").unwrap().overheat_penalty = f64::NAN;
    cases.push(p);
    let mut p = fixed_scoring_parameters();
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("quality".into(), 0.25);
    cases.push(p);
    let mut p = fixed_scoring_parameters();
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("quality".into(), 0.25);
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("range_setup".into(), 0.21);
    cases.push(p);
    for (i, p) in cases.iter().enumerate() {
        assert!(validate_scoring_parameters(p).is_err(), "case {i}");
    }
    let mut p = calibrated();
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("quality".into(), 0.29);
    p.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("range_setup".into(), 0.17);
    p.modes.get_mut("range").unwrap().overheat_penalty = 0.12;
    validate_scoring_parameters(&p).unwrap();
}

#[test]
fn factor_vector_score_uses_parameters_and_transition_averages_clamped_modes() {
    let p = fixed_scoring_parameters();
    let f = factors(0.5, 0.2);
    assert!((adaptive_score_factor_vector(&f, "range", &p).unwrap() - 9.6).abs() < 1e-12);
    assert!((adaptive_score_factor_vector(&f, "defensive", &p).unwrap() - 9.76).abs() < 1e-12);
    assert!((adaptive_score_factor_vector(&f, "transition", &p).unwrap() - 9.68).abs() < 1e-12);
    let mut sparse = f.clone();
    sparse.remove("quality");
    assert!(adaptive_score_factor_vector(&sparse, "range", &p).is_err());
    assert!(adaptive_score_factor_vector(&f, "unknown", &p).is_err());
    for invalid in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut invalid_factors = f.clone();
        invalid_factors.insert("quality".into(), invalid);
        assert!(adaptive_score_factor_vector(&invalid_factors, "range", &p).is_err());
    }
    let mut extra = f.clone();
    extra.insert("unknown".into(), 0.0);
    assert!(adaptive_score_factor_vector(&extra, "range", &p).is_err());
    let mut f = factors(0.0, 1.0);
    f.insert("quality".into(), 0.25);
    let transition = adaptive_score_factor_vector(&f, "transition", &p).unwrap();
    assert!((transition - 0.15).abs() < 1e-12);
    let mut v2 = calibrated();
    v2.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("quality".into(), 0.29);
    v2.modes
        .get_mut("range")
        .unwrap()
        .weights
        .insert("range_setup".into(), 0.17);
    assert!(
        (adaptive_score_factor_vector(&f, "range", &v2).unwrap()
            - adaptive_score_factor_vector(&f, "range", &p).unwrap())
        .abs()
            < 1e-12
    );
}

#[test]
fn explicit_rate_thresholds_and_displays_fail_closed_on_invalid_rates() {
    assert_eq!(format_percent(f64::INFINITY), "—");
    assert_eq!(format_percent(f64::MAX), "—");
    assert_eq!(format_percentage_points(f64::NAN), "—");
    assert_eq!(format_signed_percent_text(f64::MAX), "—");
    for value in [0.804352, -0.804352] {
        let stock = StockItem {
            price: 10.0,
            roe: Some(1.2),
            deducted_net_profit_margin: Some(value),
            deducted_net_profit_growth_rate: Some(value),
            ..Default::default()
        };
        assert!(matches_stock(
            &stock,
            &ScreenCriteria {
                min_deducted_net_profit_growth_rate: Some(value),
                ..Default::default()
            }
        )
        .is_none());
        assert!(matches_stock(
            &stock,
            &ScreenCriteria {
                min_deducted_net_profit_growth_rate: Some(value - 0.001),
                min_roe: Some(1.2),
                ..Default::default()
            }
        )
        .is_some());
        assert!(matches_stock(
            &stock,
            &ScreenCriteria {
                min_deducted_net_profit_margin: Some(value + 0.001),
                ..Default::default()
            }
        )
        .is_none());
    }
    let missing = StockItem {
        price: 10.0,
        ..Default::default()
    };
    assert!(matches_stock(
        &missing,
        &ScreenCriteria {
            min_roe: Some(0.0),
            ..Default::default()
        }
    )
    .is_none());
    for invalid in [f64::NAN, f64::INFINITY] {
        let stock = StockItem {
            price: 10.0,
            roe: Some(invalid),
            ..Default::default()
        };
        assert!(matches_stock(
            &stock,
            &ScreenCriteria {
                min_roe: Some(0.0),
                ..Default::default()
            }
        )
        .is_none());
    }
    assert_eq!(heuristic_criteria("ROE 0.8%").min_roe, Some(0.008));
    assert_eq!(heuristic_criteria("ROE 120%").min_roe, Some(1.2));
}

#[test]
fn v2_backtest_requires_parameters_before_data_lookup_and_preserves_legacy_defaults() {
    let old: BacktestRequest =
        serde_json::from_value(json!({"start_date":"20260101", "end_date":"20261001"})).unwrap();
    assert!(old.algorithm_version.is_none());
    assert!(old.scoring_parameters.is_none());
    let mut request = BacktestRequest {
        start_date: "20260101".into(),
        end_date: "20261001".into(),
        strategy_mode: "adaptive_swing_v2:range".into(),
        ..Default::default()
    };
    let error = backtest_with_mock(&request).unwrap_err().to_string();
    assert!(error.contains("parameters"), "{error}");
    request.scoring_parameters = Some(calibrated());
    request.parameter_set_id = Some("different".into());
    assert!(backtest_with_mock(&request)
        .unwrap_err()
        .to_string()
        .contains("parameter_set_id"));
    assert_eq!(
        normalize_backtest_strategy_mode("adaptive_swing_v2:range"),
        "adaptive_swing_v2"
    );
}

#[test]
fn ordinary_score_breakdown_includes_clamps_and_industry_adjustments() {
    let stocks = mock_stocks();
    for profile in ["balanced", "quality", "trend", "rotation"] {
        let result = screen_stocks(
            &stocks,
            &ScreenCriteria {
                score_profile: profile.into(),
                ..Default::default()
            },
        );
        for row in result.items {
            let sum = row
                .score_breakdown
                .iter()
                .filter_map(|entry| entry.contribution)
                .sum::<f64>();
            assert!(
                (sum - row.score).abs() < 1e-9,
                "{profile}/{}: {sum} != {}",
                row.stock.code,
                row.score
            );
        }
    }
}

#[test]
fn shared_ranking_neutralizes_unknown_board_and_singleton_groups() {
    let values = industry_rank_values([
        ("a".into(), "bank".into(), 1.0),
        ("b".into(), "bank".into(), 1.0),
        ("c".into(), "bank".into(), 2.0),
        ("d".into(), "single".into(), 100.0),
        ("e".into(), "".into(), 1.0),
        ("f".into(), "".into(), 2.0),
        ("g".into(), "创业板".into(), 1.0),
        ("h".into(), "创业板".into(), 2.0),
    ]);
    assert_eq!(values["a"], values["b"]);
    assert_eq!(values["a"].1, 3);
    assert!((values["c"].0 - (0.5 + 1.0 / 7.0)).abs() < 1e-12);
    for code in ["d", "e", "f", "g", "h"] {
        assert_eq!(values[code].0, 0.5);
    }
    let stocks = [
        StockItem {
            code: "e".into(),
            industry: "".into(),
            price: 10.0,
            roe: Some(0.01),
            ..Default::default()
        },
        StockItem {
            code: "f".into(),
            industry: "".into(),
            price: 10.0,
            roe: Some(0.30),
            ..Default::default()
        },
        StockItem {
            code: "g".into(),
            industry: "创业板".into(),
            price: 10.0,
            roe: Some(0.01),
            ..Default::default()
        },
        StockItem {
            code: "h".into(),
            industry: "创业板".into(),
            price: 10.0,
            roe: Some(0.30),
            ..Default::default()
        },
    ];
    for row in screen_stocks(
        &stocks,
        &ScreenCriteria {
            score_profile: "quality".into(),
            ..Default::default()
        },
    )
    .items
    {
        assert_eq!(row.factor_scores["industry_quality_pct"], 0.5);
        assert_eq!(row.factor_scores["industry_valuation_pct"], 0.5);
    }
}
