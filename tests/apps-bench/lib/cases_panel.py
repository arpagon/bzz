"""Acceptance cases for the Panel app (spec/panel.md)."""
import re

from cases_board import check_quit
from harness import CaseFail, check, contains, expect, hold, lacks, no_overflow, status_is
from panel_props import gauge_problem, read_gauge, sparkline_problem

NAMES = ["CPU", "Disk", "Latency", "Memory", "Queue", "Network"]
HIST = {"CPU": [35, 42, 50, 61, 70, 88, 93], "Disk": [40, 41, 41, 42, 43, 43, 44],
        "Latency": [120, 180, 260, 310, 420, 450, 480], "Memory": [6, 7, 7, 8, 9, 8, 8],
        "Queue": [300, 280, 350, 400, 380, 420, 390]}
MAX = {"CPU": 100, "Disk": 100, "Latency": 500, "Memory": 16, "Queue": 1000, "Network": 1000}
INIT = [("CPU", 93, "alert"), ("Disk", 44, "ok"), ("Latency", 480, "alert"), ("Memory", 8, "ok"),
        ("Queue", 390, "acked")]
BY = {n: (n, v, s) for n, v, s in INIT}
START_TIMEOUT = 10.0
SIZES = ((40, 16), (80, 24), (120, 40))
ROWS = [(n, re.compile(r"(>)? *\b" + re.escape(n) + r" +(\d+) +(\S+) +(ok|alert|acked)(?!\w)")) for n in NAMES]
TABS = re.compile(r"(\*?)All \((\d+)\).*?(\*?)Alerts \((\d+)\)")
SORT = re.compile(r"Sort: (Name|Value|Status) (asc|desc)")


def parse_panel(sess):
    rows, tabs, sort = [], None, None
    for line in sess.screen.lines()[:-1]:
        for name, rx in ROWS:
            m = rx.search(line)
            if m:
                rows.append((name, int(m.group(2)), m.group(4), m.group(1) == ">"))
                break
        t = TABS.search(line)
        if t and tabs is None:
            active = "All" if t.group(1) == "*" and not t.group(3) else (
                "Alerts" if t.group(3) == "*" and not t.group(1) else "?")
            tabs = (active, int(t.group(2)), int(t.group(4)))
        s = SORT.search(line)
        if s and sort is None:
            sort = f"{s.group(1)} {s.group(2)}"
    return rows, tabs, sort


def panel_is(want, selected=None, tabs=None, sort=None):
    def pred(sess):
        rows, got_tabs, got_sort = parse_panel(sess)
        if [(n, v, s) for n, v, s, _ in rows] != list(want):
            return False
        if selected is not None and [n for n, _, _, sel in rows if sel] != [selected]:
            return False
        if tabs is not None and got_tabs != tabs:
            return False
        if sort is not None and got_sort != sort:
            return False
        return True
    return pred


def describe(want, selected=None, tabs=None, sort=None):
    rows = ", ".join(f"'{'>' if n == selected else ' '}{n} {v} {s}'" for n, v, s in want)
    out = f"table rows {rows}" if want else "no table rows"
    if tabs is not None:
        a, n, m = tabs
        out += f", tabs '{'*' if a == 'All' else ''}All ({n})' '{'*' if a == 'Alerts' else ''}Alerts ({m})'"
    if sort is not None:
        out += f", 'Sort: {sort}'"
    return out


def want(sess, rows, selected=None, tabs=None, sort=None, timeout=3.0, extra=""):
    expect(sess, panel_is(rows, selected, tabs, sort), describe(rows, selected, tabs, sort) + extra, timeout)


def spark_ok(name, history):
    return lambda s: sparkline_problem(s.screen, name, history) is None


def check_spark(sess, name, history, where=""):
    if not sess.wait_for(spark_ok(name, history), 3.0):
        raise CaseFail(f"sparkline: {sparkline_problem(sess.screen, name, history)}{where}", sess.screen.dump())


def check_gauge(sess, value, maximum, where=""):
    if not sess.wait_for(lambda s: gauge_problem(s.screen, value, maximum) is None, 3.0):
        raise CaseFail(f"gauge: {gauge_problem(sess.screen, value, maximum)}{where}", sess.screen.dump())


