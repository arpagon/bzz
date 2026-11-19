"""When pushes begin, on the example host: the contract's rule that pushes of
a type begin with the app's first list of it.

    tests/apps-bench/pushes

The data service pushes every change to whoever subscribed; bzz tells a
script of a change only to what it was given in a list. Checks:

- an app that has listed nothing hears of no change, and after its first
  list it hears of the next one;
- a change made while the first list is on its way (`slow-load`: the reply
  is held 3 s by the service, which took its snapshot before the change)
  reaches the Board once, after the list: it is neither lost nor shown
  twice.

Exit status 0 only if every check held.
"""
import os
import sys

from drive import REPO, has, launch, stop
from harness import out_dir

QUIET_APP = """
local app = {}
app.api = 1
app.types = { "note" }
function app.init(ctx) return { pushes = 0, listed = "no" } end
function app.view(s, ctx)
  return ui.text { "pushes: " .. s.pushes, "listed: " .. s.listed }
end
function app.key(s, key, ctx)
  if key.name == "l" then data.list { type = "note" } end
  if key.name == "q" then host.quit() end
end
function app.reply(s, r, ctx)
  if r.ok then s.listed = "yes " .. #r.records end
end
function app.push(s, p, ctx) s.pushes = s.pushes + 1 end
return app
"""


def main(argv):
    checks = []

    def check(name, ok, detail=""):
        checks.append(bool(ok))
        print(f"{'ok  ' if ok else 'FAIL'} {name}" + (f" ({detail})" if detail else ""), flush=True)

    work = os.path.join(out_dir(), "work", "pushes")
    os.makedirs(work, exist_ok=True)
    quiet = os.path.join(work, "app.lua")
    with open(quiet, "w") as f:
        f.write(QUIET_APP)

    sess, svc = launch(quiet, host_log=os.path.join(out_dir(), "pushes-host.log"),
                       log=os.path.join(out_dir(), "pushes-service.jsonl"))
    try:
        check("the app starts", sess.wait_for(has("pushes: 0"), 10))
        svc.control(cmd="remote", user="bo", op="create", type="note", content={"text": "one"})
        sess.sleep(0.5)
        check("before its first list the app hears of no change", "pushes: 0" in sess.text(),
              sess.text().split("listed")[0].strip())
        sess.key("l", settle=0)
        check("the list holds the change", sess.wait_for(has("listed: yes 1"), 3))
        svc.control(cmd="remote", user="bo", op="create", type="note", content={"text": "two"})
        check("after its first list it hears of the next change", sess.wait_for(has("pushes: 1"), 2))
    finally:
        stop(sess, svc)

    board = os.path.join(REPO, "tests", "fixtures", "apps", "board")
    sess, svc = launch(board, scenario="slow-load", host_log=os.path.join(out_dir(), "pushes-board.log"),
                       log=os.path.join(out_dir(), "pushes-board-service.jsonl"))
    try:
        sess.wait_for(has("Loading"), 5)
        sess.sleep(0.3)
        svc.control(cmd="remote", user="bo", op="create", type="card",
                    content={"title": "Latecomer", "column": "todo", "order": 99})
        check("the first list is answered late", sess.wait_for(has("Loaded"), 6))
        sess.sleep(0.5)
        text = sess.text()
        check("a change made while the first list was on its way is shown", "Latecomer" in text)
        check("and shown once", text.count("Latecomer") == 1, f"{text.count('Latecomer')} times")
        check("the column counts it", "To do (4)" in text)
    finally:
        stop(sess, svc)

    passed = sum(checks)
    print(f"RESULT pushes: {passed}/{len(checks)} passed, {len(checks) - passed} failed", flush=True)
    return 0 if all(checks) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
