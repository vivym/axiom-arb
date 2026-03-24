use observability::{field_keys, metric_dimensions, span_names};

#[test]
fn observability_conventions_define_stable_span_names_and_field_keys() {
    assert_eq!(span_names::APP_BOOTSTRAP, "axiom.app.bootstrap");
    assert_eq!(span_names::REPLAY_RUN, "axiom.app_replay.run");
    assert_eq!(field_keys::SERVICE_NAME, "service.name");
    assert_eq!(field_keys::RUNTIME_MODE, "runtime_mode");
}

#[test]
fn metric_dimension_vocabularies_are_repo_owned_and_finite() {
    assert_eq!(
        metric_dimensions::Channel::User.as_pair(),
        ("channel", "user")
    );
    assert_eq!(
        metric_dimensions::HaltScope::Family.as_pair(),
        ("scope", "family")
    );
}
