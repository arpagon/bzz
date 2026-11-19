"""The hostile script on the example host: it reaches nothing, and the host
stays usable.

    tests/apps-bench/hostile

Runs `hostile-app/app.lua` on `examples/app_host.rs` in a pseudo-terminal, as
the prototype's `tests/hostile.py` did, and checks: every way out is blocked
(file, module, bytecode, native code, environment, process, exit, debug,
coroutine, the terminal); an endless loop, one that catches its stop, a
pattern that never ends, the stack and the memory are each ended and the app
answers the next key; records outside the fence are refused and never reach
the service; pushes of undeclared types never arrive; after three stops in a
row the app is given up, the host goes on drawing, a save starts the app
again, and Ctrl+C leaves with the terminal restored and no process left.

Exit status 0 only if every check held.
"""
import os
import shutil
import sys
import time

from drive import BENCH, has, launch, stop
from harness import out_dir

ATTEMPTS = 19
STOPPED = "ran longer than 250 ms and was stopped"


def hwm_mib(pid):
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1]) // 1024
    except OSError:
        pass
    return -1


def main(argv):
    work = os.path.join(out_dir(), "work", "hostile")
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    marker = os.path.join(work, "pwned")
    app = os.path.join(work, "app.lua")
    with open(os.path.join(BENCH, "hostile-app", "app.lua")) as f:
        source = f.read().replace("MARKER", marker)
    with open(app, "w") as f:
        f.write(source)
    host_log = os.path.join(out_dir(), "hostile-host.log")
    if os.path.exists(host_log):
        os.remove(host_log)

    def log():
        try:
            with open(host_log) as f:
                return f.read()
        except OSError:
            return ""

    def logged(text, seconds=5.0):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if text in log():
                return True
            sess.sleep(0.05)
        return text in log()

    checks = []

    def check(name, ok, detail=""):
        checks.append((name, bool(ok)))
        print(f"{'ok  ' if ok else 'FAIL'} {name}" + (f" ({detail})" if detail else ""), flush=True)
        return bool(ok)

    def answers(label):
        """The app handles a key: its note says so, and the count goes up."""
        before = sess.text()
        sess.key("j", settle=0)
        return check(f"the app answers the next key after {label}",
                     sess.wait_for(lambda s: "key j handled" in s.text() and s.text() != before, 3))

    sess, svc = launch(app, cols=120, rows=40, host_log=host_log,
                       log=os.path.join(out_dir(), "hostile-service.jsonl"))
    try:
        check("the app starts and shows its attempts", sess.wait_for(has("pushes received: 0"), 10))
        text = " ".join(sess.text().split())
        check(f"all {ATTEMPTS} ways out are blocked",
              text.count(": blocked (") == ATTEMPTS and "REACHED" not in text,
              f"{text.count(': blocked (')} blocked, {text.count('REACHED')} reached")
        check("the script has no os, io, package, debug, load, require or coroutine",
              "globals: _G _VERSION assert data error getmetatable host ipairs math next pairs pcall print "
              "rawequal rawget rawlen rawset select setmetatable string table tonumber tostring type ui utf8 "
              "xpcall host: after log quit time data: create delete list null update" in text)
        check("os.execute made no file", not os.path.exists(marker))
        print(sess.screen.dump("hostile: what the script reached at start"))

        for key, what in (("x", "an endless loop"), ("y", "a loop that catches its stop")):
            t0 = time.monotonic()
            sess.key(key, settle=0)
            ok = sess.wait_for(has(STOPPED), 5)
            check(f"{what} is stopped", ok, f"{(time.monotonic() - t0) * 1000:.0f} ms")
            answers(what)

        t0 = time.monotonic()
        sess.key("p", settle=0)
        ok = sess.wait_for(has("a pattern ran past the budget of 250 ms"), 5)
        check("a pattern that never ends is stopped (the prototype showed 'not responding' until it quit)",
              ok, f"{(time.monotonic() - t0) * 1000:.0f} ms")
        answers("the pattern")

        t0 = time.monotonic()
        sess.key("s", settle=0)
        ok = sess.wait_for(lambda s: "stack overflow" in s.text() or STOPPED in s.text(), 5)
        check("recursion without end is ended", ok, f"{(time.monotonic() - t0) * 1000:.0f} ms")
        answers("the recursion")

        sess.key("m", settle=0)
        ok = sess.wait_for(lambda s: "ran out of memory" in s.text() or STOPPED in s.text(), 15)
        peak = hwm_mib(sess.proc.pid)
        check("memory without end is ended, and the host's peak is under 200 MiB", ok and 0 < peak < 200,
              f"host peak RSS {peak} MiB; " + ("the cap of 64 MiB" if "ran out of memory" in sess.text()
                                                else "the time budget ended it first"))
        answers("the memory")

        for key, wanted in (("c", 'data.create: record type "card" is not declared'),
                            ("l", 'data.list: record type "card" is not declared'),
                            ("u", "data.update: record r0001 is unknown"),
                            ("d", "data.delete: record r0001 is unknown")):
            sess.key(key, settle=0)
            check(f"key {key}: refused with '{wanted}'", logged(wanted, 3))
        sess.sleep(0.3)
        counts = svc.control(cmd="stats")["counts"]
        check("no request of the script reached the service",
              all(counts.get(op, 0) == 0 for op in ("list", "create", "update", "delete")), str(counts))
        check("the service's records are untouched",
              not any(r["content"].get("title") in ("smuggled", "hijacked") for r in svc.records()))

        svc.control(cmd="remote", user="bo", op="create", type="card",
                    content={"title": "secret", "column": "todo", "order": 1001})
        svc.control(cmd="remote", user="bo", op="create", type="note", content={"text": "hi"})
        sess.wait_for(has("pushes received: 1"), 2)
        sess.sleep(0.3)
        check("of two pushes only the one of the declared type arrives", "pushes received: 1" in sess.text())

        # Three stops of one handler in a row: the app is given up.
        for _ in range(3):
            sess.key("x", settle=0)
            sess.sleep(0.6)
        check("three stops in a row give the app up", logged("The app is stopped until it is reloaded", 5))
        sess.resize(100, 30)
        check("the host goes on drawing (a resize is followed by a redraw)",
              sess.wait_for(lambda s: s.screen.rows == 30 and "pushes received" in s.text(), 3))
        print(sess.screen.dump("hostile: given up, after a resize to 100x30"))
        # Saving the file starts the app again, from init.
        with open(app, "w") as f:
            f.write(source.replace('"pushes received: "', '"pushes seen: "'))
        check("a save starts the given-up app again", sess.wait_for(has("pushes seen: 0"), 5))
        answers("it was started again")

        t0 = time.monotonic()
        sess.key("x", settle=0)   # leave while the script is inside a loop
        sess.sleep(0.05)
        sess.key("ctrl-c", settle=0)
        exited = sess.wait_exit(2)
        check("Ctrl+C leaves while the script is in a loop, with status 0",
              exited and sess.proc.returncode == 0,
              f"{(time.monotonic() - t0) * 1000:.0f} ms, status {sess.proc.returncode}")
        check("the terminal is restored", sess.termios_restored() and not sess.screen.alt_active
              and sess.screen.cursor_visible and not sess.screen.bracketed_paste)
        check("no process is left", sess.wait_session_gone(2))
    finally:
        stop(sess, svc)
    check("os.execute made no file, in the end", not os.path.exists(marker))
    failed = [name for name, ok in checks if not ok]
    print(f"RESULT hostile: {len(checks) - len(failed)}/{len(checks)} passed, {len(failed)} failed", flush=True)
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
