#!/usr/bin/env bash
# The gauntlet — worm's improvement flywheel, one command.
#
# Champion = main. A candidate change is promotable only if this whole
# gauntlet holds (ADR-009/010/013): the warm CPU must never win less than
# the cold one, reads must be genuine lift over the player's own base rate,
# and no fixed-seed suite may regress. Run it before every merge; paste the
# numbers into the commit message — the receipts ARE the ledger.
set -uo pipefail
cd "$(dirname "$0")/.."

echo "== 1/4 unit + persona + domination suites =========================="
TERM=dumb CARGO_TERM_COLOR=never cargo test --release 2>&1 \
  | grep -E "test result:|FAILED" || exit 1

echo "== 2/4 domination detail (COLD vs WARM, fixed seeds) ==============="
TERM=dumb CARGO_TERM_COLOR=never cargo test --release --test domination -- \
  --nocapture --test-threads=1 2>&1 | tr -cd '[:print:]\n' \
  | grep -E "COLD|WARM|cpu death"

echo "== 3/4 intent-persona probe (voluntary-turn reads, NULL control) ==="
cargo run --release --example intent_probe -- 24 1500 20260805 2>/dev/null \
  | grep -E "================|voluntary-turn |drivers"

echo "== 4/4 browser-board probe (engagement + death census, 240 games) =="
cargo run --release --example engagement_probe -- browser 2>/dev/null \
  | grep -E "=====|win |engagement:|corner 6x6"

echo
echo "gauntlet complete — compare against the numbers in the latest ADR."
echo "(page smoke: node scripts/page_probe.mjs — needs playwright)"
