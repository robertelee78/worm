#!/usr/bin/env bash
# Weekly learning audit (ADR-021): the plateau gate + drift + supply
# numbers over the owner corpus, appended where the flywheel reads them.
set -u
cd /opt/worm
# Same toolchain split that killed the nightly for 8 nights
# (2026-08-15 incident): cron's distro cargo cannot read lockfile v4.
export PATH="$HOME/.cargo/bin:$PATH"

OUT=.harness/learning-audit.log
mkdir -p .harness
{
  echo "==== learning audit $(date -Iseconds) ===="
  python3 scripts/collect_to_export.py 2>&1 | tail -3
  CORPUS=data/players/9d8e3a7d8d202fdd.json
  # stderr lands in the log ON PURPOSE (2>/dev/null hid a dead
  # toolchain on 08-10 and the audit recorded probe-less silence).
  for probe in sona_probe learning_probe drift_partition; do
    OUT=$(cargo run --release --example "$probe" -- "$CORPUS" 2>&1)
    if [ -z "$OUT" ]; then
      echo "PROBE FAILED: $probe produced no output"
    else
      case "$probe" in
        sona_probe) echo "$OUT" | tail -8 ;;
        learning_probe) echo "$OUT" | grep -E "era |KATA-2|thinness" -A2 | head -20 ;;
        drift_partition) echo "$OUT" | head -12 ;;
      esac
    fi
  done
} >> "$OUT" 2>&1
