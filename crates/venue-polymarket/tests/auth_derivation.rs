use chrono::{TimeZone, Utc};
use venue_polymarket::{derive_builder_relayer_auth_material, derive_l2_auth_material};

#[test]
fn derives_l2_auth_from_long_lived_secret_and_clock() {
    let derived = derive_l2_auth_material(
        "poly-api-key",
        "poly-secret",
        "poly-passphrase",
        Utc.with_ymd_and_hms(2026, 3, 29, 8, 0, 0).unwrap(),
    )
    .unwrap();

    assert_eq!(derived.api_key, "poly-api-key");
    assert_eq!(derived.passphrase, "poly-passphrase");
    assert_eq!(derived.timestamp, "1774771200");
    assert!(!derived.signature.is_empty());
}

#[test]
fn derives_builder_relayer_auth_from_long_lived_secret() {
    let derived = derive_builder_relayer_auth_material(
        "builder-api-key",
        "builder-secret",
        "builder-passphrase",
        Utc.with_ymd_and_hms(2026, 3, 29, 8, 0, 1).unwrap(),
    )
    .unwrap();

    assert_eq!(derived.api_key, "builder-api-key");
    assert_eq!(derived.passphrase, "builder-passphrase");
    assert_eq!(derived.timestamp, "1774771201");
    assert!(!derived.signature.is_empty());
}
