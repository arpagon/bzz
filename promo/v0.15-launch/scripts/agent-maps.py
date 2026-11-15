#!/usr/bin/env python3
"""Rewrite the time maps of the agent shots in index.html from the tapes' own
marks, so a new take needs no hand-measured seconds. A map is a list of
[filmSecond, tapeSecond]; two entries at one film second are a cut inside the take."""
import json, pathlib, re
film = pathlib.Path(__file__).resolve().parent.parent
def marks(name):
    return {e["mark"]: e["t"] for e in map(json.loads, (film / "assets/tapes" / f"{name}.jsonl").read_text().splitlines()) if "mark" in e}
c, h, m, t = marks("create"), marks("hands"), marks("move"), marks("there")
maps = {
    # :agent new, the form (typing fast), a cut over the first-use install, the checks,
    # the creation with its capability record, the agent idle in the panel
    "sA": [[18.1, c["form"] - 0.25], [18.7, c["form-open:seen"] + 0.1], [19.85, c["filled"]], [19.95, c["filled"] + 0.02],
           [19.95, c["checks:seen"] + 0.1], [20.55, c["checked"] - 0.2], [20.55, c["create:seen"] + 0.05],
           [21.25, c["created"] - 0.2], [21.25, c["started:seen"] + 0.2], [22.35, c["started:seen"] + 1.4]],
    # the same take: the picker, the message, a cut over the model's seconds, the reply count, the thread
    "s6": [[22.35, c["send"] - 3.72], [23.0, c["send"] - 2.45], [23.9, c["send"] - 0.02], [24.1, c["send"] + 0.23],
           [24.1, c["answer:seen"] - 0.27], [24.6, c["answer:seen"] + 0.23], [24.6, c["thread:seen"] - 0.06],
           [26.55, c["thread:seen"] + 1.9]],
    # the line at the top, Ctrl-Y, the command as the shell will get it, a, the reply count
    "s7": [[26.55, h["ask:seen"] + 0.22], [27.35, h["banner"] - 0.03], [27.5, h["dialog-open:seen"] + 0.18],
           [28.9, h["dialog"] - 0.02], [29.15, h["allowed"] - 0.05], [29.85, h["answer:seen"] + 0.44]],
    # bzz agent move in a shell (the passphrase prompts fast), then its result
    "sB1": [[29.85, 1.0], [30.3, m["ask-owner:seen"] + 0.06], [30.6, m["moved:wait"] + 0.06],
            [30.8, m["moved:seen"] + 0.15], [31.65, m["moved:seen"] + 0.95]],
    # in the bzz that received it: the same thread, and what the agent recalls
    "sB2": [[31.65, t["answer:seen"] + 1.6], [33.6, t["answer:seen"] + 3.6]],
}
page = (film / "index.html").read_text()
for shot, keys in maps.items():
    text = json.dumps([[round(a, 2), round(b, 2)] for a, b in keys])
    page, n = re.subn(r'(mountTape\(\{ id: "%s", [^\n]*?map: )\[\[.*?\]\](, width)' % shot, lambda mo: mo.group(1) + text + mo.group(2), page)
    assert n == 1, shot
    print(shot, text)
(film / "index.html").write_text(page)
