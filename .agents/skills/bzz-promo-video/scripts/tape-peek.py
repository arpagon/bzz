#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""Print a tape's screen as text at chosen seconds, plus its marks.

A quick check while writing a shot timeline: did the key land, is the
expected text on screen, is there an error in the status bar? It ignores
colour and images; render a still with HyperFrames for pixels.

Usage: tape-peek.py <tape.jsonl> <seconds> [<seconds> ...]
"""
import base64
import json
import sys

import pyte


def main() -> None:
    path, times = sys.argv[1], sorted(float(t) for t in sys.argv[2:])
    lines = [json.loads(line) for line in open(path)]
    header = lines[0]
    screen = pyte.Screen(header["cols"], header["rows"])
    stream = pyte.ByteStream(screen)
    marks = [(e["t"], e["mark"]) for e in lines[1:] if "mark" in e]
    print("marks:", ", ".join(f"{name}@{t:.2f}" for t, name in marks) or "none")
    # pyte has no image support: drop DCS strings (Sixel), keep the text.
    # A DCS string may span chunks, so the filter keeps state.
    in_dcs = False

    def strip(data: bytes) -> bytes:
        nonlocal in_dcs
        out = bytearray()
        i = 0
        while i < len(data):
            if in_dcs:
                end = data.find(b"\x1b\\", i)
                if end < 0:
                    return bytes(out)
                in_dcs, i = False, end + 2
            else:
                start = data.find(b"\x1bP", i)
                if start < 0:
                    out += data[i:]
                    return bytes(out)
                out += data[i:start]
                in_dcs, i = True, start + 2
        return bytes(out)

    events = [e for e in lines[1:] if "o" in e]
    index = 0
    for at in times:
        while index < len(events) and events[index]["t"] <= at:
            stream.feed(strip(base64.b64decode(events[index]["o"])))
            index += 1
        print(f"----- t={at:.2f}s " + "-" * 40)
        # pyte's own `display` fails on some wide-character cells.
        for y in range(screen.lines):
            line = screen.buffer[y]
            print("".join(line[x].data or "" for x in range(screen.columns)).rstrip())


if __name__ == "__main__":
    main()
