#!/usr/bin/env python3
"""Capture a bounded PNG frame sequence from an isolated X11 display.

This uses scrot rather than ffmpeg x11grab because the locally pinned FFmpeg
may lack that input device. The caller encodes the numbered PNGs afterwards.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
import time


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--display", required=True, help="isolated X display, e.g. :99")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--fps", type=float, default=10.0)
    parser.add_argument("--seconds", type=float, required=True)
    args = parser.parse_args()
    if not 1 <= args.fps <= 30:
        raise SystemExit("--fps must be from 1 through 30")
    if not 0 < args.seconds <= 120:
        raise SystemExit("--seconds must be greater than zero and at most 120")

    args.output_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
    os.chmod(args.output_dir, 0o700)
    count = int(args.fps * args.seconds)
    interval = 1 / args.fps
    started = time.monotonic()
    environment = {**os.environ, "DISPLAY": args.display}
    for number in range(count):
        destination = args.output_dir / f"frame-{number:05d}.png"
        subprocess.run(
            ["scrot", str(destination)],
            env=environment,
            check=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        if not destination.is_file() or destination.stat().st_size == 0:
            raise SystemExit(f"scrot did not produce {destination}")
        next_frame = started + (number + 1) * interval
        time.sleep(max(0.0, next_frame - time.monotonic()))
    print(f"captured {count} frames in {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
