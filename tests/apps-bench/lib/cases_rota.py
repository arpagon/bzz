"""Acceptance cases for the Rota app (spec/rota.md)."""
import re

from cases_board import check_quit
from harness import CaseFail, check, contains, expect, hold, lacks, no_overflow, status_is

DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
INIT = [("Mon", "bo"), ("Tue", None), ("Wed", "cy"), ("Thu", None), ("Fri", "bo"), ("Sat", None), ("Sun", None)]
INIT_DAYS = "bo 2, cy 1"
START_TIMEOUT = 10.0
SIZES = ((40, 16), (80, 24), (120, 40))
ROW = re.compile(r"(>)? *\b(" + "|".join(DAYS) + r") +([^\s>]+)(?!\S)")
DAYS_LINE = re.compile(r"Days: (none|[^\s,]+ \d+(?:, [^\s,]+ \d+)*)")


def parse_rota(sess):
    rows, days = [], None
    for line in sess.screen.lines()[:-1]:
        m = ROW.search(line)
        if m:
            holder = None if m.group(3) == "free" else m.group(3)
            rows.append((m.group(2), holder, m.group(1) == ">"))
        d = DAYS_LINE.search(line)
        if d:
            days = d.group(1)
    return rows, days


def rota_is(want, selected=None, days=None, show=None):
    def pred(sess):
        rows, got_days = parse_rota(sess)
        if [(d, h) for d, h, _ in rows] != list(want):
            return False
        if selected is not None and [d for d, _, s in rows if s] != [selected]:
            return False
        if days is not None and got_days != days:
            return False
        if show is not None and f"Show: {show}" not in sess.text():
            return False
        return True
    return pred


def describe(want, selected=None, days=None, show=None):
    rows = ", ".join(f"'{'>' if d == selected else ' '} {d}  {h or 'free'}'" for d, h in want)
    s = f"rota rows {rows}" if want else "no day rows"
    if days is not None:
        s += f", 'Days: {days}'"
    if show is not None:
        s += f", 'Show: {show}'"
    return s


def want(sess, rows, selected=None, days=None, show=None, timeout=3.0, extra=""):
    expect(sess, rota_is(rows, selected, days, show), describe(rows, selected, days, show) + extra, timeout)


def held(rows, **changes):
    return [(d, changes[d] if d in changes else h) for d, h in rows]


def start(ctx, scenario="default", cols=None, rows=None):
    sess, svc = ctx.launch(scenario, cols, rows)
    expect(sess, lambda s: rota_is(INIT, "Mon", INIT_DAYS, "all")(s) and "Loaded 7 days." in s.last_row(),
           describe(INIT, "Mon", INIT_DAYS, "all") + " and status 'Loaded 7 days.' after start", START_TIMEOUT)
    return sess, svc


def updates(svc):
    return svc.control(cmd="stats")["counts"].get("update", 0)


def holder(svc, day):
    rec = svc.find("day", day=day)
    return rec["content"].get("holder") if rec else "(missing)"


def order_kept(svc, sess):
    """A write replaces the whole content: the day's place must survive it."""
    got = [r["content"].get("order") for r in svc.records("day")]
    check(got == list(range(1, len(DAYS) + 1)),
          f"every day keeps its 'order' (1 to {len(DAYS)}) after the app's writes, got {got}", sess)


def c01_first_draw(ctx):
    for cols, rows in SIZES:
        sess, _ = start(ctx, cols=cols, rows=rows)
        sess.settle(0.3)
        no_overflow(sess, f"at {cols}x{rows}")
        sess.sleep(0.5)
        check(sess.alive(), f"program still running 0.5 s after the first draw at {cols}x{rows}", sess)
        ctx.cleanup()


