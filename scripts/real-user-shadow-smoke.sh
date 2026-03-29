#!/usr/bin/env bash
set -euo pipefail

require_config_placeholders_replaced() {
  local config_path="$1"

  if [[ ! -f "$config_path" ]]; then
    echo "error: config file not found: $config_path" >&2
    exit 1
  fi

  if rg -n 'YOUR_|0xYOUR_' "$config_path" >/dev/null; then
    echo "error: replace placeholder values in $config_path before running smoke" >&2
    exit 1
  fi
}

require_smoke_mode_config() {
  local config_path="$1"

  if ! rg -n '^mode = "live"$' "$config_path" >/dev/null; then
    echo "error: smoke config must set runtime.mode = \"live\" in $config_path" >&2
    exit 1
  fi

  if ! rg -n '^real_user_shadow_smoke = true$' "$config_path" >/dev/null; then
    echo "error: smoke config must set runtime.real_user_shadow_smoke = true in $config_path" >&2
    exit 1
  fi
}

CONFIG_PATH="${1:-config/axiom-arb.local.toml}"

export DATABASE_URL="${DATABASE_URL:-postgres://axiom:axiom@localhost:5432/axiom_arb}"

require_config_placeholders_replaced "$CONFIG_PATH"
require_smoke_mode_config "$CONFIG_PATH"

echo "== app-live real-user shadow smoke =="
cargo run -p app-live -- doctor --config "$CONFIG_PATH"

echo
cargo run -p app-live -- run --config "$CONFIG_PATH"

echo
echo "== replay summary =="
cargo run -p app-replay -- --config "$CONFIG_PATH" --from-seq 0 --limit 1000

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
