#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2"]
# ///
"""same-board.py <take dir> <mark> <card> <pane>...: at a mark, every pane's
Board shows the same column counts, and <card> exactly once; prints each
pane's columns and status row. With --conflict, also: exactly one pane
says "Conflict: merged with a newer change." and another "Saved." (which
one wins depends on the relay, so the check does not name it).
Exit 1 if not."""
import base64, json, re, sys
import pyte

args = sys.argv[1:]
conflict = "--conflict" in args
args = [a for a in args if a != "--conflict"]
take, mark, card, panes = args[0], args[1], args[2], args[3:]


def screen_at(path, at):
    lines = [json.loads(l) for l in open(path)]
    h = lines[0]; s = pyte.Screen(h["cols"], h["rows"]); st = pyte.ByteStream(s); inside = False
    for e in lines[1:]:
        if "o" not in e or e["t"] > at:
            continue
        data, out, i = base64.b64decode(e["o"]), bytearray(), 0
        while i < len(data):
            if inside:
                end = data.find(b"\x1b\\", i)
                if end < 0: break
                inside, i = False, end + 2
            else:
                start = data.find(b"\x1bP", i)
                if start < 0: out += data[i:]; break
                out += data[i:start]; inside, i = True, start + 2
        st.feed(bytes(out))
    return ["".join(s.buffer[y][x].data for x in range(s.columns)) for y in range(s.lines)]


sync = json.load(open(f"{take}/sync.json"))
at = {m["mark"]: m["t"] for m in sync["marks"]}[mark]
boards, said, status = {}, {}, 0
for pane in panes:
    rows = screen_at(f"{take}/{pane}.jsonl", at)
    text = "\n".join(rows)
    counts = tuple(re.findall(r"(To do|Doing|Done) \((\d+)\)", text))
    boards[pane] = counts
    said[pane] = "conflict" if "Conflict: merged with a newer change." in text else "saved" if "Saved." in text else "-"
    times = sum(r.count(card) for r in rows)
    ok = times == 1
    status |= not ok
    print(f"{pane:4} @{at:.2f}: {counts}  {card!r} x{times}  status: {said[pane]}")
if len(set(boards.values())) != 1:
    print("FAIL: the Boards differ"); status = 1
if conflict and sorted(v for v in said.values() if v != "-") != ["conflict", "saved"]:
    print(f"FAIL: expected one 'Saved.' and one 'Conflict', saw {said}"); status = 1
print("ok   same board" if not status else "FAIL same board")
sys.exit(status)
