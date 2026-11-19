"""measure: startup time, peak memory and key-to-screen latency."""
import argparse
import datetime
import json
import os
import select
import statistics
import sys
import tempfile
import threading
import time

from harness import APPS, Context, out_dir, parse_size
from term import tree_rss_kib


def parse_args(argv):
    if "--" not in argv:
        raise SystemExit("usage: measure --app APP --option LABEL [--presses N] -- COMMAND...")
    i = argv.index("--")
    ap = argparse.ArgumentParser(prog="measure")
    ap.add_argument("--app", required=True, choices=list(APPS))
    ap.add_argument("--option", required=True)
    ap.add_argument("--presses", type=int, default=120, help="key presses timed (default 120, minimum 100)")
    ap.add_argument("--size", default="80x24", type=parse_size, help="the terminal's size (default 80x24)")
    args = ap.parse_args(argv[:i])
    args.command = argv[i + 1:]
    if not args.command:
        raise SystemExit("measure: missing command after --")
    args.presses = max(100, args.presses)
    return args


LOADED_NOUN = {"board": "cards", "board-priority": "cards", "poll": "options", "rota": "days",
               "approvals": "requests", "panel": "metrics"}


def pct(values, p):
    s = sorted(values)
    k = max(0, min(len(s) - 1, int(round(p / 100 * len(s) + 0.5)) - 1))
    return s[k]


def main(argv):
    args = parse_args(argv)
    loaded = "Loaded 200 %s." % LOADED_NOUN[args.app]
    artifacts = tempfile.mkdtemp(prefix=f"measure-{args.app}-", dir=out_dir())
    cols, rows = args.size
    ctx = Context(args.app, args.command, artifacts, "measure", args.size)
    peak = [0]
    stop = threading.Event()
    result = {}
    try:
        sess, _ = ctx.launch("big")

        def sampler():
            while not stop.is_set():
                peak[0] = max(peak[0], tree_rss_kib(sess.sid))
                stop.wait(0.01)
        th = threading.Thread(target=sampler, daemon=True)
        th.start()

        t0 = sess.t_start
        first_byte = first_text = complete = None
        end = time.monotonic() + 20
        while time.monotonic() < end and complete is None:
            if sess.pump(0.005):
                now = sess.last_output
                first_byte = first_byte or now
                if first_text is None and sess.text().strip():
                    first_text = now
                if loaded in sess.last_row():
                    complete = now
        if complete is None:
            print(f"measure: no '{loaded}' on the status line within 20 s", file=sys.stderr)
            print(sess.screen.dump(), file=sys.stderr)
            return 1
        sess.settle(0.3)

        lat, timeouts = [], 0
        for i in range(args.presses):
            key = b"j" if (i // 50) % 2 == 0 else b"k"
            before = sess.text()
            t_send = time.monotonic()
            os.write(sess.master, key)
            got = None
            deadline = t_send + 2.0
            while time.monotonic() < deadline:
                r, _, _ = select.select([sess.master], [], [], 0.05)
                if not r:
                    continue
                t_chunk = time.monotonic()
                try:
                    chunk = os.read(sess.master, 65536)
                except OSError:
                    break
                sess.raw += chunk
                sess.screen.feed(chunk)
                sess.last_output = t_chunk
                if sess.text() != before:
                    got = t_chunk - t_send
                    break
            if got is None:
                timeouts += 1
            else:
                lat.append(got * 1000)
            sess.settle(0.03, limit=1.0)
        stop.set()
        th.join()
        result = {
            "time": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "app": args.app, "option": args.option, "size": f"{cols}x{rows}", "command": args.command,
            "first_output_ms": round((first_byte - t0) * 1000, 1),
            "first_text_ms": round((first_text - t0) * 1000, 1) if first_text else None,
            "first_complete_draw_ms": round((complete - t0) * 1000, 1),
            "peak_rss_mib": round(peak[0] / 1024, 1),
            "key_presses": args.presses, "key_timeouts": timeouts,
            "key_p50_ms": round(statistics.median(lat), 2) if lat else None,
            "key_p95_ms": round(pct(lat, 95), 2) if lat else None,
            "key_max_ms": round(max(lat), 2) if lat else None,
        }
    finally:
        stop.set()
        ctx.cleanup()
    print(f"measure: app={args.app} option={args.option} data=big (200 records) size={cols}x{rows}")
    print(f"  start -> first output       {result['first_output_ms']} ms")
    print(f"  start -> first complete draw {result['first_complete_draw_ms']} ms  ('{loaded}' shown)")
    print(f"  peak RSS of process tree    {result['peak_rss_mib']} MiB (sampled every 10 ms)")
    print(f"  key -> screen change        p50 {result['key_p50_ms']} ms, p95 {result['key_p95_ms']} ms, "
          f"max {result['key_max_ms']} ms over {len(lat)} presses ({timeouts} without a change in 2 s)")
    print(json.dumps(result, ensure_ascii=False))
    with open(os.path.join(out_dir(), "measures.jsonl"), "a") as f:
        f.write(json.dumps(result, ensure_ascii=False) + "\n")
    return 0 if not timeouts else 1
