"""Acceptance cases for the Board app (spec/board.md)."""
import time

from harness import (CaseFail, board_is, check, contains, describe_board, expect, hold,
                     lacks, no_overflow, parse_board, status_is)

TITLES = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Alpha v2",
          "Alpha (bo)", "Zulu", "Write tests", "Ship it", "Café 日本", "line one line two q"]
INIT = {"todo": ["Alpha", "Bravo", "Charlie"], "doing": ["Delta", "Echo"], "done": ["Foxtrot"]}
START_TIMEOUT = 10.0
SIZES = ((40, 16), (80, 24), (120, 40))


def want(cols, selected, sess=None, timeout=3.0, extra=""):
    expect(sess, board_is(TITLES, cols, selected), describe_board(cols, selected) + extra, timeout)


def start(ctx, scenario="default", cols=None, rows=None):
    sess, svc = ctx.launch(scenario, cols, rows)
    expect(sess, lambda s: board_is(TITLES, INIT, "Alpha")(s) and "Loaded 6 cards." in s.last_row(),
           describe_board(INIT, "Alpha") + " and status 'Loaded 6 cards.' after start", START_TIMEOUT)
    return sess, svc


def creates(svc):
    return svc.control(cmd="stats")["counts"].get("create", 0)


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
    want(INIT, "Bravo", sess)
    sess.key("n")
    expect(sess, lambda s: "New card" in s.text() and "Title:" in s.text(),
           "the form with 'New card' and 'Title:' after n")
    sess.type("Café 日本")
    expect(sess, contains("Title: Café 日本"), "the field showing 'Title: Café 日本' as typed")
    sess.key("esc")
    expect(sess, lacks("New card"), "the form closed after Esc")
    want(INIT, "Bravo", sess, extra=" (nothing saved, selection kept) after Esc")
    check(creates(svc) == 0 and svc.find("card", title="Café 日本") is None,
          "no create request after Esc", sess)
    sess.key("j")
    want(INIT, "Charlie", sess, extra=" after j (keys back on the board)")


def c03_save_once(ctx):
    sess, svc = start(ctx, "slow-write")
    sess.key("n")
    expect(sess, contains("New card"), "the form 'New card' after n")
    sess.key("enter")
    expect(sess, contains("Title is required."), "'Title is required.' after Enter on an empty title")
    check(creates(svc) == 0, "no create request for an empty title", sess)
    sess.type("Write tests")
    sess.key("enter", settle=0.05)
    sess.key("enter", settle=0)
    cols = dict(INIT, todo=INIT["todo"] + ["Write tests"])
    want(cols, "Write tests", sess, extra=" and status 'Saved.' after Enter pressed twice")
    expect(sess, status_is("Saved."), "status 'Saved.' after the card was created")
    sess.sleep(0.8)
    want(cols, "Write tests", sess, extra=" 0.8 s later (no duplicate)")
    made = [r["content"] for r in svc.records("card") if r["content"].get("title") == "Write tests"]
    check(len(made) == 1, f"exactly one 'Write tests' record in the service, found {len(made)}", sess)
    order = made[0].get("order")
    check(isinstance(order, (int, float)) and not isinstance(order, bool) and order > 6
          and {k: v for k, v in made[0].items() if k != "order"} == {"title": "Write tests", "column": "todo"},
          "the new card's content {'title': 'Write tests', 'column': 'todo'} and an 'order' after the "
          f"six cards' (a number above 6), got {made[0]}", sess)


def c04_search_clear(ctx):
    sess, _ = start(ctx)
    sess.key("j")
    want(INIT, "Bravo", sess)
    sess.key("/")
    expect(sess, contains("Search:"), "the input 'Search:' after /")
    sess.type("echo")
    only_echo = {"todo": [], "doing": ["Echo"], "done": []}
    want(only_echo, "Echo", sess, extra=" while the query is 'echo'")
    sess.key("enter")
    expect(sess, contains("Search: echo"), "'Search: echo' still visible after Enter")
    want(only_echo, "Echo", sess)
    sess.key("esc")
    expect(sess, lambda s: "Search: echo" not in s.text() and board_is(TITLES, INIT, "Echo")(s),
           describe_board(INIT, "Echo") + " after Esc cleared the search")
    sess.key("k")
    want(INIT, "Delta", sess, extra=" after k")


