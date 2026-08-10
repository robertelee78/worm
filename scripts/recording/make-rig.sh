#!/bin/bash
# Materialize the video-recording rig into a working directory.
#
#   scripts/recording/make-rig.sh <rig-dir>
#
# Copies the live web/ tree, applies rig-app.patch (four hooks: the
# window.game/__t0 handle, the __onFrame state tap, the sfx tee, and the
# on-canvas VIDEO BANNER showing round / cumulative score / PREDICTS /
# PROVEN READ), and links the recording scripts in. The banner reads
# window.__vidRound and window.__vidScore, which driver.mjs publishes.
#
# The rig needs: node with playwright installed (npm i playwright +
# npx playwright install chromium), ffmpeg, and a static server for the
# rig web dir (scripts/serve.py <port>, default expected on 8082).
#
# Record:  cd <rig-dir> && ./roll-until-arc.sh   (or: node driver.mjs)
# Ship:    node sound-render.mjs && ./mux.sh     -> claude-vs-cpu.mp4
#
# If rig-app.patch stops applying, web/app.js has drifted: re-derive the
# four hooks by hand and regenerate the patch with
#   diff -u web/app.js <rig-dir>/web/app.js > scripts/recording/rig-app.patch
set -e
RIG="${1:?usage: make-rig.sh <rig-dir>}"
SRC="$(cd "$(dirname "$0")/../.." && pwd)"
mkdir -p "$RIG"
cp -r "$SRC/web" "$RIG/"
patch "$RIG/web/app.js" < "$SRC/scripts/recording/rig-app.patch"
cp "$SRC/scripts/recording/driver.mjs" \
   "$SRC/scripts/recording/sound-render.mjs" \
   "$SRC/scripts/recording/mux.sh" \
   "$SRC/scripts/recording/roll-until-arc.sh" "$RIG/"
mkdir -p "$RIG/scripts"
cp "$SRC/scripts/serve.py" "$RIG/scripts/"
chmod +x "$RIG/mux.sh" "$RIG/roll-until-arc.sh"
echo "rig ready at $RIG — start: (cd $RIG && python3 scripts/serve.py 8082 &) then ./roll-until-arc.sh"
