#!/usr/bin/env bash
# Nightly worm-native Darwin sweep (unattended wrapper for scripts/darwin.py).
#
# Guarantees that keep unattended receipts trustworthy:
#   * refuses a dirty tree — a sweep must measure a committed champion, or
#     the receipt can't name what it measured;
#   * stamps every run with the champion's commit hash;
#   * surfaces winners OUT of the log: appends .darwin/WINNERS.md and stores
#     a pattern in project memory, so the next session's recall sees the win
#     without anyone reading cron logs.
# Promotion stays HUMAN (ADR-015 discipline): this never edits defaults.
set -uo pipefail
cd "$(dirname "$0")/.."
# Cron's PATH carries the distro cargo (1.75, cannot read lockfile v4);
# the toolchain that built this repo lives in ~/.cargo/bin. Eight
# nights of Aug 2026 failed on exactly this split.
export PATH="$HOME/.cargo/bin:$PATH"

mkdir -p .darwin
STAMP=$(date +%Y-%m-%dT%H:%M)
LOG=".darwin/cron-$(date +%Y%m%d).log"

if ! git diff --quiet -- src tests examples 2>/dev/null; then
  echo "$STAMP skipped: working tree dirty — a sweep must measure a committed champion" >> "$LOG"
  exit 0
fi
HEAD=$(git rev-parse --short HEAD)

echo "$STAMP sweep starting at champion $HEAD" >> "$LOG"
SWEEP_START=$(mktemp .darwin/.start-XXXX)
python3 scripts/darwin.py >> "$LOG" 2>&1
STATUS=$?
echo "$STAMP sweep finished (exit $STATUS)" >> "$LOG"

# WITNESS DISCIPLINE (2026-08-15 incident: eight consecutive nights
# re-reported the Aug-7 winner off a stale last-run.json while the
# sweep itself crashed on the cargo split): a failed sweep records a
# FAILURE, never winners — and winners only count from a last-run.json
# this very sweep wrote.
if [ "$STATUS" -ne 0 ]; then
  echo "$STAMP SWEEP FAILED (exit $STATUS) — no winners recorded; fix me" >> "$LOG"
  rm -f "$SWEEP_START"
  exit "$STATUS"
fi
if [ ! .darwin/last-run.json -nt "$SWEEP_START" ]; then
  echo "$STAMP sweep wrote no fresh last-run.json — no winners recorded" >> "$LOG"
  rm -f "$SWEEP_START"
  exit 1
fi
rm -f "$SWEEP_START"

WINNERS=$(python3 - << 'EOF'
import json
try:
    run = json.load(open(".darwin/last-run.json"))
    for w in run.get("winners", []):
        print(f"WORM_TUNE_{w['knob']}={w['value']} fitness {w['fitness']} vs {run['baselineFitness']}")
except Exception:
    pass
EOF
)

if [ -n "$WINNERS" ]; then
  {
    echo "## $STAMP · champion $HEAD"
    echo '```'
    echo "$WINNERS"
    echo '```'
    echo "Verify through scripts/eval.sh (incl. browser probe + stacking check — see the epistasis note in the ce9190b commit) before promoting."
    echo
  } >> .darwin/WINNERS.md
  command -v ruflo >/dev/null && ruflo memory store \
    -k "worm-darwin-cron-$(date +%Y%m%d)" \
    --value "Nightly Darwin sweep at champion $HEAD found unpromoted winner(s): $(echo "$WINNERS" | tr '\n' '; ') — verify via eval.sh + browser probe + stacking check, then promote by editing the default." \
    -n patterns >/dev/null 2>&1
  echo "$STAMP WINNERS FOUND — recorded in .darwin/WINNERS.md and project memory" >> "$LOG"
fi
