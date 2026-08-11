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
# THE CROSSOVER GATE (owner, on the 20-round takes: 'if they played
# more rounds, would the cpu start to dominate? That's left to the
# imagination of the viewer right now'). 40 rounds; a take passes iff
# the DOMINATION is on camera:
#   - Claude owns the opening (>= 4 of rounds 1-6),
#   - the cumulative score CROSSES OVER (some round ends CPU ahead),
#   - the CPU ends the session ahead AND wins >= 6 of the last 8.
act1 = c = k = cross = late = 0
last = 8
rounds = []
for line in open(sys.argv[1]):
    m = re.match(r'round (\d+) (\{.*\})', line.strip())
    if not m:
        continue
    r = json.loads(m.group(2))
    rounds.append(r)
for r in rounds:
    if r.get('winner') == 0:
        c += 1
        if r['round'] <= 6:
            act1 += 1
    elif r.get('winner') == 1:
        k += 1
        if r['round'] > len(rounds) - last:
            late += 1
    if k > c:
        cross = 1
coils = max((r.get('coils', 0) for r in rounds), default=0)
print(f"claude act1 {act1} final {c}-{k} crossover {cross} cpu last-{last} {late} coils {coils}")
# ADR-028: the wrap must be ON CAMERA (owner: 'a distinct kill tactic
# I'd expect it to learn' — and to SEE).
sys.exit(0 if (act1 >= 4 and cross and k > c and late >= 6 and coils >= 1) else 1)
PY
  if [ $? -eq 0 ]; then
    echo "ARC FOUND on roll $i"
    exit 0
  fi
  echo "roll $i: no arc, re-rolling"
done
echo "NO ARC after 3 rolls"
exit 2
