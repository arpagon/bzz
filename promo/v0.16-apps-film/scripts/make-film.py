#!/usr/bin/env python3
"""Write index.html, the apps feature film, from the takes' own marks.

Every product window is a bzz tape of a take recorded by record-multi.py
(assets/tapes/<take>/<pane>.jsonl, sync.json beside them): several people's
screens of ONE take on ONE clock. Each take has ONE time map, built here
from its marks (never from seconds read by eye), and every window of the
take is replayed with it, so a cut or a speed change happens at the same
tape second in every window. Chapter 10 lays Buzz desktop's screen
recording (assets/interop/buzz.mp4) beside a bzz tape of the same take,
placed by the alignment that take measured (assets/interop/align.json).

Writes index.html and timing/film.json (chapters, the narration's slots,
key chips, every cut and what it leaves out). With timing/narration.json
(scripts/build-narration.py), the captions follow the measured speech.

Usage: scripts/make-film.py"""
import json, pathlib

FILM = pathlib.Path(__file__).resolve().parent.parent
TAPES = FILM / "assets/tapes"
TIMING = FILM / "timing"
TIMING.mkdir(exist_ok=True)
W, H = 1920, 1080
CW, CH, BAR = 12, 26, 64          # tape cell size (px) and a window's name bar at scale 1
NAMES = {"you": "You", "ada": "Ada", "lin": "Lin"}


def load(take):
    sync = json.loads((TAPES / take / "sync.json").read_text())
    marks = {m["mark"]: m["t"] for m in sync["marks"]}
    panes = {name: (h["cols"], h["rows"]) for name, h in sync["panes"].items()}
    return {"sync": sync, "marks": marks, "panes": panes}


TAKE = {t: load(t) for t in ("together", "poll", "approvals", "agent", "records")}
NARR = json.loads((TIMING / "narration.json").read_text()) if (TIMING / "narration.json").exists() else None


def mk(take, name, off=0.0):
    m = TAKE[take]["marks"]
    if name == "compose" and name not in m and "send" in m:   # rehearsal tapes only: no mark before the mention
        print(f"WARNING {take}: no mark compose (a rehearsal tape); using send - 3.4")
        return m["send"] - 3.4 + off
    if name not in m:
        raise KeyError(f"{take}: no mark {name}")
    return m[name] + off


class Map:
    """[filmSecond, tapeSecond] keyframes of one take; every window of it uses them."""

    def __init__(self, take):
        self.take, self.k, self.cuts, self.speeds = take, [], [], []

    @property
    def f(self):
        return self.k[-1][0]

    @property
    def t(self):
        return self.k[-1][1]

    def at(self, f, t):
        if self.k and f < self.f - 1e-6:
            raise ValueError(f"{self.take}: film time goes back at {f}")
        self.k.append([round(f, 3), round(t, 3)])
        return self

    def play(self, t1, speed=1.0, why=None):
        f1 = self.f + (t1 - self.t) / speed
        if speed != 1.0:
            self.speeds.append({"film": [round(self.f, 2), round(f1, 2)], "tape": [round(self.t, 2), round(t1, 2)],
                                "speed": speed, "what": why})
        return self.at(f1, t1)

    def hold(self, f):
        return self.at(f, self.t)

    def cut(self, t1, why, chip=False):
        """A jump inside the take: what lies between is not shown."""
        self.cuts.append({"film": round(self.f, 2), "tape": [round(self.t, 2), round(t1, 2)],
                          "left_out_s": round(t1 - self.t, 2), "what": why, "chip": chip})
        return self.at(self.f, t1)

    def film_of(self, tape):
        """The film second at which a tape second is shown (the first one)."""
        for (f0, t0), (f1, t1) in zip(self.k, self.k[1:]):
            if t1 > t0 and t0 - 1e-6 <= tape <= t1 + 1e-6:
                return f0 + (tape - t0) * (f1 - f0) / (t1 - t0)
        return None


chapters, chips, tags, waits, looks, layouts = [], [], [], [], [], {}


def chapter(n, kicker, line, start, end, **kw):
    chapters.append({"n": n, "kicker": kicker, "line": line, "start": round(start, 2), "end": round(end, 2), **kw})


pending_chips = []


def chip(take, pane, label, tape=None, film=None, hold=1.1):
    pending_chips.append((take, pane, label, tape, film, hold))


def resolve_chips():
    for take, pane, label, tape, film, hold in pending_chips:
        f = film if film is not None else MAPS[take].film_of(tape)
        if f is None:
            raise ValueError(f"chip {label} at tape {tape} is not shown in {take}")
        chips.append({"take": take, "pane": pane, "label": label, "at": round(f, 2), "out": round(f + hold, 2)})


