"""Reload with real apps, on the example host: the six fixtures are edited
while a person uses them.

    tests/apps-bench/reload [--saves N] [--size 40x16] [--only edit,swap,moment] [app ...]

Three parts, each one line of result for each app:

edit    Bring the app into a user's state (data loaded, a selection that is
        not the first row, a filter or search, a form open with text in it),
        then N times change a visible label and save. After every save the
        data, the selection, the filter and the open form with its text must
        still be there. Then **a broken save**: the old version keeps
        running (a change by bo still shows), the error names file and line,
        and the fix clears it. Then one write of its own (checked in the
        service) and one change by bo (checked on screen): the connection
        survived too. The time is from the write of the file to the new
        label on the screen.
swap    The owner's example. The Board runs with a search and a selection;
        its file is replaced by the Board with priority. The board changes in
        place: the indicator appears, selection, search and data stay, and
        the new keys work at once.
moment  A save at a bad moment: while a write waits for its reply (the reply
        reaches the new version, and there is one write), and while the
        first list waits (the list reaches the new version).

The scripts are copies under `$APPS_BENCH_OUT/work`; the fixtures are never
written. Exit status 0 only if every check held.
"""
import argparse
import json
import os
import random
import shutil
import statistics
import sys
import time

from drive import REPO, launch, stop
from harness import out_dir, parse_size, pid

FIXTURES = os.path.join(REPO, "tests", "fixtures", "apps")


# ------------------------------------------------------------------ helpers
def fixture(app):
    return os.path.join(FIXTURES, app, "app.lua")


def fresh_copy(src_file, name):
    d = os.path.join(out_dir(), "work", name)
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    dst = os.path.join(d, "app.lua")
    shutil.copy(src_file, dst)
    return dst


def save(path, text):
    """Writes in place, as most editors do. Returns the time after the write."""
    with open(path, "w") as f:
        f.write(text)
    return time.monotonic()


def row(*parts):
    """Predicate: some screen line holds every part, in this order."""
    def pred(s):
        for line in s.screen.lines():
            at = 0
            for p in parts:
                at = line.find(p, at)
                if at < 0:
                    break
                at += len(p)
            else:
                return True
        return False
    return pred


def text_has(t):
    return lambda s: t in s.text()


def text_lacks(t):
    return lambda s: t not in s.text()


def all_of(*preds):
    return lambda s: all(p(s) for p in preds)


def pct(values, p):
    v = sorted(values)
    return v[max(0, int(round(p / 100 * len(v))) - 1)] if v else None


def replace_once(text, old, new):
    n = text.count(old)
    if n != 1:
        raise SystemExit(f"expected exactly one {old!r} in the script, found {n}")
    return text.replace(old, new)


def run_steps(sess, steps):
    """Keys, `type:`, `wait:`, `sleep:`. False if a wait failed."""
    ok = True
    for step in steps:
        kind, _, arg = step.partition(":")
        if kind == "type" and arg:
            sess.type(arg)
        elif kind == "wait" and arg:
            ok &= sess.wait_for(text_has(arg), 5)
        elif kind == "sleep" and arg:
            sess.sleep(float(arg))
        else:
            sess.key(step)
        sess.settle(0.1, 1.0)
    return ok


def log_text(path):
    try:
        with open(path) as f:
            return f.read()
    except OSError:
        return ""


def content_of(svc, rtype, **fields):
    rec = svc.find(rtype, **fields)
    return rec["content"] if rec else {}


def remote_update(svc, rtype, by, change, user="bo"):
    """bo changes fields of the record found by `by`, keeping the rest."""
    rec = svc.find(rtype, **by)
    svc.control(cmd="remote", user=user, op="update", record_id=rec["id"], content=dict(rec["content"], **change))


def remote_create(svc, rtype, content, user="bo"):
    svc.control(cmd="remote", user=user, op="create", type=rtype, content=content)


class Result:
    """Named checks of one run; the failed ones are what is reported."""

    def __init__(self, part, app):
        self.part, self.app = part, app
        self.checks = []
        self.note = ""
        self.screens = []

    def check(self, name, ok):
        self.checks.append((name, bool(ok)))
        return bool(ok)

    def failed(self):
        return [name for name, ok in self.checks if not ok]

    def line(self):
        bad = self.failed()
        head = f"{'PASS' if not bad else 'FAIL'} reload {self.part} {self.app}: {len(self.checks) - len(bad)}/{len(self.checks)} checks"
        if self.note:
            head += f"; {self.note}"
        if bad:
            head += "\n     failed: " + "; ".join(bad)
        return head


