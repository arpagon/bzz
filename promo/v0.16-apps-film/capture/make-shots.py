#!/usr/bin/env python3
"""Write the final takes of the apps feature film (shots/<take>.json), in
the form of kit/record-multi.py: several bzz panes, one timeline, one clock.
Story order, one fixture, one session (shoot-all.sh). The takes agent and
records are answered by a REAL model (one turn each); the others by nobody:

  warmup    not for the film: You makes three cards and moves one to Doing;
            Scout's host is installed (single pane)
  together  acts 1-3: You, Ada, Lin in #team, the Board open beside the chat
            in each; Ada's card reaches the other two; Ada and Lin move the
            same card at the same instant ("together"); nothing is lost
  poll      act 4a: #launch, Lin asks a question, three people vote
  approvals act 4b: #design, Ada files a request, cannot decide it herself,
            You approves it, Ada sees it decided
  agent     acts 5-8: You (120x32) asks Scout for a priority and a filter,
            reads the version in the overlay, approves it, the Board changes
            in place; Ada's Board beside it is still version 1; You shares
            it; Ada reads the offer and approves it; her Board changes too
  records   act 9: You asks Scout to move a card to Done; Scout does it
            through the records, without a screen; all three Boards show it
"""
import json, os, pathlib
HERE = pathlib.Path(__file__).parent
OUT = HERE / "shots"
OUT.mkdir(exist_ok=True)
ESC, ENTER, TAB, CTRL_C, CTRL_Y = "\u001b", "\r", "\t", "\u0003", "\u0019"
SMALL = {"cols": 80, "rows": 24, "cellWidth": 12, "cellHeight": 26}
LARGE = {"cols": 120, "rows": 32, "cellWidth": 12, "cellHeight": 26}


def pane(who, size=SMALL):
    return {**size, "command": [str(HERE / "bzz.sh"), who]}


def T(who, text, step=0.035): return {"pane": who, "type": text, "step": step}
def W(s): return {"wait": s}
def M(name): return {"mark": name}
def U(who, text, timeout=60, name=None, **kw):
    step = {"pane": who, "until": text, "timeout": timeout, **kw}
    if name:
        step["name"] = name
    return step


def P(who, key, text, name, most=6):
    return {"pane": who, "press": key, "until": text, "max": most, "name": name}


def ready(who):
    return [U(who, "online", 90, f"{who}-online")]


def open_channel(who, channel, settle=1.5):
    return [T(who, "  ", 0.15), W(0.4), T(who, channel, 0.08), W(0.3), T(who, ENTER), W(settle)]


def app_open(who, name, wait_for, label):
    return [T(who, f":app open {name}" + ENTER, 0.05), U(who, wait_for, 30, label)]


def quit_(who, from_app=True):
    return ([T(who, CTRL_C), W(0.5)] if from_app else []) + [T(who, "q"), W(0.7), T(who, "y"), W(1.0)]


def mention(who, text, mark="send"):
    return [M("compose"), T(who, "i"), W(0.35), T(who, "@Sco", 0.11), W(0.8), T(who, TAB), W(0.3),
            T(who, text, 0.028), W(0.5), M(mark), T(who, ENTER)]


takes = {}
CARDS = ["Publish the launch post", "Check the status page", "macOS smoke test"]

# warmup: You alone, the three cards, one moved to Doing, Scout's host installed.
w = ready("you") + [W(1.0)] + open_channel("you", "team") + app_open("you", "tasks", "To do (0)", "opened")
for i, card in enumerate(CARDS, 1):
    w += [T("you", "n"), U("you", "New card", 10, f"form{i}"), W(0.3), T("you", card, 0.02), W(0.3),
          T("you", ENTER), U("you", f"To do ({i})", 15, f"card{i}"), W(0.5)]
w += [P("you", "k", "> Check the status page", "sel"), W(0.4), T("you", "]"),
      U("you", "Doing (1)", 10, "moved"), W(0.8), T("you", CTRL_C), W(0.6),
      T("you", ":agents" + ENTER, 0.03),
      U("you", ["Scout · pi-durable · idle", "Scout · pi-durable · ready"], 300, "agent"), W(1.5), T("you", ESC), W(0.5)]
w += quit_("you", from_app=False)
takes["warmup"] = {"panes": {"you": pane("you", LARGE)}, "startAfter": 2.0, "tail": 1.0, "steps": w}
# together: acts 1-3.
s = []
for who in ("you", "ada", "lin"):
    s += ready(who)
