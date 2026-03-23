CREATE TABLE approval_states (
    token_id TEXT NOT NULL REFERENCES tokens(token_id) ON DELETE CASCADE,
    spender TEXT NOT NULL,
    owner_address TEXT NOT NULL,
    funder_address TEXT NOT NULL,
    wallet_route TEXT NOT NULL CHECK (wallet_route IN ('eoa', 'proxy', 'safe')),
    signature_type TEXT NOT NULL CHECK (signature_type IN ('eoa', 'proxy', 'safe')),
    allowance NUMERIC NOT NULL,
    required_min_allowance NUMERIC NOT NULL,
    last_checked_at TIMESTAMPTZ NOT NULL,
    approval_status TEXT NOT NULL CHECK (approval_status IN ('unknown', 'missing', 'pending', 'approved', 'rejected')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (token_id, spender, owner_address)
);

CREATE INDEX idx_approval_states_owner ON approval_states(owner_address);
CREATE INDEX idx_approval_states_funder ON approval_states(funder_address);

CREATE TABLE resolution_states (
    condition_id TEXT PRIMARY KEY REFERENCES conditions(condition_id) ON DELETE CASCADE,
    resolution_status TEXT NOT NULL CHECK (resolution_status IN ('unresolved', 'resolved', 'cancelled')),
    payout_vector JSONB NOT NULL,
    resolved_at TIMESTAMPTZ,
    dispute_state TEXT NOT NULL CHECK (dispute_state IN ('none', 'disputed', 'challenged', 'under_review')),
    redeemable_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE relayer_transactions (
    relayer_transaction_id TEXT PRIMARY KEY,
    transaction_id TEXT NOT NULL,
    nonce TEXT NOT NULL,
    signer_address TEXT NOT NULL,
    proxy_address TEXT,
    safe_address TEXT,
    status TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (transaction_id)
);

CREATE INDEX idx_relayer_transactions_signer ON relayer_transactions(signer_address, nonce);
CREATE UNIQUE INDEX idx_relayer_transactions_linkage
    ON relayer_transactions (nonce, signer_address, COALESCE(proxy_address, ''), COALESCE(safe_address, ''));

CREATE TABLE ctf_operations (
    ctf_operation_id TEXT PRIMARY KEY,
    operation_kind TEXT NOT NULL,
    condition_id TEXT REFERENCES conditions(condition_id) ON DELETE SET NULL,
    token_id TEXT REFERENCES tokens(token_id) ON DELETE SET NULL,
    transaction_id TEXT NOT NULL,
    relayer_transaction_id TEXT REFERENCES relayer_transactions(relayer_transaction_id) ON DELETE SET NULL,
    nonce TEXT NOT NULL,
    signer_address TEXT NOT NULL,
    proxy_address TEXT,
    safe_address TEXT,
    amount NUMERIC,
    status TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ctf_operations_linkage ON ctf_operations(transaction_id, nonce, signer_address);

CREATE TABLE inventory_buckets (
    token_id TEXT NOT NULL REFERENCES tokens(token_id) ON DELETE CASCADE,
    owner_address TEXT NOT NULL,
    bucket TEXT NOT NULL CHECK (
        bucket IN (
            'free',
            'reserved_for_order',
            'matched_unsettled',
            'pending_ctf_in',
            'pending_ctf_out',
            'redeemable',
            'quarantined'
        )
    ),
    quantity NUMERIC NOT NULL,
    linked_order_id TEXT REFERENCES orders(order_id) ON DELETE SET NULL,
    ctf_operation_id TEXT REFERENCES ctf_operations(ctf_operation_id) ON DELETE SET NULL,
    relayer_transaction_id TEXT REFERENCES relayer_transactions(relayer_transaction_id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (token_id, owner_address, bucket)
);

CREATE INDEX idx_inventory_buckets_owner ON inventory_buckets(owner_address, bucket);