# ------------------------------------------------------------- the six apps
# For each app: how to bring it into a user's state, a label of its screen to
# change, what must survive, and a write of its own and two of bo's.
def board(priority):
    def bo4(svc):
        remote_update(svc, "card", {"title": "Bravo"}, {"priority": 3} if priority else {"title": "Bravo v2"})

    def bo5(svc):
        remote_update(svc, "card", {"title": "Foxtrot"}, {"priority": 1} if priority else {"title": "Foxtrot v2"})

    return dict(
        seed="board-priority" if priority else "board",
        setup=["wait:Loaded 6", "/", "type:r", "enter", "j", "n", "type:half"],
        state="search 'r' active, Charlie selected (not the first card), the New card form open with 'half'",
        label=('"To do"', '"T{i:02d} do"', "T{i:02d} do"), label_seen="To do",
        survive={
            # Three cards match the search; in the pane the form covers the third.
            "data": lambda t: all_of(text_has("Bravo"), text_has("do (2)"), text_has("Done (1)")),
            "selection": lambda t: row("> Charlie"),
            "search": lambda t: text_has("Search: r"),
            "form": lambda t: all_of(text_has("New card"), text_has("Title: half")),
        },
        bo4=bo4, bo4_shows=text_has("Bravo P3" if priority else "Bravo v2"),
        own=["enter"], own_desc="Enter in the form creates one card 'half'",
        own_check=lambda svc: len([r for r in svc.records("card") if r["content"].get("title") == "half"]) == 1,
        bo5=bo5, bo5_shows=text_has("Foxtrot P1" if priority else "Foxtrot v2"),
        write=["2"] if priority else ["]"], write_ok="Saved.", loaded="Loaded 6 cards.", lists=1,
    )


def poll():
    return dict(
        seed="poll",
        setup=["wait:Loaded 4", "j", "enter", "wait:Voted for Sushi.", "/", "type:s", "enter", "a", "type:half"],
        state="voted for Sushi and Sushi selected, search 's' active, the New option form open with 'half'",
        label=('"Total: "', '"T{i:02d}: "', "T{i:02d}: "), label_seen="Total: ",
        survive={
            "data": lambda t: row("Sushi", "3 votes"),
            "selection": lambda t: row("> [x] Sushi"),
            "search": lambda t: all_of(text_has("Search: s"), text_lacks("Pizza")),
            "form": lambda t: all_of(text_has("New option"), text_has("Option: half")),
        },
        bo4=lambda svc: remote_create(svc, "vote", {"voter": pid("eve"), "option": "Salad"}, user="eve"),
        bo4_shows=row("Salad", "1 vote"),
        own=["enter"], own_desc="Enter in the form adds the option 'half' to the poll",
        own_check=lambda svc: content_of(svc, "poll").get("options", [])[-1:] == ["half"],
        bo5=lambda svc: remote_update(svc, "vote", {"voter": pid("bo")}, {"option": "Tacos"}),
        bo5_shows=row("Tacos", "1 vote"),
        write=["enter"], write_ok="Voted for Tacos.", loaded="Loaded 4 options.", lists=2,
        # While the first load waits the poll shows no total: the new label
        # is only seen once the lists have answered.
        label_while_loading=False,
    )


def rota():
    return dict(
        seed="rota",
        setup=["wait:Loaded 7", "j", "t", "wait:Took Tue.", "f", "f", "g", "wait:Give up Tue?"],
        state="Tue taken by ana and selected, filter 'mine', the give-up confirmation open",
        label=('"Days: "', '"D{i:02d}: "', "D{i:02d}: "), label_seen="Days: ",
        survive={
            "data": lambda t: text_has("ana 1, bo 2, cy 1"),
            "selection": lambda t: row("> Tue", "ana"),
            "filter": lambda t: text_has("Show: mine"),
            "form": lambda t: text_has("Give up Tue?"),
        },
        bo4=lambda svc: remote_update(svc, "day", {"day": "Thu"}, {"holder": "ana"}),
        bo4_shows=row("Thu", "ana"),
        data_after_bo4=lambda t: text_has("ana 2, bo 2, cy 1"),
        own=["enter"], own_desc="Enter on the confirmation gives Tue up (holder null in the service)",
        own_check=lambda svc: content_of(svc, "day", day="Tue").get("holder", "?") is None,
        bo5=lambda svc: remote_update(svc, "day", {"day": "Sat"}, {"holder": "ana"}),
        bo5_shows=row("Sat", "ana"),
        write=["j", "t"], write_ok="Took Tue.", loaded="Loaded 7 days.", lists=1,
        moment_label=('"Show: "', '"Zz{i}: "', "Zz{i}: "),
    )


