use config::{ConfigError, Settings};

#[test]
fn load_settings_requires_database_url_and_mode() {
    let err = Settings::from_env_iter([("AXIOM_MODE", "paper")])
        .expect_err("database url should be required");

    assert!(matches!(err, ConfigError::MissingVar("DATABASE_URL")));
}

#[test]
fn load_settings_requires_axiom_mode() {
    let err = Settings::from_env_iter([(
        "DATABASE_URL",
        "postgres://axiom:axiom@localhost:5432/axiom_arb",
    )])
    .expect_err("axiom mode should be required");

    assert!(matches!(err, ConfigError::MissingVar("AXIOM_MODE")));
}

#[test]
fn load_settings_rejects_malformed_url_fields() {
    for (key, value, expected_key) in [
        ("DATABASE_URL", "not-a-url", "DATABASE_URL"),
        ("POLY_CLOB_HOST", "not-a-url", "POLY_CLOB_HOST"),
        ("POLY_DATA_API_HOST", "not-a-url", "POLY_DATA_API_HOST"),
        ("POLY_RELAYER_HOST", "not-a-url", "POLY_RELAYER_HOST"),
    ] {
        let mut vars = vec![
            (
                "DATABASE_URL",
                "postgres://axiom:axiom@localhost:5432/axiom_arb",
            ),
            ("AXIOM_MODE", "paper"),
        ];
        vars.push((key, value));

        let err = Settings::from_env_iter(vars).expect_err("malformed url should fail");

        assert!(
            matches!(err, ConfigError::InvalidVar { key, .. } if key == expected_key),
            "unexpected error for {expected_key}: {err:?}"
        );
    }
}

#[test]
fn load_settings_rejects_wrong_url_schemes() {
    let db_err = Settings::from_env_iter([
        ("DATABASE_URL", "https://localhost/axiom_arb"),
        ("AXIOM_MODE", "paper"),
    ])
    .expect_err("database url scheme should be rejected");
    assert!(matches!(
        db_err,
        ConfigError::InvalidVar {
            key: "DATABASE_URL",
            ..
        }
    ));

    for (key, value) in [
        ("POLY_CLOB_HOST", "http://clob.polymarket.com"),
        ("POLY_DATA_API_HOST", "http://data-api.polymarket.com"),
        ("POLY_RELAYER_HOST", "http://relayer-v2.polymarket.com"),
    ] {
        let mut vars = vec![
            (
                "DATABASE_URL",
                "postgres://axiom:axiom@localhost:5432/axiom_arb",
            ),
            ("AXIOM_MODE", "paper"),
        ];
        vars.push((key, value));

        let err = Settings::from_env_iter(vars).expect_err("host scheme should be rejected");

        assert!(
            matches!(err, ConfigError::InvalidVar { key: _, .. }),
            "unexpected error for {key}: {err:?}"
        );
    }
}