def c05_late_search(ctx):
    sess, svc = start(ctx, "late-search")
    sess.key("/")
    expect(sess, contains("Search:"), "the input 'Search:' after /")
    sess.type("e")
    sess.sleep(0.2)
    sess.type("c")
    only_echo = {"todo": [], "doing": ["Echo"], "done": []}
    expect(sess, lambda s: "Search: ec" in s.text() and board_is(TITLES, only_echo)(s),
           "'Search: ec' and " + describe_board(only_echo, None) + " (reply to 'ec' arrived)")
    hold(sess, lambda s: board_is(TITLES, only_echo)(s),
         describe_board(only_echo, None) + " to stay after the late reply to 'e' arrives "
         "(it would add Charlie and Delta)", 2.2)
    sent = svc.control(cmd="stats")["list_matches"]
    check("e" in sent and "ec" in sent,
          f"list requests with match 'e' and then 'ec' (one per change of the query); service saw {sent}", sess)


def c06_small_screen(ctx):
    sess, _ = start(ctx)
    sess.key("j")
    want(INIT, "Bravo", sess)
    sess.settle(0.3)
    sess.resize(40, 12)
    sess.screen.reset_counters()
    expect(sess, contains("Terminal too small. Need 40x16."), "'Terminal too small. Need 40x16.' at 40x12")
    sess.settle(0.3)
    no_overflow(sess, "at 40x12")
    check(not any(t in sess.text() for t in INIT["todo"]), "no cards drawn at 40x12", sess)
    cols, rows = ctx.size
    sess.resize(cols, rows)
    sess.screen.reset_counters()
    want(INIT, "Bravo", sess, extra=f" back at {cols}x{rows} (same selection)")
    sess.settle(0.3)
    no_overflow(sess, f"after growing back to {cols}x{rows}")
    sess.resize(40, 12)
    expect(sess, contains("Terminal too small"), "'Terminal too small' again at 40x12")
    sess.key("q", settle=0)
    check(sess.wait_exit(2.0), "q quits from the small-screen state within 2 s", sess)


def c07_paste(ctx):
    sess, svc = start(ctx)
    sess.paste("n\nq\n")
    sess.sleep(0.6)
    check(sess.alive(), "a paste on the board is ignored (the pasted q did not quit)", sess)
    want(INIT, "Alpha", sess, extra=" and no form after pasting 'n\\nq\\n' on the board")
    check("New card" not in sess.text(), "no form opened by the pasted n", sess)
    sess.key("n")
    expect(sess, contains("New card"), "the form 'New card' after n")
    sess.paste("line one\nline two\r\nq")
    expect(sess, contains("Title: line one line two q"),
           "one field 'Title: line one line two q' after pasting three lines")
    sess.sleep(0.3)
    check(sess.alive() and "New card" in sess.text(), "the form still open after the paste", sess)
    check(creates(svc) == 0, "no create caused by the pasted line breaks", sess)
    sess.key("esc")
    expect(sess, lacks("New card"), "the form closed after Esc")


def c08_load_error(ctx):
    sess, svc = ctx.launch("load-error")
    expect(sess, status_is("Load failed. Press r to retry."),
           "status 'Load failed. Press r to retry.' when the first load fails", START_TIMEOUT)
    sess.key("r")
    want(INIT, "Alpha", sess, extra=" after r")
    expect(sess, status_is("Loaded 6 cards."), "status 'Loaded 6 cards.' after the retry")
    sess.key("j")
    want(INIT, "Bravo", sess)
    svc.control(cmd="fail_next", op="list")
    sess.key("r")
    expect(sess, status_is("Load failed. Press r to retry."), "'Load failed. Press r to retry.' after a failed reload")
    want(INIT, "Bravo", sess, extra=" kept after the failed reload")
    sess.key("r")
    expect(sess, lambda s: "Loaded 6 cards." in s.last_row() and board_is(TITLES, INIT, "Bravo")(s),
           describe_board(INIT, "Bravo") + " and 'Loaded 6 cards.' after the second retry")


def check_quit(sess, how, codes):
    if not sess.wait_exit(2.0):
        raise CaseFail(f"the program exits within 2 s after {how} while a load is pending", sess.screen.dump())
    rc = sess.proc.returncode
    check(rc in codes, f"exit status {' or '.join(map(str, codes))} after {how}, got {rc}", sess)
    if not sess.wait_session_gone(2.0):
        from term import session_pids
        raise CaseFail(f"no process left in the program's session 2 s after exit; still running: "
                       f"{session_pids(sess.sid)}", sess.screen.dump())
    s = sess.screen
    problems = []
    if not sess.termios_restored():
        problems.append("terminal still in raw/no-echo mode")
    if s.alt_active:
        problems.append("alternate screen not left")
    if not s.cursor_visible:
        problems.append("cursor still hidden")
    if s.bracketed_paste:
        problems.append("bracketed paste still on")
    if s.mouse:
        problems.append(f"mouse reporting still on {sorted(s.mouse)}")
    if s.focus_events:
        problems.append("focus reporting still on")
    check(not problems, "terminal restored after exit; problems: " + "; ".join(problems), sess)


