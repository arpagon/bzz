#!/usr/bin/env python3
"""Record a headless bzz session as a replayable terminal tape.

bzz runs on a private PTY whose window size (cells and pixels) is fixed, so the
replay terminal can reproduce the exact layout and Sixel image geometry. Every
output chunk is stored byte-for-byte with its time. Optional generated test
keys are injected from a timeline; named marks record when each action began,
so the edit can cut and place sound on real actions.

The tape is JSON Lines:
  {"version":1,"cols":…,"rows":…,"cellWidth":…,"cellHeight":…,"term":…}
  {"t":0.512,"o":"<base64 output>"}
  {"t":4.000,"mark":"open-thread"}

The recorder never answers terminal queries and never repaints output.
bzz takes its cell size from the PTY window size, not from queries.
"""
from __future__ import annotations

import argparse
import base64
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import sys
import termios
import time


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    value.add_argument("--out", type=Path, required=True, help="tape file to create (must not exist)")
    value.add_argument("--cols", type=int, default=160)
    value.add_argument("--rows", type=int, default=45)
    value.add_argument("--cell-width", type=int, default=12, help="cell width in pixels (Sixel geometry)")
    value.add_argument("--cell-height", type=int, default=24, help="cell height in pixels (Sixel geometry)")
    value.add_argument("--term", default="xterm-256color")
    value.add_argument(
        "--timeline",
        type=Path,
        help="owner-only JSON list of steps: {wait}, {type, step}, {mark}; generated test keys only",
    )
    value.add_argument("--start-after", type=float, default=3.0, help="seconds before the first timeline step")
    value.add_argument("--tail", type=float, default=2.0, help="seconds recorded after the last step")
    value.add_argument("--max-seconds", type=float, default=600.0)
    value.add_argument("command", nargs=argparse.REMAINDER, help="bzz command after --")
    return value


def load_timeline(path: Path | None) -> list[dict]:
    if path is None:
        return []
    stat = path.stat()
    if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise SystemExit("--timeline must be owned by this user and mode 0600 or stricter")
    steps = json.loads(path.read_text())
    if not isinstance(steps, list):
        raise SystemExit("timeline must be a JSON list")
    for index, step in enumerate(steps):
        keys = set(step)
        if not (keys <= {"wait"} or keys <= {"type", "step"} or keys <= {"mark"}) or not keys:
            raise SystemExit(f"timeline step {index} must be one of {{wait}}, {{type, step}}, {{mark}}: {step}")
    return steps


def schedule(steps: list[dict], start: float) -> tuple[list[tuple[float, bytes]], list[tuple[float, str]], float]:
    """Flatten the timeline into absolute key-byte and mark times."""
    now = start
    keys: list[tuple[float, bytes]] = []
    marks: list[tuple[float, str]] = []
    for step in steps:
        if "wait" in step:
            now += float(step["wait"])
        elif "mark" in step:
            marks.append((now, str(step["mark"])))
        else:
            gap = float(step.get("step", 0.06))
            for char in step["type"]:
                keys.append((now, char.encode()))
                now += gap
    return keys, marks, now


def main() -> int:
    args = parser().parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        raise SystemExit("missing bzz command after --")
    steps = load_timeline(args.timeline)
    keys, marks, end = schedule(steps, args.start_after)
    stop_at = min(end + args.tail, args.max_seconds) if steps else args.max_seconds

    fd = os.open(args.out, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    out = os.fdopen(fd, "w")
    out.write(json.dumps({
        "version": 1,
        "cols": args.cols,
        "rows": args.rows,
        "cellWidth": args.cell_width,
        "cellHeight": args.cell_height,
        "term": args.term,
    }) + "\n")

    pid, master = pty.fork()
    if pid == 0:
        os.environ["TERM"] = args.term
        os.environ.pop("TMUX", None)
        os.environ.pop("TERM_PROGRAM", None)
        os.environ.pop("KITTY_WINDOW_ID", None)
        os.environ["COLUMNS"], os.environ["LINES"] = str(args.cols), str(args.rows)
        os.execvp(command[0], command)

    size = struct.pack("HHHH", args.rows, args.cols, args.cols * args.cell_width, args.rows * args.cell_height)
    fcntl.ioctl(master, termios.TIOCSWINSZ, size)
    os.kill(pid, signal.SIGWINCH)

    origin = time.monotonic()
    key_index = mark_index = 0
    exited = False
    try:
        while True:
            now = time.monotonic() - origin
            while mark_index < len(marks) and marks[mark_index][0] <= now:
                out.write(json.dumps({"t": round(marks[mark_index][0], 4), "mark": marks[mark_index][1]}) + "\n")
                mark_index += 1
            while key_index < len(keys) and keys[key_index][0] <= now:
                os.write(master, keys[key_index][1])
                key_index += 1
            if now >= stop_at:
                break
            upcoming = [stop_at]
            if key_index < len(keys):
                upcoming.append(keys[key_index][0])
            if mark_index < len(marks):
                upcoming.append(marks[mark_index][0])
            timeout = max(0.0, min(upcoming) - now)
            ready, _, _ = select.select([master], [], [], min(timeout, 0.05))
            if ready:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    data = b""
                if not data:
                    exited = True
                    break
                stamp = round(time.monotonic() - origin, 4)
                out.write(json.dumps({"t": stamp, "o": base64.b64encode(data).decode()}) + "\n")
    finally:
        out.close()
        if not exited:
            os.kill(pid, signal.SIGTERM)
        for _ in range(50):
            if os.waitpid(pid, os.WNOHANG) != (0, 0):
                break
            time.sleep(0.1)
        else:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
    if key_index < len(keys) or mark_index < len(marks):
        print(f"recording stopped before the timeline finished ({key_index}/{len(keys)} keys)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
