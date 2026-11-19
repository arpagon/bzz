"""Drive `examples/app_host.rs` in a pseudo-terminal and read its screen as
text, with the bench's terminal emulator and data service.

By hand, from the repository's root:

    tests/apps-bench/drive --app approvals tests/fixtures/apps/approvals -- \\
        wait:Loaded j n "type:Gym pass" enter "wait:Request filed." show \\
        'remote:{"op":"create","type":"request","content":{"title":"Desk","order":1001}}' dump

--app      the data service's seed: board poll rota approvals panel board-priority
--size     COLSxROWS (default 40x16, the app pane)
--scenario slow-load,slow-write,... (default: default)
--user     MINIAPP_USER (default ana)

Steps, in order (a final screen is always printed):
  j, q, /, ...      one key: a character, or enter esc tab backtab backspace
                    space up down left right home end delete pageup pagedown ctrl-c
  type:TEXT         type TEXT one character at a time      paste:TEXT  a paste
  wait:TEXT         wait up to 5 s until TEXT is on screen (fails the run if not)
  gone:TEXT         wait up to 5 s until TEXT is not on screen
  sleep:SECONDS     resize:COLSxROWS     show (print the screen now)
  remote:JSON       a change by another participant ("user" defaults to "bo"),
                    e.g. {"op":"update","record_id":"r0002","content":{...}}
  control:JSON      any other control command (delay_next, fail_next, arm_conflict)
  dump              print every record in the service, one per line

The host is `$APP_HOST_BIN`, or `examples/app_host` of the debug build in
`$CARGO_TARGET_DIR` (or `target/`). Its log goes to `$APPS_BENCH_OUT`.
Exit status: 0 if every wait/gone step held and the host is still running or
exited with 0.

As a library (the reload and hostile tests): launch(), has(), stop(), show().
"""
import json
import os
import sys

from harness import BENCH, Service, out_dir, parse_size
from term import Session

REPO = os.path.dirname(os.path.dirname(BENCH))
EXTRA_KEYS = {
    "backtab": b"\x1b[Z", "home": b"\x1b[H", "end": b"\x1b[F", "delete": b"\x1b[3~",
    "pageup": b"\x1b[5~", "pagedown": b"\x1b[6~",
}


def host_bin():
    given = os.environ.get("APP_HOST_BIN")
    if given:
        return given
    target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(REPO, "target")
    path = os.path.join(target, "debug", "examples", "app_host")
    if not os.path.exists(path):
        raise SystemExit(f"no host at {path}: run `cargo build --example app_host`, or set APP_HOST_BIN")
    return path


def launch(app_path, cols=80, rows=24, scenario="default", svc_app="board", log=None, host_log=None, user="ana"):
    """Starts a data service with `svc_app`'s seed and the host on `app_path`."""
    svc = Service(svc_app, scenario, log or os.path.join(out_dir(), "drive-service.jsonl"))
    env = dict(os.environ)
    env.update({"MINIAPP_DATA_ADDR": svc.data_addr, "MINIAPP_USER": user, "TERM": "xterm-256color",
                "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"})
    for k in ("COLUMNS", "LINES", "TMUX", "HERDR_ENV", "STY"):
        env.pop(k, None)
    if host_log:
        env["APP_HOST_LOG"] = host_log
    try:
        sess = Session([host_bin(), app_path], cols, rows, env)
    except Exception:
        svc.stop()
        raise
    return sess, svc


def has(text):
    return lambda s: text in s.text()


def stop(sess, svc):
    if sess.alive():
        sess.key("ctrl-c", settle=0)
        sess.wait_exit(2.0)
    sess.close()
    svc.stop()


def show(sess, title=""):
    sess.pump(0.05)
    print(sess.screen.dump(title))


def press(sess, name):
    if name in EXTRA_KEYS:
        sess.send(EXTRA_KEYS[name])
        sess.sleep(0.08)
    else:
        sess.key(name)


def main(argv):
    import argparse

    p = argparse.ArgumentParser(description="Drive the example host by hand (see the top of lib/drive.py).")
    p.add_argument("--app", required=True, help="data service seed: board poll rota approvals panel board-priority")
    p.add_argument("--size", default="40x16", type=parse_size)
    p.add_argument("--scenario", default="default")
    p.add_argument("--user", default="ana")
    p.add_argument("script", help="app.lua, or the folder that holds it")
    p.add_argument("steps", nargs="*")
    a = p.parse_args(argv)
    cols, rows = a.size
    host_log = os.environ.get("APP_HOST_LOG") or os.path.join(out_dir(), "drive-host.log")
    sess, svc = launch(a.script, cols, rows, a.scenario, a.app, host_log=host_log, user=a.user)
    ok = True
    try:
        sess.settle(0.3, 3.0)
        for step in a.steps:
            kind, _, arg = step.partition(":")
            if kind == "type" and arg:
                sess.type(arg)
            elif kind == "paste" and arg:
                sess.paste(arg)
                sess.sleep(0.1)
            elif kind == "wait" and arg:
                if not sess.wait_for(has(arg), 5):
                    ok = False
                    show(sess, f"FAILED wait:{arg}")
            elif kind == "gone" and arg:
                if not sess.wait_for(lambda s: arg not in s.text(), 5):
                    ok = False
                    show(sess, f"FAILED gone:{arg}")
            elif kind == "sleep" and arg:
                sess.sleep(float(arg))
            elif kind == "resize" and arg:
                c, r = parse_size(arg)
                sess.resize(c, r)
                sess.sleep(0.2)
            elif kind == "remote" and arg:
                msg = {"user": "bo", **json.loads(arg), "cmd": "remote"}
                print("remote:", json.dumps(svc.control(**msg)))
                sess.settle(0.2)
            elif kind == "control" and arg:
                print("control:", json.dumps(svc.control(**json.loads(arg))))
            elif step == "show":
                show(sess, "show")
            elif step == "dump":
                for r in svc.records():
                    print(json.dumps(r, ensure_ascii=False))
            else:
                press(sess, step)
            sess.settle(0.15, 1.0)
        show(sess, f"final screen ({cols}x{rows})")
        if not sess.alive():
            ok &= sess.proc.returncode == 0
            print("host exited with", sess.proc.returncode)
    finally:
        stop(sess, svc)
    print("log:", host_log)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
