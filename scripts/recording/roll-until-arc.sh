#!/bin/bash
# Roll 20-round takes until one shows the learning arc:
#   - CPU wins >= 4 of rounds 11-20 (the flip is visible), and
#   - CPU total wins >= 5.
# Keeps every take's log; the winning take's video/ and sfx-log.json
# stay in place for the sound/mux pipeline.
# Run from the rig dir (roll logs + video/ land in cwd).
for i in 1 2 3; do
  rm -rf video
  node driver.mjs 2>&1 | tee "take-roll$i.log"
  python3 - "take-roll$i.log" <<'PY'
import json, re, sys
back = tot = 0
for line in open(sys.argv[1]):
    m = re.match(r'round (\d+) (\{.*\})', line.strip())
    if not m:
        continue
    r = json.loads(m.group(2))
    if r.get('winner') == 1:
        tot += 1
        if r['round'] >= 11:
            back += 1
print(f"cpu total {tot} back-half {back}")
sys.exit(0 if (back >= 4 and tot >= 5) else 1)
PY
  if [ $? -eq 0 ]; then
    echo "ARC FOUND on roll $i"
    exit 0
  fi
  echo "roll $i: no arc, re-rolling"
done
echo "NO ARC after 3 rolls"
exit 2
