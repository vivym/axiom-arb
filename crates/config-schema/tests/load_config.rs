use std::path::{Path, PathBuf};

use config_schema::{load_raw_config_from_path, RuntimeModeToml};

#[test]
fn load_raw_config_from_path_parses_minimal_operator_paper_fixture() {
    let fixture = fixture_path("app-live-ux-paper.toml");
    let raw = load_raw_config_from_path(&fixture).expect("fixture should parse");

    assert_eq!(raw.runtime.mode, RuntimeModeToml::Paper);
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}
