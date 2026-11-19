"""Acceptance cases for the Board with priorities (spec/board-priority.md).

All twelve Board cases run first (cases 01-12, against the board-priority seed,
whose six cards are the Board's with priorities), then cases 13-17.
"""
import re

from cases_board import CASES as BOARD_CASES
from cases_board import INIT, TITLES, start
from harness import (_find_titles, board_is, check, contains, describe_board, expect, parse_board,
                     status_is)

PRIO = {"Alpha": 1, "Charlie": 2, "Delta": 1, "Echo": 3}
TAG = re.compile(r" P([123])(?!\w)")


def priorities(sess):
    """{title: priority or None} for every known title shown below the header row."""
    b = parse_board(sess, TITLES)
    if b is None:
        return None
    out = {}
    scr = sess.screen
    for y in range(b["header_y"] + 1, scr.rows - 1):
        text, _ = scr.row_cells(y)
        for i, title in _find_titles(text, TITLES):
            m = TAG.match(text, i + len(title))
            out[title] = int(m.group(1)) if m else None
    return out


def shows(cols, selected=None, prio=None, tags=None):
    """Board with exactly ``cols``, selection, `Priority: <prio>` and the given tags."""
    def pred(sess):
        if not board_is(TITLES, cols, selected)(sess):
            return False
        if prio is not None and f"Priority: {prio}" not in sess.text():
            return False
        if tags is not None:
            got = priorities(sess)
            if got is None or any(got.get(t, "absent") != p for t, p in tags.items()):
                return False
        return True
    return pred


def describe(cols, selected=None, prio=None, tags=None):
    s = describe_board(cols, selected)
    if prio is not None:
        s += f", 'Priority: {prio}'"
    if tags:
        s += ", tags " + ", ".join(f"'{t} P{p}'" if p else f"'{t}' with no tag" for t, p in tags.items())
    return s


def want(sess, cols, selected=None, prio=None, tags=None, timeout=3.0, extra=""):
    expect(sess, shows(cols, selected, prio, tags), describe(cols, selected, prio, tags) + extra, timeout)


def content(svc, title):
    rec = svc.find("card", title=title)
    return rec["content"] if rec else None


def pstart(ctx, scenario="default", cols=None, rows=None):
    sess, svc = start(ctx, scenario, cols, rows)
    want(sess, INIT, "Alpha", "all", dict(PRIO, Bravo=None, Foxtrot=None), extra=" after start")
    return sess, svc


def c13_set_priority(ctx):
    sess, svc = pstart(ctx)
    before = content(svc, "Bravo")
    sess.key("j")
    want(sess, INIT, "Bravo")
    sess.key("2")
    want(sess, INIT, "Bravo", tags={"Bravo": 2}, extra=" after 2")
    expect(sess, status_is("Saved."), "status 'Saved.' after setting the priority")
    got = content(svc, "Bravo")
    check(got == dict(before, priority=2),
          f"service content {dict(before, priority=2)} (the rest of the card kept), got {got}", sess)
    sess.key("0")
    want(sess, INIT, "Bravo", tags={"Bravo": None}, extra=" after 0 (no tag)")
    expect(sess, status_is("Saved."), "status 'Saved.' after removing the priority")
    got = content(svc, "Bravo")
    check(got is not None and got.get("priority") is None
          and {k: v for k, v in got.items() if k != "priority"} == before,
          f"service content for Bravo with no priority and the rest kept ({before}), got {got}", sess)


def c14_filter_cycle(ctx):
    sess, _ = pstart(ctx)
    sess.key("p")
    want(sess, {"todo": ["Alpha"], "doing": ["Delta"], "done": []}, "Alpha", "P1", extra=" after p")
    sess.key("p")
    want(sess, {"todo": ["Charlie"], "doing": [], "done": []}, "Charlie", "P2", extra=" after p")
    sess.key("p")
    want(sess, {"todo": [], "doing": ["Echo"], "done": []}, "Echo", "P3", extra=" after p")
    sess.key("p")
    want(sess, INIT, "Echo", "all", extra=" after p back to all (selection kept)")
    sess.key("k")
    want(sess, INIT, "Delta", extra=" after k")


