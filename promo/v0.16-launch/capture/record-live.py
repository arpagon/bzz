#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""Record a live terminal session as a tape, waiting on what the screen shows.

Same tape format as bzz-promo-video's record-tape.py (byte-for-byte output with
its time, marks). The difference: a step may wait until a text is on screen,
because a real model answers when it answers. Steps:

  {"wait": 1.5}
  {"type": "text", "step": 0.05}
  {"mark": "name"}
  {"until": "text on screen", "timeout": 120}        # also {"gone": "text"}
  {"until": [...any of...], ...}
  {"until": "text", "count": 2}                      # on screen at least twice
  {"until": ..., "soft": true}                       # film-v016 copy: on timeout, mark
                                                     # "<n>:timeout" and go on, so a late
                                                     # reply cannot kill a real model's turn

Every `until`/`gone` writes two marks, "<n>:wait" and "<n>:seen", with n the
step's own "name" or its index: the edit shortens exactly those spans and says
how long each really took. A step that times out stops the recording, prints
the screen and exits 1. The recorder never answers terminal queries.
"""
from __future__ import annotations

import argparse
import base64
import fcntl
import json
import os
import pty
import select
import signal
import struct
import sys
import termios
import time
from pathlib import Path

import pyte


class Dcs:
    """Drop DCS strings (Sixel images): pyte has no image support."""

    def __init__(self) -> None:
        self.inside = False

    def strip(self, data: bytes) -> bytes:
        out, i = bytearray(), 0
        while i < len(data):
            if self.inside:
                end = data.find(b"\x1b\\", i)
                if end < 0:
                    break
                self.inside, i = False, end + 2
            else:
                start = data.find(b"\x1bP", i)
                if start < 0:
                    out += data[i:]
                    break
                out += data[i:start]
                self.inside, i = True, start + 2
        return bytes(out)


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--cols", type=int, default=120)
    p.add_argument("--rows", type=int, default=32)
    p.add_argument("--cell-width", type=int, default=12)
    p.add_argument("--cell-height", type=int, default=26)
    p.add_argument("--term", default="xterm-256color")
    p.add_argument("--timeline", type=Path, required=True)
    p.add_argument("--start-after", type=float, default=2.0)
    p.add_argument("--tail", type=float, default=2.0)
    p.add_argument("command", nargs=argparse.REMAINDER)
    args = p.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    steps = json.loads(args.timeline.read_text())

    if args.out.exists():
        args.out.unlink()
    fd = os.open(args.out, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    out = os.fdopen(fd, "w")
    out.write(json.dumps({"version": 1, "cols": args.cols, "rows": args.rows,
                          "cellWidth": args.cell_width, "cellHeight": args.cell_height,
                          "term": args.term}) + "\n")

    pid, master = pty.fork()
    if pid == 0:
        os.environ["TERM"] = args.term
        for name in ("TMUX", "TERM_PROGRAM", "KITTY_WINDOW_ID"):
            os.environ.pop(name, None)
        os.environ["COLUMNS"], os.environ["LINES"] = str(args.cols), str(args.rows)
        os.execvp(command[0], command)
    fcntl.ioctl(master, termios.TIOCSWINSZ,
                struct.pack("HHHH", args.rows, args.cols, args.cols * args.cell_width, args.rows * args.cell_height))
    os.kill(pid, signal.SIGWINCH)

    screen = pyte.Screen(args.cols, args.rows)
    stream = pyte.ByteStream(screen)
    dcs = Dcs()
    origin = time.monotonic()
    state = {"exited": False}

    def now() -> float:
        return time.monotonic() - origin

    def text() -> str:
        return "\n".join("".join(screen.buffer[y][x].data or "" for x in range(screen.columns)).rstrip()
                         for y in range(screen.lines))

    def pump(until: float) -> None:
        """Copy output to the tape until the given time."""
        while not state["exited"]:
            left = until - now()
            if left <= 0:
                return
            ready, _, _ = select.select([master], [], [], min(left, 0.05))
            if not ready:
                continue
            try:
                data = os.read(master, 65536)
            except OSError:
                data = b""
            if not data:
                state["exited"] = True
                return
            out.write(json.dumps({"t": round(now(), 4), "o": base64.b64encode(data).decode()}) + "\n")
            stream.feed(dcs.strip(data))

    def mark(name: str) -> None:
        out.write(json.dumps({"t": round(now(), 4), "mark": name}) + "\n")

    status = 0
    try:
        pump(args.start_after)
        for index, step in enumerate(steps):
            if state["exited"]:
                break
            if "wait" in step:
                pump(now() + float(step["wait"]))
            elif "mark" in step:
                mark(str(step["mark"]))
            elif "type" in step:
                gap = float(step.get("step", 0.06))
                for char in step["type"]:
                    os.write(master, char.encode())
                    pump(now() + gap)
            elif "until" in step or "gone" in step:
                name = str(step.get("name", index))
                wanted = step.get("until", step.get("gone"))
                wanted = [wanted] if isinstance(wanted, str) else list(wanted)
                present = "until" in step
                deadline = now() + float(step.get("timeout", 60))
                mark(f"{name}:wait")
                while True:
                    shown = text()
                    need = int(step.get("count", 1))
                    hit = any(shown.count(w) >= need for w in wanted) if present else not any(w in shown for w in wanted)
                    if hit:
                        break
                    if now() >= deadline and step.get("soft") and not state["exited"]:
                        print(f"step {index} ({name}): soft timeout, never saw {wanted!r}", file=sys.stderr)
                        mark(f"{name}:timeout")
                        break
                    if now() >= deadline or state["exited"]:
                        print(f"step {index} ({name}): {'never saw' if present else 'still shows'} {wanted!r}\n{shown}",
                              file=sys.stderr)
                        status = 1
                        break
                    pump(now() + 0.1)
                if status:
                    break
                if hit:
                    mark(f"{name}:seen")
            else:
                raise SystemExit(f"bad step {index}: {step}")
        if not status:
            pump(now() + args.tail)
    finally:
        out.write(json.dumps({"t": round(now(), 4), "stop": True}) + "\n")
        out.close()
        if not state["exited"]:
            os.kill(pid, signal.SIGTERM)
        for _ in range(50):
            if os.waitpid(pid, os.WNOHANG) != (0, 0):
                break
            time.sleep(0.1)
        else:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
