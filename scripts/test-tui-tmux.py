#!/usr/bin/env python3
"""Run the scenarios of scripts/tui-herdr-scenarios.toml in tmux.

The same manifest as scripts/test-tui-herdr.sh, driven through a tmux server
on a private socket instead of a Herdr pane: for a machine where Herdr is not
running, and for what Herdr cannot do (an exact window size, the bytes bzz
wrote to the terminal). It never creates an identity, relay or channel; the
harness scenarios run on the disposable profile of examples/harness_fixture.rs
(scripts/test-relay.sh --tmux writes it).

Environment:
  BZZ_BIN               the release binary (absolute)
  BZZ_HERDR_FIXTURE     the harness fixture's folder (harness scenarios)
  BZZ_TMUX_SCENARIOS    automated | harness | all | ID[,ID...] (default harness)
  BZZ_TMUX_CAPTURES     a folder to keep a text capture of every step in
  BZZ_TMUX_SIZE         the window's size when a scenario names none (160x45)

What the manifest may say besides what test-tui-herdr.sh documents:
  scenario `size = "WxH"`     the window's size for the scenario
  scenario `beside = "second"` a second window runs the TUI of the fixture's
                               second profile; steps with `pane = "second"`
                               go to it. `beside = "other"`: that window
                               runs another person's profile (v0.16 M6)
  scenario `restored = true`  after the exit, the terminal is checked: the
                               alternate screen left, the mouse and bracketed
                               paste off, the cursor shown, in what bzz wrote
                               and in tmux's own state
  step `size = "WxH"`         the window is resized before the keys
  step `paste = "text"`       a bracketed paste of the text before the keys
                              (`\\n` in it is a line break)
"""

import os
import re
import shlex
import subprocess
import sys
import tempfile
import time
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANIFEST = os.environ.get("BZZ_HERDR_MANIFEST", os.path.join(ROOT, "scripts/tui-herdr-scenarios.toml"))
EXIT = "__BZZ_HERDR_EXIT__:"
MAIN = "bzz:main"
SECOND = "bzz:second"

# Herdr's key names as tmux's.
NAMED = {
    "enter": "Enter", "esc": "Escape", "tab": "Tab", "space": "Space",
    "backspace": "BSpace", "delete": "DC", "up": "Up", "down": "Down",
    "left": "Left", "right": "Right", "home": "Home", "end": "End",
    "pageup": "PPage", "pagedown": "NPage", "shift+tab": "BTab", "backtab": "BTab",
}


def tmux_key(key):
    if key in NAMED:
        return [NAMED[key]], False
    if key.startswith("ctrl+") and len(key) == 6:
        return ["C-" + key[-1]], False
    if key.startswith("alt+") and len(key) == 5:
        return ["M-" + key[-1]], False
    if key.startswith("shift+") and len(key) == 7:
        return [key[-1]], True
    if re.fullmatch(r"f([1-9]|1[0-2])", key):
        return [key.upper()], False
    if len(key) == 1:
        return [key], True
    raise SystemExit(f"a key tmux cannot send: {key!r}")


class Tmux:
    def __init__(self, socket):
        self.socket = socket

    def __call__(self, *arguments, check=True):
        result = subprocess.run(["tmux", "-S", self.socket, *arguments],
                                capture_output=True, text=True)
        if check and result.returncode != 0:
            raise SystemExit(f"tmux {' '.join(arguments)}: {result.stderr.strip()}")
        return result.stdout


def size_of(text):
    match = re.fullmatch(r"(\d+)x(\d+)", text)
    if not match:
        raise SystemExit(f"a size is WxH, not {text!r}")
    return match.group(1), match.group(2)


