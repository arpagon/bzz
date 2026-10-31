#!/usr/bin/env python3
"""Byte-transparent PTY relay for a direct Ghostty bzz visual-capture session.

It owns a child PTY solely to inject optional, generated test key bytes. Child
stdout is copied directly to the outer terminal so Kitty/Sixel/iTerm escapes
reach Ghostty unchanged. It is deliberately not a terminal multiplexer.
"""
from __future__ import annotations

import argparse
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import sys
import termios
import time


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument(
        "--inject-file",
        type=Path,
        help="owner-only generated test key bytes to send to bzz after --inject-after seconds",
    )
    value.add_argument(
        "--inject-after",
        type=float,
        default=4.0,
        help="seconds before injecting --inject-file (default: 4)",
    )
    value.add_argument("command", nargs=argparse.REMAINDER, help="command after --")
    return value


def injection_bytes(path: Path | None) -> bytes | None:
    if path is None:
        return None
    stat = path.stat()
    if stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise SystemExit("--inject-file must be owned by this user and mode 0600 or stricter")
    return path.read_bytes()


def copy_window_size(master: int) -> None:
    try:
        size = fcntl.ioctl(sys.stdout.fileno(), termios.TIOCGWINSZ, b"\0" * 8)
        fcntl.ioctl(master, termios.TIOCSWINSZ, size)
    except OSError:
        pass


def main() -> int:
    args = parser().parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        raise SystemExit("pass the release bzz command after --")
    if args.inject_after < 0:
        raise SystemExit("--inject-after must be non-negative")
    pending = injection_bytes(args.inject_file)

    child, master = pty.fork()
    if child == 0:
        env = os.environ.copy()
        # An outer direct Ghostty is authoritative. Prevent environment hints
        # from a caller's multiplexer changing bzz's media protocol path.
        env["TERM_PROGRAM"] = "ghostty"
        env.pop("TMUX", None)
        os.execvpe(command[0], command, env)

    copy_window_size(master)
    stopped = False

    def forward_signal(signum: int, _frame: object) -> None:
        nonlocal stopped
        stopped = True
        try:
            os.kill(child, signum)
        except ProcessLookupError:
            pass

    for value in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(value, forward_signal)

    started = time.monotonic()
    while True:
        if pending is not None and time.monotonic() - started >= args.inject_after:
            os.write(master, pending)
            pending = None
        ready, _, _ = select.select([master], [], [], 0.05)
        if master in ready:
            try:
                output = os.read(master, 65_536)
            except OSError:
                output = b""
            if not output:
                break
            os.write(sys.stdout.fileno(), output)
        waited, status = os.waitpid(child, os.WNOHANG)
        if waited:
            return os.waitstatus_to_exitcode(status)
        if stopped:
            _, status = os.waitpid(child, 0)
            return os.waitstatus_to_exitcode(status)
    _, status = os.waitpid(child, 0)
    return os.waitstatus_to_exitcode(status)


if __name__ == "__main__":
    raise SystemExit(main())
