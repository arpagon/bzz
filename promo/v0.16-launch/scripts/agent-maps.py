#!/usr/bin/env python3
"""Rewrite the time maps of the agent shots in index.html from the tapes' own
marks, so a new take needs no hand-measured seconds. A map is a list of
[filmSecond, tapeSecond]; two entries at one film second are a cut inside the take.
It also prints the film second of each key the chips show."""
import json, pathlib, re
film = pathlib.Path(__file__).resolve().parent.parent
def marks(name):
    return {e["mark"]: e["t"] for e in map(json.loads, (film / "assets/tapes" / f"{name}.jsonl").read_text().splitlines()) if "mark" in e}
c, h, a = marks("mention"), marks("hands"), marks("app")
maps = {
    # the picker, the message, a cut over the model's seconds, the reply count, the thread
    "s6": [[18.1, c["send"] - 3.3], [18.75, c["send"] - 2.1], [19.65, c["send"] - 0.02], [19.85, c["send"] + 0.23],
           [19.85, c["answer:seen"] - 0.27], [20.35, c["answer:seen"] + 0.23], [20.35, c["thread:seen"] - 0.06],
           [22.3, c["thread:seen"] + 1.9]],
    # the line at the top, Ctrl-Y, the command as the shell will get it, a, the reply count
    "s7": [[22.3, h["ask:seen"] + 0.22], [23.1, h["banner"] - 0.03], [23.25, h["dialog-open:seen"] + 0.18],
           [24.65, h["dialog"] - 0.02], [24.9, h["allowed"] - 0.05], [25.6, h["answer:seen"] + 0.44]],
    # N1: the Board open beside #team, j j, the mention typed fast (about 4.6x), Enter;
    # a cut over the model's work (send -> banner:seen); the line at the top; the reply count
    "sN1": [[25.6, a["opened:seen"] + 0.35], [26.2, a["selected:seen"] + 0.1], [27.3, a["send"] + 0.01],
            [27.3, a["banner:seen"] - 0.15], [29.85, a["reply:seen"] + 0.59]],
    # N2: Ctrl-Y (pressed at banner-shown), the head of the approval; a cut over the reading
    # (the brief and the difference, j and G) to "the end"; a (pressed at approve); in place
    "sN2": [[29.85, a["banner-shown"] - 0.05], [31.1, a["overlay:seen"] + 1.2],
            [31.1, a["approve"] - 0.2], [33.6, a["approve"] + 2.3]],
}
def film_at(m, tape):
    for (f0, t0), (f1, t1) in zip(m, m[1:]):
        if t0 <= tape <= t1 and t1 > t0:
            return f0 + (tape - t0) * (f1 - f0) / (t1 - t0)
keys = {"s7 Ctrl-Y": film_at(maps["s7"], h["banner"]), "s7 a": film_at(maps["s7"], h["dialog"]),
        "N1 j": film_at(maps["sN1"], a["select"]), "N1 j (2)": film_at(maps["sN1"], a["select"] + 0.35),
        "N1 Enter": film_at(maps["sN1"], a["send"]), "N2 Ctrl-Y": film_at(maps["sN2"], a["banner-shown"]),
        "N2 a": film_at(maps["sN2"], a["approve"])}
page = (film / "index.html").read_text()
for shot, points in maps.items():
    text = json.dumps([[round(x, 2), round(y, 2)] for x, y in points])
    page, n = re.subn(r'(mountTape\(\{ id: "%s", [^\n]*?map: )\[\[.*?\]\](, width)' % shot, lambda mo: mo.group(1) + text + mo.group(2), page)
    assert n == 1, shot
    print(shot, text)
(film / "index.html").write_text(page)
print("keys at film s:", {k: round(v, 2) for k, v in keys.items()})
print("cuts inside a take (tape seconds left out):", {
    "s6 the model's answer": round(c["answer:seen"] - 0.27 - (c["send"] + 0.23), 2),
    "s6 to the thread": round(c["thread:seen"] - 0.06 - (c["answer:seen"] + 0.23), 2),
    "N1 the model's work": round(a["banner:seen"] - 0.15 - (a["send"] + 0.01), 2),
    "N1/N2 the hold before Ctrl-Y": round(a["banner-shown"] - 0.05 - (a["reply:seen"] + 0.59), 2),
    "N2 the reading": round(a["approve"] - 0.2 - (a["overlay:seen"] + 1.2), 2)})