def tag(text, start, end):
    tags.append({"text": text, "start": round(start, 2), "end": round(end, 2)})


def wait_chip(take, cut, label):
    waits.append({"take": take, "at": cut["film"], "label": label, "left_out_s": cut["left_out_s"]})


def look(at, px, py, s, dur=0.55):
    looks.append(["look", round(at, 2), px, py, s, dur])


def rest(at, dur=0.45):
    looks.append(["rest", round(at, 2), dur])


def layout(take, f, **panes):
    """At film second f, each named pane's window: (x, y, scale[, opacity])."""
    layouts.setdefault(take, []).append((round(f, 2), panes))


def mmss(s):
    s = round(s)
    return f"{s // 60} min {s % 60:02d} s" if s >= 60 else f"{s} s"


MAPS = {t: Map(t) for t in TAKE}
SLOT3 = [(23, 340), (653, 340), (1283, 340)]     # three windows of 80x24 at scale 0.64
S3 = 0.64

# ---- 0 · Open, and 1 · Beside the chat, 2 · Shared, live, 3 · At the same moment: take together
m = MAPS["together"]
lb = mk("together", "lin-board:seen")
m.at(0.0, lb).play(lb + 1.4).hold(6.0)
chapter(0, "apps in bzz", "L00", 0.0, 6.0, title=True)
layout("together", 0.0, ada=(*SLOT3[0], S3, 1), lin=(*SLOT3[1], S3, 1), you=(*SLOT3[2], S3, 1))
# 1: back to the start of the take (the opening was 1.4 s of its later picture).
bes = mk("together", "beside")
m.cut(bes - 0.5, "the opening showed the Boards already open; chapter 1 starts the take from its beginning, You alone")
m.play(mk("together", "you-board:seen")).play(mk("together", "you-board:seen", 1.1)).hold(13.0)
f13 = m.f
m.play(mk("together", "ada-new", -0.6)).hold(18.0)
chapter(1, "1 · beside the chat", "L01", 6.0, 18.0)
layout("together", 6.0, ada=(*SLOT3[0], S3, 0), lin=(*SLOT3[1], S3, 0), you=(504, 290, 0.95, 1))
layout("together", 13.0, ada=(*SLOT3[0], S3, 1), lin=(*SLOT3[1], S3, 1), you=(*SLOT3[2], S3, 1))
chip("together", "you", ":app open tasks", tape=bes + 0.05, hold=1.4)
look(8.4, 1130, 600, 1.25), rest(12.6)
# 2: Ada's card, on every screen at the product's speed.
m.play(mk("together", "ada-new"))
f_new = m.f
m.play(mk("together", "ada-save"))
f_save = m.f
m.play(mk("together", "select", -0.2))
m.hold(max(32.0, m.f + 6.0))
chapter(2, "2 · shared, live", "L02", 18.0, m.f)
chip("together", "ada", "n", tape=mk("together", "ada-new") + 0.01)
chip("together", "ada", "⏎", tape=mk("together", "ada-save") + 0.01)
tag("same take · real speed", f_save - 0.3, m.f - 0.5)
look(f_save + 0.25, 960, 470, 1.3, 0.5), rest(f_save + 4.5)
# 3: the same card, the same instant; one saved, one told; nothing lost.
c3 = m.f
m.play(mk("together", "same-moment", -0.15))
f_half = m.f
m.play(mk("together", "same-moment", 0.45), speed=0.5, why="½ speed around the instant two people move one card, in every window, labelled")
m.play(mk("together", "settled"))
m.hold(max(c3 + 16.0, m.f + 7.0))
chapter(3, "3 · at the same moment", "L03", c3, m.f)
chip("together", "ada", "l", tape=mk("together", "ada-sel:wait") + 0.005)
chip("together", "lin", "l", tape=mk("together", "lin-sel:wait") + 0.005)
chip("together", "ada", "]", tape=mk("together", "same-moment") + 0.001, hold=1.6)
chip("together", "lin", "[", tape=mk("together", "same-moment") + 0.001, hold=1.6)
tag("½ speed · the same instant", f_half - 0.4, f_half + 1.8)
look(f_half - 0.6, 645, 480, 1.3, 0.5), look(f_half + 1.4, 645, 584, 1.3, 0.5), rest(f_half + 5.0)
end_together = m.f