def main():
    binary = os.environ.get("BZZ_BIN", os.path.join(ROOT, "target/release/bzz"))
    if not (os.path.isabs(binary) and os.access(binary, os.X_OK)):
        raise SystemExit("BZZ_BIN must be an executable absolute path")
    fixture = os.environ.get("BZZ_HERDR_FIXTURE", "")
    selection = os.environ.get("BZZ_TMUX_SCENARIOS", "harness")
    captures = os.environ.get("BZZ_TMUX_CAPTURES")
    default_size = os.environ.get("BZZ_TMUX_SIZE", "160x45")
    with open(MANIFEST, "rb") as handle:
        document = tomllib.load(handle)
    wanted = None if selection in {"automated", "harness", "all"} else set(selection.split(","))
    scenarios = [
        scenario for scenario in document["scenario"]
        if (wanted is None and (selection == "all" or scenario["kind"] == selection))
        or (wanted is not None and scenario["id"] in wanted)
    ]
    scenarios = [s for s in scenarios if s["kind"] != "operator"]
    if not scenarios:
        raise SystemExit("no scenarios selected")
    work = tempfile.mkdtemp(prefix="bzz-tmux.")
    tmux = Tmux(os.path.join(work, "sock"))
    width, height = size_of(default_size)
    tmux("-f", "/dev/null", "new-session", "-d", "-s", "bzz", "-n", "keep", "-x", width,
         "-y", height, "sleep 86400")
    tmux("new-window", "-d", "-t", "bzz:", "-n", "main", "bash --norc --noprofile")
    tmux("set-option", "-g", "history-limit", "5000")
    with open(os.path.join(work, "null-asound.conf"), "w") as handle:
        handle.write("pcm.!default { type null }\n")
    failures = 0
    try:
        for scenario in scenarios:
            started = time.monotonic()
            try:
                run(tmux, scenario, binary, fixture, work, captures, default_size)
                print(f"scenario {scenario['id']} passed in {time.monotonic() - started:.1f} s", flush=True)
            except Failure as failure:
                failures += 1
                print(f"scenario {scenario['id']} FAILED: {failure}", flush=True)
                # Leave no TUI in the panes for the next scenario: a new
                # shell takes the place of whatever runs there.
                tmux("pipe-pane", "-t", MAIN, check=False)
                tmux("respawn-pane", "-k", "-t", MAIN, "bash --norc --noprofile", check=False)
                if "second" in tmux("list-windows", "-F", "#{window_name}", check=False).split():
                    tmux("kill-window", "-t", SECOND, check=False)
    finally:
        tmux("kill-server", check=False)
        print(f"tmux server on {tmux.socket} killed", flush=True)
    print(f"{len(scenarios) - failures} of {len(scenarios)} scenarios passed", flush=True)
    return 1 if failures else 0


class Failure(Exception):
    pass


def command_for(scenario, binary, fixture, work, profile):
    if scenario["kind"] == "automated":
        home = os.path.join(work, scenario["id"])
        for part in ("config", "data", "cache"):
            os.makedirs(os.path.join(home, part), mode=0o700, exist_ok=True)
        if scenario.get("keymap"):
            with open(os.path.join(home, "config", "keymap.toml"), "w") as handle:
                handle.write(scenario["keymap"])
        words = [f"BZZ_CONFIG_DIR={shlex.quote(home + '/config')}",
                 f"BZZ_DATA_DIR={shlex.quote(home + '/data')}",
                 f"BZZ_CACHE_DIR={shlex.quote(home + '/cache')}", shlex.quote(binary)]
        # An automated scenario may name a sub-command (`bzz demo`, v0.17).
        words.extend(shlex.quote(argument) for argument in scenario.get("args", []))
        return "env " + " ".join(words) + '; printf "__BZZ_HERDR_EXIT__:%s\\n" "$?"'
    if not fixture:
        raise SystemExit("harness scenarios need BZZ_HERDR_FIXTURE")
    if profile not in {"first", "second", "other"}:
        raise SystemExit(f"no profile {profile!r} in the harness fixture")
    home = fixture if profile == "first" else os.path.join(fixture, profile)
    words = [f"BZZ_CONFIG_DIR={shlex.quote(home + '/config')}",
             f"BZZ_DATA_DIR={shlex.quote(home + '/data')}",
             f"BZZ_CACHE_DIR={shlex.quote(home + '/cache')}",
             "BZZ_PASSPHRASE_FD=3",
             f"ALSA_CONFIG_PATH={shlex.quote(work + '/null-asound.conf')}"]
    env_file = os.path.join(fixture, "env")
    if os.path.isfile(env_file):
        for line in open(env_file):
            line = line.strip()
            if line:
                name, value = line.split("=", 1)
                words.append(f"{name}={shlex.quote(value)}")
    words.append(shlex.quote(binary))
    for argument in scenario.get("args", []):
        words.append(shlex.quote(argument.replace("{fixture}", fixture)))
    return ("env " + " ".join(words) + f" 3<{shlex.quote(fixture + '/passphrase')}"
            + '; printf "__BZZ_HERDR_EXIT__:%s\\n" "$?"')


