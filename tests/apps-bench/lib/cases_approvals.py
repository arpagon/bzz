"""Acceptance cases for the Approvals app (spec/approvals.md)."""
import re

from cases_board import check_quit
from harness import CaseFail, check, contains, expect, hold, lacks, no_overflow, status_is

TITLES = ["New laptop", "Team lunch", "Conference trip", "Standing desk", "Extra monitor",
          "Gym pass", "Desk lamp", "Café 日本", "one two q"]
INIT = [("pending", "New laptop"), ("pending", "Team lunch"), ("approved", "Conference trip"),
        ("rejected", "Standing desk"), ("pending", "Extra monitor")]
START_TIMEOUT = 10.0
SIZES = ((40, 16), (80, 24), (120, 40))
ROWS = [(t, re.compile(r"(>)? *\b(pending|approved|rejected) +" + re.escape(t) + r"(?!\w)"))
        for t in sorted(TITLES, key=len, reverse=True)]


def parse_requests(sess):
    rows = []
    for line in sess.screen.lines()[:-1]:
        for title, rx in ROWS:
            m = rx.search(line)
            if m:
                rows.append((m.group(2), title, m.group(1) == ">"))
                break
    return rows


def requests_is(want, selected=None, filt=None):
    def pred(sess):
        rows = parse_requests(sess)
        if [(st, t) for st, t, _ in rows] != list(want):
            return False
        if selected is not None and [t for _, t, s in rows if s] != [selected]:
            return False
        if filt is not None and f"Filter: {filt}" not in sess.text():
            return False
        return True
    return pred


def describe(want, selected=None, filt=None):
    rows = ", ".join(f"'{'>' if t == selected else ' '} {st}  {t}'" for st, t in want)
    s = f"request rows {rows}" if want else "no request rows"
    if filt is not None:
        s += f", 'Filter: {filt}'"
    return s


def want(sess, rows, selected=None, filt=None, timeout=3.0, extra=""):
    expect(sess, requests_is(rows, selected, filt), describe(rows, selected, filt) + extra, timeout)


def with_state(rows, title, state):
    return [(state if t == title else st, t) for st, t in rows]


def start(ctx, scenario="default", cols=None, rows=None):
    sess, svc = ctx.launch(scenario, cols, rows)
    expect(sess, lambda s: requests_is(INIT, "New laptop", "all")(s) and "Loaded 5 requests." in s.last_row(),
           describe(INIT, "New laptop", "all") + " and status 'Loaded 5 requests.' after start", START_TIMEOUT)
    return sess, svc


def counts(svc):
    return svc.control(cmd="stats")["counts"]


def writes(svc):
    c = counts(svc)
    return c.get("create", 0) + c.get("update", 0)


def content(svc, title):
    rec = svc.find("request", title=title)
    return rec["content"] if rec else None


def detail_shows(*texts):
    return lambda s: all(t in s.text() for t in texts)


def c01_first_draw(ctx):
    for cols, rows in SIZES:
        sess, _ = start(ctx, cols=cols, rows=rows)
        sess.settle(0.3)
        no_overflow(sess, f"at {cols}x{rows}")
        sess.sleep(0.5)
        check(sess.alive(), f"program still running 0.5 s after the first draw at {cols}x{rows}", sess)
        ctx.cleanup()


