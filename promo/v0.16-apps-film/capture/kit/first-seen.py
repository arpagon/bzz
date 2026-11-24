#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""first-seen.py <take dir> <pane> <text> [after-second]: the first tape second
at which a pane's screen shows a text (after a given second), from the bytes,
not from the recorder's polling. Used to say how long a change took to reach
another person's screen."""
import base64, json, sys
import pyte
take, pane, text = sys.argv[1:4]
after = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
lines = [json.loads(l) for l in open(f"{take}/{pane}.jsonl")]
h = lines[0]; screen = pyte.Screen(h["cols"], h["rows"]); stream = pyte.ByteStream(screen)
inside = False
def strip(data):
    global inside
    out, i = bytearray(), 0
    while i < len(data):
        if inside:
            end = data.find(b"\x1b\\", i)
            if end < 0: break
            inside, i = False, end + 2
        else:
            start = data.find(b"\x1bP", i)
            if start < 0: out += data[i:]; break
            out += data[i:start]; inside, i = True, start + 2
    return bytes(out)
for e in lines[1:]:
    if "o" not in e: continue
    stream.feed(strip(base64.b64decode(e["o"])))
    if e["t"] >= after and any(text in "".join(screen.buffer[y][x].data for x in range(screen.columns)) for y in range(screen.lines)):
        print(e["t"]); break
else:
    print("never")