# ---- 4 · Three apps ship: takes poll, then approvals
c4 = end_together
m = MAPS["poll"]
m.at(c4, mk("poll", "open", -0.1)).play(mk("poll", "asked"), speed=3.4, why="Lin types the question and three options")
m.play(mk("poll", "you-poll:seen"), speed=1.5, why="Ada and You type :app open poll")
m.play(mk("poll", "votes", 0.2)).play(mk("poll", "v3:seen", 1.4))
end_poll = m.f
layout("poll", c4, lin=(*SLOT3[0], S3, 1), ada=(*SLOT3[1], S3, 1), you=(*SLOT3[2], S3, 1))
chip("poll", "lin", "a", tape=mk("poll", "ask") + 0.01, hold=0.8)
chip("poll", "ada", "⏎", tape=mk("poll", "v1:wait") - 0.001, hold=0.9)
chip("poll", "lin", "⏎", tape=mk("poll", "v2:wait") - 0.001, hold=0.9)
chip("poll", "you", "⏎", tape=mk("poll", "v3:wait") - 0.001, hold=0.9)
m = MAPS["approvals"]
m.at(end_poll, mk("approvals", "file", -0.1)).play(mk("approvals", "you-sees:wait"), speed=2.0, why="Ada types her request")
m.play(mk("approvals", "decided", 0.3))
end_appr = m.f
layout("approvals", end_poll, ada=(60, 290, 0.92, 1), you=(977, 290, 0.92, 1))
chip("approvals", "ada", "n", tape=mk("approvals", "file") + 0.01, hold=0.8)
chip("approvals", "ada", "a", tape=mk("approvals", "own") + 0.01)
chip("approvals", "you", "a", tape=mk("approvals", "decide") + 0.01)
look(MAPS["approvals"].film_of(mk("approvals", "own-refused:seen")) + 0.1, 500, 880, 1.3, 0.45)
rest(MAPS["approvals"].film_of(mk("approvals", "decide")) - 0.2)
chapter(4, "4 · three apps ship", "L04", c4, end_appr)

# ---- 5-8: take agent (the real model's turn); You large, Ada small, then swapped
c5 = end_appr
m = MAPS["agent"]
YOU_L, ADA_S = (110, 280, 0.74, 1), (1216, 280, 0.6, 1)
ADA_L, YOU_S = (930, 280, 0.95, 1), (100, 330, 0.5, 1)
layout("agent", c5, you=YOU_L, ada=ADA_S)
m.at(c5, mk("agent", "select", -0.4)).play(mk("agent", "sel1:seen", 0.3))
m.play(mk("agent", "compose"), speed=2.0, why="Ctrl-c, the hold before the mention")
m.play(mk("agent", "send"), speed=2.0, why="the mention typed")
f_send = m.f
chip("agent", "you", "j", tape=mk("agent", "sel1:wait") + 0.01, hold=0.7)
chip("agent", "you", "@Scout", tape=mk("agent", "compose") + 0.4, hold=1.4)
chip("agent", "you", "⏎", film=f_send - 0.02, hold=0.8)
m.play(mk("agent", "send", 0.3))
cut = m.cut(mk("agent", "banner:seen", -0.15), "the model's turn: it read the app and the guide, tried its script apart and proposed it", chip=True).cuts[-1]
wait_chip("agent", cut, f"⏩ {mmss(cut['left_out_s'])} · the model's turn")
m.play(mk("agent", "banner-shown", -0.05)).hold(max(c5 + 14.0, m.f + 3.0))
look(f_send + 0.3, 700, 330, 1.3, 0.45), look(f_send + 4.0, 700, 520, 1.2, 0.45), rest(m.f - 0.6)
chapter(5, "5 · ask your agent", "L05", c5, m.f)
c6 = m.f
chip("agent", "you", "Ctrl-Y", tape=mk("agent", "banner-shown") + 0.01)
m.play(mk("agent", "overlay-shown")).hold(m.f + 2.5)
m.play(mk("agent", "scrolled"))
chip("agent", "you", "j ×12", tape=mk("agent", "overlay-shown") + 0.05, hold=1.3)
m.play(mk("agent", "approve"))
chip("agent", "you", "G", tape=mk("agent", "scrolled") + 0.01)
m.hold(max(c6 + 16.0, m.f + 0.6))
look(c6 + 0.8, 650, 420, 1.35, 0.5), look(c6 + 4.8, 650, 600, 1.3, 0.5), rest(m.f - 4.0)
look(m.f - 2.0, 650, 900, 1.3, 0.45), rest(m.f - 0.3, 0.3)
chapter(6, "6 · read the exact bytes", "L06", c6, m.f)
c7 = m.f
chip("agent", "you", "a", tape=mk("agent", "approve") + 0.01)
ins = "in-place"
m.play(mk("agent", ins))
v1 = TAKE["agent"]["marks"].get("ada-still-v1", mk("agent", ins, 3.0))
m.play(v1).hold(max(c7 + 10.0, m.f + 0.5))
look(c7 + 1.2, 640, 634, 1.15, 0.45), look(c7 + 6.4, 1500, 520, 1.3, 0.5), rest(m.f - 0.4, 0.35)
tag("Ada: still version 1", c7 + 6.6, m.f)
chapter(7, "7 · it changes in place", "L07", c7, m.f)
c8 = m.f
m.play(mk("agent", "share", -0.1))
chip("agent", "you", ":app share", tape=mk("agent", "share") + 0.05, hold=1.4)
m.play(mk("agent", "ada-banner-shown"))
f_swap = MAPS["agent"].film_of(mk("agent", "shared:seen", 0.5))
layout("agent", f_swap, you=YOU_S, ada=ADA_L)
m.play(mk("agent", "ada-overlay-shown")).hold(m.f + 2.0)
chip("agent", "ada", "Ctrl-Y", tape=mk("agent", "ada-banner-shown") + 0.45)
m.play(mk("agent", "ada-approve"))
chip("agent", "ada", "G", tape=mk("agent", "ada-overlay-shown") + 0.01)
m.play(mk("agent", "ada-in-place")).hold(max(c8 + 18.0, m.f + 0.5))
chip("agent", "ada", "a", tape=mk("agent", "ada-approve") + 0.01)
fo = MAPS["agent"].film_of(mk("agent", "ada-overlay-shown"))
look(fo - 0.4, 1386, 450, 1.2, 0.5), rest(m.f - 3.5)
chapter(8, "8 · offer it to the channel", "L08", c8, m.f)
end_agent = m.f

