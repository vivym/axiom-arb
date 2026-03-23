CREATE TABLE orders (
    order_id TEXT PRIMARY KEY,
    market_id TEXT NOT NULL,
    condition_id TEXT NOT NULL,
    token_id TEXT NOT NULL,
    quantity NUMERIC NOT NULL,
    price NUMERIC NOT NULL,
    submission_state TEXT NOT NULL,
    venue_state TEXT NOT NULL,
    settlement_state TEXT NOT NULL,
    signed_order_hash TEXT,
    salt TEXT,
    nonce TEXT,
    signature TEXT,
    retry_of_order_id TEXT REFERENCES orders(order_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT orders_identifier_map_link_valid
        FOREIGN KEY (market_id, condition_id, token_id)
        REFERENCES identifier_map(market_id, condition_id, token_id)
        ON DELETE RESTRICT,
    CONSTRAINT orders_submission_state_valid CHECK (
        submission_state IN ('draft', 'planned', 'risk_approved', 'signed', 'submitted', 'acked', 'rejected', 'unknown')
    ),
    CONSTRAINT orders_venue_state_valid CHECK (
        venue_state IN ('live', 'matched', 'delayed', 'unmatched', 'cancel_pending', 'cancelled', 'expired', 'unknown')
    ),
    CONSTRAINT orders_settlement_state_valid CHECK (
        settlement_state IN ('matched', 'mined', 'confirmed', 'retrying', 'failed', 'unknown')
    ),
    CONSTRAINT orders_signed_order_identity_complete CHECK (
        (
            signed_order_hash IS NULL
            AND salt IS NULL
            AND nonce IS NULL
            AND signature IS NULL
        )
        OR (
            signed_order_hash IS NOT NULL
            AND salt IS NOT NULL
            AND nonce IS NOT NULL
            AND signature IS NOT NULL
        )
    )
);

CREATE INDEX idx_orders_market_id ON orders(market_id);
CREATE INDEX idx_orders_condition_id ON orders(condition_id);
CREATE INDEX idx_orders_token_id ON orders(token_id);
CREATE INDEX idx_orders_retry_of_order_id ON orders(retry_of_order_id);
CREATE UNIQUE INDEX orders_signed_order_hash_unique
    ON orders(signed_order_hash)
    WHERE signed_order_hash IS NOT NULL;

CREATE TABLE event_journal (
    journal_seq BIGSERIAL PRIMARY KEY,
    stream TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    source_event_id TEXT NOT NULL,
    dedupe_key TEXT NOT NULL,
    causal_parent_id BIGINT REFERENCES event_journal(journal_seq) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    event_ts TIMESTAMPTZ NOT NULL,
    payload JSONB NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_event_journal_dedupe_key ON event_journal(dedupe_key);
CREATE INDEX idx_event_journal_stream_seq ON event_journal(stream, journal_seq);
CREATE INDEX idx_event_journal_source ON event_journal(source_kind, source_session_id, source_event_id);

CREATE OR REPLACE FUNCTION prevent_event_journal_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'event_journal is append-only';
END;
$$;

CREATE TRIGGER event_journal_no_update
BEFORE UPDATE ON event_journal
FOR EACH ROW
EXECUTE FUNCTION prevent_event_journal_mutation();

CREATE TRIGGER event_journal_no_delete
BEFORE DELETE ON event_journal
FOR EACH ROW
EXECUTE FUNCTION prevent_event_journal_mutation();
