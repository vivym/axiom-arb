CREATE TABLE neg_risk_family_validations (
    event_family_id TEXT PRIMARY KEY,
    validation_status TEXT NOT NULL,
    exclusion_reason TEXT,
    metadata_snapshot_hash TEXT NOT NULL,
    last_seen_discovery_revision BIGINT NOT NULL,
    member_count INTEGER NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    validated_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_neg_risk_family_validations_revision
    ON neg_risk_family_validations(last_seen_discovery_revision);

CREATE TABLE family_halt_settings (
    event_family_id TEXT PRIMARY KEY,
    halted BOOLEAN NOT NULL,
    reason TEXT,
    blocks_new_risk BOOLEAN NOT NULL,
    metadata_snapshot_hash TEXT,
    last_seen_discovery_revision BIGINT NOT NULL,
    set_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_family_halt_settings_active
    ON family_halt_settings(halted, blocks_new_risk);

CREATE INDEX idx_family_halt_settings_revision
    ON family_halt_settings(last_seen_discovery_revision);