# ---- 9 · Agents use the records: take records (the real model's turn)
c9 = end_agent
m = MAPS["records"]
layout("records", c9, you=(*SLOT3[0], S3, 1), ada=(*SLOT3[1], S3, 1), lin=(*SLOT3[2], S3, 1))
m.at(c9, mk("records", "compose", -0.2)).play(mk("records", "send"), speed=2.5, why="the mention typed")
f_send9 = m.f
chip("records", "you", "@Scout", tape=mk("records", "compose") + 0.4, hold=0.6)
chip("records", "you", "⏎", film=f_send9 - 0.02, hold=0.8)
m.play(mk("records", "send", 0.3))
moved = mk("records", "lin-moved:seen")
if moved - 0.6 > m.t + 0.5:
    cut = m.cut(moved - 0.6, "the model's turn: it listed the apps here, read the Board's cards and changed one", chip=True).cuts[-1]
    wait_chip("records", cut, f"⏩ {mmss(cut['left_out_s'])} · the model's turn")
else:                                   # (the rehearsal's stand-in answered at once)
    print("WARNING records: no wait to cut"); cut = {"film": m.f}
# to the reply under the mention and a beat after it (the take's own "moved" mark came after a
# soft wait that never matched: the reply count wraps at 80 columns), then held
m.play(min(mk("records", "moved"), moved + 5.5)).hold(max(c9 + 14.0, m.f + 2.0))
look(cut["film"] + 0.3, 960, 560, 1.3, 0.5), rest(m.f - 1.5)
chapter(9, "9 · agents use the records", "L09", c9, m.f)
end_records = m.f

# ---- 10 · And on Buzz desktop: a screen recording and a bzz tape of one take
IO = json.loads((FILM / "assets/interop/align.json").read_text())
c10 = end_records
io_len = IO["film_length_s"]
chapter(10, "10 · and on Buzz desktop", "L10", c10, c10 + io_len, interop=True)
# chips of chapter 10, at the take's own key and click times (tape seconds; film = c10 + tape - start)
io_keys = IO.get("keys", [])
for who, label, tape_s in io_keys:
    f = c10 + tape_s - IO["tape_start_s"]
    chips.append({"take": "io", "pane": who, "label": label, "at": round(f, 2), "out": round(f + 1.1, 2)})
tag("another take · re-enacted by scripts", c10 + 0.3, c10 + io_len - 0.2)
c11 = c10 + io_len
chapter(11, "bzz", "L11", c11, c11 + 12.0, endcard=True)
TOTAL = round(c11 + 12.0, 2)

resolve_chips()