def approvals():
    def request(title, order):
        return {"title": title, "author": pid("bo"), "state": "pending", "decided_by": None, "reason": None,
                "order": order}

    return dict(
        seed="approvals",
        setup=["wait:Loaded 5", "j", "f", "n", "type:half typ"],
        state="Team lunch selected, filter 'pending', the New request form open with 'half typ'",
        label=('"Filter: "', '"F{i:02d}: "', "F{i:02d}: "), label_seen="Filter: ",
        survive={
            "data": lambda t: text_has("Extra monitor"),
            "selection": lambda t: row("> pending", "Team lunch"),
            "filter": lambda t: all_of(text_has(t + "pending"), text_lacks("Conference trip")),
            "form": lambda t: all_of(text_has("New request"), text_has("Title: half typ")),
        },
        bo4=lambda svc: remote_create(svc, "request", request("Desk", 1001)), bo4_shows=text_has("Desk"),
        own=["enter"], own_desc="Enter in the form files one request 'half typ'",
        own_check=lambda svc: len([r for r in svc.records("request")
                                   if r["content"].get("title") == "half typ"]) == 1,
        bo5=lambda svc: remote_create(svc, "request", request("Chair", 1002)), bo5_shows=text_has("Chair"),
        write=["n", "type:Pend", "enter"], write_ok="Request filed.", loaded="Loaded 5 requests.", lists=1,
    )


def panel():
    def value(n):
        return lambda svc: remote_update(svc, "metric", {"name": "CPU"},
                                         {"history": content_of(svc, "metric", name="CPU")["history"] + [n]})

    return dict(
        seed="panel",
        setup=["wait:Loaded 5", "j", "j", "s", "tab", "a", "type:half"],
        state="Latency selected, sort Value, tab Alerts, the acknowledge form open with 'half'",
        label=('"Sort: "', '"S{i:02d}: "', "S{i:02d}: "), label_seen="Sort: ",
        survive={
            "data": lambda t: row("CPU", "93"),
            "selection": lambda t: row(">", "Latency"),
            "tab and sort": lambda t: all_of(text_has("*Alerts (2)"), text_has(t + "Value")),
            "form": lambda t: all_of(text_has("Acknowledge Latency"), text_has("Note: half")),
        },
        bo4=value(95), bo4_shows=row("CPU", "95"), data_after_bo4=lambda t: row("CPU", "95"),
        own=["enter"], own_desc="Enter in the form acknowledges Latency as ana with the note 'half'",
        own_check=lambda svc: (content_of(svc, "metric", name="Latency").get("acked_by"),
                               content_of(svc, "metric", name="Latency").get("note")) == ("ana", "half"),
        bo5=value(97), bo5_shows=row("CPU", "97"),
        write=["a", "type:x", "enter"], write_ok="Acknowledged.", loaded="Loaded 5 metrics.", lists=1,
    )


APPS = {
    "board": lambda: board(False),
    "poll": poll,
    "rota": rota,
    "approvals": approvals,
    "panel": panel,
    "board-priority": lambda: board(True),
}


