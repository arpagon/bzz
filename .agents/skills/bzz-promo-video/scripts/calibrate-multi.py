#!/usr/bin/env python3
"""How precisely do the tapes of one record-multi.py take line up?

Three panes each run a raw-mode echo (every byte read is written straight
back). The timeline sends one key to all three "together", 40 times, 0.25 s
apart. For each trial and pane: write time (from sync.json) and the stamp of
the first output chunk after it (from the tape). Reports
  - the echo latency per pane (write -> stamp: PTY, the echo process, the
    recorder's read), and
  - the spread across panes of the stamps of one trial: the error of placing
    "the same instant" in two tapes, which is what alignment means.
Usage: calibrate-multi.py <out-dir>   (needs nothing but python3 and uv)
"""
import base64, json, os, statistics, subprocess, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
out = Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=True)
echo = out / "echo.py"
echo.write_text(
    "import os, sys, tty\n"
    "tty.setraw(0)\n"
    "os.write(1, b'ready\\r\\n')\n"
    "while True:\n"
    "    b = os.read(0, 64)\n"
    "    if not b or b == b'q': break\n"
    "    os.write(1, b)\n")
panes = {n: {"cols": 40, "rows": 5, "command": [sys.executable, str(echo)]} for n in ("a", "b", "c")}
steps = [{"pane": "a", "until": "ready", "timeout": 10}, {"pane": "b", "until": "ready", "timeout": 10},
         {"pane": "c", "until": "ready", "timeout": 10}, {"wait": 0.5}]
for i in range(40):
    steps += [{"together": {"a": "x", "b": "x", "c": "x"}, "mark": f"t{i}"}, {"wait": 0.25}]
session = out / "session.json"
fd = os.open(session, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    json.dump({"panes": panes, "startAfter": 0.3, "tail": 0.5, "steps": steps}, f)
subprocess.run([str(HERE / "record-multi.py"), "--session", str(session), "--out-dir", str(out / "tapes")], check=True)
sync = json.loads((out / "tapes" / "sync.json").read_text())
chunks = {}
for name in panes:
    lines = [json.loads(l) for l in open(out / "tapes" / f"{name}.jsonl")]
    chunks[name] = [(e["t"], base64.b64decode(e["o"])) for e in lines if "o" in e]
writes = {}
for k in sync["keys"]:
    if "together" in k:
        writes.setdefault(k["together"], {})[k["pane"]] = k["t"]
lat, spread, write_spread = {n: [] for n in panes}, [], []
for trial, w in sorted(writes.items()):
    stamps = {}
    for name, t in w.items():
        after = [c for c in chunks[name] if c[0] >= t - 1e-4 and b"x" in c[1]]
        stamps[name] = after[0][0]
        lat[name].append((after[0][0] - t) * 1000)
    spread.append((max(stamps.values()) - min(stamps.values())) * 1000)
    write_spread.append((max(w.values()) - min(w.values())) * 1000)
report = {
    "trials": len(spread),
    "write_spread_ms": {"max": round(max(write_spread), 3), "median": round(statistics.median(write_spread), 3)},
    "echo_latency_ms": {n: {"median": round(statistics.median(v), 2), "max": round(max(v), 2)} for n, v in lat.items()},
    "stamp_spread_ms": {"median": round(statistics.median(spread), 2), "p95": round(sorted(spread)[int(0.95 * len(spread)) - 1], 2),
                        "max": round(max(spread), 2)},
    "note": "tape stamps are rounded to 0.1 ms (4 decimals), as the kit's recorders write them",
}
(out / "calibration.json").write_text(json.dumps(report, indent=1) + "\n")
print(json.dumps(report, indent=1))
