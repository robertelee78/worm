#!/usr/bin/env bash
# serve_ruv_slim_opencode.sh — launch opencode in the "ruv-slim" profile:
# the full rUv stack reachable, without the ~96k-token tool/skill catalog
# burned into every request to the local model.
#
# What the slim profile does (verified 2026-08-04):
#   * Disables the claude-flow MCP server (~250 tool schemas ≈ 45k tokens)
#     via the ruv-slim opencode plugin (OPENCODE_RUV_SLIM=1). ruflo stays
#     reachable through the ruv-gateway MCP (ONE schema ≈ 350 tokens) and
#     the ruflo CLI in bash.
#   * Disables skills catalogs (~150 entries ≈ 9k) and external skill scans
#     (OPENCODE_DISABLE_EXTERNAL_SKILLS / OPENCODE_DISABLE_CLAUDE_CODE_SKILLS).
#   * On the gemma path: routes the model through the ruv-shim observability
#     proxy on 127.0.0.1:8083 (usage + x_hf2q_timing per request logged to
#     /opt/ruv-shim/logs/requests.jsonl; /stats for aggregates).
#   * Defaults to the ruv-local primary agent on the selected model.
#
# Division of labour (agreed 2026-08-04): YOU own the model servers
# (start/stop them yourself). This script checks the SELECTED model's
# health and refuses with instructions if down; it NEVER launches models.
# It DOES own the wrapper layer:
#   * ruv-shim on 127.0.0.1:8083 (gemma path only; auto-started here)
#   * slim plugin + agent deployment into ~/.config/opencode (ak-sync
#     insurance — redeployed on every launch, idempotent)
#
# Usage:
#   scripts/serve_ruv_slim_opencode.sh                # opencode TUI (slim, qwen)
#   scripts/serve_ruv_slim_opencode.sh run "hi"       # one-shot (slim, qwen)
#   SLIM_PROVIDER=hf2q-gemma \
#   SLIM_MODEL='Gemma4 Ara 2pass Baseline' \
#       scripts/serve_ruv_slim_opencode.sh            # slim, gemma (8083→8082)
set -euo pipefail

SLIM_PROVIDER="${SLIM_PROVIDER:-qwen36-local}"
SLIM_MODEL="${SLIM_MODEL:-jenerallee78/Qwen3.6-35B-A3B-Abliterix-EGA-abliterated}"

HF2Q_HEALTH="${HF2Q_HEALTH:-http://127.0.0.1:8082/health}"
QWEN_HEALTH="${QWEN_HEALTH:-http://127.0.0.1:8081/v1/models}"
QWEN_KEY_FILE="${QWEN_KEY_FILE:-/opt/qwen3/api.key}"
SHIM_HEALTH="${SHIM_HEALTH:-http://127.0.0.1:8083/health}"
SHIM_DIR="${SHIM_DIR:-/opt/ruv-shim}"
OPENCODE_CFG_DIR="${OPENCODE_CFG_DIR:-$HOME/.config/opencode}"

healthy() { curl -sf -m 3 "$1" >/dev/null 2>&1; }
healthy_auth() { curl -sf -m 3 -H "Authorization: Bearer $(cat "$2")" "$1" >/dev/null 2>&1; }

# --- 1. selected model health (operator owns the server) --------------------
case "$SLIM_PROVIDER" in
    hf2q-gemma)
        if healthy "$HF2Q_HEALTH"; then
            echo "[ruv-slim] gemma upstream healthy (8082)"
        else
            echo "[ruv-slim] model server DOWN at $HF2Q_HEALTH" >&2
            echo "[ruv-slim] start it yourself (you own the model), e.g.:" >&2
            echo "    /opt/hf2q/scripts/serve_gemma4_opencode.sh" >&2
            echo "[ruv-slim] then re-run this script." >&2
            exit 1
        fi
        ;;
    qwen36-local)
        if healthy_auth "$QWEN_HEALTH" "$QWEN_KEY_FILE"; then
            echo "[ruv-slim] qwen upstream healthy (8081)"
        else
            echo "[ruv-slim] model server DOWN at $QWEN_HEALTH" >&2
            echo "[ruv-slim] start it yourself (you own the model) — vllm serve" >&2
            echo "    /opt/qwen3/model-mlx-8bit on 127.0.0.1:8081 (key: $QWEN_KEY_FILE)" >&2
            echo "[ruv-slim] then re-run this script." >&2
            exit 1
        fi
        ;;
    *)
        echo "[ruv-slim] unknown provider '$SLIM_PROVIDER' — skipping health check" >&2
        ;;
esac

# --- 2. ruv-shim ------------------------------------------------------------
# REMOVED from the opencode path (2026-08-04, operator request): opencode now
# talks to the model servers DIRECTLY — no 8083 baseURL override, no shim
# auto-start, no metering. The shim remains available as a standalone
# observability tool (start it manually: /opt/ruv-shim/scripts/keepalive.sh).
# To re-enable: reinstate the baseURL override in OPENCODE_CONFIG_CONTENT
# below and uncomment the startup here.

# --- 3. deploy slim plugin + agent (ak-sync insurance) ----------------------
mkdir -p "$OPENCODE_CFG_DIR/plugins" "$OPENCODE_CFG_DIR/agent"
cp -f "$SHIM_DIR/plugins/ruv-slim.js" "$OPENCODE_CFG_DIR/plugins/ruv-slim.js"
cp -f "$SHIM_DIR/agents/ruv-local.md" "$OPENCODE_CFG_DIR/agent/ruv-local.md"
# Point the deployed agent copy at the selected model (canonical file keeps
# the gemma default; per-launch selection lives only in the deployed copy).
sed -i '' "s|^model: .*|model: \"$SLIM_PROVIDER/$SLIM_MODEL\"|" "$OPENCODE_CFG_DIR/agent/ruv-local.md"

# --- 4. launch opencode slim ------------------------------------------------
# OPENCODE_CONFIG_CONTENT deep-merges as final local scope: default model +
# default agent only — no proxy detour (2026-08-04, operator request). The
# selected provider's own baseURL/apiKey (from opencode.json) is used as-is.
export OPENCODE_RUV_SLIM=1
export OPENCODE_DISABLE_EXTERNAL_SKILLS=1
export OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1
export OPENCODE_CONFIG_CONTENT="{
  \"model\": \"$SLIM_PROVIDER/$SLIM_MODEL\",
  \"default_agent\": \"ruv-local\"
}"

echo "[ruv-slim] launching opencode (slim profile: $SLIM_PROVIDER/$SLIM_MODEL direct; claude-flow MCP off, skills catalog off)"
exec opencode "$@"