def c15_filter_and_search(ctx):
    sess, svc = pstart(ctx)
    sess.key("p")
    want(sess, {"todo": ["Alpha"], "doing": ["Delta"], "done": []}, "Alpha", "P1", extra=" after p")
    sess.key("/")
    expect(sess, contains("Search:"), "the input 'Search:' after /")
    sess.type("e")
    only_delta = {"todo": [], "doing": ["Delta"], "done": []}
    want(sess, only_delta, "Delta", extra=" with 'Priority: P1' and the query 'e'")
    sess.key("enter")
    expect(sess, lambda s: "Search: e" in s.text() and "Priority: P1" in s.text(),
           "'Search: e' and 'Priority: P1' both visible after Enter")
    sess.key("p")
    want(sess, {"todo": ["Charlie"], "doing": [], "done": []}, "Charlie", "P2", extra=" after p (search kept)")
    sess.key("p")
    sess.key("esc")
    want(sess, {"todo": [], "doing": ["Echo"], "done": []}, "Echo", "P3",
         extra=" after p and Esc (search cleared, priority filter kept)")
    check("Search: e" not in sess.text(), "the search cleared by Esc", sess)


def c16_remote_filter(ctx):
    sess, svc = pstart(ctx)
    check(svc.wait_subscribed(), "the program subscribes to changes (subscribe request)", sess)
    sess.key("p")
    want(sess, {"todo": ["Alpha"], "doing": ["Delta"], "done": []}, "Alpha", "P1", extra=" after p")
    lists = svc.control(cmd="stats")["counts"].get("list", 0)
    delta = svc.find("card", title="Delta")
    svc.control(cmd="remote", user="bo", op="create", type="card",
                content={"title": "Zulu", "column": "doing", "priority": 1, "order": 1001})
    svc.control(cmd="remote", user="bo", op="create", type="card",
                content={"title": "Ship it", "column": "todo", "priority": 3, "order": 1002})
    svc.control(cmd="remote", user="bo", op="update", record_id=delta["id"],
                content=dict(delta["content"], priority=2))
    cols = {"todo": ["Alpha"], "doing": ["Zulu"], "done": []}
    want(sess, cols, "Alpha", "P1", timeout=2.0, extra=" within 2 s of bo's changes (Ship it is P3, Delta now P2)")
    check("Ship it" not in sess.text(), "the P3 card 'Ship it' not shown under 'Priority: P1'", sess)
    now = svc.control(cmd="stats")["counts"].get("list", 0)
    check(now == lists, f"no reload to show remote changes ({now - lists} extra list requests)", sess)


def c17_move_keeps_priority(ctx):
    sess, svc = pstart(ctx)
    before = content(svc, "Alpha")
    sess.key("]")
    cols = {"todo": ["Bravo", "Charlie"], "doing": ["Alpha", "Delta", "Echo"], "done": ["Foxtrot"]}
    want(sess, cols, "Alpha", tags={"Alpha": 1}, extra=" after ] on Alpha")
    expect(sess, status_is("Saved."), "status 'Saved.' after the move")
    got = content(svc, "Alpha")
    check(got == dict(before, column="doing") and got.get("priority") == 1,
          f"service content {dict(before, column='doing')} (priority 1 kept), got {got}", sess)


CASES = list(BOARD_CASES) + [
    ("13", "set-priority", c13_set_priority),
    ("14", "priority-filter", c14_filter_cycle),
    ("15", "filter-and-search", c15_filter_and_search),
    ("16", "remote-filter", c16_remote_filter),
    ("17", "move-keeps-priority", c17_move_keeps_priority),
]