# ---- narration slots: each line from its chapter's start (+ a breath)
lead = {0: 1.0}
slots = {c["line"]: round(c["start"] + lead.get(c["n"], 0.6), 2) for c in chapters}
LINES = {"L00": "One channel. Three people. One app, beside the chat.",
         "L01": "An app is a small script that bzz runs for a channel, and draws right beside the <b>conversation.</b>",
         "L02": "Its records belong to the channel. Ada adds a card, and it lands on Lin's screen <b>and on yours.</b>",
         "L03": "Two people move the same card at the same instant.|One move is saved. The other is told, and the Board merges it. <b>Nothing is lost.</b>",
         "L04": "Three apps ship with bzz: a Board, a Poll, and Approvals,|where nobody <b>signs off their own request.</b>",
         "L05": "Want something else? Ask your agent, right in the channel:|<b>add a priority and a filter to the board.</b>",
         "L06": "Nothing it wrote runs <b>until you have read it:</b>|who it comes from, who asked, its hash, what changed. <b>Down to the last line.</b>",
         "L07": "Approve it, and the open app <b>changes in place.</b>|The same card is still selected.",
         "L08": "To give it to the others, <b>offer it in the channel.</b>|Ada sees who signed it, reads the same bytes, and <b>decides for herself.</b>",
         "L09": "Agents work with an app's records too, without a screen: they read its brief.|<b>Move the release notes card to Done.</b>",
         "L10": "The records travel, and <b>each client draws its own board.</b>|Here, Buzz desktop, with a plugin written from the brief.",
         "L11": "bzz. <b>Apps in the channel.</b>"}
captions = []
for c in chapters:
    if c.get("endcard"):          # the end card says it in its own type
        continue
    parts = LINES[c["line"]].split("|")
    if NARR and c["line"] in NARR["lines"]:
        segs = NARR["lines"][c["line"]]["captions"]           # [[start, end], …] per part, film seconds
    else:
        start, n = slots[c["line"]], len(parts)
        total = sum(len(p) for p in parts)
        segs, at = [], start
        for p in parts:
            d = 0.065 * len(p.replace("<b>", "").replace("</b>", ""))
            segs.append([at, at + d]); at += d
    for i, (p, (a, b)) in enumerate(zip(parts, segs)):
        nxt = segs[i + 1][0] if i + 1 < len(segs) else c["end"] - 0.15
        captions.append({"id": f"c{c['n']}-{i}", "html": p, "start": round(a - 0.15, 2),
                         "end": round(min(nxt - 0.17 if i + 1 < len(segs) else nxt, c["end"] - 0.1), 2)})


# ---- write the page
def layout_at(take, pane, f):
    state = None
    for (lf, panes) in layouts[take]:
        if lf <= f + 1e-6 and pane in panes:
            state = panes[pane]
    return state


def take_span(take):
    k = MAPS[take].k
    return k[0][0], k[-1][0]


CSS = open(FILM / "scripts/film.css").read()
html = [f"""<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=1920, height=1080" />
    <!-- Generated by scripts/make-film.py from the takes' marks. Do not edit by hand. -->
    <link rel="stylesheet" href="lib/xterm.css" />
    <script src="lib/gsap.min.js"></script>
    <script src="lib/xterm.js"></script>
    <script src="lib/addon-image.js"></script>
    <script src="lib/addon-unicode11.js"></script>
    <script src="lib/tape-replay.js"></script>
    <style>
{CSS}
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="main" data-start="0" data-duration="{TOTAL}" data-width="1920" data-height="1080">
      <div id="bg" class="clip" data-start="0" data-duration="{TOTAL}" data-track-index="0"></div>
      <div id="camera" class="clip" data-start="0" data-duration="{c11}" data-track-index="1" data-hold>
"""]
mounts = []
for take in ("together", "poll", "approvals", "agent", "records"):
    a, b = take_span(take)
    channel = {"poll": "#launch", "approvals": "#design"}.get(take, "#team")
    for pane, (cols, rows) in TAKE[take]["panes"].items():
        w0, h0 = cols * CW, rows * CH
        wid = f"w-{take}-{pane}"
        html.append(f"""        <div id="{wid}" class="win clip" style="width:{w0}px;height:{h0 + BAR}px" data-start="{a}" data-duration="{round(b - a, 2)}" data-track-index="2">
          <div class="bar" data-layout-ignore><img src="assets/avatars/{pane}.png" alt=""><span>{NAMES[pane]}</span><small>bzz 0.16.0 · {channel}</small></div>
          <div class="shot" style="width:{w0}px;height:{h0}px" data-layout-ignore><div class="term" id="t-{take}-{pane}"></div></div>
        </div>
""")
        mounts.append(f'      mountTape({{ id: "{take}-{pane}", el: "#t-{take}-{pane}", url: "assets/tapes/{take}/{pane}.jsonl", map: {json.dumps(MAPS[take].k)}, width: {w0}, height: {h0} }});')