def c02_confirm_cancel(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    want(sess, INIT, "Tue")
    sess.key("t")
    took = held(INIT, Tue="ana")
    want(sess, took, "Tue", extra=" after t on Tue")
    expect(sess, status_is("Took Tue."), "status 'Took Tue.'")
    sess.key("g")
    expect(sess, lambda s: "Give up Tue?" in s.text() and "Enter confirm  Esc cancel" in s.text(),
           "the confirmation 'Give up Tue?' with 'Enter confirm  Esc cancel' after g")
    sess.key("q")
    sess.key("j")
    sess.sleep(0.3)
    check(sess.alive() and "Give up Tue?" in sess.text(),
          "the confirmation still open after q and j (only Enter and Esc act)", sess)
    sess.key("esc")
    expect(sess, lacks("Give up Tue?"), "the confirmation closed after Esc")
    want(sess, took, "Tue", extra=" (nothing given up, selection kept) after Esc")
    sess.sleep(0.3)
    check(updates(svc) == 1 and holder(svc, "Tue") == "ana",
          f"one update (the take) and Tue still held by ana after Esc; saw {updates(svc)} updates, "
          f"holder {holder(svc, 'Tue')!r}", sess)
    sess.key("j")
    want(sess, took, "Wed", extra=" after j (keys back on the list)")


def c03_save_once(ctx):
    sess, svc = start(ctx, "slow-write")
    sess.key("j")
    sess.key("t", settle=0.05)
    sess.key("t", settle=0)
    took = held(INIT, Tue="ana")
    want(sess, took, "Tue", extra=" after t pressed twice with slow writes")
    expect(sess, status_is("Took Tue."), "status 'Took Tue.'")
    sess.sleep(0.8)
    check(updates(svc) == 1, f"exactly one update for two t presses, saw {updates(svc)}", sess)
    sess.key("g")
    expect(sess, contains("Give up Tue?"), "the confirmation 'Give up Tue?' after g")
    sess.key("enter", settle=0.05)
    sess.key("enter", settle=0)
    want(sess, INIT, "Tue", extra=" after Enter pressed twice in the confirmation")
    expect(sess, status_is("Gave up Tue."), "status 'Gave up Tue.'")
    sess.sleep(0.8)
    check(updates(svc) == 2 and holder(svc, "Tue") is None,
          f"exactly two updates in all and Tue free in the service; saw {updates(svc)} updates, "
          f"holder {holder(svc, 'Tue')!r}", sess)
    order_kept(svc, sess)


def c04_filter(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    sess.key("t")
    took = held(INIT, Tue="ana")
    want(sess, took, "Tue")
    expect(sess, status_is("Took Tue."), "status 'Took Tue.'")
    sess.key("j")
    sess.key("j")
    want(sess, took, "Thu")
    sess.key("f")
    free = [("Thu", None), ("Sat", None), ("Sun", None)]
    want(sess, free, "Thu", show="free", extra=" after f")
    sess.key("j")
    want(sess, free, "Sat", show="free", extra=" after j")
    sess.key("k")
    sess.key("f")
    want(sess, [("Tue", "ana")], "Tue", show="mine", extra=" after k, f (Thu is not yours: first shown)")
    sess.key("f")
    want(sess, took, "Tue", show="all", extra=" after f back to all (selection kept)")
    sess.key("j")
    want(sess, took, "Wed", extra=" after j")


def c05_late_reload(ctx):
    sess, svc = start(ctx)
    svc.control(cmd="delay_next", op="list", ms=1500)
    sess.key("r")
    expect(sess, status_is("Loading..."), "status 'Loading...' during a slow reload")
    sess.key("j")
    sess.key("t")
    took = held(INIT, Tue="ana")
    expect(sess, lambda s: rota_is(took, "Tue")(s) and "Took Tue." in s.last_row(),
           describe(took, "Tue") + " and 'Took Tue.' after taking Tue during the slow reload")
    hold(sess, rota_is(took, "Tue"), describe(took, "Tue") + " to stay after the older reload reply "
         "arrives (it predates the take)", 2.2)


def c06_narrow(ctx):
    sess, _ = start(ctx)
    sess.key("j")
    want(sess, INIT, "Tue")
    sess.settle(0.3)
    ow, oh = ctx.other
    at = f"at {ow}x{oh}"
    sess.resize(ow, oh)
    sess.screen.reset_counters()
    expect(sess, lambda s: rota_is(INIT, "Tue", INIT_DAYS, "all")(s) and "Loaded 7 days." in s.last_row()
           and s.screen.rows == oh,
           f"{at}: " + describe(INIT, "Tue", INIT_DAYS, "all") + " and status 'Loaded 7 days.'")
    sess.settle(0.3)
    no_overflow(sess, at)
    sess.key("t")
    took = held(INIT, Tue="ana")
    expect(sess, lambda s: rota_is(took, "Tue")(s) and "Took Tue." in s.last_row(),
           describe(took, "Tue") + f" and 'Took Tue.' after t {at}")
    sess.key("f")
    want(sess, [("Thu", None), ("Sat", None), ("Sun", None)], "Thu", show="free", extra=f" after f {at}")
    sess.key("f")
    sess.key("f")
    want(sess, took, show="all", extra=f" after f f {at}")
    sess.settle(0.3)
    no_overflow(sess, f"{at} after taking a day and filtering")
    bw, bh = ctx.size
    sess.resize(bw, bh)
    sess.screen.reset_counters()
    want(sess, took, show="all", extra=f" back at {bw}x{bh}")
    sess.settle(0.3)
    no_overflow(sess, f"after coming back to {bw}x{bh}")


def c07_paste(ctx):
    sess, svc = start(ctx)
    sess.paste("j\nt\nf\nq\n")
    sess.sleep(0.6)
    check(sess.alive(), "a paste on the list is ignored (the pasted q did not quit)", sess)
    want(sess, INIT, "Mon", show="all", extra=" unchanged after pasting 'j\\nt\\nf\\nq\\n' on the list")
    check(updates(svc) == 0, "no write caused by the paste", sess)
    sess.key("j")
    sess.key("t")
    took = held(INIT, Tue="ana")
    want(sess, took, "Tue")
    expect(sess, status_is("Took Tue."), "status 'Took Tue.'")
    sess.key("g")
    expect(sess, contains("Give up Tue?"), "the confirmation 'Give up Tue?' after g")
    sess.paste("x\r\nq")
    sess.sleep(0.4)
    check(sess.alive() and "Give up Tue?" in sess.text(), "the confirmation still open after a paste", sess)
    check(updates(svc) == 1, "no write caused by the pasted line break", sess)
    sess.key("esc")
    expect(sess, lacks("Give up Tue?"), "the confirmation closed after Esc")


def c08_load_error(ctx):
    sess, svc = ctx.launch("load-error")
    expect(sess, status_is("Load failed. Press r to retry."),
           "status 'Load failed. Press r to retry.' when the first load fails", START_TIMEOUT)
    sess.key("r")
    want(sess, INIT, "Mon", extra=" after r")
    expect(sess, status_is("Loaded 7 days."), "status 'Loaded 7 days.' after the retry")
    sess.key("j")
    want(sess, INIT, "Tue")
    svc.control(cmd="fail_next", op="list")
    sess.key("r")
    expect(sess, status_is("Load failed. Press r to retry."), "'Load failed. Press r to retry.' after a failed reload")
    want(sess, INIT, "Tue", INIT_DAYS, extra=" kept after the failed reload")
    sess.key("r")
    expect(sess, lambda s: "Loaded 7 days." in s.last_row() and rota_is(INIT, "Tue")(s),
           describe(INIT, "Tue") + " and 'Loaded 7 days.' after the second retry")


def c09_quit_pending(ctx):
    sess, svc = start(ctx)
    svc.control(cmd="delay_next", op="update", ms=10000)
    sess.key("j")
    sess.key("t")
    expect(sess, status_is("Saving..."), "status 'Saving...' while taking Tue (reply delayed 10 s)")
    sess.key("q", settle=0)
    check_quit(sess, "q", (0,))


def c10_repeatable(ctx):
    final = [("Sat", None), ("Sun", None)]
    screens = []
    for i in range(3):
        sess, _ = start(ctx)
        sess.key("j")
        sess.key("t")
        expect(sess, status_is("Took Tue."), f"status 'Took Tue.' (run {i + 1})")
        sess.key("down")
        sess.key("down")
        sess.key("t")
        expect(sess, status_is("Took Thu."), f"status 'Took Thu.' (run {i + 1})")
        sess.key("f")
        want(sess, final, "Sat", show="free", extra=f" after f (run {i + 1})")
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
    tue, wed = svc.find("day", day="Tue"), svc.find("day", day="Wed")
    svc.control(cmd="remote", user="bo", op="update", record_id=tue["id"], content=dict(tue["content"], holder="bo"))
    svc.control(cmd="remote", user="cy", op="update", record_id=wed["id"], content=dict(wed["content"], holder=None))
    rows = held(INIT, Tue="bo", Wed=None)
    want(sess, rows, "Mon", "bo 3", timeout=2.0, extra=" within 2 s of bo's and cy's changes, with no key pressed")
    now = svc.control(cmd="stats")["counts"].get("list", 0)
    check(now == lists, f"no reload to show remote changes ({now - lists} extra list requests)", sess)


def c12_conflict(ctx):
    sess, svc = start(ctx)
    tue = svc.find("day", day="Tue")
    svc.control(cmd="arm_conflict", record_id=tue["id"], user="bo", content=dict(tue["content"], holder="bo"))
    sess.key("j")
    sess.key("t")
    rows = held(INIT, Tue="bo")
    expect(sess, lambda s: rota_is(rows, "Tue")(s) and "Tue was taken by bo." in s.last_row(),
           describe(rows, "Tue") + " and status 'Tue was taken by bo.' after taking Tue while bo took it")
    sess.sleep(0.5)
    check(holder(svc, "Tue") == "bo" and updates(svc) == 1,
          f"Tue held by bo in the service and no second update (no overwrite); saw holder "
          f"{holder(svc, 'Tue')!r} and {updates(svc)} updates", sess)


def c13_held_day(ctx):
    sess, svc = start(ctx)
    sess.key("t")
    expect(sess, status_is("Held by bo."), "status 'Held by bo.' after t on Mon (held by bo)")
    sess.key("g")
    expect(sess, status_is("Not your day."), "status 'Not your day.' after g on Mon")
    check("Give up" not in sess.text(), "no confirmation for a day you do not hold", sess)
    sess.sleep(0.4)
    want(sess, INIT, "Mon", INIT_DAYS, extra=" unchanged")
    check(updates(svc) == 0 and holder(svc, "Mon") == "bo",
          f"no update and Mon still held by bo; saw {updates(svc)} updates, holder {holder(svc, 'Mon')!r}", sess)


def c14_counts_mine(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    sess.key("t")
    one = held(INIT, Tue="ana")
    want(sess, one, "Tue", "ana 1, bo 2, cy 1", extra=" after taking Tue")
    sess.key("j")
    sess.key("j")
    sess.key("t")
    two = held(INIT, Tue="ana", Thu="ana")
    want(sess, two, "Thu", "ana 2, bo 2, cy 1", extra=" after taking Thu")
    sess.key("f")
    sess.key("f")
    want(sess, [("Tue", "ana"), ("Thu", "ana")], "Tue", "ana 2, bo 2, cy 1", "mine",
         extra=" after f f (Sat was selected under 'free', so the first shown day)")
    sess.key("j")
    want(sess, [("Tue", "ana"), ("Thu", "ana")], "Thu", show="mine", extra=" after j")
    sess.key("g")
    expect(sess, contains("Give up Thu?"), "the confirmation 'Give up Thu?'")
    sess.key("enter")
    want(sess, [("Tue", "ana")], "Tue", "ana 1, bo 2, cy 1", "mine",
         extra=" after giving up Thu (it leaves 'Show: mine')")
    expect(sess, status_is("Gave up Thu."), "status 'Gave up Thu.'")


CASES = [
    ("01", "first-draw", c01_first_draw),
    ("02", "confirm-cancel", c02_confirm_cancel),
    ("03", "save-once", c03_save_once),
    ("04", "filter", c04_filter),
    ("05", "late-reload", c05_late_reload),
    ("06", "narrow", c06_narrow),
    ("07", "paste", c07_paste),
    ("08", "load-error", c08_load_error),
    ("09", "quit-pending", c09_quit_pending),
    ("10", "repeatable", c10_repeatable),
    ("11", "remote-change", c11_remote_change),
    ("12", "conflict", c12_conflict),
    ("13", "held-day", c13_held_day),
    ("14", "counts-mine", c14_counts_mine),
]
