"""Acceptance cases for the Poll app (spec/poll.md)."""
from cases_board import check_quit
from harness import (CaseFail, check, contains, describe_poll, expect, hold, lacks,
                     no_overflow, pid, poll_is, status_is)

LABELS = ["Tacos", "Sushi", "Pizza", "Salad", "Ramen", "Curry", "Café 日本", "one two q"]
INIT = [("Tacos", 0), ("Sushi", 2), ("Pizza", 1), ("Salad", 0)]
QUESTION = "Lunch on Friday?"
START_TIMEOUT = 10.0
SIZES = ((40, 16), (80, 24), (120, 40))
# A load is one list for each of the two types: the contract's list names one.
LISTS_PER_LOAD = 2


def want(rows, sess, selected=None, mine=None, total=None, timeout=3.0, extra=""):
    expect(sess, poll_is(LABELS, rows, selected, mine, total),
           describe_poll(rows, selected, mine, total) + extra, timeout)


def start(ctx, scenario="default", cols=None, rows=None):
    sess, svc = ctx.launch(scenario, cols, rows)
    expect(sess, lambda s: poll_is(LABELS, INIT, "Tacos", "", 3)(s) and QUESTION in s.text()
           and "Loaded 4 options." in s.last_row(),
           f"'{QUESTION}', " + describe_poll(INIT, "Tacos", None, 3) + " and status 'Loaded 4 options.' after start",
           START_TIMEOUT)
    return sess, svc


def counts(svc):
    return svc.control(cmd="stats")["counts"]


def poll_record(svc):
    return svc.records("poll")[0]


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
    want(INIT, sess, "Sushi")
    sess.key("a")
    expect(sess, lambda s: "New option" in s.text() and "Option:" in s.text(),
           "the form with 'New option' and 'Option:' after a")
    sess.type("Café 日本")
    expect(sess, contains("Option: Café 日本"), "the field showing 'Option: Café 日本' as typed")
    sess.key("esc")
    expect(sess, lacks("New option"), "the form closed after Esc")
    want(INIT, sess, "Sushi", "", 3, extra=" (nothing saved, selection kept) after Esc")
    c = counts(svc)
    check(c.get("update", 0) == 0 and c.get("create", 0) == 0
          and poll_record(svc)["content"]["options"] == ["Tacos", "Sushi", "Pizza", "Salad"],
          "no write after Esc", sess)
    sess.key("j")
    want(INIT, sess, "Pizza", extra=" after j (keys back on the list)")


def c03_save_once(ctx):
    sess, svc = start(ctx, "slow-write")
    sess.key("a")
    expect(sess, contains("New option"), "the form 'New option' after a")
    sess.key("enter")
    expect(sess, contains("Option is required."), "'Option is required.' after Enter on an empty field")
    sess.type("sushi")
    sess.key("enter")
    expect(sess, contains("Option already exists."), "'Option already exists.' for 'sushi'")
    check(counts(svc).get("update", 0) == 0, "no write for an empty or duplicate option", sess)
    for _ in range(5):
        sess.key("backspace", settle=0.03)
    sess.type("Ramen")
    sess.key("enter", settle=0.05)
    sess.key("enter", settle=0)
    rows = INIT + [("Ramen", 0)]
    want(rows, sess, "Tacos", "", 3, extra=" after Enter pressed twice")
    expect(sess, status_is("Option added."), "status 'Option added.'")
    sess.sleep(0.8)
    want(rows, sess, "Tacos", "", 3, extra=" 0.8 s later (no duplicate)")
    opts = poll_record(svc)["content"]["options"]
    check(opts == ["Tacos", "Sushi", "Pizza", "Salad", "Ramen"],
          f"service options end with one 'Ramen', got {opts}", sess)
    sess.key("enter", settle=0.05)
    sess.key("enter", settle=0)
    voted = [("Tacos", 1)] + INIT[1:] + [("Ramen", 0)]
    want(voted, sess, "Tacos", "Tacos", 4, extra=" after voting with Enter pressed twice")
    expect(sess, status_is("Voted for Tacos."), "status 'Voted for Tacos.'")
    sess.sleep(0.8)
    want(voted, sess, "Tacos", "Tacos", 4, extra=" 0.8 s later")
    mine = [r for r in svc.records("vote") if r["content"].get("voter") == pid("ana")]
    check(len(mine) == 1, f"exactly one vote record for ana in the service, found {len(mine)}", sess)