# chapter 10: Buzz desktop and bzz
io = IO
vw, vh = io["video_box"]
tw, th = io["tape_cols"] * CW, io["tape_rows"] * CH
th_shown = io.get("tape_rows_shown", io["tape_rows"]) * CH     # a fixed crop of the lower rows (align.json says why)
html.append(f"""        <div id="w-io-bzz" class="win clip" style="width:{tw}px;height:{th_shown + BAR}px" data-start="{c10}" data-duration="{io_len}" data-track-index="2">
          <div class="bar" data-layout-ignore><img src="assets/avatars/{io['bzz_person']}.png" alt=""><span>{NAMES[io['bzz_person']]}</span><small>bzz 0.16.0 · #team · Board</small></div>
          <div class="shot" style="width:{tw}px;height:{th_shown}px" data-layout-ignore><div class="term" id="t-io"></div></div>
        </div>
""")
mounts.append(f'      mountTape({{ id: "io", el: "#t-io", url: "assets/interop/bzz.jsonl", map: [[{c10}, {io["tape_start_s"]}]], width: {tw}, height: {th} }});')
html.append("      </div>\n")
html.append(f"""        <div id="w-io-desk" class="win" style="width:{vw}px;height:{vh + BAR}px;opacity:0">
          <div class="bar" data-layout-ignore><img src="assets/avatars/{io['desk_person']}.png" alt=""><span>{io['desk_label']}</span><small>demo plugin, not shipped</small></div>
          <video id="io-video" class="clip" src="assets/interop/buzz.mp4" muted playsinline style="position:absolute;left:0;top:{BAR}px;width:{vw}px;height:{vh}px" data-start="{c10}" data-duration="{io_len}" data-media-start="{io['video_start_s']}" data-track-index="3"></video>
        </div>""")
# Title, chapter kickers, captions, chips, tags, the end card.
html.append(f"""      <div id="capbg" class="clip" data-start="0" data-duration="{c11}" data-track-index="4" data-layout-ignore></div>
      <div id="title" class="clip" data-start="0.2" data-duration="5.6" data-track-index="5"><svg class="mark sm" viewBox="0 0 64 64"><path id="hex0" d="M32 4 L56 18 V46 L32 60 L8 46 V18 Z" pathLength="1"/><rect x="19" y="23" width="26" height="5" rx="2.5"/><rect x="19" y="33" width="17" height="5" rx="2.5"/><rect x="19" y="43" width="10" height="5" rx="2.5"/><rect class="cur" x="33" y="43" width="6" height="5" rx="1"/></svg><div><div class="t1">Apps in bzz</div><div class="t2">bzz 0.16.0</div></div></div>
""")
for c in chapters:
    if c.get("endcard") or c.get("title"):
        continue
    html.append(f'      <div id="k{c["n"]}" class="kicker clip" data-start="{c["start"]}" data-duration="{round(c["end"] - c["start"], 2)}" data-track-index="5">{c["kicker"]}</div>\n')
for cap in captions:
    html.append(f'      <div id="{cap["id"]}" class="cap clip" data-start="{cap["start"]}" data-duration="{round(cap["end"] - cap["start"], 2)}" data-track-index="6">{cap["html"]}</div>\n')
for i, t in enumerate(tags):
    html.append(f'      <div id="tag{i}" class="tag clip" data-start="{t["start"]}" data-duration="{round(t["end"] - t["start"], 2)}" data-track-index="7">{t["text"]}</div>\n')
for i, wc in enumerate(waits):
    html.append(f'      <div id="wait{i}" class="wait clip" data-start="{round(wc["at"] - 0.05, 2)}" data-duration="3.0" data-track-index="7">{wc["label"]}</div>\n')
chip_html = []
for i, c in enumerate(chips):
    if c["take"] == "io":
        if c["pane"] == IO["bzz_person"]:
            x = IO["bzz_xy"][0] + IO["bzz_scale"] * IO["tape_cols"] * CW / 2
            y = IO["bzz_xy"][1] + IO["bzz_scale"] * (IO.get("tape_rows_shown", IO["tape_rows"]) * CH + BAR) + 14
        else:
            x = IO["desk_xy"][0] + IO["desk_scale"] * IO["video_box"][0] / 2
            y = IO["desk_xy"][1] + IO["desk_scale"] * (IO["video_box"][1] + BAR) + 12
    else:
        st = layout_at(c["take"], c["pane"], c["at"])
        cols, rows = TAKE[c["take"]]["panes"][c["pane"]]
        x = st[0] + st[2] * cols * CW / 2
        y = min(st[1] + st[2] * (rows * CH + BAR) + 22, 1000)
    chip_html.append(f'      <div id="chip{i}" class="chip clip" style="left:{round(x)}px;top:{round(y)}px" data-start="{round(c["at"] - 0.05, 2)}" data-duration="{round(c["out"] - c["at"] + 0.35, 2)}" data-track-index="8"><img src="assets/avatars/{c["pane"]}.png" alt=""><span>{c["label"]}</span></div>\n')
