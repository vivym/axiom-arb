#!/usr/bin/env bash
set -euo pipefail

require_replaced() {
  local name="$1"
  local value="$2"

  if [[ -z "$value" || "$value" == *"YOUR_"* || "$value" == *"0xYOUR_"* ]]; then
    echo "error: replace placeholder value for ${name} before running smoke" >&2
    exit 1
  fi
}

export DATABASE_URL="${DATABASE_URL:-postgres://axiom:axiom@localhost:5432/axiom_arb}"
export AXIOM_MODE=live
export AXIOM_REAL_USER_SHADOW_SMOKE=1

# Fill these with real values before running.
export AXIOM_LOCAL_SIGNER_CONFIG="${AXIOM_LOCAL_SIGNER_CONFIG:-{
  \"signer\": {
    \"address\": \"0xYOUR_ADDRESS\",
    \"funder_address\": \"0xYOUR_FUNDER_ADDRESS\",
    \"signature_type\": \"Eoa\",
    \"wallet_route\": \"Eoa\"
  },
  \"l2_auth\": {
    \"api_key\": \"YOUR_API_KEY\",
    \"passphrase\": \"YOUR_PASSPHRASE\",
    \"timestamp\": \"1700000000\",
    \"signature\": \"YOUR_L2_SIGNATURE\"
  },
  \"relayer_auth\": {
    \"kind\": \"builder_api_key\",
    \"api_key\": \"YOUR_BUILDER_API_KEY\",
    \"timestamp\": \"1700000001\",
    \"passphrase\": \"YOUR_BUILDER_PASSPHRASE\",
    \"signature\": \"YOUR_BUILDER_SIGNATURE\"
  }
}}"

export AXIOM_POLYMARKET_SOURCE_CONFIG="${AXIOM_POLYMARKET_SOURCE_CONFIG:-{
  \"clob_host\": \"https://clob.polymarket.com\",
  \"data_api_host\": \"https://data-api.polymarket.com\",
  \"relayer_host\": \"https://relayer-v2.polymarket.com\",
  \"market_ws_url\": \"wss://ws-subscriptions-clob.polymarket.com/ws/market\",
  \"user_ws_url\": \"wss://ws-subscriptions-clob.polymarket.com/ws/user\",
  \"heartbeat_interval_seconds\": 15,
  \"relayer_poll_interval_seconds\": 5,
  \"metadata_refresh_interval_seconds\": 60
}}"

export AXIOM_NEG_RISK_LIVE_TARGETS="${AXIOM_NEG_RISK_LIVE_TARGETS:-[
  {
    \"family_id\": \"family-a\",
    \"members\": [
      {
        \"condition_id\": \"condition-1\",
        \"token_id\": \"token-1\",
        \"price\": \"0.43\",
        \"quantity\": \"5\"
      }
    ]
  }
]}"

# Comma-separated, not JSON.
export AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES="${AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES:-family-a}"
export AXIOM_NEG_RISK_LIVE_READY_FAMILIES="${AXIOM_NEG_RISK_LIVE_READY_FAMILIES:-family-a}"

require_replaced "AXIOM_LOCAL_SIGNER_CONFIG" "$AXIOM_LOCAL_SIGNER_CONFIG"
require_replaced "AXIOM_NEG_RISK_LIVE_TARGETS" "$AXIOM_NEG_RISK_LIVE_TARGETS"

echo "== app-live real-user shadow smoke =="
cargo run -p app-live

echo
echo "== replay summary =="
cargo run -p app-replay -- --from-seq 0 --limit 1000

echo
echo "== SQL checks to run manually =="
cat <<'SQL'
SELECT execution_mode, route, count(*)
FROM execution_attempts
WHERE route = 'neg-risk'
GROUP BY execution_mode, route
ORDER BY execution_mode;

SELECT ea.attempt_id, ea.execution_mode, sa.stream, sa.payload
FROM execution_attempts ea
JOIN shadow_execution_artifacts sa
  ON sa.attempt_id = ea.attempt_id
WHERE ea.route = 'neg-risk'
ORDER BY ea.attempt_id, sa.stream;
SQL
