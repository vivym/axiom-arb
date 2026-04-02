use config_schema::{load_raw_config_from_path, ValidatedConfig};

#[path = "../src/commands/targets/config_file.rs"]
mod config_file;

use config_file::{rewrite_operator_target_revision, rewrite_smoke_rollout_families};

const MINIMAL_TARGET_SOURCE_CONFIG: &str = r#"
# preserve this comment
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = ""
legacy_note = "keep-me"
"#;

#[test]
fn rewrite_operator_target_revision_updates_the_same_config_file_for_minimal_target_source_config()
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(&path, MINIMAL_TARGET_SOURCE_CONFIG).unwrap();

    rewrite_operator_target_revision(&path, "targets-rev-12").unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# preserve this comment"));
    assert!(text.contains("operator_target_revision = \"targets-rev-12\""));
    assert!(text.contains("legacy_note = \"keep-me\""));

    let raw = load_raw_config_from_path(&path).unwrap();
    let validated = ValidatedConfig::new(raw).unwrap();
    let target_source = validated.target_source().unwrap();
    assert!(target_source.is_adopted());
    assert_eq!(
        target_source.operator_target_revision(),
        Some("targets-rev-12")
    );
}

#[test]
fn rewrite_operator_target_revision_fails_when_target_source_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(
        &path,
        r#"
[runtime]
mode = "live"
"#,
    )
    .unwrap();

    let error = rewrite_operator_target_revision(&path, "targets-rev-12").unwrap_err();
    assert!(error
        .to_string()
        .contains("missing required section: negrisk.target_source"));
}

#[test]
fn rewrite_smoke_rollout_families_updates_both_lists_and_preserves_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("axiom-arb.local.toml");
    std::fs::write(
        &path,
        r#"
# preserve this comment
[runtime]
mode = "live"

[negrisk.target_source]
source = "adopted"
operator_target_revision = "targets-rev-12"

[negrisk.rollout]
approved_families = []
ready_families = []
legacy_note = "keep-me"
"#,
    )
    .unwrap();

    rewrite_smoke_rollout_families(&path, &["family-b".into(), "family-a".into()]).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# preserve this comment"));
    assert!(text.contains("approved_families = [\"family-a\", \"family-b\"]"));
    assert!(text.contains("ready_families = [\"family-a\", \"family-b\"]"));
    assert!(text.contains("legacy_note = \"keep-me\""));

    let raw = load_raw_config_from_path(&path).unwrap();
    let validated = ValidatedConfig::new(raw.clone()).unwrap();
    let rollout = raw.negrisk.unwrap().rollout.unwrap();
    let _ = validated;
    assert_eq!(
        rollout.approved_families,
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
    assert_eq!(
        rollout.ready_families,
        vec!["family-a".to_owned(), "family-b".to_owned()]
    );
}
