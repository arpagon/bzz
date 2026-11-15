#!/usr/bin/env python3
"""Write the shot timelines of the v0.15 proof reel (shots/NN-name.json)."""
import json, pathlib
OUT = pathlib.Path(__file__).parent / "shots"
ESC, ENTER, TAB, CTRL_S, CTRL_Y = "\u001b", "\r", "\t", "\u0013", "\u0019"
def T(text, step=0.035): return {"type": text, "step": step}
def W(s): return {"wait": s}
def M(name): return {"mark": name}
def U(text, timeout=60, name=None, **kw):
    step = {"until": text, "timeout": timeout, **kw}
    if name: step["name"] = name
    return step
def open_team(settle=2.0):
    return [U("online", 40, "start"), W(1.2), T("  ", 0.15), W(0.4), T("team", 0.08), W(0.3), T(ENTER), W(settle)]
def mention(prefix):
    return [T("@" + prefix, 0.11), W(0.8), T(TAB), W(0.3)]
def say(prefixes, text, mark="send"):
    steps = [T("i"), W(0.35)]
    for prefix in prefixes: steps += mention(prefix)
    return steps + [T(text, 0.028), W(0.5), M(mark), T(ENTER)]
def cmd(text, step=0.03): return T(text + ENTER, step)
PASS = T("@PASS@" + ENTER, 0.01)
MOVE = "marigold-moves-today"
shots = {}

shots["01-doctor"] = [cmd("bzz --version"), W(1.5), cmd("bzz agent doctor | cut -c1-98"), U("claude:", 30, "doctor"), W(4)]

