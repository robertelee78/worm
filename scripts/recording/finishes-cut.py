#!/usr/bin/env python3
"""UFC-undercard cut (owner: 'strip each to the final 30 seconds of a
match'): keep each round's final ~30s through the kill. Round ends come
from the sfx log's death-riff timestamps; windows [t-27s, t+3s] merge
when rounds are shorter than the window. Cuts the MUXED mp4 with
per-segment -ss/-t extracts + the concat demuxer (sound stays synced;
never a multi-trim filtergraph — the 30GB OOM lesson).

Usage: finishes-cut.py <death-event-name> [pre_s] [post_s]
"""
import json
import subprocess
import sys
import os

ev_kind = int(sys.argv[1])
pre = float(sys.argv[2]) if len(sys.argv) > 2 else 27.0
post = float(sys.argv[3]) if len(sys.argv) > 3 else 3.0

log = json.load(open('sfx-log.json'))
dur = float(subprocess.check_output([
    'ffprobe', '-v', 'error', '-show_entries', 'format=duration',
    '-of', 'csv=p=0', 'claude-vs-cpu.mp4']).strip())

deaths = sorted(t / 1000.0 for (t, ev) in log['sfx'] if ev[0] == ev_kind)
if not deaths:
    sys.exit(f"no kind-{ev_kind} events in sfx-log.json")
# The recording stops on the last round's end before its riff lands:
# treat the video end as the final finish.
if dur - deaths[-1] > 5.0:
    deaths.append(dur - post)

windows = []
for t in deaths:
    a, b = max(0.0, t - pre), min(dur, t + post)
    if windows and a <= windows[-1][1]:
        windows[-1][1] = b
    else:
        windows.append([a, b])

os.makedirs('cutseg', exist_ok=True)
listfile = open('cutseg/list.txt', 'w')
total = 0.0
for i, (a, b) in enumerate(windows):
    seg = f'cutseg/seg{i:03d}.mp4'
    # Audio fades at both seams: 30 independently-encoded AAC joints
    # otherwise click, chop riffs mid-note, and make the continuous
    # engine hum jump discontinuously at every cut ('the sound seems
    # off' — owner, on the seamless-cut v1).
    dur = b - a
    subprocess.run([
        'ffmpeg', '-y', '-loglevel', 'error', '-ss', f'{a:.2f}',
        '-i', 'claude-vs-cpu.mp4', '-t', f'{dur:.2f}',
        '-af', f'afade=t=in:d=0.25,afade=t=out:st={max(0.0, dur - 0.25):.2f}:d=0.25',
        '-c:v', 'libx264', '-preset', 'fast', '-crf', '23',
        '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '160k', seg
    ], check=True)
    listfile.write(f"file 'seg{i:03d}.mp4'\n")
    total += b - a
listfile.close()
subprocess.run([
    'ffmpeg', '-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0',
    '-i', 'cutseg/list.txt', '-c', 'copy', 'claude-vs-cpu-finishes.mp4'
], check=True)
print(f"{len(deaths)} finishes -> {len(windows)} segments, {total/60:.1f} min")