def c04_search_clear(ctx):
    sess, _ = start(ctx)
    sess.key("j")
    want(INIT, sess, "Sushi")
    sess.key("/")
    expect(sess, contains("Search:"), "the input 'Search:' after /")
    sess.type("sa")
    want([("Salad", 0)], sess, "Salad", total=3, extra=" while the query is 'sa'")
    sess.key("enter")
    expect(sess, contains("Search: sa"), "'Search: sa' still visible after Enter")
    sess.key("esc")
    expect(sess, lambda s: "Search: sa" not in s.text() and poll_is(LABELS, INIT, "Salad")(s),
           describe_poll(INIT, "Salad") + " after Esc cleared the search")
    sess.key("k")
    want(INIT, sess, "Pizza", extra=" after k")


def c05_late_reload(ctx):
    sess, svc = start(ctx)
    lists = counts(svc).get("list", 0)
    # Both lists of the reload are slow: the one of the votes is the one
    # that predates the vote.
    for _ in range(LISTS_PER_LOAD):
        svc.control(cmd="delay_next", op="list", ms=1500)
    sess.key("r")
    expect(sess, status_is("Loading..."), "status 'Loading...' during a slow reload")
    now = counts(svc).get("list", 0)
    check(now == lists + LISTS_PER_LOAD,
          f"one more list of each type after r ({LISTS_PER_LOAD} requests), saw {now - lists}", sess)
    sess.key("enter")
    voted = [("Tacos", 1)] + INIT[1:]
    want(voted, sess, "Tacos", "Tacos", 4, extra=" after voting during the slow reload")
    hold(sess, poll_is(LABELS, voted, "Tacos", "Tacos", 4),
         describe_poll(voted, "Tacos", "Tacos", 4) + " to stay after the older reload reply arrives "
         "(it predates the vote)", 2.2)


def c06_small_screen(ctx):
    sess, _ = start(ctx)
    sess.settle(0.3)
    sess.resize(40, 12)
    sess.screen.reset_counters()
    expect(sess, lambda s: poll_is(LABELS, INIT, "Tacos", "", 3)(s) and QUESTION in s.text()
           and "Loaded 4 options." in s.last_row() and s.screen.rows == 12,
           f"at 40x12: '{QUESTION}', " + describe_poll(INIT, "Tacos", None, 3) + " and the status line")
    sess.settle(0.3)
    no_overflow(sess, "at 40x12")
    sess.key("j")
    sess.key("space")
    voted = [("Tacos", 0), ("Sushi", 3), ("Pizza", 1), ("Salad", 0)]
    want(voted, sess, "Sushi", "Sushi", 4, extra=" after j, Space at 40x12")
    expect(sess, status_is("Voted for Sushi."), "status 'Voted for Sushi.' at 40x12")
    sess.settle(0.3)
    no_overflow(sess, "at 40x12 after voting")


def c07_paste(ctx):
    sess, svc = start(ctx)
    sess.paste("j\n \nq\n")
    sess.sleep(0.6)
    check(sess.alive(), "a paste on the list is ignored (the pasted q did not quit)", sess)
    want(INIT, sess, "Tacos", "", 3, extra=" unchanged after pasting 'j\\n \\nq\\n' on the list")
    c = counts(svc)
    check(c.get("create", 0) == 0 and c.get("update", 0) == 0, "no vote caused by the paste", sess)
    sess.key("a")
    expect(sess, contains("New option"), "the form 'New option' after a")
    sess.paste("one\ntwo\r\nq")
    expect(sess, contains("Option: one two q"), "one field 'Option: one two q' after pasting three lines")
    sess.sleep(0.3)
    check(sess.alive() and "New option" in sess.text(), "the form still open after the paste", sess)
    check(counts(svc).get("update", 0) == 0, "no write caused by the pasted line breaks", sess)
    sess.key("esc")
    expect(sess, lacks("New option"), "the form closed after Esc")


def c08_load_error(ctx):
    sess, svc = ctx.launch("load-error")
    expect(sess, status_is("Load failed. Press r to retry."),
           "status 'Load failed. Press r to retry.' when the first load fails", START_TIMEOUT)
    sess.key("r")
    want(INIT, sess, "Tacos", "", 3, extra=" after r")
    expect(sess, status_is("Loaded 4 options."), "status 'Loaded 4 options.' after the retry")
    sess.key("j")
    want(INIT, sess, "Sushi")
    svc.control(cmd="fail_next", op="list")
    sess.key("r")
    expect(sess, status_is("Load failed. Press r to retry."), "'Load failed. Press r to retry.' after a failed reload")
    want(INIT, sess, "Sushi", "", 3, extra=" kept after the failed reload")
    sess.key("r")
    expect(sess, lambda s: "Loaded 4 options." in s.last_row() and poll_is(LABELS, INIT, "Sushi")(s),
           describe_poll(INIT, "Sushi") + " and 'Loaded 4 options.' after the second retry")