# --------------------------------------------------------------------- edit
def edit(app, saves, size):
    cfg = APPS[app]()
    res = Result("edit", app)
    path = fresh_copy(fixture(app), f"edit-{app}")
    original = open(path).read()
    old, code, shown = cfg["label"]
    host_log = os.path.join(out_dir(), f"reload-edit-{app}-host.log")
    if os.path.exists(host_log):
        os.remove(host_log)
    sess, svc = launch(path, size[0], size[1], svc_app=cfg["seed"], host_log=host_log,
                       log=os.path.join(out_dir(), f"reload-edit-{app}-service.jsonl"))

    def survivors(token):
        return {k: bool(f(token)(sess)) for k, f in cfg["survive"].items()}

    try:
        res.check("the state was reached: " + cfg["state"], run_steps(sess, cfg["setup"]))
        for k, v in survivors(cfg["label_seen"]).items():
            res.check(f"before any save: {k}", v)
        times, lost = [], {}
        for i in range(1, saves + 1):
            token = shown.format(i=i)
            t0 = save(path, replace_once(original, old, code.format(i=i)))
            if not sess.wait_for(text_has(token), 3):
                res.check(f"save {i} shows the new label {token!r}", False)
                res.screens.append(sess.screen.dump(f"{app}: save {i} did not show {token!r}"))
                break
            times.append((time.monotonic() - t0) * 1000)
            sess.settle(0.05, 0.5)
            for k, v in survivors(token).items():
                if not v:
                    lost[k] = lost.get(k, 0) + 1
            # Saves land at any point of the host's 50 ms look at the file.
            sess.sleep(0.12 + random.random() * 0.1)
        res.check(f"all {saves} saves were shown", len(times) == saves)
        for k in cfg["survive"]:
            res.check(f"{k} survived every save (lost after {lost.get(k, 0)})", k not in lost)
        if times:
            res.note = (f"{len(times)} saves, write to screen p50 {statistics.median(times):.1f} ms, "
                        f"p95 {pct(times, 95):.1f} ms, max {max(times):.1f} ms")
            res.times = times

        # A broken save: line 4 gets a syntax error, the rest is the last good version.
        token = shown.format(i=saves)
        good = replace_once(original, old, code.format(i=saves))
        lines = good.split("\n")
        save(path, "\n".join(lines[:3] + ["local = 1"] + lines[3:]))
        res.check("a broken save: the error names the file and line 4 on the screen",
                  sess.wait_for(text_has("app.lua:4:"), 3))
        sess.settle(0.1, 0.5)
        res.check("a broken save: the log says the previous version keeps running",
                  "reload failed: the previous version keeps running" in log_text(host_log))
        res.screens.append(sess.screen.dump(f"{app}: a broken save"))
        cfg["bo4"](svc)
        res.check("a broken save: the old version still shows a change by bo", sess.wait_for(cfg["bo4_shows"], 2))
        if "data_after_bo4" in cfg:  # bo's change is part of the data from here on
            cfg["survive"]["data"] = cfg["data_after_bo4"]
        # The error covers the last row or two of the region: what is under
        # it is not lost, and is looked at after the fix.
        fixed = shown.format(i=99)
        save(path, replace_once(original, old, code.format(i=99)))
        res.check("the fix clears the error and shows its label",
                  sess.wait_for(all_of(text_has(fixed), text_lacks("app.lua:4:")), 3))
        sess.settle(0.1, 0.5)
        for k, v in survivors(fixed).items():
            res.check(f"after the broken save and its fix: {k}", v)
        res.screens.append(sess.screen.dump(f"{app}: after the fix"))

        # One write of its own, one change by another participant.
        run_steps(sess, cfg["own"])
        end, ok = time.monotonic() + 3, False
        while time.monotonic() < end and not ok:
            ok = bool(cfg["own_check"](svc))
            sess.sleep(0.1)
        res.check("after the reloads: " + cfg["own_desc"], ok)
        cfg["bo5"](svc)
        res.check("after the reloads: a change by bo shows (the connection survived)",
                  sess.wait_for(cfg["bo5_shows"], 2))
        res.check("the app runs on", sess.alive())
        res.screens.append(sess.screen.dump(f"{app}: after its own write and bo's change"))
    finally:
        stop(sess, svc)
    res.check("Ctrl+C leaves with status 0", sess.proc.returncode == 0)
    return res