def screen(tmux, target, recent):
    if recent:
        return tmux("capture-pane", "-p", "-J", "-S", "-400", "-t", target)
    return tmux("capture-pane", "-p", "-t", target)


def wait(tmux, target, wanted, pattern, timeout_ms, recent):
    deadline = time.monotonic() + timeout_ms / 1000
    while True:
        shown = screen(tmux, target, recent)
        found = re.search(wanted, shown) if pattern else wanted in shown
        if found:
            return shown
        if time.monotonic() > deadline:
            raise Failure(f"{wanted!r} did not show within {timeout_ms} ms. The pane shows:\n{shown}")
        time.sleep(0.05)


def at_prompt(tmux, target):
    for _ in range(100):
        command = tmux("display", "-p", "-t", target, "#{pane_current_command}").strip()
        if command == "bash":
            return
        time.sleep(0.1)
    raise Failure(f"the pane {target} is not at its shell prompt ({command})")


def send(tmux, target, keys):
    # Forty keys to a call, as test-tui-herdr.sh sends them: keys sent
    # together reach the terminal together (Esc and a key are read as Alt
    # and that key, as under Herdr). A lone `;` is tmux's command separator.
    names = []
    for key in keys:
        name, _ = tmux_key(key)
        names.append("\\;" if name[0] == ";" else name[0])
    for start in range(0, len(names), 40):
        tmux("send-keys", "-t", target, *names[start:start + 40])


def launch(tmux, target, command):
    at_prompt(tmux, target)
    tmux("send-keys", "-t", target, "clear", "Enter")
    tmux("clear-history", "-t", target)
    time.sleep(0.2)
    tmux("send-keys", "-t", target, "-l", "--", command)
    tmux("send-keys", "-t", target, "Enter")