def start(ctx, scenario="default", cols=None, rows=None):
    sess, svc = ctx.launch(scenario, cols, rows)
    expect(sess, lambda s: panel_is(INIT, "CPU", ("All", 5, 2), "Name asc")(s) and "Loaded 5 metrics." in s.last_row(),
           describe(INIT, "CPU", ("All", 5, 2), "Name asc") + " and status 'Loaded 5 metrics.' after start",
           START_TIMEOUT)
    return sess, svc


def updates(svc):
    return svc.control(cmd="stats")["counts"].get("update", 0)


def content(svc, name):
    rec = svc.find("metric", name=name)
    return rec["content"] if rec else None


def replace(rows, **changes):
    return [changes.get(n, (n, v, s)) for n, v, s in rows]


def c01_first_draw(ctx):
    for cols, rows in SIZES:
        sess, _ = start(ctx, cols=cols, rows=rows)
        sess.settle(0.3)
        check_spark(sess, "CPU", HIST["CPU"], f" at {cols}x{rows}")
        check_gauge(sess, 93, 100, f" at {cols}x{rows}")
        no_overflow(sess, f"at {cols}x{rows}")
        sess.sleep(0.5)
        check(sess.alive(), f"program still running 0.5 s after the first draw at {cols}x{rows}", sess)
        ctx.cleanup()


def c02_form_cancel(ctx):
    sess, svc = start(ctx)
    sess.key("a")
    expect(sess, lambda s: "Acknowledge CPU" in s.text() and "Note:" in s.text(),
           "the form with 'Acknowledge CPU' and 'Note:' after a")
    sess.type("Café 日本")
    expect(sess, contains("Note: Café 日本"), "the field showing 'Note: Café 日本' as typed")
    sess.key("esc")
    expect(sess, lacks("Acknowledge CPU"), "the form closed after Esc")
    want(sess, INIT, "CPU", ("All", 5, 2), extra=" (nothing saved, selection kept) after Esc")
    sess.sleep(0.3)
    check(updates(svc) == 0, "no write after Esc", sess)
    sess.key("j")
    want(sess, INIT, "Disk", extra=" after j (keys back on the table)")


def c03_save_once(ctx):
    sess, svc = start(ctx, "slow-write")
    sess.key("a")
    expect(sess, contains("Acknowledge CPU"), "the form 'Acknowledge CPU' after a")
    sess.key("enter")
    expect(sess, contains("Note is required."), "'Note is required.' after Enter on an empty note")
    check(updates(svc) == 0, "no write for an empty note", sess)
    sess.type("On it")
    sess.key("enter", settle=0.05)
    sess.key("enter", settle=0)
    rows = replace(INIT, CPU=("CPU", 93, "acked"))
    want(sess, rows, "CPU", ("All", 5, 1), extra=" after Enter pressed twice")
    expect(sess, status_is("Acknowledged."), "status 'Acknowledged.'")
    sess.sleep(0.8)
    check(updates(svc) == 1, f"exactly one update for two Enter presses, saw {updates(svc)}", sess)
    got = content(svc, "CPU")
    check(got and got["status"] == "acked" and got["acked_by"] == "ana" and got["note"] == "On it"
          and got["history"] == HIST["CPU"],
          f"service CPU acked by ana with note 'On it' and the same history, got {got}", sess)


def c04_tabs(ctx):
    sess, _ = start(ctx)
    sess.key("j")
    sess.key("j")
    want(sess, INIT, "Latency")
    sess.key("tab")
    alerts = [BY["CPU"], BY["Latency"]]
    want(sess, alerts, "Latency", ("Alerts", 5, 2), extra=" after Tab")
    sess.key("tab")
    want(sess, INIT, "Latency", ("All", 5, 2), extra=" after Tab back to All (selection kept)")
    sess.key("k")
    want(sess, INIT, "Disk", extra=" after k")
    sess.key("tab")
    want(sess, alerts, "CPU", ("Alerts", 5, 2), extra=" after Tab with Disk selected (not in alert: first row)")


def c05_late_reload(ctx):
    sess, svc = start(ctx)
    svc.control(cmd="delay_next", op="list", ms=1500)
    sess.key("r")
    expect(sess, status_is("Loading..."), "status 'Loading...' during a slow reload")
    sess.key("a")
    expect(sess, contains("Acknowledge CPU"), "the form 'Acknowledge CPU' after a")
    sess.type("x")
    sess.key("enter")
    rows = replace(INIT, CPU=("CPU", 93, "acked"))
    expect(sess, lambda s: panel_is(rows, "CPU", ("All", 5, 1))(s) and "Acknowledged." in s.last_row(),
           describe(rows, "CPU", ("All", 5, 1)) + " and 'Acknowledged.' during the slow reload")
    hold(sess, panel_is(rows, "CPU", ("All", 5, 1)), describe(rows, "CPU", ("All", 5, 1))
         + " to stay after the older reload reply arrives (it predates the acknowledgement)", 2.2)