# --------------------------------------------------------------------- swap
def swap(seed, size):
    """The running Board becomes the Board with priority."""
    res = Result("swap", f"board->board-priority (seed {seed})")
    path = fresh_copy(fixture("board"), f"swap-{seed}")
    host_log = os.path.join(out_dir(), f"reload-swap-{seed}-host.log")
    if os.path.exists(host_log):
        os.remove(host_log)
    sess, svc = launch(path, size[0], size[1], svc_app=seed, host_log=host_log,
                       log=os.path.join(out_dir(), f"reload-swap-{seed}-service.jsonl"))

    def priority(title):
        return content_of(svc, "card", title=title).get("priority")

    def service(pred, seconds=3):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if pred():
                return True
            sess.sleep(0.1)
        return bool(pred())

    try:
        res.check("the Board was brought to: search 'r', Charlie selected",
                  run_steps(sess, ["wait:Loaded 6", "j", "/", "type:r", "enter", "j"])
                  and all_of(row("> Charlie"), text_has("Search: r"), text_lacks("Priority:"))(sess))
        res.screens.append(sess.screen.dump("the Board, before"))
        t0 = save(path, open(fixture("board-priority")).read())
        res.check("the indicator 'Priority: all' appears", sess.wait_for(text_has("Priority: all"), 3))
        res.note = f"the new board on screen {(time.monotonic() - t0) * 1000:.0f} ms after the save"
        sess.settle(0.1, 0.5)
        res.check("the selection is kept", row("> Charlie")(sess))
        res.check("the search is kept", all_of(text_has("Search: r"), text_lacks("Alpha"))(sess))
        res.check("the data is kept", all_of(text_has("Bravo"), text_has("Foxtrot"))(sess))
        res.check("no error", "error:" not in log_text(host_log))
        res.screens.append(sess.screen.dump("the Board with priority, right after the save"))
        sess.key("2")
        res.check("2 shows 'Charlie P2'", sess.wait_for(row("> Charlie P2"), 3))
        res.check("2 is stored: priority 2 in the service", service(lambda: priority("Charlie") == 2))
        sess.key("p")
        res.check("p shows 'Priority: P1'", sess.wait_for(text_has("Priority: P1"), 3))
        sess.key("p")
        res.check("p again shows 'Priority: P2' with Charlie",
                  sess.wait_for(all_of(text_has("Priority: P2"), row("> Charlie P2")), 3))
        sess.key("p")
        sess.key("p")
        res.check("p p is back at 'Priority: all'", sess.wait_for(text_has("Priority: all"), 3))
        # Under P3 no card of the search is shown, so the first shown card
        # is selected afterwards: go to Charlie again.
        for _ in range(3):
            if row("> Charlie")(sess):
                break
            sess.key("j")
        sess.key("1")
        sess.wait_for(row("> Charlie P1"), 3)
        sess.wait_for(text_has("Saved."), 3)
        sess.key("3")
        res.check("1 then 3 shows 'Charlie P3'", sess.wait_for(row("> Charlie P3"), 3))
        res.check("3 is stored: priority 3 in the service", service(lambda: priority("Charlie") == 3))
        remote_update(svc, "card", {"title": "Bravo"}, {"priority": 1})
        res.check("a priority set by bo shows ('Bravo P1')", sess.wait_for(text_has("Bravo P1"), 2))
        res.check("no error in the end", "error:" not in log_text(host_log))
        res.screens.append(sess.screen.dump("in the end"))
    finally:
        stop(sess, svc)
    res.check("Ctrl+C leaves with status 0", sess.proc.returncode == 0)
    return res


# ------------------------------------------------------------------- moment
def writes(svc):
    counts = svc.control(cmd="stats")["counts"]
    return sum(v for k, v in counts.items() if k in ("create", "update", "delete"))


def moment_write(app, size):
    cfg = APPS[app]()
    res = Result("moment", f"{app}, a write waiting")
    old, code, shown = cfg.get("moment_label", cfg["label"])
    path = fresh_copy(fixture(app), f"moment-write-{app}")
    original = open(path).read()
    host_log = os.path.join(out_dir(), f"reload-moment-write-{app}-host.log")
    if os.path.exists(host_log):
        os.remove(host_log)
    sess, svc = launch(path, size[0], size[1], svc_app=cfg["seed"], scenario="slow-write", host_log=host_log,
                       log=os.path.join(out_dir(), f"reload-moment-write-{app}-service.jsonl"))
    try:
        res.check("loaded", run_steps(sess, [f"wait:{cfg['loaded']}"]))
        before = writes(svc)
        run_steps(sess, cfg["write"][:-1])
        sess.key(cfg["write"][-1], settle=0)  # the key that sends the write
        t_key = time.monotonic()
        res.check("'Saving...' while the reply is delayed 500 ms", sess.wait_for(text_has("Saving..."), 0.4))
        save(path, replace_once(original, old, code.format(i=1)))
        token = shown.format(i=1)
        seen = sess.wait_for(text_has(token), 0.45)
        ms = (time.monotonic() - t_key) * 1000
        res.check(f"the new version is on screen before the reply ({ms:.0f} ms after the key)", seen and ms < 500)
        res.check(f"the reply reaches the new version: '{cfg['write_ok']}' with the new label",
                  sess.wait_for(all_of(text_has(cfg["write_ok"]), text_has(token)), 3))
        sess.sleep(0.8)
        res.check(f"exactly one write in the service (saw {writes(svc) - before})", writes(svc) - before == 1)
        res.check("no error", "error:" not in log_text(host_log))
        res.screens.append(sess.screen.dump(f"{app}: after the reply"))
    finally:
        stop(sess, svc)
    return res