shots["02-create"] = open_team() + [
    M("form"), T(":agent new Scout" + ENTER, 0.06), U("new local agent", 20, "form-open"), W(1.0),
    T("openai-codex/gpt-6-luna" + ENTER, 0.04), W(0.4),
    T("You are Scout, the team's assistant. Answer in one or two short lines." + ENTER, 0.025), W(0.5),
    T(ENTER), W(0.3), T(ENTER), W(0.3), T(ENTER), W(0.5),          # who may wake it, people, folders: defaults
    T("github.com" + ENTER, 0.05), W(0.4), T(ENTER), W(0.4),       # hosts; picture
    T("off", 0.08), W(1.2), M("filled"),
    T(CTRL_S), U("Enter sign and create", 240, "checks"), W(2.5), M("checked"),
    T(ENTER), U("created in Studio", 120, "create"), W(3.5), M("created"),
    T(ENTER), U("Scout · pi-durable · idle", 120, "started"), W(2.5), M("panel"),
    T("i"), U("Scout added to #team", 40, "added"), W(2.0), T(ESC), W(0.8),
] + say(["Sco"], "our launch codeword is marigold. What is 17 times 23?") + [
    U("↳ 1 reply", 120, "answer"), W(1.0), T(ENTER), U("context · 1 reply", 20, "thread"), W(4.0), M("shown"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["03-restart"] = open_team() + [
    T("G"), W(0.5), T(ENTER), U("context · 1 reply", 20, "thread"), W(2.0), M("before"),
] + say(["Sco"], "what was the codeword? Save it to /notes/launch.txt on your disk, then show me `ls -l /notes`.", "ask-memory") + [
    U("context · 3 replies", 180, "memory"), W(0.8), T("G"), W(4.0), M("memory-shown"),
] + say(["Sco"], "clone https://github.com/octocat/Hello-World into /repos/hello and show `git log --oneline -3`. Then try `curl -sI https://example.com` and say what happened.", "ask-git") + [
    U("context · 5 replies", 240, "git"), W(0.8), T("G"), W(5.0), M("git-shown"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["04-disk"] = [cmd("ls /notes"), W(2), cmd("ls /tmp/bzz-demo/here/data/agents/*/"), W(2.5),
                    cmd("bzz agent show Scout | head -12 | cut -c1-98"), W(4)]

shots["05-grant"] = [cmd("cat site/test.sh"), W(2.5),
                     cmd("bzz agent edit Scout --allow-host github.com --folder site=/tmp/bzz-demo/site:rw:exec"),
                     U("next start", 30, "saved"), W(4)]

shots["06-hands"] = open_team() + say(["Sco"], "run ./test.sh in /mnt/site and tell me the result.") + [
    U("asks to run a command", 180, "ask"), W(2.5), M("banner"),
    T(CTRL_Y), U("a real command waits for your answer", 10, "dialog-open"), W(4.0), M("dialog"),
    T("a"), W(0.8), M("allowed"),
    U("↳ 1 reply", 180, "answer"), W(0.8), T("G"), W(0.4), T(ENTER), U("context · 1 reply", 20, "thread"), W(4.0), M("shown"),
] + say(["Sco"], "now run `rm -rf dist` in /mnt/site.", "ask-rm") + [
    U("asks to run a command", 180, "ask2"), W(2.0),
    T(CTRL_Y), U("a real command waits for your answer", 10, "dialog2-open"), W(3.0), M("dialog2"),
    T("r"), W(1.2), T("not today", 0.07), W(0.8), M("reason"), T(ENTER), W(0.8),
    U("context · 3 replies", 180, "answer2"), W(0.8), T("G"), W(4.5), M("refused"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["06b-after"] = [cmd("ls site"), W(2), cmd("bzz agent show Scout | grep -A4 '^activity' | cut -c1-98"), W(4)]

new = []
for index, (name, harness, model) in enumerate([("Grok", "grok", "grok-4.5"), ("Codex", "codex", "gpt-5.6-luna"), ("Claude", "claude-code", "haiku")]):
    new += [cmd("clear") , W(0.4)] if index else []
    new += [cmd(f"bzz agent new {name} --harness {harness} --model {model} --thinking low --yes | grep -v '^  '"),
            U("passphrase:", 60, f"new-{harness}-ask"), W(0.6), PASS,
            U("next: add it", 240, f"new-{harness}"), W(2.5),
            cmd(f"bzz agent add {name} team"),
            U("passphrase:", 30, f"add-{harness}-ask", count=2), W(0.6), PASS,
            U("mention it there", 60, f"add-{harness}"), W(2.0)]
new += [cmd("clear"), W(0.4), cmd("bzz agent list | cut -c1-64"), W(4)]
shots["07-harnesses"] = new

shots["08-talk"] = open_team() + [
    T(":agents" + ENTER, 0.06), U("Claude · claude-code", 60, "panel-open"), W(3.5), M("panel"), T(ESC), W(0.8),
] + say(["Gro", "Cod", "Cla"], "each of you: which program are you running on? One short line.") + [
    W(1.0), T("G"), W(0.5), T(ENTER), U("context · 3 replies", 300, "answers"), W(5.0), M("shown"),
    T("q"), W(0.8), T(":agents" + ENTER, 0.06), W(1.5), T("j"), W(3.0), M("grok"), T("j"), W(3.0), M("codex"), T("j"), W(3.0), M("claude"),
    T(ESC), W(0.8), T("q"), W(0.7), T("y"), W(1.5)]

shots["09-wake-a"] = open_team() + [
    M("waiting"), U("what is 6 times 7?", 60, "lin-asked"), W(14.0), M("silence"),
    T(":agents" + ENTER, 0.06), U("wakes for:", 30, "panel-open"), W(4.0), M("panel"),
    T(ESC), W(0.8), T("q"), W(0.7), T("y"), W(1.5)]

shots["09b-wake-edit"] = [cmd("bzz agent edit Scout --wake anyone"), U("passphrase:", 30, "ask"), W(0.6), PASS, W(6)]

shots["09c-wake-c"] = open_team() + [
    M("waiting"), U("and now: what is 6 times 7?", 60, "lin-asked"), W(0.5),
    U("↳ 1 reply", 180, "answer"), W(1.0), T("G"), W(0.5), T(ENTER), U("context · 1 reply", 20, "thread"), W(4.5), M("shown"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["10-control"] = open_team() + [
    T(":agents" + ENTER, 0.06), U("Grok · grok · idle", 90, "panel-open"), W(1.5), T("j"), W(1.5),
    M("pause"), T(" "), U("Grok · grok · paused", 30, "paused"), W(3.0),
    M("resume"), T(" "), U(["Grok · grok · idle", "Grok · grok · ready"], 60, "resumed"), W(2.5), T(ESC), W(0.8),
] + say(["Cod"], "count from 1 to 400, slowly, one number per line.") + [
    W(0.8), T(":agents" + ENTER, 0.06), W(0.8), T("kkk", 0.15), W(0.3), T("jj", 0.4),
    U("Codex · codex (experimental) · answering", 120, "answering"), W(2.5), M("stop"), T("x"),
    U("its running turn was stopped", 30, "stopped"), W(1.0), U("last outcome: failed: stopped", 60, "outcome"), W(4.0), M("stopped-shown"),
    T(ESC), W(0.8), T("G"), W(0.5), T(ENTER), U("context ·", 20, "thread"), W(4.0), M("notice"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["10b-delete"] = [cmd("bzz agent delete Claude"), U("[y/N]", 30, "ask"), W(2.5), T("y" + ENTER, 0.2),
                       U("passphrase:", 30, "ask-owner"), W(0.6), PASS, W(8), M("deleted"),
                       cmd("bzz agent list | cut -c1-64"), W(4)]

shots["11-move"] = [cmd(f"bzz agent move Scout --to scout.bzzagent"),
                    U("Owner identity passphrase:", 30, "ask-owner"), W(0.8), PASS,
                    U("Passphrase for the file", 30, "ask-file"), W(1.2), T(MOVE + ENTER, 0.03),
                    U("Confirm passphrase:", 30, "ask-confirm"), W(0.8), T(MOVE + ENTER, 0.03),
                    U("next: copy the file", 180, "moved"), W(5),
                    cmd("clear"), W(0.3), cmd("ls -l scout.bzzagent; bzz agent list | cut -c1-64"), W(4)]

shots["12-receive"] = [cmd("bzz agent list"), W(2),
                       cmd("bzz agent receive scout.bzzagent"),
                       U("Passphrase of the file:", 30, "ask-wrong"), W(0.8), T("not-the-passphrase" + ENTER, 0.03),
                       U("the passphrase is wrong", 60, "wrong"), W(3.5),
                       cmd("bzz agent receive scout.bzzagent"),
                       U("Passphrase of the file:", 30, "ask-file", count=2), W(0.8), T(MOVE + ENTER, 0.03),
                       U("Owner identity passphrase:", 60, "ask-owner"), W(0.8), PASS,
                       U("run it in one place only", 180, "received"), W(5)]

def first_thread(replies):
    """Open Scout's first thread, wherever the channel has grown to: from the top, the 12th row."""
    return [T("gg", 0.12), W(0.5), T("j" * 11, 0.09), W(0.6), T(ENTER), U(f"context · {replies} replies", 30, "thread"), W(0.8), T("G"), W(2.0), M("before")]

shots["13-there"] = open_team(3.0) + first_thread(5) + say(["Sco"],
    "you were just moved to another bzz. What was the codeword, and what does `cat /notes/launch.txt; ls /repos` show?") + [
    U("context · 7 replies", 240, "answer"), W(0.8), T("G"), W(5.0), M("shown"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["14-kit"] = [cmd("bzz agent launch-kit Scout --to scout-kit"), U("[y/N]", 30, "ask"), W(3.5), T("y" + ENTER, 0.2),
                   U("Owner identity passphrase:", 30, "ask-owner"), W(0.6), PASS,
                   U("next: copy the folder", 180, "written"), W(5), M("written-shown"),
                   cmd("clear"), W(0.3), cmd("ls scout-kit"), W(2.5),
                   cmd("(cd scout-kit/host && npm ci --ignore-scripts --omit=optional --omit=dev | tail -1)"),
                   U("vulnerabilities", 300, "npm"), W(1.5),
                   cmd("nohup scout-kit/start.sh > kit.log 2>&1 &"), W(6),
                   cmd("grep -o 'buzz-acp starting\\|ready: remote copy.*\\|presence set to online' kit.log"), W(4)]

shots["15-kit-talk"] = open_team(3.0) + [
    T(":agents" + ENTER, 0.06), U("moved away", 60, "panel-open"), W(4.0), M("panel"), T(ESC), W(0.8),
] + first_thread(7) + say(["Sco"], "now you run under Buzz's own buzz-acp, with no bzz. Do you still know the codeword? One line.") + [
    U("context · 9 replies", 240, "answer"), W(0.8), T("G"), W(5.0), M("shown"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

shots["16-back"] = [cmd("bzz agent shutdown Scout"), U("passphrase:", 30, "ask"), W(0.6), PASS,
                    U(["Scout left", "still present"], 120, "shutdown"), W(3.5),
                    cmd("bzz agent receive scout-kit"), U("passphrase:", 30, "ask2", count=2), W(0.6), PASS,
                    U("run it in one place only", 120, "back"), W(5), M("back-shown"),
                    cmd("clear"), W(0.3), cmd("rm -rf scout-kit scout.bzzagent; bzz agent list | cut -c1-64"), W(4)]

shots["17-home"] = open_team(3.0) + first_thread(9) + say(["Sco"], "welcome home. One line: the codeword, and where have you been?") + [
    U("context · 11 replies", 240, "answer"), W(0.8), T("G"), W(5.0), M("shown"),
    T("q"), W(0.7), T("q"), W(0.7), T("y"), W(1.5)]

json_dump = lambda steps: json.dumps(steps, ensure_ascii=True)
for name, steps in shots.items():
    (OUT / f"{name}.json").write_text(json_dump(steps) + "\n")
print(len(shots), "shots")