def c06_narrow(ctx):
    sess, _ = start(ctx)
    sess.settle(0.3)
    ow, oh = ctx.other
    at = f"at {ow}x{oh}"
    sess.resize(ow, oh)
    sess.screen.reset_counters()
    expect(sess, lambda s: panel_is(INIT, "CPU", ("All", 5, 2), "Name asc")(s)
           and "Loaded 5 metrics." in s.last_row() and s.screen.rows == oh,
           f"{at}: " + describe(INIT, "CPU", ("All", 5, 2), "Name asc") + " and status 'Loaded 5 metrics.'")
    check_spark(sess, "CPU", HIST["CPU"], f" {at}")
    check_gauge(sess, 93, 100, f" {at}")
    sess.settle(0.3)
    no_overflow(sess, at)
    sess.key("j")
    want(sess, INIT, "Disk", extra=f" after j {at}")
    check_gauge(sess, 44, 100, f" for Disk {at}")
    sess.key("s")
    by_value = [BY["Memory"], BY["Disk"], BY["CPU"], BY["Queue"], BY["Latency"]]
    want(sess, by_value, "Disk", sort="Value asc", extra=f" after s {at}")
    sess.key("tab")
    want(sess, [BY["CPU"], BY["Latency"]], "CPU", ("Alerts", 5, 2), extra=f" after Tab {at}")
    sess.settle(0.3)
    no_overflow(sess, f"{at} after sorting and switching tabs")
    bw, bh = ctx.size
    sess.resize(bw, bh)
    sess.screen.reset_counters()
    want(sess, [BY["CPU"], BY["Latency"]], "CPU", ("Alerts", 5, 2), "Value asc", extra=f" back at {bw}x{bh}")
    sess.settle(0.3)
    no_overflow(sess, f"after coming back to {bw}x{bh}")


def c07_paste(ctx):
    sess, svc = start(ctx)
    sess.paste("a\ns\nd\tq\n")
    sess.sleep(0.6)
    check(sess.alive(), "a paste on the table is ignored (the pasted q did not quit)", sess)
    want(sess, INIT, "CPU", ("All", 5, 2), "Name asc", extra=" unchanged after pasting 'a\\ns\\nd\\tq\\n'")
    check("Acknowledge" not in sess.text(), "no form opened by the paste", sess)
    check(updates(svc) == 0, "no write caused by the paste", sess)
    sess.key("a")
    expect(sess, contains("Acknowledge CPU"), "the form 'Acknowledge CPU' after a")
    sess.paste("one\ntwo\r\nq")
    expect(sess, contains("Note: one two q"), "one field 'Note: one two q' after pasting three lines")
    sess.sleep(0.3)
    check(sess.alive() and "Acknowledge CPU" in sess.text(), "the form still open after the paste", sess)
    check(updates(svc) == 0, "no write caused by the pasted line breaks", sess)
    sess.key("esc")
    expect(sess, lacks("Acknowledge CPU"), "the form closed after Esc")


def c08_load_error(ctx):
    sess, svc = ctx.launch("load-error")
    expect(sess, status_is("Load failed. Press r to retry."),
           "status 'Load failed. Press r to retry.' when the first load fails", START_TIMEOUT)
    sess.key("r")
    want(sess, INIT, "CPU", extra=" after r")
    expect(sess, status_is("Loaded 5 metrics."), "status 'Loaded 5 metrics.' after the retry")
    sess.key("j")
    want(sess, INIT, "Disk")
    svc.control(cmd="fail_next", op="list")
    sess.key("r")
    expect(sess, status_is("Load failed. Press r to retry."), "'Load failed. Press r to retry.' after a failed reload")
    want(sess, INIT, "Disk", ("All", 5, 2), extra=" kept after the failed reload")
    sess.key("r")
    expect(sess, lambda s: "Loaded 5 metrics." in s.last_row() and panel_is(INIT, "Disk")(s),
           describe(INIT, "Disk") + " and 'Loaded 5 metrics.' after the second retry")