def c09_quit_pending(ctx):
    sess, svc = start(ctx)
    svc.control(cmd="delay_next", op="list", ms=10000)
    sess.key("r")
    expect(sess, status_is("Loading..."), "status 'Loading...' during a reload")
    sess.key("q", settle=0)
    check_quit(sess, "q", (0,))


def c10_repeatable(ctx):
    final = {"todo": ["Alpha", "Bravo", "Charlie", "Ship it"], "doing": ["Delta"], "done": ["Echo", "Foxtrot"]}
    screens = []
    for i in range(3):
        sess, _ = start(ctx)
        sess.key("right")
        want(INIT, "Delta", sess, extra=f" after Right (run {i + 1})")
        sess.key("down")
        want(INIT, "Echo", sess, extra=f" after Down (run {i + 1})")
        sess.key("]")
        moved = {"todo": INIT["todo"], "doing": ["Delta"], "done": ["Echo", "Foxtrot"]}
        want(moved, "Echo", sess, extra=f" after ] (run {i + 1})")
        expect(sess, status_is("Saved."), f"status 'Saved.' after the move (run {i + 1})")
        sess.key("n")
        expect(sess, contains("New card"), f"the form after n (run {i + 1})")
        sess.type("Ship it")
        sess.key("enter")
        want(final, "Ship it", sess, extra=f" after saving 'Ship it' (run {i + 1})")
        expect(sess, status_is("Saved."), f"status 'Saved.' (run {i + 1})")
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
    alpha = svc.find("card", title="Alpha")
    charlie = svc.find("card", title="Charlie")
    svc.control(cmd="remote", user="bo", op="update", record_id=alpha["id"],
                content=dict(alpha["content"], title="Alpha v2"))
    svc.control(cmd="remote", user="bo", op="create", type="card",
                content={"title": "Zulu", "column": "doing", "order": 1001})
    svc.control(cmd="remote", user="bo", op="delete", record_id=charlie["id"])
    cols = {"todo": ["Alpha v2", "Bravo"], "doing": ["Delta", "Echo", "Zulu"], "done": ["Foxtrot"]}
    want(cols, "Alpha v2", sess, timeout=2.0, extra=" within 2 s of bo's changes, with no key pressed")
    now = svc.control(cmd="stats")["counts"].get("list", 0)
    check(now == lists, f"no reload to show remote changes ({now - lists} extra list requests)", sess)


def c12_conflict(ctx):
    sess, svc = start(ctx)
    alpha = svc.find("card", title="Alpha")
    svc.control(cmd="arm_conflict", record_id=alpha["id"], user="bo",
                content=dict(alpha["content"], title="Alpha (bo)"))
    sess.key("]")
    cols = {"todo": ["Bravo", "Charlie"], "doing": ["Alpha (bo)", "Delta", "Echo"], "done": ["Foxtrot"]}
    expect(sess, lambda s: board_is(TITLES, cols, "Alpha (bo)")(s)
           and "Conflict: merged with a newer change." in s.last_row(),
           describe_board(cols, "Alpha (bo)") + " and status 'Conflict: merged with a newer change.' "
           "after moving Alpha while bo renamed it")
    rec = svc.control(cmd="dump")["records"]
    rec = next(r for r in rec if r["id"] == alpha["id"])
    merged = dict(alpha["content"], title="Alpha (bo)", column="doing")
    check(rec["content"] == merged, f"service record {merged} (bo's title, the new column, the same order), "
          f"got {rec['content']}", sess)


CASES = [
    ("01", "first-draw", c01_first_draw),
    ("02", "form-cancel", c02_form_cancel),
    ("03", "save-once", c03_save_once),
    ("04", "search-clear", c04_search_clear),
    ("05", "late-search", c05_late_search),
    ("06", "small-screen", c06_small_screen),
    ("07", "paste", c07_paste),
    ("08", "load-error", c08_load_error),
    ("09", "quit-pending", c09_quit_pending),
    ("10", "repeatable", c10_repeatable),
    ("11", "remote-change", c11_remote_change),
    ("12", "conflict", c12_conflict),
]
