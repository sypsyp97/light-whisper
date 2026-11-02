use super::AppState;

#[test]
fn api_warning_extends_only_the_marked_sessions_short_hide_delay() {
    let state = AppState::new();
    assert_eq!(state.ui.subtitle_hide_delay_ms(10, 1500), 1500);
    state.ui.mark_api_warning(10);
    // Eight seconds for reading plus the frontend fade, before native hiding.
    assert_eq!(state.ui.subtitle_hide_delay_ms(10, 1500), 8500);
    assert_eq!(state.ui.subtitle_hide_delay_ms(10, 12000), 12000);
    assert_eq!(state.ui.subtitle_hide_delay_ms(11, 1500), 1500);
}

#[test]
fn api_warning_stale_markers_do_not_replace_the_latest_session() {
    let state = AppState::new();
    state.ui.mark_api_warning(10);
    state.ui.mark_api_warning(9);
    assert_eq!(state.ui.subtitle_hide_delay_ms(10, 1500), 8500);
    assert_eq!(state.ui.subtitle_hide_delay_ms(9, 1500), 1500);
    state.ui.mark_api_warning(11);
    state.ui.mark_api_warning(10);
    assert_eq!(state.ui.subtitle_hide_delay_ms(11, 1500), 8500);
    assert_eq!(state.ui.subtitle_hide_delay_ms(10, 1500), 1500);
}

#[test]
fn api_warning_zero_is_not_a_session_and_cannot_clear_a_positive_marker() {
    let state = AppState::new();
    state.ui.mark_api_warning(0);
    assert_eq!(state.ui.subtitle_hide_delay_ms(0, 1500), 1500);
    state.ui.mark_api_warning(10);
    state.ui.mark_api_warning(0);
    assert_eq!(state.ui.subtitle_hide_delay_ms(10, 1500), 8500);
    assert_eq!(state.ui.subtitle_hide_delay_ms(0, 1500), 1500);
}
