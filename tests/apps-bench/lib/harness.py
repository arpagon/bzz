"""Shared pieces for the acceptance cases: data service, launch, checks."""
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import time

from term import Session

BENCH = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
USER = "ana"
# app name -> module with its CASES
APPS = {"board": "cases_board", "poll": "cases_poll", "rota": "cases_rota",
        "approvals": "cases_approvals", "panel": "cases_panel", "board-priority": "cases_board_priority"}


def out_dir():
    """Where runs leave their files: `APPS_BENCH_OUT`, or a folder under the
    system's temporary directory. Never inside the repository."""
    path = os.environ.get("APPS_BENCH_OUT") or os.path.join(tempfile.gettempdir(), "bzz-apps-bench")
    os.makedirs(path, exist_ok=True)
    return path


def parse_size(text):
    """`40x16` -> (40, 16)."""
    try:
        cols, rows = text.lower().split("x")
        return int(cols), int(rows)
    except ValueError:
        raise ValueError(f"a size is written 40x16, got {text!r}")


PANE = (40, 16)     # the app pane of bzz, docked beside the conversation
WINDOW = (80, 24)   # the bench's own size


class CaseFail(Exception):
    def __init__(self, expected, screen=None, detail=None):
        super().__init__(expected)
        self.expected = expected
        self.screen = screen
        self.detail = detail


class Service:
    def __init__(self, app, scenario, log_path):
        self.proc = subprocess.Popen(
            [sys.executable, os.path.join(BENCH, "data-service"), "--app", app,
             "--scenario", scenario, "--log", log_path],
            stdout=subprocess.PIPE, stdin=subprocess.DEVNULL, start_new_session=True)
        line = self.proc.stdout.readline()
        if not line:
            raise RuntimeError("data-service did not start")
        info = json.loads(line)
        self.data_addr = info["data_addr"]
        host, port = info["control_addr"].rsplit(":", 1)
        self.ctl = socket.create_connection((host, int(port)))
        self.ctl_file = self.ctl.makefile("rwb")

    def control(self, **msg):
        self.ctl_file.write((json.dumps(msg) + "\n").encode())
        self.ctl_file.flush()
        return json.loads(self.ctl_file.readline())

    def records(self, rtype=None):
        recs = self.control(cmd="dump")["records"]
        return [r for r in recs if rtype is None or r["type"] == rtype]

    def find(self, rtype, **fields):
        for r in self.records(rtype):
            if all(r["content"].get(k) == v for k, v in fields.items()):
                return r
        return None

    def wait_subscribed(self, timeout=5.0):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.control(cmd="stats")["subscribers"] >= 1:
                return True
            time.sleep(0.05)
        return False

    def stop(self):
        try:
            self.ctl.close()
        except OSError:
            pass
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        if self.proc.stdout:
            self.proc.stdout.close()