s += [W(1.0)]
for who in ("you", "ada", "lin"):
    s += open_channel(who, "team", 1.0)
s += [M("beside")] + app_open("you", "tasks", "To do (2)", "you-board") + [W(1.5)]
s += app_open("ada", "tasks", "To do (2)", "ada-board") + app_open("lin", "tasks", "To do (2)", "lin-board") + [W(1.5)]
s += [M("ada-new"), T("ada", "n"), U("ada", "New card", 10, "ada-form"), W(0.3),
      T("ada", "Write the release notes", 0.06), W(0.4), M("ada-save"), T("ada", ENTER),
      U("ada", "To do (3)", 10, "ada-saved"), U("lin", "Write the release notes", 15, "lin-sees"),
      U("you", "Write the release notes", 15, "you-sees"), W(2.0)]
# Both select the one card in Doing.
s += [M("select"), P("ada", "l", "> Check the status page", "ada-sel", 3),
      P("lin", "l", "> Check the status page", "lin-sel", 3), W(1.5),
      {"together": {"ada": "]", "lin": "["}, "mark": "same-moment"}, W(0.2),
      U("ada", ["Conflict", "Saved."], 15, "ada-answer", soft=True),
      U("lin", ["Conflict", "Saved."], 15, "lin-answer", soft=True), W(3.0), M("settled"), W(2.0)]
for who in ("you", "ada", "lin"):
    s += quit_(who)
takes["together"] = {"panes": {w_: pane(w_) for w_ in ("you", "ada", "lin")}, "startAfter": 2.0, "tail": 1.0, "steps": s}

# poll: #launch.
p = []
for who in ("lin", "ada", "you"):
    p += ready(who)
p += [W(1.0)]
for who in ("lin", "ada", "you"):
    p += open_channel(who, "launch", 1.0)
p += [M("open")] + app_open("lin", "poll", "No poll yet", "lin-poll") + [W(1.0), M("ask"),
      T("lin", "a"), W(0.6), T("lin", "Launch day lunch?", 0.06), W(0.3), T("lin", ENTER), W(0.8)]
for option in ("Tacos", "Sushi", "Pizza"):
    p += [T("lin", "a"), W(0.5), T("lin", option, 0.07), W(0.3), T("lin", ENTER), U("lin", option, 10), W(0.6)]
p += [M("asked")] + app_open("ada", "poll", "Launch day lunch?", "ada-poll") + app_open("you", "poll", "Launch day lunch?", "you-poll") + [W(1.0)]
p += [M("votes"), T("ada", "j"), W(0.3), T("ada", ENTER), U("you", "1 vote", 10, "v1", soft=True), W(1.2),
      T("lin", "j"), W(0.3), T("lin", ENTER), U("you", "Total: 2 votes", 10, "v2", soft=True), W(1.2),
      T("you", ENTER), U("ada", "Total: 3 votes", 10, "v3", soft=True), W(3.0), M("voted"), W(1.0)]
for who in ("lin", "ada", "you"):
    p += quit_(who)
takes["poll"] = {"panes": {w_: pane(w_) for w_ in ("lin", "ada", "you")}, "startAfter": 2.0, "tail": 1.0, "steps": p}

# approvals: #design.
a = []
for who in ("ada", "you"):
    a += ready(who)
a += [W(1.0)]
for who in ("ada", "you"):
    a += open_channel(who, "design", 1.0)
a += [M("open")] + app_open("ada", "approvals", "Filter: all", "ada-app") + app_open("you", "approvals", "Filter: all", "you-app") + [W(1.0)]
a += [M("file"), T("ada", "n"), W(0.6), T("ada", "Order a test Mac mini", 0.06), W(0.3), T("ada", ENTER),
      U("you", "Order a test Mac mini", 15, "you-sees"), W(1.5), M("own"), T("ada", "a"),
      U("ada", "You cannot decide your own request.", 10, "own-refused"), W(2.0),
      M("decide"), T("you", "a"), U("ada", "approved", 15, "ada-sees", soft=True), W(1.0), T("ada", ENTER), W(3.0), M("decided"), W(1.0)]
for who in ("ada", "you"):
    a += quit_(who)
takes["approvals"] = {"panes": {"ada": pane("ada"), "you": pane("you")}, "startAfter": 2.0, "tail": 1.0, "steps": a}

