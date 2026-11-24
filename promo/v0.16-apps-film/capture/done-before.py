#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""done-before.py: how many cards Lin's Done holds after the together take
(its mark "settled"), read from her tape's bytes. Prints the number."""
import base64, json, pathlib, re, pyte
take = pathlib.Path(__file__).parent / "tapes/together"
at = {m["mark"]: m["t"] for m in json.loads((take / "sync.json").read_text())["marks"]}["settled"]
lines = [json.loads(l) for l in open(take / "lin.jsonl")]
h = lines[0]; s = pyte.Screen(h["cols"], h["rows"]); st = pyte.ByteStream(s)
for e in lines[1:]:
    if "o" in e and e["t"] <= at + 5:
        st.feed(base64.b64decode(e["o"]))
text = "\n".join("".join(s.buffer[y][x].data for x in range(s.columns)) for y in range(s.lines))
print(re.findall(r"Done \((\d+)\)", text)[-1])