class Context:
    """One case's world: launches services and the program, cleans up after."""

    def __init__(self, app, argv, artifacts, case_id, size=WINDOW):
        self.app = app
        # The size every launch of this case starts at, and another one to
        # resize to and come back from: the pane's, or the window's when
        # the case already runs at the pane's.
        self.size = size
        self.other = PANE if size != PANE else WINDOW
        self.argv = argv
        self.dir = os.path.join(artifacts, case_id)
        os.makedirs(self.dir, exist_ok=True)
        self.items = []
        self.n = 0

    def launch(self, scenario="default", cols=None, rows=None):
        if cols is None or rows is None:
            cols, rows = self.size
        self.n += 1
        svc = Service(self.app, scenario, os.path.join(self.dir, f"service-{self.n}.jsonl"))
        env = dict(os.environ)
        env.update({"MINIAPP_DATA_ADDR": svc.data_addr, "MINIAPP_USER": USER,
                    "TERM": "xterm-256color", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"})
        for k in ("COLUMNS", "LINES", "TMUX", "HERDR_ENV", "STY"):
            env.pop(k, None)
        try:
            sess = Session(self.argv, cols, rows, env)
        except Exception:
            svc.stop()
            raise
        self.items.append((sess, svc))
        return sess, svc

    def cleanup(self):
        for i, (sess, svc) in enumerate(self.items, 1):
            try:
                sess.pump(0)
                with open(os.path.join(self.dir, f"screen-{i}.txt"), "w") as f:
                    f.write(sess.screen.dump("(last screen before cleanup)") + "\n")
            except OSError:
                pass
            try:
                if sess.alive():
                    sess.key("ctrl-c", settle=0)
                    sess.wait_exit(1.0)
            except OSError:
                pass
            try:
                with open(os.path.join(self.dir, f"output-{i}.raw"), "wb") as f:
                    f.write(bytes(sess.raw))
            except OSError:
                pass
            sess.close()
            svc.stop()
        self.items = []


# ------------------------------------------------------------------ checks
def expect(sess, pred, expected, timeout=3.0):
    if not sess.wait_for(pred, timeout):
        if not sess.alive():
            expected += f" (the program exited with status {sess.proc.returncode})"
        raise CaseFail(expected, sess.screen.dump())


def hold(sess, pred, expected, seconds):
    """``pred`` must stay true for ``seconds``."""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        sess.pump(0.02)
        ok = False
        try:
            ok = pred(sess)
        except Exception:
            pass
        if not ok:
            raise CaseFail(expected, sess.screen.dump())


def check(cond, expected, sess=None, detail=None):
    if not cond:
        raise CaseFail(expected, sess.screen.dump() if sess else None, detail)


def no_overflow(sess, where):
    s = sess.screen
    if s.wraps or s.lost_lines:
        raise CaseFail(
            f"no text outside the viewport {where}; saw {s.wraps} autowrap(s) past the last column "
            f"and {s.lost_lines} non-blank line(s) scrolled off the top"
            + (f" (first lost: {s.lost_samples[0]!r})" if s.lost_samples else ""), s.dump())


def status(sess):
    return sess.last_row()


def status_is(text):
    return lambda s: text in s.last_row()


def contains(text):
    return lambda s: text in s.text()


def lacks(text):
    return lambda s: text not in s.text()


# ------------------------------------------------------------------ board parsing
HEADERS = [("todo", "To do"), ("doing", "Doing"), ("done", "Done")]


def _find_titles(text, titles):
    """Yield (index, title) of whole-title matches, longest titles first, no overlaps."""
    taken = [False] * len(text)
    found = []
    for title in sorted(set(titles), key=len, reverse=True):
        start = 0
        while True:
            i = text.find(title, start)
            if i < 0:
                break
            j = i + len(title)
            before_ok = i == 0 or not text[i - 1].isalnum()
            after_ok = j == len(text) or not text[j].isalnum()
            if before_ok and after_ok and not any(taken[i:j]):
                for k in range(i, j):
                    taken[k] = True
                found.append((i, title))
            start = i + 1
    return sorted(found)


def parse_board(sess, titles):
    """Return dict with columns -> [titles], counts, selected (list), header row.

    The three headers stand on one row, with each card under the header that
    starts nearest to its marker; or, where the board is too narrow for that,
    one above the other, each followed by its own cards."""
    scr = sess.screen
    heads = {}
    for y in range(scr.rows):
        text, xs = scr.row_cells(y)
        for k, label in HEADERS:
            m = re.search(re.escape(label) + r" \((\d+)\)", text)
            if m and k not in heads:
                heads[k] = (y, xs[m.start()], int(m.group(1)))
    if len(heads) != len(HEADERS):
        return None
    counts = {k: n for k, (_, _, n) in heads.items()}
    ys = [heads[k][0] for k, _ in HEADERS]
    header_y = min(ys)
    cols = {k: [] for k, _ in HEADERS}
    selected = []

    def cards(y):
        text, xs = scr.row_cells(y)
        for i, title in _find_titles(text, titles):
            if i >= 2 and text[i - 2:i] == "> ":
                selected.append(title)
            yield title, (xs[i] - 2 if xs[i] >= 2 else 0)

    if len(set(ys)) == 1:
        hx = {k: x for k, (_, x, _) in heads.items()}
        for y in range(header_y + 1, scr.rows - 1):
            for title, marker_x in cards(y):
                cols[min(hx, key=lambda k: abs(hx[k] - marker_x))].append(title)
    elif ys == sorted(set(ys)):
        ends = ys[1:] + [scr.rows - 1]
        for (k, _), top, end in zip(HEADERS, ys, ends):
            for y in range(top + 1, end):
                cols[k].extend(title for title, _ in cards(y))
    else:
        return None
    return {"cols": cols, "counts": counts, "selected": selected, "header_y": header_y}


def board_is(titles, cols=None, selected=None, counts=True):
    """Predicate: the board shows exactly ``cols`` (dict col -> list) and selection."""
    def pred(sess):
        b = parse_board(sess, titles)
        if b is None:
            return False
        if cols is not None:
            for k, want in cols.items():
                if b["cols"][k] != want:
                    return False
            if counts and any(b["counts"][k] != len(v) for k, v in cols.items()):
                return False
        if selected is not None and b["selected"] != [selected]:
            return False
        return True
    return pred


def describe_board(cols, selected):
    parts = [f"{label} ({len(cols[k])}): {', '.join(cols[k]) or '-'}" for k, label in HEADERS if k in cols]
    s = "board " + " | ".join(parts)
    if selected:
        s += f"; selected '> {selected}'"
    return s


# ------------------------------------------------------------------ poll parsing
def parse_poll(sess, labels):
    """Return {'rows': [(label, selected, mine, count)], 'total': int|None}."""
    rows = []
    for y, line in enumerate(sess.screen.lines()[:-1]):
        for label in sorted(labels, key=len, reverse=True):
            m = re.search(r"(?:^|(?<=[^\w]))([> ]) \[([x ])\] " + re.escape(label) + r" +(\d+) votes?(?!\w)", line)
            if m:
                n = int(m.group(3))
                ok_word = (n == 1) == (m.group(0).endswith("vote"))
                rows.append((label, m.group(1) == ">", m.group(2) == "x", n if ok_word else -1, y))
                break
    total = None
    for line in sess.screen.lines()[:-1]:
        m = re.search(r"Total: (\d+) votes?(?!\w)", line)
        if m:
            total = int(m.group(1))
    return {"rows": rows, "total": total}


def poll_is(labels, want, selected=None, mine=None, total=None):
    """``want`` is a list of (label, count) in display order."""
    def pred(sess):
        p = parse_poll(sess, labels)
        got = [(r[0], r[3]) for r in p["rows"]]
        if got != list(want):
            return False
        sel = [r[0] for r in p["rows"] if r[1]]
        if selected is not None and sel != [selected]:
            return False
        if mine is not None:
            m = [r[0] for r in p["rows"] if r[2]]
            if m != ([mine] if mine else []):
                return False
        if total is not None and p["total"] != total:
            return False
        return True
    return pred


def describe_poll(want, selected=None, mine=None, total=None):
    rows = []
    for label, n in want:
        sel = ">" if label == selected else " "
        box = "[x]" if label == mine else "[ ]"
        rows.append(f"'{sel} {box} {label}  {n} vote{'s' if n != 1 else ''}'")
    s = "poll rows " + ", ".join(rows)
    if total is not None:
        s += f" and 'Total: {total} vote{'s' if total != 1 else ''}'"
    return s
