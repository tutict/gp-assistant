#[test]
fn candidate_batch_window_clamps_to_available_batches() {
    assert_eq!(candidate_batch_window(0, 0, Some(4)), (0, 0, 0));
    assert_eq!(candidate_batch_window(600, 0, Some(2)), (0, 2, 5));
    assert_eq!(candidate_batch_window(600, 4, Some(4)), (4, 5, 5));
    assert_eq!(candidate_batch_window(600, 9, Some(1)), (5, 5, 5));
}

#[test]
fn trend_history_prefetch_codes_normalizes_and_deduplicates_candidates() {
    assert_eq!(
        normalize_trend_prefetch_codes(["000001", "000001.SZ", "600000.SH", "invalid"]),
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
