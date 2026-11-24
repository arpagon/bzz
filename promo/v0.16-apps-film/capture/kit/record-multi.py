#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""Record several terminals of one take as tapes on ONE clock.

A feature film shows two or three people's screens side by side. Each screen
is its own bzz, on its own private PTY, recorded as the kit's tape (the
format of record-tape.py and record-live.py, so `mountTape()`, tape-peek.py
and tape-expect.py read every one of them unchanged). The difference: one
process drives all of them from one timeline and stamps every output chunk of
every pane with the same monotonic clock, so tape second 12.40 is the same
instant in every tape of the take. Replayed with the same time map, they are
in step.

Usage:
  record-multi.py --session take.json --out-dir tapes/<take>/

take.json (owner-only, mode 0600: it may name a profile, never a key):
  {
    "panes": {
      "you": {"cols": 80, "rows": 24, "cellWidth": 12, "cellHeight": 26,
              "command": ["capture/bzz.sh", "you"]},
      "ada": {...}, "lin": {...}
    },
    "startAfter": 3.0,          # seconds before the first step
    "tail": 2.0,                # seconds recorded after the last step
    "steps": [ ... ]
  }

Steps (record-live.py's, each with the pane it acts on):
  {"wait": 1.5}                                      all panes keep recording
  {"pane": "ada", "type": "text", "step": 0.05}
  {"mark": "name"}                                   written to EVERY tape
  {"pane": "lin", "until": "text", "timeout": 60}    also "gone", a list, "count",
                                                     "soft": true (mark <n>:timeout, go on)
  {"pane": "you", "press": "j", "until": "> card", "max": 6, "gap": 0.35}
        press a key until the pane shows a text (a selection's start the
        take cannot know); marks as until does
  {"together": {"ada": "]", "lin": "["}, "mark": "name"}
        one key (or short text) to each pane at the same instant: the bytes
        are written back to back in one loop turn, and the exact write time
        of each pane is kept in sync.json

Every until/gone writes "<n>:wait" and "<n>:seen" (or ":timeout") marks,
n = the step's "name" or its index. A hard timeout stops the take, prints the
pane's screen and exits 1.

Optional "sinks": {"desk": {"fifo": "/path"}}: a FIFO that another program
reads; "together" may write to it as to a pane (for a window that is not a
terminal: see kit/calibrate-window.py).

Out: <out-dir>/<pane>.jsonl for each pane, and <out-dir>/sync.json: the
session id, the monotonic and wall-clock origin, each pane's header, every
key write (pane, time, length; never the bytes) and every mark. The header
of each tape carries {"clock": {"session", "pane", "origin"}} beside the
usual fields.
The recorder never answers terminal queries and never repaints output.
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
import uuid
from pathlib import Path

import pyte


class Dcs:
    """Drop DCS strings (Sixel images) before pyte: it has no image support."""

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


class Pane:
    def __init__(self, name: str, spec: dict, out_dir: Path, origin_wall: float, session: str) -> None:
        self.name = name
        self.cols, self.rows = int(spec.get("cols", 80)), int(spec.get("rows", 24))
        self.cw, self.ch = int(spec.get("cellWidth", 12)), int(spec.get("cellHeight", 26))
        self.term = spec.get("term", "xterm-256color")
        self.command = list(spec["command"])
        self.header = {"version": 1, "cols": self.cols, "rows": self.rows, "cellWidth": self.cw,
                       "cellHeight": self.ch, "term": self.term,
                       "clock": {"session": session, "pane": name, "origin": round(origin_wall, 6)}}
        path = out_dir / f"{name}.jsonl"
        if path.exists():
            path.unlink()
        self.out = os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w")
        self.out.write(json.dumps(self.header) + "\n")
        self.screen = pyte.Screen(self.cols, self.rows)
        self.stream = pyte.ByteStream(self.screen)
        self.dcs = Dcs()
        self.exited = False
        self.pid = self.master = -1

    def spawn(self) -> None:
        pid, master = pty.fork()
        if pid == 0:
            os.environ["TERM"] = self.term
            for name in ("TMUX", "TERM_PROGRAM", "KITTY_WINDOW_ID"):
                os.environ.pop(name, None)
            os.environ["COLUMNS"], os.environ["LINES"] = str(self.cols), str(self.rows)
            os.execvp(self.command[0], self.command)
        fcntl.ioctl(master, termios.TIOCSWINSZ,
                    struct.pack("HHHH", self.rows, self.cols, self.cols * self.cw, self.rows * self.ch))
        os.kill(pid, signal.SIGWINCH)
        self.pid, self.master = pid, master

    def text(self) -> str:
        return "\n".join("".join(self.screen.buffer[y][x].data or "" for x in range(self.screen.columns)).rstrip()
                         for y in range(self.screen.lines))

    def write_event(self, event: dict) -> None:
        self.out.write(json.dumps(event) + "\n")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--session", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    args = p.parse_args()
    stat = args.session.stat()
    if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise SystemExit("--session must be owned by this user and mode 0600 or stricter")
    spec = json.loads(args.session.read_text())
    steps = spec["steps"]
    for index, step in enumerate(steps):
        if "pane" in step and step["pane"] not in spec["panes"]:
            raise SystemExit(f"step {index} names an unknown pane: {step}")
        if ("type" in step or "until" in step or "gone" in step or "press" in step) and "pane" not in step:
            raise SystemExit(f"step {index} needs a pane: {step}")
    args.out_dir.mkdir(parents=True, exist_ok=True, mode=0o700)

    session = str(uuid.uuid4())
    origin_wall = time.time()
    origin = time.monotonic()
    panes = {name: Pane(name, value, args.out_dir, origin_wall, session) for name, value in spec["panes"].items()}
    by_fd: dict[int, Pane] = {}
    for pane in panes.values():
        pane.spawn()
        by_fd[pane.master] = pane
    keys_log: list[dict] = []
    marks_log: list[dict] = []
    # Sinks: a FIFO another program reads (a window that is not a terminal of
    # ours, e.g. a calibration window in an X display that ffmpeg records).
    # Only "together" writes to them; their write times are in sync.json.
    sinks = {name: os.open(value["fifo"], os.O_RDWR | os.O_NONBLOCK)
             for name, value in spec.get("sinks", {}).items()}

    def now() -> float:
        return time.monotonic() - origin

    def pump(until: float) -> None:
        """Copy every pane's output to its tape until the given time."""
        while True:
            left = until - now()
            if left <= 0:
                return
            live = [fd for fd, pane in by_fd.items() if not pane.exited]
            if not live:
                time.sleep(min(left, 0.05))
                continue
            ready, _, _ = select.select(live, [], [], min(left, 0.05))
            for fd in ready:
                pane = by_fd[fd]
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    data = b""
                stamp = round(now(), 4)
                if not data:
                    pane.exited = True
                    continue
                pane.write_event({"t": stamp, "o": base64.b64encode(data).decode()})
                pane.stream.feed(pane.dcs.strip(data))

    def mark(name: str) -> None:
        stamp = round(now(), 4)
        for pane in panes.values():
            pane.write_event({"t": stamp, "mark": name})
        marks_log.append({"t": stamp, "mark": name})

    def send(pane: Pane, data: bytes) -> None:
        os.write(pane.master, data)
        keys_log.append({"pane": pane.name, "t": round(now(), 6), "bytes": len(data)})

    status = 0
    try:
        pump(float(spec.get("startAfter", 3.0)))
        for index, step in enumerate(steps):
            if "wait" in step:
                pump(now() + float(step["wait"]))
            elif "together" in step:
                # One write per pane, back to back, before any output is read.
                written = {}
                for name, text in step["together"].items():
                    if name in sinks:
                        os.write(sinks[name], text.encode())
                        written[name] = round(now(), 6)
                        keys_log.append({"sink": name, "t": written[name], "bytes": len(text.encode()), "together": index})
                        continue
                    pane = panes[name]
                    os.write(pane.master, text.encode())
                    written[name] = round(now(), 6)
                    keys_log.append({"pane": name, "t": written[name], "bytes": len(text.encode()), "together": index})
                if "mark" in step:
                    mark(str(step["mark"]))
                pump(now() + float(step.get("after", 0.0)))
            elif "mark" in step:
                mark(str(step["mark"]))
            elif "type" in step:
                pane = panes[step["pane"]]
                gap = float(step.get("step", 0.06))
                for char in step["type"]:
                    send(pane, char.encode())
                    pump(now() + gap)
            elif "press" in step:
                # Press a key until the pane shows a text, at most `max` times
                # (a selection whose start the take cannot know in advance).
                pane = panes[step["pane"]]
                name = str(step.get("name", index))
                mark(f"{name}:wait")
                for _ in range(int(step.get("max", 6)) + 1):
                    if step["until"] in pane.text():
                        mark(f"{name}:seen")
                        break
                    send(pane, step["press"].encode())
                    pump(now() + float(step.get("gap", 0.35)))
                else:
                    print(f"step {index} ({name}): {pane.name} never showed {step['until']!r}\n{pane.text()}", file=sys.stderr)
                    status = 1
                    break
            elif "until" in step or "gone" in step:
                pane = panes[step["pane"]]
                name = str(step.get("name", index))
                wanted = step.get("until", step.get("gone"))
                wanted = [wanted] if isinstance(wanted, str) else list(wanted)
                present = "until" in step
                need = int(step.get("count", 1))
                deadline = now() + float(step.get("timeout", 60))
                mark(f"{name}:wait")
                while True:
                    shown = pane.text()
                    hit = (any(shown.count(w) >= need for w in wanted) if present
                           else not any(w in shown for w in wanted))
                    if hit:
                        mark(f"{name}:seen")
                        break
                    if now() >= deadline or pane.exited:
                        if step.get("soft") and not pane.exited:
                            print(f"step {index} ({name}, {pane.name}): soft timeout, never saw {wanted!r}", file=sys.stderr)
                            mark(f"{name}:timeout")
                            break
                        print(f"step {index} ({name}): {pane.name} {'never showed' if present else 'still shows'} "
                              f"{wanted!r}\n{shown}", file=sys.stderr)
                        status = 1
                        break
                    pump(now() + 0.02)
                if status:
                    break
            else:
                raise SystemExit(f"bad step {index}: {step}")
        if not status:
            pump(now() + float(spec.get("tail", 2.0)))
    finally:
        stop = round(now(), 4)
        for pane in panes.values():
            pane.write_event({"t": stop, "stop": True})
            pane.out.close()
        for pane in panes.values():
            if not pane.exited:
                try:
                    os.kill(pane.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
        for pane in panes.values():
            for _ in range(50):
                if os.waitpid(pane.pid, os.WNOHANG) != (0, 0):
                    break
                time.sleep(0.1)
            else:
                os.kill(pane.pid, signal.SIGKILL)
                os.waitpid(pane.pid, 0)
        for fd in sinks.values():
            os.close(fd)
        manifest = {"session": session, "originWall": origin_wall, "originMonotonic": origin,
                    "stop": stop,
                    "panes": {name: pane.header for name, pane in panes.items()},
                    "keys": keys_log, "marks": marks_log, "status": status}
        fd = os.open(args.out_dir / "sync.json", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as handle:
            json.dump(manifest, handle, indent=1)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
