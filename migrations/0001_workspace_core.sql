CREATE TABLE event_families (
    event_family_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE events (
    event_id TEXT PRIMARY KEY,
    event_family_id TEXT NOT NULL REFERENCES event_families(event_family_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE conditions (
    condition_id TEXT PRIMARY KEY,
    market_id TEXT NOT NULL,
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE markets (
    market_id TEXT PRIMARY KEY,
    condition_id TEXT NOT NULL REFERENCES conditions(condition_id) ON DELETE CASCADE,
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE CASCADE,
    route TEXT NOT NULL CHECK (route IN ('standard', 'negrisk')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE tokens (
    token_id TEXT PRIMARY KEY,
    condition_id TEXT NOT NULL REFERENCES conditions(condition_id) ON DELETE CASCADE,
    outcome_label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE identifier_map (
    token_id TEXT PRIMARY KEY REFERENCES tokens(token_id) ON DELETE CASCADE,
    condition_id TEXT NOT NULL REFERENCES conditions(condition_id) ON DELETE CASCADE,
    market_id TEXT NOT NULL REFERENCES markets(market_id) ON DELETE CASCADE,
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE CASCADE,
    event_family_id TEXT NOT NULL REFERENCES event_families(event_family_id) ON DELETE CASCADE,
    outcome_label TEXT NOT NULL,
    route TEXT NOT NULL CHECK (route IN ('standard', 'negrisk')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (condition_id, token_id),
    UNIQUE (condition_id, outcome_label)
);

CREATE INDEX idx_identifier_map_condition_id ON identifier_map(condition_id);
CREATE INDEX idx_identifier_map_market_id ON identifier_map(market_id);
CREATE INDEX idx_identifier_map_event_family_id ON identifier_map(event_family_id);
