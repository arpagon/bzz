#!/usr/bin/env python3
"""A window that is not a terminal tape, beside a tape: how well do they line up?

Stands in for Buzz desktop with an xterm in a private Xvfb display that
ffmpeg records with x11grab, as a screen recording of Buzz desktop would be.
x11grab stamps every frame with the wall clock; record-multi.py writes its
wall-clock origin (originWall) in sync.json. So a frame's tape second is
    frame_pts - originWall
and the recording is placed in the composition at that offset.

The proof: record-multi sends one change "together" to a tape pane (a raw
echo, as calibrate-multi.py) and to the window (a FIFO that the xterm's `cat`
prints), 24 times, alternating a full block row and a clear screen. For each
trial: when the tape shows it (its chunk stamp) and the first frame of the
recording that shows it (mean brightness of the window's region), both in
tape seconds. Reports the offset window - tape: its median (the constant
latency of X + xterm + capture, which a film may subtract) and its spread
(what no subtraction can remove), against one frame at 30 fps (33.3 ms).

Usage: calibrate-window.py <out-dir>   (needs Xvfb, xterm, /usr/bin/ffmpeg with x11grab)
"""
import base64, json, os, signal, statistics, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
out = Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=True)
DISPLAY = ":97"
FFMPEG = "/usr/bin/ffmpeg"
FFPROBE = "/usr/bin/ffprobe"
fifo = out / "desk.fifo"
if fifo.exists():
    fifo.unlink()
os.mkfifo(fifo, 0o600)
echo = out / "echo.py"
echo.write_text("import os, tty\ntty.setraw(0)\nos.write(1, b'ready\\r\\n')\n"
                "while True:\n    b = os.read(0, 4096)\n    if not b: break\n    os.write(1, b)\n")
env = {**os.environ, "DISPLAY": DISPLAY}
procs = []
try:
    procs.append(subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "640x360x24", "-nolisten", "tcp"],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL))
    time.sleep(1.0)
    procs.append(subprocess.Popen(["xterm", "-geometry", "40x8+0+0", "-fa", "Monospace", "-fs", "20", "-bg", "black",
                                   "-fg", "white", "-e", "sh", "-c", f"while :; do cat '{fifo}'; done"],
                                  env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL))
    time.sleep(1.5)
    video = out / "desk.mkv"
    if video.exists():
        video.unlink()
    procs.append(subprocess.Popen([FFMPEG, "-hide_banner", "-loglevel", "error", "-f", "x11grab", "-framerate", "30",
                                   "-video_size", "640x360", "-i", DISPLAY, "-copyts", "-c:v", "libx264", "-preset",
                                   "ultrafast", "-qp", "0", str(video)],
                                  env=env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    time.sleep(1.0)
    ON = "\x1b[2J\x1b[H" + "\n".join(["\u2588" * 40] * 7)
    OFF = "\x1b[2J\x1b[H"
    steps = [{"pane": "tape", "until": "ready", "timeout": 10}, {"wait": 0.5}]
    for i in range(40):
        text = ON if i % 2 == 0 else OFF
        # gaps that sweep the phase of the 30 fps frame clock (0.4 s alone is 12 frames: phase-locked)
        steps += [{"together": {"tape": "#" if i % 2 == 0 else ".", "desk": text}, "mark": f"t{i}"},
                  {"wait": 0.4 + (i * 0.0071) % (1 / 30)}]
    session = out / "session.json"
    fd = os.open(session, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump({"panes": {"tape": {"cols": 40, "rows": 5, "command": [sys.executable, str(echo)]}},
                   "sinks": {"desk": {"fifo": str(fifo)}}, "startAfter": 0.3, "tail": 0.5, "steps": steps}, f)
    subprocess.run([str(HERE / "record-multi.py"), "--session", str(session), "--out-dir", str(out / "tapes")], check=True)
    time.sleep(0.5)
finally:
    ff = procs[-1] if len(procs) == 3 else None
    if ff:
        ff.communicate(b"q", timeout=20)
    for p in reversed(procs[:2]):
        p.send_signal(signal.SIGTERM)
        p.wait(timeout=10)

sync = json.loads((out / "tapes" / "sync.json").read_text())
origin = sync["originWall"]
tape = [json.loads(l) for l in open(out / "tapes" / "tape.jsonl")]
chunks = [(e["t"], base64.b64decode(e["o"])) for e in tape if "o" in e]
# Every frame's own wall-clock stamp, and the mean brightness of the window region.
probe = subprocess.run([FFPROBE, "-v", "error", "-select_streams", "v", "-show_entries", "frame=pts_time",
                        "-of", "csv=p=0", str(video)], capture_output=True, text=True, check=True).stdout.split()
pts = [float(x.strip(",")) for x in probe if x.strip(",")]
raw = subprocess.run([FFMPEG, "-v", "error", "-i", str(video), "-vf", "crop=320:120:20:20,scale=32:12,format=gray",
                      "-f", "rawvideo", "-"], capture_output=True, check=True).stdout
size = 32 * 12
bright = [sum(raw[i:i + size]) / size for i in range(0, len(raw), size)]
frames = [(p - origin, b) for p, b in zip(pts, bright)]
writes = {k["together"]: k["t"] for k in sync["keys"] if k.get("sink") == "desk"}
offsets, latency_tape, latency_win = [], [], []
for i, (trial, w) in enumerate(sorted(writes.items())):
    on = i % 2 == 0
    want = b"#" if on else b"."
    tape_t = next(t for t, data in chunks if t >= w - 1e-4 and want in data)
    win_t = next(t for t, b in frames if t >= w - 0.04 and ((b > 128) if on else (b < 64)))
    offsets.append((win_t - tape_t) * 1000)
    latency_tape.append((tape_t - w) * 1000)
    latency_win.append((win_t - w) * 1000)
report = {
    "trials": len(offsets),
    "video": {"frames": len(frames), "fps_measured": round((len(pts) - 1) / (pts[-1] - pts[0]), 2),
              "first_frame_tape_s": round(frames[0][0], 3)},
    "tape_echo_latency_ms": {"median": round(statistics.median(latency_tape), 2), "max": round(max(latency_tape), 2)},
    "window_latency_ms": {"median": round(statistics.median(latency_win), 1), "min": round(min(latency_win), 1),
                          "max": round(max(latency_win), 1)},
    "window_minus_tape_ms": {"median": round(statistics.median(offsets), 1), "min": round(min(offsets), 1),
                             "max": round(max(offsets), 1),
                             "spread_after_median_removed_ms": round(max(abs(o - statistics.median(offsets)) for o in offsets), 1)},
    "frame_ms": round(1000 / 30, 1),
}
(out / "calibration-window.json").write_text(json.dumps(report, indent=1) + "\n")
print(json.dumps(report, indent=1))
