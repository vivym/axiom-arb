use config_schema::{load_raw_config_from_str, render_raw_config_to_string};

#[test]
fn raw_config_round_trips_target_source_operator_target_revision() {
    let raw = load_raw_config_from_str(
        r#"
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-9"
"#,
    )
    .unwrap();

    let text = render_raw_config_to_string(&raw).unwrap();
    assert!(text.contains("operator_target_revision = \"targets-rev-9\""));
}