def moment_load(app, size):
    cfg = APPS[app]()
    res = Result("moment", f"{app}, the first list waiting")
    old, code, shown = cfg.get("moment_label", cfg["label"])
    path = fresh_copy(fixture(app), f"moment-load-{app}")
    original = open(path).read()
    host_log = os.path.join(out_dir(), f"reload-moment-load-{app}-host.log")
    if os.path.exists(host_log):
        os.remove(host_log)
    sess, svc = launch(path, size[0], size[1], svc_app=cfg["seed"], scenario="slow-load", host_log=host_log,
                       log=os.path.join(out_dir(), f"reload-moment-load-{app}-service.jsonl"))
    try:
        t0 = time.monotonic()
        res.check("'Loading...' while the first list is delayed 3000 ms", sess.wait_for(text_has("Loading..."), 2))
        sess.sleep(0.5)
        save(path, replace_once(original, old, code.format(i=2)))
        token = shown.format(i=2)
        end = time.monotonic() + 2
        while time.monotonic() < end and "saved: reload" not in log_text(host_log):
            sess.sleep(0.02)
        res.check("the reload happened before the reply",
                  "saved: reload" in log_text(host_log) and cfg["loaded"] not in sess.text())
        if cfg.get("label_while_loading", True):
            res.check("the new version is on screen before the reply", sess.wait_for(text_has(token), 1)
                      and cfg["loaded"] not in sess.text())
        res.check(f"the list reaches the new version: '{cfg['loaded']}' with the new label",
                  sess.wait_for(all_of(text_has(cfg["loaded"]), text_has(token)), 5))
        res.note = f"loaded {(time.monotonic() - t0) * 1000:.0f} ms after start"
        lists = svc.control(cmd="stats")["counts"].get("list", 0)
        res.check(f"no list was asked for again (saw {lists}, a load is {cfg['lists']})", lists == cfg["lists"])
        res.check("no error", "error:" not in log_text(host_log))
        res.screens.append(sess.screen.dump(f"{app}: after the reply"))
    finally:
        stop(sess, svc)
    return res


# ---------------------------------------------------------------------- main
def main(argv):
    ap = argparse.ArgumentParser(prog="reload", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--saves", type=int, default=25, help="saves of each app in the part 'edit' (default 25)")
    ap.add_argument("--size", default="80x24", type=parse_size, help="the terminal's size (default 80x24)")
    ap.add_argument("--only", default="edit,swap,moment", help="parts to run: edit,swap,moment")
    ap.add_argument("apps", nargs="*", help="apps (default: all six)")
    args = ap.parse_args(argv)
    apps = args.apps or list(APPS)
    for a in apps:
        if a not in APPS:
            raise SystemExit(f"unknown app {a!r}; known: {', '.join(APPS)}")
    parts = [p for p in args.only.split(",") if p]
    results = []

    def done(res):
        results.append(res)
        print(res.line(), flush=True)
        if res.failed():
            for s in res.screens[-2:]:
                print("\n".join("     " + l for l in s.splitlines()))

    if "edit" in parts:
        for app in apps:
            done(edit(app, args.saves, args.size))
    if "swap" in parts:
        for seed in ("board", "board-priority"):
            done(swap(seed, args.size))
    if "moment" in parts:
        for app in apps:
            done(moment_write(app, args.size))
            done(moment_load(app, args.size))

    failed = [r for r in results if r.failed()]
    times = [t for r in results for t in getattr(r, "times", [])]
    summary = f"RESULT reload size={args.size[0]}x{args.size[1]}: {len(results) - len(failed)}/{len(results)} passed, {len(failed)} failed"
    if times:
        summary += (f"; {len(times)} saves, write to screen p50 {statistics.median(times):.1f} ms, "
                    f"p95 {pct(times, 95):.1f} ms, max {max(times):.1f} ms")
    print(summary, flush=True)
    with open(os.path.join(out_dir(), "reload-screens.txt"), "w") as f:
        f.write("\n".join(s for r in results for s in r.screens) + "\n")
    with open(os.path.join(out_dir(), "reload.json"), "w") as f:
        json.dump([{"part": r.part, "app": r.app, "note": r.note, "checks": r.checks,
                    "times_ms": [round(t, 1) for t in getattr(r, "times", [])]} for r in results], f, indent=1)
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
