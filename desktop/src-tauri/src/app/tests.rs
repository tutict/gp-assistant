use super::AppState;

#[test]
fn close_confirmation_marks_saved_and_clears_handler_readiness() {
    let state = AppState::default();
    state.set_close_handler_ready(true);

    assert!(state.should_prevent_close());
    assert!(!state.should_mark_clean_shutdown());

    state.confirm_close(true);

    assert!(!state.should_prevent_close());
    assert!(state.should_mark_clean_shutdown());
}

#[test]
fn startup_warnings_keep_health_degraded_until_no_warnings_are_recorded() {
    let state = AppState::default();
    assert_eq!(state.startup_warnings().unwrap(), Vec::<&str>::new());

    state.add_startup_warning("jobs_storage_unavailable");

    assert_eq!(
        state.startup_warnings().unwrap(),
        vec!["jobs_storage_unavailable"]
    );
}