def c09_quit_pending(ctx):
    sess, svc = start(ctx)
    svc.control(cmd="delay_next", op="list", ms=10000)
    sess.key("r")
    expect(sess, status_is("Loading..."), "status 'Loading...' during a reload")
    sess.key("q", settle=0)
    check_quit(sess, "q", (0,))


def c10_repeatable(ctx):
    screens = []
    for i in range(3):
        sess, _ = start(ctx)
        sess.key("s")
        sess.key("down")
        sess.key("tab")
        want(sess, [BY["CPU"], BY["Latency"]], "CPU", ("Alerts", 5, 2), "Value asc",
             extra=f" after s, Down, Tab (run {i + 1})")
        sess.key("down")
        sess.key("a")
        expect(sess, contains("Acknowledge Latency"), f"the form 'Acknowledge Latency' (run {i + 1})")
        sess.type("ok")
        sess.key("enter")
        want(sess, [BY["CPU"]], "CPU", ("Alerts", 5, 1), extra=f" after acknowledging Latency (run {i + 1})")
        expect(sess, status_is("Acknowledged."), f"status 'Acknowledged.' (run {i + 1})")
        sess.settle(0.4)
        screens.append(sess.screen.dump())
        ctx.cleanup()
    if not (screens[0] == screens[1] == screens[2]):
        diff = next(j for j in range(3) if screens[j] != screens[0])
        raise CaseFail(f"three runs of the same keys give the same screen; run 1 and run {diff + 1} differ",
                       screens[0] + "\n--- run %d ---\n" % (diff + 1) + screens[diff])


def c11_remote_change(ctx):
    sess, svc = start(ctx)
    check(svc.wait_subscribed(), "the program subscribes to changes (subscribe request)", sess)
    lists = svc.control(cmd="stats")["counts"].get("list", 0)
    cpu, mem, disk = (svc.find("metric", name=n) for n in ("CPU", "Memory", "Disk"))
    before = read_gauge(sess.screen)
    svc.control(cmd="remote", user="bo", op="update", record_id=cpu["id"],
                content=dict(cpu["content"], history=HIST["CPU"] + [100]))
    svc.control(cmd="remote", user="cy", op="update", record_id=mem["id"],
                content=dict(mem["content"], status="alert", history=HIST["Memory"] + [15]))
    svc.control(cmd="remote", user="bo", op="create", type="metric",
                content={"name": "Network", "unit": "Mbps", "status": "ok", "max": 1000,
                         "history": [200, 250, 300], "acked_by": None, "note": None})
    svc.control(cmd="remote", user="bo", op="delete", record_id=disk["id"])
    rows = [("CPU", 100, "alert"), BY["Latency"], ("Memory", 15, "alert"), ("Network", 300, "ok"), BY["Queue"]]
    want(sess, rows, "CPU", ("All", 5, 3), timeout=2.0, extra=" within 2 s of bo's and cy's changes, no key pressed")
    check_spark(sess, "CPU", HIST["CPU"] + [100], " after bo's new value 100")
    check_gauge(sess, 100, 100, " after bo's new value 100")
    after = read_gauge(sess.screen)
    check(before and after and after["full"] > before["full"],
          f"the gauge's filled part grows from 93% to 100% (cells '█': {before and before['full']} -> "
          f"{after and after['full']})", sess)
    now = svc.control(cmd="stats")["counts"].get("list", 0)
    check(now == lists, f"no reload to show remote changes ({now - lists} extra list requests)", sess)


def c12_conflict(ctx):
    sess, svc = start(ctx)
    cpu = svc.find("metric", name="CPU")
    svc.control(cmd="arm_conflict", record_id=cpu["id"], user="bo",
                content=dict(cpu["content"], history=HIST["CPU"] + [97]))
    sess.key("a")
    expect(sess, contains("Acknowledge CPU"), "the form 'Acknowledge CPU' after a")
    sess.type("On it")
    sess.key("enter")
    rows = replace(INIT, CPU=("CPU", 97, "acked"))
    expect(sess, lambda s: panel_is(rows, "CPU", ("All", 5, 1))(s)
           and "Conflict: merged with a newer change." in s.last_row(),
           describe(rows, "CPU", ("All", 5, 1)) + " and status 'Conflict: merged with a newer change.' after "
           "acknowledging CPU while bo added the value 97")
    got = content(svc, "CPU")
    check(got and got["history"] == HIST["CPU"] + [97] and got["status"] == "acked" and got["acked_by"] == "ana",
          f"service CPU with bo's value 97 kept and acked by ana, got {got}", sess)


