#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""Assert what a tape's screen shows at a moment, so a shot proves its claim.

Usage: tape-expect.py <tape.jsonl> <when> [+TEXT | -TEXT] ...

<when> is seconds (`12.5`), a mark (`send`), a mark with an offset
(`send+2.0`), or `end`. `+TEXT` must be on screen; `-TEXT` must not.
Exits 1 and prints the screen when any expectation fails.
"""
import base64
import json
import re
import sys

import pyte


def resolve(when: str, lines: list[dict]) -> float:
    if when == "end":
        return lines[-1]["t"]
    try:
        return float(when)
    except ValueError:
        pass
    match = re.fullmatch(r"([\w-]+)([+-][\d.]+)?", when)
    if not match:
        raise SystemExit(f"bad time {when!r}")
    marks = {e["mark"]: e["t"] for e in lines if "mark" in e}
    if match.group(1) not in marks:
        raise SystemExit(f"no mark {match.group(1)!r}; marks: {sorted(marks)}")
    return marks[match.group(1)] + float(match.group(2) or 0)


def screen_at(lines: list[dict], at: float) -> list[str]:
    header = lines[0]
    screen = pyte.Screen(header["cols"], header["rows"])
    stream = pyte.ByteStream(screen)
    in_dcs = False
    for event in lines[1:]:
        if "o" not in event or event["t"] > at:
            continue
        data, out, i = base64.b64decode(event["o"]), bytearray(), 0
        while i < len(data):
            if in_dcs:
                end = data.find(b"\x1b\\", i)
                if end < 0:
                    break
                in_dcs, i = False, end + 2
            else:
                start = data.find(b"\x1bP", i)
                if start < 0:
                    out += data[i:]
                    break
                out += data[i:start]
                in_dcs, i = True, start + 2
        stream.feed(bytes(out))
    return [
        "".join(screen.buffer[y][x].data or "" for x in range(screen.columns)).rstrip()
        for y in range(screen.lines)
    ]


def main() -> None:
    path, when, *checks = sys.argv[1:]
    lines = [json.loads(line) for line in open(path)]
    at = resolve(when, lines)
    rows = screen_at(lines, at)
    text = "\n".join(rows)
    failed = []
    for check in checks:
        wanted, needle = check[0], check[1:]
        if wanted not in "+-":
            raise SystemExit(f"expectation must start with + or -: {check!r}")
        if (needle in text) != (wanted == "+"):
            failed.append(check)
    name = path.rsplit("/", 1)[-1]
    if failed:
        print(f"FAIL {name} @{at:.2f}s ({when}): {failed}")
        print(text)
        sys.exit(1)
    print(f"ok   {name} @{at:.2f}s ({when}): {len(checks)} expectation(s)")


if __name__ == "__main__":
    main()