def c09_quit_pending(ctx):
    sess, _ = ctx.launch("slow-load")
    expect(sess, status_is("Loading..."), "status 'Loading...' during the slow first load", START_TIMEOUT)
    check(not any(l in sess.text() for l in ("Tacos", "Sushi")), "no option rows before the load ends", sess)
    sess.key("ctrl-c", settle=0)
    check_quit(sess, "Ctrl+C", (0, 130))


def c10_repeatable(ctx):
    final = [("Tacos", 0), ("Sushi", 2), ("Pizza", 2), ("Salad", 0), ("Ramen", 0)]
    screens = []
    for i in range(3):
        sess, _ = start(ctx)
        sess.key("down")
        sess.key("down")
        want(INIT, sess, "Pizza", extra=f" after Down Down (run {i + 1})")
        sess.key("enter")
        expect(sess, status_is("Voted for Pizza."), f"status 'Voted for Pizza.' (run {i + 1})")
        sess.key("a")
        expect(sess, contains("New option"), f"the form after a (run {i + 1})")
        sess.type("Ramen")
        sess.key("enter")
        want(final, sess, "Pizza", "Pizza", 4, extra=f" after adding Ramen (run {i + 1})")
        expect(sess, status_is("Option added."), f"status 'Option added.' (run {i + 1})")
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
    bo = svc.find("vote", voter=pid("bo"))
    poll = poll_record(svc)
    svc.control(cmd="remote", user="bo", op="update", record_id=bo["id"],
                content={"voter": pid("bo"), "option": "Tacos"})
    svc.control(cmd="remote", user="eve", op="create", type="vote",
                content={"voter": pid("eve"), "option": "Salad"})
    svc.control(cmd="remote", user="bo", op="update", record_id=poll["id"],
                content={"question": QUESTION, "options": ["Tacos", "Sushi", "Pizza", "Salad", "Ramen"]})
    rows = [("Tacos", 1), ("Sushi", 1), ("Pizza", 1), ("Salad", 1), ("Ramen", 0)]
    want(rows, sess, "Tacos", "", 4, timeout=2.0, extra=" within 2 s of bo's and eve's changes, no key pressed")
    now = counts(svc).get("list", 0)
    check(now == lists, f"no reload to show remote changes ({now - lists} extra list requests)", sess)


def c12_conflict(ctx):
    sess, svc = start(ctx)
    poll = poll_record(svc)
    svc.control(cmd="arm_conflict", record_id=poll["id"], user="bo",
                content={"question": QUESTION, "options": ["Tacos", "Sushi", "Pizza", "Salad", "Ramen"]})
    sess.key("a")
    expect(sess, contains("New option"), "the form 'New option' after a")
    sess.type("Curry")
    sess.key("enter")
    rows = INIT + [("Ramen", 0), ("Curry", 0)]
    expect(sess, lambda s: poll_is(LABELS, rows, "Tacos", "", 3)(s)
           and "Conflict: merged with a newer change." in s.last_row(),
           describe_poll(rows, "Tacos", None, 3) + " and status 'Conflict: merged with a newer change.' "
           "after adding Curry while bo added Ramen")
    opts = poll_record(svc)["content"]["options"]
    want_opts = ["Tacos", "Sushi", "Pizza", "Salad", "Ramen", "Curry"]
    check(opts == want_opts, f"service options {want_opts}, got {opts}", sess)


CASES = [
    ("01", "first-draw", c01_first_draw),
    ("02", "form-cancel", c02_form_cancel),
    ("03", "save-once", c03_save_once),
    ("04", "search-clear", c04_search_clear),
    ("05", "late-reload", c05_late_reload),
    ("06", "small-screen", c06_small_screen),
    ("07", "paste", c07_paste),
    ("08", "load-error", c08_load_error),
    ("09", "quit-pending", c09_quit_pending),
    ("10", "repeatable", c10_repeatable),
    ("11", "remote-change", c11_remote_change),
    ("12", "conflict", c12_conflict),
]