def c13_sort(ctx):
    sess, _ = start(ctx)
    steps = [
        ("s", "Value asc", ["Memory", "Disk", "CPU", "Queue", "Latency"]),
        ("d", "Value desc", ["Latency", "Queue", "CPU", "Disk", "Memory"]),
        ("s", "Status asc", ["Queue", "CPU", "Latency", "Disk", "Memory"]),
        ("d", "Status desc", ["Disk", "Memory", "CPU", "Latency", "Queue"]),
        ("s", "Name asc", ["CPU", "Disk", "Latency", "Memory", "Queue"]),
        ("d", "Name desc", ["Queue", "Memory", "Latency", "Disk", "CPU"]),
    ]
    for key, sort, names in steps:
        sess.key(key)
        want(sess, [BY[n] for n in names], "CPU", sort=sort, extra=f" after {key} (CPU stays selected)")
    sess.key("k")
    want(sess, [BY[n] for n in steps[-1][2]], "Disk", sort="Name desc", extra=" after k")


def c14_spark_gauge(ctx):
    sess, _ = start(ctx)
    cpu_full = read_gauge(sess.screen)
    for name in ("Disk", "Latency", "Memory", "Queue"):
        sess.key("j")
        want(sess, INIT, name)
        check_spark(sess, name, HIST[name], f" after selecting {name}")
        check_gauge(sess, HIST[name][-1], MAX[name], f" after selecting {name}")
        if name == "Disk":
            disk_full = read_gauge(sess.screen)
            check(cpu_full and disk_full and disk_full["full"] < cpu_full["full"],
                  f"a shorter filled gauge for Disk (44%) than for CPU (93%) (cells '█': "
                  f"{disk_full and disk_full['full']} vs {cpu_full and cpu_full['full']})", sess)


def c15_ack_rules(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    sess.key("a")
    expect(sess, status_is("Nothing to acknowledge."), "status 'Nothing to acknowledge.' after a on Disk (ok)")
    check("Acknowledge" not in sess.text(), "no form for a metric that is not in alert", sess)
    for _ in range(3):
        sess.key("j")
    want(sess, INIT, "Queue")
    sess.key("r")
    expect(sess, status_is("Loaded 5 metrics."), "status 'Loaded 5 metrics.' after r")
    sess.key("a")
    expect(sess, status_is("Nothing to acknowledge."), "status 'Nothing to acknowledge.' after a on Queue (acked)")
    check(updates(svc) == 0, "no write for metrics that are not in alert", sess)
    for _ in range(4):
        sess.key("k")
    want(sess, INIT, "CPU")
    cpu = svc.find("metric", name="CPU")
    theirs = dict(cpu["content"], status="acked", acked_by="bo", note="Mine")
    svc.control(cmd="arm_conflict", record_id=cpu["id"], user="bo", content=theirs)
    sess.key("a")
    expect(sess, contains("Acknowledge CPU"), "the form 'Acknowledge CPU' after a")
    sess.type("On it")
    sess.key("enter")
    rows = replace(INIT, CPU=("CPU", 93, "acked"))
    expect(sess, lambda s: panel_is(rows, "CPU", ("All", 5, 1))(s) and "Already acknowledged by bo." in s.last_row(),
           describe(rows, "CPU", ("All", 5, 1)) + " and status 'Already acknowledged by bo.'")
    sess.sleep(0.5)
    got = content(svc, "CPU")
    check(got == theirs and updates(svc) == 1,
          f"bo's acknowledgement kept ({theirs}) and no second update; got {got}, {updates(svc)} updates", sess)


CASES = [
    ("01", "first-draw", c01_first_draw),
    ("02", "form-cancel", c02_form_cancel),
    ("03", "save-once", c03_save_once),
    ("04", "tabs", c04_tabs),
    ("05", "late-reload", c05_late_reload),
    ("06", "narrow", c06_narrow),
    ("07", "paste", c07_paste),
    ("08", "load-error", c08_load_error),
    ("09", "quit-pending", c09_quit_pending),
    ("10", "repeatable", c10_repeatable),
    ("11", "remote-change", c11_remote_change),
    ("12", "conflict", c12_conflict),
    ("13", "sort", c13_sort),
    ("14", "sparkline-gauge", c14_spark_gauge),
    ("15", "ack-rules", c15_ack_rules),
]
