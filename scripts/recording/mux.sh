#!/bin/bash
# Mux the Playwright webm with the offline-rendered soundtrack.
# Run from the rig dir (after driver.mjs -> video/*.webm + sfx-log.json,
# and sound-render.mjs -> soundtrack.wav). Output: claude-vs-cpu.mp4.
set -e
OUT="${1:-$(pwd)}"
VIDEO=$(ls "$OUT"/video/*.webm)
ffmpeg -y -i "$VIDEO" -i "$OUT/soundtrack.wav" \
  -c:v libx264 -preset medium -crf 23 -pix_fmt yuv420p \
  -c:a aac -b:a 160k -shortest \
  "$OUT/claude-vs-cpu.mp4" 2>&1 | tail -2
ls -la "$OUT/claude-vs-cpu.mp4"