# agent: acts 5-8, with the REAL model (one turn). Nothing waits on the
# model's design: the version it writes is its own (keys, badges, sizes).
g = ready("you") + ready("ada") + [W(1.0)] + open_channel("you", "team") + open_channel("ada", "team", 1.0)
g += [M("open")] + app_open("you", "tasks", "To do", "you-board") + app_open("ada", "tasks", "To do", "ada-board") + [W(1.0)]
g += [M("select"), P("you", "j", "> macOS smoke test", "sel1")]
g += [W(0.8), T("you", CTRL_C), W(0.5)] + mention("you", "add a priority and a filter to the board") + [
    U("you", "waits for your approval", 720, "banner"), W(0.4),
    U("you", "last reply just now", 150, "reply", soft=True), W(2.5), M("banner-shown"),
    T("you", CTRL_Y), U("you", "tasks (Tasks), version 2", 15, "overlay"), W(3.5), M("overlay-shown"),
    T("you", "jjjjjjjjjjjj", 0.1), W(2.0), M("scrolled"),
    T("you", "G"), U("you", [": the end", "that is the whole version"], 10, "end"), W(1.5), M("approve"),
    T("you", "a"), U("you", "runs in place of 1, its state kept", 30, "applied"), W(4.0), M("in-place"), W(3.0), M("ada-still-v1"),
    # Act 8: offered to the channel; Ada reads and approves it.
    T("you", CTRL_C), W(0.5), M("share"), T("you", ":app share" + ENTER, 0.05),
    U("you", "is shared in #team", 20, "shared"), W(1.0),
    U("ada", "a person of the channel", 30, "ada-banner"), W(2.0), M("ada-banner-shown"),
    T("ada", CTRL_C), W(0.4), T("ada", CTRL_Y), U("ada", "tasks (Tasks), version 2", 15, "ada-overlay"), W(3.5), M("ada-overlay-shown"),
    T("ada", "G"), U("ada", [": the end", "that is the whole version"], 10, "ada-end"), W(1.5), M("ada-approve"),
    T("ada", "a"), U("ada", "runs in place", 20, "ada-applied", soft=True), W(4.0), M("ada-in-place"), W(1.0)]
g += quit_("you") + quit_("ada")
takes["agent"] = {"panes": {"you": pane("you", LARGE), "ada": pane("ada")}, "startAfter": 2.0, "tail": 1.0, "steps": g}

# records: act 9, with the REAL model (one turn). Lin still runs version 1 of
# the Board (she approved nothing new), whose heading counts are the
# template's own; DONE_BEFORE is how many cards her Done holds before the
# turn (from the together take's settled Boards).
done_before = int(os.environ.get("DONE_BEFORE", "-1"))
r = []
for who in ("you", "ada", "lin"):
    r += ready(who)
r += [W(1.0)]
for who in ("you", "ada", "lin"):
    r += open_channel(who, "team", 1.0)
r += app_open("you", "tasks", "To do", "you-board") + app_open("ada", "tasks", "To do", "ada-board") + \
     app_open("lin", "tasks", f"Done ({done_before})", "lin-board") + [W(1.0), T("you", CTRL_C), W(0.5)]
r += mention("you", "move the release notes card to done") + [
    U("lin", f"Done ({done_before + 1})", 600, "lin-moved", soft=True),
    U("you", "last reply just now", 150, "reply", soft=True), W(3.0), M("moved"), W(2.0)]
r += quit_("you", from_app=False) + quit_("ada") + quit_("lin")
takes["records"] = {"panes": {w_: pane(w_) for w_ in ("you", "ada", "lin")}, "startAfter": 2.0, "tail": 1.0, "steps": r}

# replies: not for the film: the threads of the two mentions, to keep Scout's replies (no model turn).
q = ready("you") + [W(1.0)] + open_channel("you", "team") + [
    T("you", "G"), W(0.5), T("you", ENTER), U("you", "context · ", 20, "thread1", soft=True), W(2.5), M("records-thread"),
    T("you", "q"), W(0.8), T("you", "G"), W(0.4), T("you", "k"), W(0.4), T("you", "k"), W(0.4), T("you", ENTER),
    U("you", "context · ", 20, "thread2", soft=True), W(2.5), M("agent-thread"), T("you", "q"), W(0.8)] + quit_("you", from_app=False)
takes["replies"] = {"panes": {"you": pane("you", LARGE)}, "startAfter": 2.0, "tail": 1.0, "steps": q}

for name, take in takes.items():
    (OUT / f"{name}.json").write_text(json.dumps(take, ensure_ascii=True, indent=None) + "\n")
print(len(takes), "takes:", ", ".join(takes))
