#!/usr/bin/env python3
"""Write the timelines of the v0.16 film's final take (shots/<name>.json), in
the form of record-live.py. A real model answers, so every step that waits
for it waits on what the screen shows. Story order, one session, one Scout:

  warmup   not for the film: the Board's three cards; the agent's host ready
  mention  line 6: a mention, and the reply in the thread       (one turn)
  hands    line 7: the agent asks to run a command; Ctrl-Y; a    (one turn)
  app      N1, N2: the Board open, a card selected; Scout asked for a
           priority and a filter; its version announced, read, approved;
           the open Board changed in place, the card still selected (one turn)
  replies  not for the film: the app mention's thread, to keep the replies

APP_REPLIES=<n>: how many '↳ 1 reply' rows #team shows once Scout has
answered the app mention (the earlier threads' rows plus this one); set it
from what the hands tape ends on."""
import json, os, pathlib
OUT = pathlib.Path(__file__).parent / "shots"
OUT.mkdir(exist_ok=True)
ESC, ENTER, TAB, CTRL_C, CTRL_Y = "\u001b", "\r", "\t", "\u0003", "\u0019"
def T(text, step=0.035): return {"type": text, "step": step}
def W(s): return {"wait": s}
def M(name): return {"mark": name}
def U(text, timeout=60, name=None, **kw):
    step = {"until": text, "timeout": timeout, **kw}
    if name: step["name"] = name
    return step
def open_team(settle=2.0):
    return [U("online", 60, "start"), W(1.2), T("  ", 0.15), W(0.4), T("team", 0.08), W(0.3), T(ENTER), W(settle)]
def mention(prefix):
    return [T("@" + prefix, 0.11), W(0.8), T(TAB), W(0.3)]
def say(text, mark="send"):
    return [M("compose"), T("i"), W(0.35)] + mention("Sco") + [T(text, 0.028), W(0.5), M(mark), T(ENTER)]
def quit_(): return [W(0.8), T("q"), W(0.7), T("y"), W(1.5)]
CARDS = ["Publish the launch post", "Check the status page", "macOS smoke test"]
shots = {}

warm = open_team() + [T(":app open tasks" + ENTER, 0.03), U("To do (0)", 30, "opened"), W(0.8)]
for index, card in enumerate(CARDS, 1):
    warm += [T("n"), U("New card", 10, f"form{index}"), W(0.3), T(card, 0.02), W(0.3), T(ENTER), U(f"To do ({index})", 15, f"card{index}"), W(0.5)]
warm += [T(CTRL_C), W(0.6), T(":app close" + ENTER, 0.03), W(1.5), T(":agents" + ENTER, 0.03),
         U(["Scout · pi-durable · idle", "Scout · pi-durable · ready"], 300, "agent"), W(1.5), T(ESC)] + quit_()
shots["warmup"] = warm

shots["mention"] = open_team() + say("what is 17 times 23?") + [
    U("↳ 1 reply", 300, "answer"), W(1.0), T("G"), W(0.4), T(ENTER),
    U("context · 1 reply", 20, "thread", soft=True), W(4.0), M("shown")] + [T("q"), W(0.7)] + quit_()

shots["hands"] = open_team() + say("run ./test.sh in /mnt/site and tell me the result.") + [
    U("asks to run a command", 300, "ask"), W(2.5), M("banner"),
    T(CTRL_Y), U("a real command waits for your answer", 15, "dialog-open"), W(4.0), M("dialog"),
    T("a"), W(0.8), M("allowed"),
    U("↳ 1 reply", 300, "answer", count=2, soft=True), W(0.8), T("G"), W(0.4), T(ENTER),
    U("context · ", 20, "thread", soft=True), W(4.0), M("shown")] + [T("q"), W(0.7)] + quit_()

replies = int(os.environ.get("APP_REPLIES", "3"))
shots["app"] = open_team() + [
    M("open"), T(":app open tasks" + ENTER, 0.06), U("To do (3)", 20, "opened"), W(1.2),
    M("select"), T("jj", 0.35), U("> macOS smoke test", 10, "selected"), W(1.0),
    T(CTRL_C), W(0.5),
] + say("add a priority and a filter to the board") + [
    U("waits for your approval", 900, "banner"), W(0.4),
    U("↳ 1 reply", 150, "reply", count=replies, soft=True), W(2.5), M("banner-shown"),
    T(CTRL_Y), U("version 2", 15, "overlay"), W(3.5), M("overlay-shown"),
    T("jjjjjjjjjjjj", 0.1), W(2.0), M("scrolled"),
    T("G"), U([": the end", "that is the whole version"], 10, "end"), W(1.5), M("approve"),
    T("a"), U("its state kept", 30, "applied"), W(4.0), M("in-place"), W(2.0),
    T(CTRL_C),
] + quit_()

shots["replies"] = open_team() + [T("G"), W(0.4), T(ENTER), U("context · ", 20, "thread"), W(2.0), M("top"),
                                  T("G"), W(2.0), M("bottom"), T("q"), W(0.7)] + quit_()

for name, steps in shots.items():
    (OUT / f"{name}.json").write_text(json.dumps(steps, ensure_ascii=True) + "\n")
print(len(shots), "shots")