html += chip_html
html.append(f"""      <div id="io-note" class="ionote clip" data-start="{c10}" data-duration="{io_len}" data-track-index="7">{io['note']}</div>
      <div id="end" class="center clip" data-start="{c11}" data-duration="12" data-track-index="5">
        <div class="lockup">
          <svg class="mark" viewBox="0 0 64 64"><path id="hex2" d="M32 4 L56 18 V46 L32 60 L8 46 V18 Z" pathLength="1"/><rect class="l1" x="19" y="23" width="26" height="5" rx="2.5"/><rect class="l2" x="19" y="33" width="17" height="5" rx="2.5"/><rect class="l3" x="19" y="43" width="10" height="5" rx="2.5"/><rect class="cur" x="33" y="43" width="6" height="5" rx="1"/></svg>
          <div class="word">bzz</div>
        </div>
        <div class="sub">Apps in the channel</div>
        <div class="install"><span class="p">$</span>bzz app new tasks</div>
        <div class="install"><span class="p">$</span>curl -fsSL https://getbzz.dev/install.sh | sh</div>
        <div class="url">getbzz.dev/#apps</div>
        <div class="fine">Shown: bzz 0.16.0 on Linux, a test relay, generated people, every key from a script. Apps ran on Linux x86_64; on macOS and Windows their tests run in CI. {io['fine']}</div>
        <div class="fine">Independent open-source project. Not affiliated with, endorsed by, or sponsored by Block, Inc.</div>
      </div>
      <audio id="mix" src="assets/audio/mix.wav" data-start="0" data-duration="{TOTAL}" data-track-index="9" data-volume="1"></audio>
    </div>
    <script>
      // Real bzz 0.16.0 output (the published Linux archive) on the pinned relay with
      // generated people; every window of a take is replayed with that take's one map.
""")
html += [m_ + "\n" for m_ in mounts]
anim = ["      const tl = gsap.timeline({ paused: true });"]
for take, items in layouts.items():
    for idx, (f, panes) in enumerate(items):
        for pane, st in panes.items():
            x, y, s = st[:3]
            op = st[3] if len(st) > 3 else 1
            sel = f"#w-{take}-{pane}"
            if idx == 0:
                anim.append(f'      tl.set("{sel}", {{ x: {x}, y: {y}, scale: {s}, opacity: {op} }}, {max(0, f - 0.001)});')
            else:
                anim.append(f'      tl.to("{sel}", {{ x: {x}, y: {y}, scale: {s}, opacity: {op}, duration: 0.6, ease: "power3.inOut" }}, {f});')
anim.append(f'      tl.set("#w-io-desk", {{ x: {io["desk_xy"][0]}, y: {io["desk_xy"][1]}, scale: {io["desk_scale"]}, opacity: 0 }}, 0);')
anim.append(f'      tl.to("#w-io-desk", {{ opacity: 1, duration: 0.3, ease: "none" }}, {c10}); tl.to("#w-io-desk", {{ opacity: 0, duration: 0.3, ease: "none" }}, {round(c11 - 0.5, 2)});')
anim.append(f'      tl.set("#w-io-bzz", {{ x: {io["bzz_xy"][0]}, y: {io["bzz_xy"][1]}, scale: {io["bzz_scale"]} }}, {c10 - 0.001});')
for c in chapters:
    if c["n"] in (1, 4, 9, 10):   # a new scene, or (1) the take from its beginning: a dip the viewer sees
        anim.append(f'      tl.to("#camera", {{ opacity: 0, duration: 0.2, ease: "none" }}, {round(c["start"] - 0.2, 2)}); tl.to("#camera", {{ opacity: 1, duration: 0.3, ease: "none" }}, {c["start"]});')
anim.append("""      const OX = 960, OY = 1080 * 0.22;
      const look = (at, px, py, s, dur = 0.55) =>
        tl.to("#camera", { scale: s, x: -s * (px - OX), y: 600 - (OY + s * (py - OY)), duration: dur, ease: "power3.inOut" }, at);
      const rest = (at, dur = 0.45) => tl.to("#camera", { scale: 1, x: 0, y: 0, duration: dur, ease: "power3.inOut" }, at);""")
for l_ in sorted(looks, key=lambda x: x[1]):
    if l_[0] == "look":
        anim.append(f"      look({l_[1]}, {l_[2]}, {l_[3]}, {l_[4]}, {l_[5]});")
    else:
        anim.append(f"      rest({l_[1]}, {l_[2]});")
anim.append(f'      tl.to("#camera", {{ opacity: 0, scale: 0.94, x: 0, y: 0, duration: 0.5, ease: "power2.in" }}, {c11 - 0.5});')
anim.append("""      const words = (sel) => {
        const el = document.querySelector(sel);
        el.innerHTML = '<span class="line">' + el.innerHTML.replace(/(<b>|<\\/b>)|([^\\s<>]+)(?![^<]*>)/g, (m, tag, w) =>
          tag ? tag : `<span class="w">${w}</span>`) + '</span>';
        return el.querySelectorAll(".w");
      };""")