def run(tmux, scenario, binary, fixture, work, captures, default_size):
    identifier = scenario["id"]
    width, height = size_of(scenario.get("size", default_size))
    tmux("resize-window", "-t", MAIN, "-x", width, "-y", height)
    recent = bool(scenario.get("args"))
    raw = os.path.join(work, f"{identifier}.raw")
    tmux("pipe-pane", "-t", MAIN, "-o", f"cat >> {shlex.quote(raw)}")
    launch(tmux, MAIN, command_for(scenario, binary, fixture, work, scenario.get("profile", "first")))
    beside = scenario.get("beside")
    if beside:
        tmux("new-window", "-d", "-t", "bzz:", "-n", "second", "bash --norc --noprofile")
        tmux("resize-window", "-t", SECOND, "-x", width, "-y", height)
        launch(tmux, SECOND, command_for(scenario, binary, fixture, work, beside))
    wait(tmux, MAIN, scenario["ready"], False, 30000, recent)
    print(f"scenario {identifier} ready", flush=True)
    exited = {MAIN: False, SECOND: not beside}
    for number, step in enumerate(scenario["step"], 1):
        target = SECOND if step.get("pane") == "second" else MAIN
        time.sleep(step.get("pause", 0) / 1000)
        if "size" in step:
            w, h = size_of(step["size"])
            tmux("resize-window", "-t", target, "-x", w, "-y", h)
        if "run" in step:
            # What the owner does outside bzz meanwhile: save a file, run a
            # command at another shell. `{fixture}` is the fixture's folder.
            done = subprocess.run(["bash", "-c", step["run"].replace("{fixture}", fixture)],
                                  env=dict(os.environ, BZZ_BIN=binary), capture_output=True, text=True)
            if done.returncode != 0:
                raise Failure(f"step {number}: `{step['run']}` failed: {done.stderr.strip()}")
        started = time.monotonic()
        if "paste" in step:
            tmux("set-buffer", "-b", "scenario", step["paste"])
            tmux("paste-buffer", "-p", "-d", "-b", "scenario", "-t", target)
        keys = []
        for key in step["keys"]:
            if key.startswith("type:"):
                keys.extend("space" if c == " " else c for c in key[5:].replace("{fixture}", fixture))
            else:
                keys.append(key)
        send(tmux, target, keys)
        visible = step["visible"]
        if not step.get("regex"):
            visible = visible.replace("{fixture}", fixture)
        shown = wait(tmux, target, visible, step.get("regex", False), step.get("timeout", 15000), recent)
        elapsed = (time.monotonic() - started) * 1000
        absent = step.get("absent")
        if absent:
            for attempt in range(10):
                shown = screen(tmux, target, False) + screen(tmux, target, True)
                if absent not in shown:
                    break
                if attempt == 9:
                    raise Failure(f"{absent!r} is on the screen and must not be")
                time.sleep(0.5)
        if captures:
            folder = os.path.join(captures, identifier)
            os.makedirs(folder, exist_ok=True)
            with open(os.path.join(folder, f"{number:02d}.txt"), "w") as handle:
                handle.write(f"# keys {keys} paste {step.get('paste')!r} size {step.get('size')} "
                             f"pane {target} · {visible!r} in {elapsed:.0f} ms\n")
                handle.write(screen(tmux, target, False))
        print(f"scenario {identifier} step {number} observed {visible!r} in {elapsed:.0f} ms", flush=True)
        if visible.startswith(EXIT):
            exited[target] = True
    if not exited[MAIN]:
        raise Failure("no exit marker")
    if beside and not exited[SECOND]:
        raise Failure("the second window's bzz did not exit")
    tmux("pipe-pane", "-t", MAIN)
    if scenario.get("restored"):
        check_restored(tmux, raw, identifier, captures)
    if beside:
        tmux("kill-window", "-t", SECOND)


def check_restored(tmux, raw, identifier, captures):
    data = open(raw, "rb").read()
    # From bzz's own alternate screen to the exit marker it printed: the
    # shell's prompt switches bracketed paste on again after it.
    start = data.find(b"\x1b[?1049h")
    end = data.rfind(EXIT.encode())
    data = data[max(start, 0):end if end > 0 else len(data)]
    problems = []
    for name, on, off in [
        ("alternate screen", b"\x1b[?1049h", b"\x1b[?1049l"),
        ("bracketed paste", b"\x1b[?2004h", b"\x1b[?2004l"),
        ("mouse", b"\x1b[?1000h", b"\x1b[?1000l"),
    ]:
        if data.rfind(on) < 0:
            problems.append(f"{name} was never switched on")
        elif data.rfind(off) < data.rfind(on):
            problems.append(f"{name} was left on")
    if data.rfind(b"\x1b[?25l") > data.rfind(b"\x1b[?25h"):
        problems.append("the cursor was left hidden")
    state = tmux("display", "-p", "-t", MAIN,
                 "alternate_on=#{alternate_on} mouse_any_flag=#{mouse_any_flag} "
                 "mouse_sgr_flag=#{mouse_sgr_flag} cursor_flag=#{cursor_flag}").strip()
    for wanted in ("alternate_on=0", "mouse_any_flag=0", "mouse_sgr_flag=0", "cursor_flag=1"):
        if wanted not in state:
            problems.append(f"tmux says {state}")
            break
    report = (f"terminal after exit: {state}; in {len(data)} bytes written, "
              f"last ?1049l at {data.rfind(b'\x1b[?1049l')}, last ?2004l at {data.rfind(b'\x1b[?2004l')}, "
              f"last ?1000l at {data.rfind(b'\x1b[?1000l')}, last ?25h at {data.rfind(b'\x1b[?25h')}")
    print(f"scenario {identifier} {report}", flush=True)
    if captures:
        with open(os.path.join(captures, identifier, "restored.txt"), "w") as handle:
            handle.write(report + "\n")
    if problems:
        raise Failure("; ".join(problems))


if __name__ == "__main__":
    sys.exit(main())