def c02_form_cancel(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    want(sess, INIT, "Team lunch")
    sess.key("n")
    expect(sess, lambda s: "New request" in s.text() and "Title:" in s.text(),
           "the form with 'New request' and 'Title:' after n")
    sess.type("Café 日本")
    expect(sess, contains("Title: Café 日本"), "the field showing 'Title: Café 日本' as typed")
    sess.key("esc")
    expect(sess, lacks("New request"), "the form closed after Esc")
    want(sess, INIT, "Team lunch", extra=" (nothing saved, selection kept) after Esc")
    sess.sleep(0.3)
    check(writes(svc) == 0, "no write after Esc", sess)
    sess.key("j")
    want(sess, INIT, "Conference trip", extra=" after j (keys back on the list)")


def c03_save_once(ctx):
    sess, svc = start(ctx, "slow-write")
    sess.key("n")
    expect(sess, contains("New request"), "the form 'New request' after n")
    sess.key("enter")
    expect(sess, contains("Title is required."), "'Title is required.' after Enter on an empty title")
    check(writes(svc) == 0, "no create for an empty title", sess)
    sess.type("Gym pass")
    sess.key("enter", settle=0.05)
    sess.key("enter", settle=0)
    rows = INIT + [("pending", "Gym pass")]
    want(sess, rows, "Gym pass", extra=" after Enter pressed twice")
    expect(sess, status_is("Request filed."), "status 'Request filed.'")
    sess.sleep(0.8)
    want(sess, rows, "Gym pass", extra=" 0.8 s later (no duplicate)")
    recs = [r for r in svc.records("request") if r["content"].get("title") == "Gym pass"]
    check(len(recs) == 1, f"exactly one 'Gym pass' record in the service, found {len(recs)}", sess)
    want_c = {"title": "Gym pass", "author": "ana", "state": "pending", "decided_by": None, "reason": None}
    got = dict(recs[0]["content"])
    order = got.pop("order", None)
    check(got == want_c and isinstance(order, (int, float)) and not isinstance(order, bool) and order > 5,
          f"service content {want_c} and an 'order' after the five requests' (a number above 5), "
          f"got {recs[0]['content']}", sess)


def c04_filter(ctx):
    sess, _ = start(ctx)
    for _ in range(4):
        sess.key("j")
    want(sess, INIT, "Extra monitor")
    sess.key("f")
    pending = [r for r in INIT if r[0] == "pending"]
    want(sess, pending, "Extra monitor", "pending", extra=" after f")
    sess.key("f")
    want(sess, [("approved", "Conference trip")], "Conference trip", "approved", extra=" after f")
    sess.key("f")
    want(sess, [("rejected", "Standing desk")], "Standing desk", "rejected", extra=" after f")
    sess.key("f")
    want(sess, INIT, "Standing desk", "all", extra=" after f back to all (selection kept)")
    sess.key("k")
    want(sess, INIT, "Conference trip", extra=" after k")


def c05_late_reload(ctx):
    sess, svc = start(ctx)
    svc.control(cmd="delay_next", op="list", ms=1500)
    sess.key("r")
    expect(sess, status_is("Loading..."), "status 'Loading...' during a slow reload")
    sess.key("a")
    rows = with_state(INIT, "New laptop", "approved")
    expect(sess, lambda s: requests_is(rows, "New laptop")(s) and "Approved." in s.last_row(),
           describe(rows, "New laptop") + " and 'Approved.' after approving during the slow reload")
    hold(sess, requests_is(rows, "New laptop"), describe(rows, "New laptop") + " to stay after the older "
         "reload reply arrives (it predates the approval)", 2.2)


def c06_narrow(ctx):
    sess, svc = start(ctx)
    sess.settle(0.3)
    ow, oh = ctx.other
    at = f"at {ow}x{oh}"
    sess.resize(ow, oh)
    sess.screen.reset_counters()
    expect(sess, lambda s: requests_is(INIT, "New laptop", "all")(s) and "Loaded 5 requests." in s.last_row()
           and s.screen.rows == oh,
           f"{at}: " + describe(INIT, "New laptop", "all") + " and status 'Loaded 5 requests.'")
    sess.settle(0.3)
    no_overflow(sess, at)
    sess.key("enter")
    expect(sess, detail_shows("Request: New laptop", "Author: bo", "State: pending"),
           f"the detail 'Request: New laptop', 'Author: bo', 'State: pending' after Enter {at}")
    sess.key("esc")
    sess.key("x")
    expect(sess, lambda s: "Reject request" in s.text() and "Reason:" in s.text(),
           f"the form 'Reject request' with 'Reason:' after x {at}")
    sess.type("Too old")
    sess.key("enter")
    rows = with_state(INIT, "New laptop", "rejected")
    expect(sess, lambda s: requests_is(rows, "New laptop")(s) and "Rejected." in s.last_row(),
           describe(rows, "New laptop") + f" and 'Rejected.' {at}")
    sess.key("enter")
    expect(sess, detail_shows("Reason: Too old", "Decided by: ana"),
           f"the detail with 'Decided by: ana' and 'Reason: Too old' {at}")
    sess.settle(0.3)
    no_overflow(sess, f"{at} after the detail and the reject form")
    sess.key("esc")
    bw, bh = ctx.size
    sess.resize(bw, bh)
    sess.screen.reset_counters()
    want(sess, rows, "New laptop", extra=f" back at {bw}x{bh}")
    sess.settle(0.3)
    no_overflow(sess, f"after coming back to {bw}x{bh}")


def c07_paste(ctx):
    sess, svc = start(ctx)
    sess.paste("a\nx\nn\nq\n")
    sess.sleep(0.6)
    check(sess.alive(), "a paste on the list is ignored (the pasted q did not quit)", sess)
    want(sess, INIT, "New laptop", extra=" unchanged after pasting 'a\\nx\\nn\\nq\\n' on the list")
    check("New request" not in sess.text() and "Reject request" not in sess.text(), "no form opened by the paste", sess)
    check(writes(svc) == 0, "no write caused by the paste", sess)
    sess.key("n")
    expect(sess, contains("New request"), "the form 'New request' after n")
    sess.paste("one\ntwo\r\nq")
    expect(sess, contains("Title: one two q"), "one field 'Title: one two q' after pasting three lines")
    sess.sleep(0.3)
    check(sess.alive() and "New request" in sess.text(), "the form still open after the paste", sess)
    check(writes(svc) == 0, "no create caused by the pasted line breaks", sess)
    sess.key("esc")
    expect(sess, lacks("New request"), "the form closed after Esc")


def c08_load_error(ctx):
    sess, svc = ctx.launch("load-error")
    expect(sess, status_is("Load failed. Press r to retry."),
           "status 'Load failed. Press r to retry.' when the first load fails", START_TIMEOUT)
    sess.key("r")
    want(sess, INIT, "New laptop", extra=" after r")
    expect(sess, status_is("Loaded 5 requests."), "status 'Loaded 5 requests.' after the retry")
    sess.key("j")
    want(sess, INIT, "Team lunch")
    svc.control(cmd="fail_next", op="list")
    sess.key("r")
    expect(sess, status_is("Load failed. Press r to retry."), "'Load failed. Press r to retry.' after a failed reload")
    want(sess, INIT, "Team lunch", extra=" kept after the failed reload")
    sess.key("r")
    expect(sess, lambda s: "Loaded 5 requests." in s.last_row() and requests_is(INIT, "Team lunch")(s),
           describe(INIT, "Team lunch") + " and 'Loaded 5 requests.' after the second retry")


def c09_quit_pending(ctx):
    sess, _ = ctx.launch("slow-load")
    expect(sess, status_is("Loading..."), "status 'Loading...' during the slow first load", START_TIMEOUT)
    check(not any(t in sess.text() for t in ("New laptop", "Team lunch")), "no request rows before the load ends", sess)
    sess.key("ctrl-c", settle=0)
    check_quit(sess, "Ctrl+C", (0, 130))


def c10_repeatable(ctx):
    screens = []
    for i in range(3):
        sess, _ = start(ctx)
        sess.key("a")
        expect(sess, status_is("Approved."), f"status 'Approved.' (run {i + 1})")
        for _ in range(4):
            sess.key("down")
        sess.key("x")
        expect(sess, contains("Reject request"), f"the reject form after x (run {i + 1})")
        sess.type("Later")
        sess.key("enter")
        expect(sess, status_is("Rejected."), f"status 'Rejected.' (run {i + 1})")
        sess.key("f")
        want(sess, [("pending", "Team lunch")], "Team lunch", "pending", extra=f" after f (run {i + 1})")
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
    lists = counts(svc).get("list", 0)
    laptop = svc.find("request", title="New laptop")
    desk = svc.find("request", title="Standing desk")
    svc.control(cmd="remote", user="bo", op="create", type="request",
                content={"title": "Desk lamp", "author": "bo", "state": "pending", "decided_by": None,
                         "reason": None, "order": 1001})
    svc.control(cmd="remote", user="cy", op="update", record_id=laptop["id"],
                content=dict(laptop["content"], state="approved", decided_by="cy"))
    svc.control(cmd="remote", user="bo", op="delete", record_id=desk["id"])
    rows = [("approved", "New laptop"), ("pending", "Team lunch"), ("approved", "Conference trip"),
            ("pending", "Extra monitor"), ("pending", "Desk lamp")]
    want(sess, rows, "New laptop", timeout=2.0, extra=" within 2 s of bo's and cy's changes, with no key pressed")
    now = counts(svc).get("list", 0)
    check(now == lists, f"no reload to show remote changes ({now - lists} extra list requests)", sess)


def c12_conflict(ctx):
    sess, svc = start(ctx)
    laptop = svc.find("request", title="New laptop")
    theirs = dict(laptop["content"], state="rejected", decided_by="bo", reason="Duplicate")
    svc.control(cmd="arm_conflict", record_id=laptop["id"], user="bo", content=theirs)
    sess.key("a")
    rows = with_state(INIT, "New laptop", "rejected")
    expect(sess, lambda s: requests_is(rows, "New laptop")(s) and "Already rejected by bo." in s.last_row(),
           describe(rows, "New laptop") + " and status 'Already rejected by bo.' after approving while bo "
           "rejected it")
    sess.sleep(0.5)
    got = content(svc, "New laptop")
    check(got == theirs and counts(svc).get("update", 0) == 1,
          f"bo's decision kept in the service ({theirs}) and no second update; got {got}, "
          f"{counts(svc).get('update', 0)} updates", sess)


def c13_own_request(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    want(sess, INIT, "Team lunch")
    sess.key("a")
    expect(sess, status_is("You cannot decide your own request."),
           "status 'You cannot decide your own request.' after a on ana's request")
    sess.key("r")
    expect(sess, status_is("Loaded 5 requests."), "status 'Loaded 5 requests.' after r")
    sess.key("x")
    expect(sess, status_is("You cannot decide your own request."),
           "status 'You cannot decide your own request.' after x on ana's request")
    check("Reject request" not in sess.text(), "no reject form for your own request", sess)
    sess.sleep(0.4)
    want(sess, INIT, "Team lunch", extra=" unchanged")
    check(writes(svc) == 0, f"no write; saw {writes(svc)}", sess)


def c14_reject_reason(ctx):
    sess, svc = start(ctx)
    before = content(svc, "New laptop")
    sess.key("x")
    expect(sess, lambda s: "Reject request" in s.text() and "Reason:" in s.text(),
           "the form 'Reject request' with 'Reason:' after x on New laptop")
    sess.key("enter")
    expect(sess, contains("Reason is required."), "'Reason is required.' after Enter on an empty reason")
    sess.type("  ")
    sess.key("enter")
    expect(sess, contains("Reason is required."), "'Reason is required.' after Enter on a blank reason")
    sess.sleep(0.3)
    check(writes(svc) == 0, "no write for an empty or blank reason", sess)
    sess.type("Over budget")
    sess.key("enter")
    rows = with_state(INIT, "New laptop", "rejected")
    expect(sess, lambda s: requests_is(rows, "New laptop")(s) and "Rejected." in s.last_row(),
           describe(rows, "New laptop") + " and status 'Rejected.'")
    sess.key("enter")
    expect(sess, detail_shows("Request: New laptop", "State: rejected", "Decided by: ana", "Reason: Over budget"),
           "the detail with 'State: rejected', 'Decided by: ana' and 'Reason: Over budget'")
    got = content(svc, "New laptop")
    want_c = dict(before, state="rejected", decided_by="ana", reason="Over budget")
    check(got == want_c, f"service content {want_c} (title, author and order kept), got {got}", sess)


def c15_already_decided(ctx):
    sess, svc = start(ctx)
    sess.key("j")
    sess.key("j")
    want(sess, INIT, "Conference trip")
    sess.key("a")
    expect(sess, status_is("Already approved by bo."), "status 'Already approved by bo.' after a on Conference trip")
    sess.key("j")
    sess.key("x")
    expect(sess, status_is("Already rejected by bo."), "status 'Already rejected by bo.' after x on Standing desk")
    check("Reject request" not in sess.text(), "no reject form for a decided request", sess)
    sess.key("enter")
    expect(sess, detail_shows("Request: Standing desk", "Decided by: bo", "Reason: No budget"),
           "the detail of Standing desk with 'Decided by: bo' and 'Reason: No budget'")
    sess.key("esc")
    sess.sleep(0.4)
    want(sess, INIT, "Standing desk", extra=" unchanged")
    check(writes(svc) == 0, f"no write; saw {writes(svc)}", sess)


CASES = [
    ("01", "first-draw", c01_first_draw),
    ("02", "form-cancel", c02_form_cancel),
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
    ("13", "own-request", c13_own_request),
    ("14", "reject-reason", c14_reject_reason),
    ("15", "already-decided", c15_already_decided),
]