for cap in captions:
    anim.append(f'      tl.fromTo(words("#{cap["id"]}"), {{ y: 24, opacity: 0 }}, {{ y: 0, opacity: 1, duration: 0.32, ease: "power3.out", stagger: 0.035 }}, {cap["start"]});')
for i, c in enumerate(chips):
    anim.append(f'      tl.fromTo("#chip{i}", {{ opacity: 0, y: 10, scale: 0.85 }}, {{ opacity: 1, y: 0, scale: 1, duration: 0.12, ease: "power2.out" }}, {c["at"]}); tl.to("#chip{i}", {{ opacity: 0, duration: 0.25 }}, {c["out"]});')
for i, t in enumerate(tags):
    anim.append(f'      tl.fromTo("#tag{i}", {{ opacity: 0 }}, {{ opacity: 1, duration: 0.25 }}, {t["start"]});')
for i, wc in enumerate(waits):
    anim.append(f'      tl.fromTo("#wait{i}", {{ opacity: 0, scale: 0.9 }}, {{ opacity: 1, scale: 1, duration: 0.18, ease: "power2.out" }}, {wc["at"]}); tl.to("#wait{i}", {{ opacity: 0, duration: 0.3 }}, {round(wc["at"] + 2.6, 2)});')
anim.append(f"""      tl.fromTo("#hex0", {{ strokeDasharray: 1, strokeDashoffset: 1 }}, {{ strokeDashoffset: 0, duration: 0.6, ease: "power2.inOut" }}, 0.3);
      tl.fromTo("#title", {{ opacity: 0 }}, {{ opacity: 1, duration: 0.4 }}, 0.2);
      tl.to("#title", {{ opacity: 0, duration: 0.4 }}, 5.4);
      tl.fromTo("#hex2", {{ strokeDasharray: 1, strokeDashoffset: 1 }}, {{ strokeDashoffset: 0, duration: 0.6, ease: "power2.inOut" }}, {c11 + 0.1});
      tl.fromTo("#end rect", {{ scaleX: 0, transformOrigin: "0% 50%" }}, {{ scaleX: 1, duration: 0.25, stagger: 0.08, ease: "power2.out" }}, {c11 + 0.5});
      tl.fromTo("#end .word", {{ clipPath: "inset(0 100% 0 0)" }}, {{ clipPath: "inset(0 0% 0 0)", duration: 0.45, ease: "power3.out" }}, {c11 + 0.7});
      tl.fromTo("#end .sub", {{ opacity: 0, y: 20 }}, {{ opacity: 1, y: 0, duration: 0.4 }}, {c11 + 1.0});
      tl.fromTo("#end .install", {{ opacity: 0, y: 20 }}, {{ opacity: 1, y: 0, duration: 0.4, stagger: 0.25, ease: "power3.out" }}, {c11 + 1.3});
      tl.fromTo("#end .url", {{ opacity: 0 }}, {{ opacity: 1, duration: 0.4 }}, {c11 + 2.0});
      tl.fromTo("#end .fine", {{ opacity: 0 }}, {{ opacity: 1, duration: 0.5, stagger: 0.2 }}, {c11 + 1.6});
      tl.to("#end .cur", {{ opacity: 0, duration: 0.01, repeat: 15, yoyo: true, repeatDelay: 0.5 }}, {c11 + 3.0});
      tl.fromTo("#bg", {{ x: 0, y: 0 }}, {{ x: -80, y: 40, duration: {TOTAL}, ease: "none" }}, 0);
      window.__timelines["main"] = tl;
      tl.seek(0);""")
html += [a_ + "\n" for a_ in anim]
html.append("    </script>\n  </body>\n</html>\n")
(FILM / "index.html").write_text("".join(html))

out = {"total_s": TOTAL, "chapters": chapters, "narration_slots": slots, "captions": captions, "chips": chips,
       "tags": tags, "waits": waits,
       "maps": {t: {"keyframes": MAPS[t].k, "cuts": MAPS[t].cuts, "speeds": MAPS[t].speeds} for t in MAPS},
       "interop": {k: v for k, v in IO.items() if k not in ("note", "fine")}}
(TIMING / "film.json").write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
print(f"index.html: {TOTAL} s, {len(chapters)} chapters, {len(captions)} captions, {len(chips)} chips")
for c in chapters:
    print(f"  {c['n']:>2} {c['start']:7.2f}-{c['end']:7.2f} ({c['end'] - c['start']:5.2f} s) {c['kicker']}")
for t, mp in MAPS.items():
    for cu in mp.cuts:
        print(f"  cut {t} @ {cu['film']}: {cu['left_out_s']} s left out ({cu['what']})")
