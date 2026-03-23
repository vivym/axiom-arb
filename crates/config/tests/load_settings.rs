use config::Settings;

#[test]
fn load_settings_requires_database_url_and_mode() {
    let settings = Settings::from_env_iter([
        (
            "DATABASE_URL",
            "postgres://axiom:axiom@localhost:5432/axiom_arb",
        ),
        ("AXIOM_MODE", "paper"),
    ])
    .expect("settings should parse");

    assert_eq!(settings.runtime.mode.as_str(), "paper");
}
