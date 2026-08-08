#!/usr/bin/env bash
# Weekly learning audit (ADR-021): the plateau gate + drift + supply
# numbers over the owner corpus, appended where the flywheel reads them.
set -u
cd /opt/worm
OUT=.harness/learning-audit.log
mkdir -p .harness
{
  echo "==== learning audit $(date -Iseconds) ===="
  python3 scripts/collect_to_export.py 2>&1 | tail -3
  CORPUS=data/players/9d8e3a7d8d202fdd.json
  cargo run --release --example sona_probe -- "$CORPUS" 2>/dev/null | tail -8
  cargo run --release --example learning_probe -- "$CORPUS" 2>/dev/null | grep -E "era |KATA-2|thinness" -A2 | head -20
  cargo run --release --example drift_partition -- "$CORPUS" 2>/dev/null | head -12
} >> "$OUT" 2>&1
