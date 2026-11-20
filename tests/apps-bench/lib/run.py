"""run: run the acceptance cases against a program in a pseudo-terminal."""
import argparse
import datetime
import importlib
import json
import os
import sys
import time
import traceback

from harness import APPS, CaseFail, Context, out_dir, parse_size


def parse_args(argv):
    if "--" not in argv:
        raise SystemExit("usage: run --app board|poll|rota|approvals|panel|board-priority --option LABEL "
                         "[--size 40x16] [--case 1,5] -- COMMAND...")
    i = argv.index("--")
    ap = argparse.ArgumentParser(prog="run")
    ap.add_argument("--app", required=True, choices=list(APPS))
    ap.add_argument("--option", required=True, help="label recorded in runs.jsonl, e.g. A, B, e3-a-1")
    ap.add_argument("--case", help="comma-separated case numbers to run (default: all)")
    ap.add_argument("--size", default="80x24", type=parse_size,
                    help="the size every case starts at (default 80x24; the app pane is 40x16)")
    ap.add_argument("--artifacts", help="directory for screens, raw output and service logs")
    args = ap.parse_args(argv[:i])
    args.command = argv[i + 1:]
    if not args.command:
        raise SystemExit("run: missing command after --")
    return args


def main(argv):
    args = parse_args(argv)
    CASES = importlib.import_module(APPS[args.app]).CASES
    wanted = None
    if args.case:
        wanted = {c.strip().zfill(2) for c in args.case.split(",") if c.strip()}
    cases = [c for c in CASES if wanted is None or c[0] in wanted]
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    cols, rows = args.size
    artifacts = args.artifacts or os.path.join(
        out_dir(), f"{args.app}-{args.option}-{cols}x{rows}-{stamp}-{os.getpid()}")
    os.makedirs(artifacts, exist_ok=True)

    print(f"bench: app={args.app} option={args.option} size={cols}x{rows} command={' '.join(args.command)}",
          flush=True)
    passed, failed = [], []
    for cid, name, fn in cases:
        ctx = Context(args.app, args.command, artifacts, f"case-{cid}", args.size)
        t0 = time.monotonic()
        err = None
        try:
            fn(ctx)
        except CaseFail as e:
            err = e
        except Exception as e:  # a crash of the bench itself, or of a control call
            err = CaseFail(f"case raised {type(e).__name__}: {e}", detail=traceback.format_exc())
        finally:
            ctx.cleanup()
        dt = time.monotonic() - t0
        if err is None:
            passed.append(cid)
            print(f"PASS {args.app} {cid} {name} ({dt:.1f}s)", flush=True)
        else:
            failed.append(cid)
            print(f"FAIL {args.app} {cid} {name} ({dt:.1f}s): expected {err.expected}", flush=True)
            if err.screen:
                print("     screen seen:")
                print("\n".join("     " + l for l in err.screen.splitlines()))
            if err.detail:
                print("\n".join("     " + l for l in err.detail.rstrip().splitlines()))
            print(f"     artifacts: {os.path.join(artifacts, 'case-' + cid)}", flush=True)

    total = len(passed) + len(failed)
    print(f"RESULT {args.app} option={args.option} size={cols}x{rows}: {len(passed)}/{total} passed, "
          f"{len(failed)} failed"
          + (f" (failed: {', '.join(failed)})" if failed else ""), flush=True)
    entry = {
        "time": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "app": args.app, "option": args.option, "size": f"{cols}x{rows}",
        "passed": len(passed), "failed": len(failed),
        "failed_cases": failed, "cases_run": [c[0] for c in cases], "command": args.command,
        "cwd": os.getcwd(),
    }
    line = (json.dumps(entry, ensure_ascii=False) + "\n").encode()
    fd = os.open(os.path.join(out_dir(), "runs.jsonl"), os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o644)
    try:
        os.write(fd, line)
    finally:
        os.close(fd)
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
