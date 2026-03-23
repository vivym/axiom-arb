CREATE TABLE event_families (
    event_family_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE events (
    event_id TEXT PRIMARY KEY,
    event_family_id TEXT NOT NULL REFERENCES event_families(event_family_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (event_id, event_family_id)
);

CREATE TABLE conditions (
    condition_id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (condition_id, event_id)
);

CREATE TABLE markets (
    market_id TEXT PRIMARY KEY,
    condition_id TEXT NOT NULL,
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE CASCADE,
    route TEXT NOT NULL CHECK (route IN ('standard', 'negrisk')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (market_id, condition_id),
    UNIQUE (market_id, route),
    UNIQUE (condition_id),
    CONSTRAINT markets_condition_event_consistent
        FOREIGN KEY (condition_id, event_id)
        REFERENCES conditions(condition_id, event_id)
        ON DELETE CASCADE
);

CREATE TABLE tokens (
    token_id TEXT PRIMARY KEY,
    condition_id TEXT NOT NULL REFERENCES conditions(condition_id) ON DELETE CASCADE,
    outcome_label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (token_id, condition_id)
);

CREATE TABLE identifier_map (
    token_id TEXT PRIMARY KEY,
    condition_id TEXT NOT NULL,
    market_id TEXT NOT NULL,
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE CASCADE,
    event_family_id TEXT NOT NULL REFERENCES event_families(event_family_id) ON DELETE CASCADE,
    outcome_label TEXT NOT NULL,
    route TEXT NOT NULL CHECK (route IN ('standard', 'negrisk')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (condition_id, token_id),
    UNIQUE (condition_id, outcome_label),
    UNIQUE (market_id, condition_id, token_id),
    CONSTRAINT identifier_map_token_condition_consistent
        FOREIGN KEY (token_id, condition_id)
        REFERENCES tokens(token_id, condition_id)
        ON DELETE CASCADE,
    CONSTRAINT identifier_map_market_condition_consistent
        FOREIGN KEY (market_id, condition_id)
        REFERENCES markets(market_id, condition_id)
        ON DELETE CASCADE,
    CONSTRAINT identifier_map_condition_event_consistent
        FOREIGN KEY (condition_id, event_id)
        REFERENCES conditions(condition_id, event_id)
        ON DELETE CASCADE,
    CONSTRAINT identifier_map_event_family_consistent
        FOREIGN KEY (event_id, event_family_id)
        REFERENCES events(event_id, event_family_id)
        ON DELETE CASCADE,
    CONSTRAINT identifier_map_market_route_consistent
        FOREIGN KEY (market_id, route)
        REFERENCES markets(market_id, route)
        ON DELETE CASCADE
);

CREATE INDEX idx_identifier_map_condition_id ON identifier_map(condition_id);
CREATE INDEX idx_identifier_map_market_id ON identifier_map(market_id);
CREATE INDEX idx_identifier_map_event_family_id ON identifier_map(event_family_id);
