use super::{apply_screen_context_mode, apply_web_search_mode, FeatureMode};
use crate::state::user_profile::{
    CorrectionPattern, CorrectionSource, JevProvider, UserProfile, WebSearchProvider,
};

fn seeded_profile() -> UserProfile {
    let mut profile = UserProfile {
        ai_polish_screen_context_enabled: false,
        assistant_screen_context_enabled: false,
        ..UserProfile::default()
    };
    profile.jev.enabled = true;
    profile.jev.provider = JevProvider::Vercel;
    profile.jev.screen_routing = true;
    profile.jev.correction_review = true;
    profile.jev.search_routing = true;
    profile.jev.polish_audit = true;
    profile.web_search.enabled = false;
    profile.web_search.provider = WebSearchProvider::Google;
    profile.web_search.max_results = 9;
    profile.correction_patterns.push(CorrectionPattern {
        original: "用户原词".to_string(),
        corrected: "用户修正".to_string(),
        count: 4,
        last_seen: 123,
        source: CorrectionSource::User,
    });
    profile
}

fn correction_snapshot(profile: &UserProfile) -> serde_json::Value {
    serde_json::to_value(&profile.correction_patterns).expect("correction rules should serialize")
}

fn assert_unrelated_state_preserved(before: &UserProfile, after: &UserProfile) {
    assert_eq!(after.jev.enabled, before.jev.enabled);
    assert_eq!(after.jev.provider, before.jev.provider);
    assert_eq!(after.jev.correction_review, before.jev.correction_review);
    assert_eq!(after.jev.polish_audit, before.jev.polish_audit);
    assert_eq!(after.web_search.provider, before.web_search.provider);
    assert_eq!(after.web_search.max_results, before.web_search.max_results);
    assert_eq!(correction_snapshot(after), correction_snapshot(before));
    assert!(after
        .correction_patterns
        .iter()
        .any(|rule| rule.source == CorrectionSource::User
            && rule.original == "用户原词"
            && rule.corrected == "用户修正"));
}

#[test]
fn feature_mode_parse_accepts_only_the_three_persisted_modes() {
    assert_eq!(FeatureMode::parse("off"), Ok(FeatureMode::Off));
    assert_eq!(FeatureMode::parse(" on "), Ok(FeatureMode::On));
    assert_eq!(FeatureMode::parse("auto"), Ok(FeatureMode::Auto));
    assert!(FeatureMode::parse("").is_err());
    assert!(FeatureMode::parse("always").is_err());
}

#[test]
fn screen_context_modes_update_both_context_flags_and_jev_routing_atomically() {
    let cases = [
        ("off", false, false, false),
        ("on", true, true, false),
        ("auto", true, true, true),
    ];

    for (raw_mode, expected_enabled, expected_assistant, expected_jev) in cases {
        let before = seeded_profile();
        let mut after = before.clone();
        let mode = FeatureMode::parse(raw_mode).expect("mode should parse");

        apply_screen_context_mode(&mut after, mode);

        assert_eq!(
            (
                after.ai_polish_screen_context_enabled,
                after.assistant_screen_context_enabled,
                after.jev.screen_routing,
            ),
            (expected_enabled, expected_assistant, expected_jev),
            "screen mode {raw_mode} should update all owned fields together"
        );
        assert_unrelated_state_preserved(&before, &after);
        assert_eq!(after.jev.search_routing, before.jev.search_routing);
        assert_eq!(after.web_search.enabled, before.web_search.enabled);
    }
}

#[test]
fn web_search_modes_update_search_and_jev_routing_without_touching_other_settings() {
    let cases = [
        ("off", false, false),
        ("on", true, false),
        ("auto", true, true),
    ];

    for (raw_mode, expected_search_enabled, expected_jev) in cases {
        let before = seeded_profile();
        let mut after = before.clone();
        let mode = FeatureMode::parse(raw_mode).expect("mode should parse");

        apply_web_search_mode(&mut after, mode);

        assert_eq!(
            (after.web_search.enabled, after.jev.search_routing),
            (expected_search_enabled, expected_jev),
            "search mode {raw_mode} should update both owned fields together"
        );
        assert_unrelated_state_preserved(&before, &after);
        assert_eq!(
            (
                after.ai_polish_screen_context_enabled,
                after.assistant_screen_context_enabled,
                after.jev.screen_routing,
            ),
            (
                before.ai_polish_screen_context_enabled,
                before.assistant_screen_context_enabled,
                before.jev.screen_routing,
            )
        );
    }
}
