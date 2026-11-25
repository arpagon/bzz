#!/usr/bin/env bash
# Run bzz-owned, non-secret release-TUI acceptance scenarios in a Herdr pane.
# This is deliberately an operator/self-hosted gate: CI uses the deterministic
# TestBackend harness instead. It never creates identities, relays, channels,
# credentials, or secret-bearing configuration.
set -euo pipefail

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
manifest=${BZZ_HERDR_MANIFEST:-"$repo_root/scripts/tui-herdr-scenarios.toml"}
selection=${BZZ_HERDR_SCENARIOS:-automated}

usage() {
    cat <<'EOF'
Usage: ./scripts/test-tui-herdr.sh [--list] [--scenario ID[,ID...]]

Required environment for execution:
  HERDR_ENV=1                 run from a Herdr-managed controlling pane
  BZZ_HERDR_PANE=<pane-id>    disposable shell pane used for the release TUI
  BZZ_BIN=<absolute path>     release bzz binary (default: target/release/bzz)

Optional environment:
  BZZ_HERDR_SCENARIOS=automated|harness|all|ID[,ID...]
  BZZ_HERDR_MANIFEST=<path>
  BZZ_HERDR_FIXTURE=<absolute dir> harness scenarios: the disposable profile
                              written by examples/harness_fixture.rs (use
                              scripts/test-relay.sh --herdr)
  BZZ_HERDR_SECOND_PANE=<pane-id> a second disposable shell pane, for a
                              scenario with `beside = "second"` (v0.16)

Only automated empty-profile scenarios run by default. Operator scenarios
require the public disposable fixture and postconditions in docs/e2e-herdr.md;
the runner refuses to drive them so it cannot encounter credentials or real
community content.
EOF
}

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    usage
    exit 0
fi
if [[ ${1:-} == --list ]]; then
    selection=all
elif [[ ${1:-} == --scenario ]]; then
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    selection=$2
elif [[ $# -ne 0 ]]; then
    usage >&2
    exit 2
fi

[[ -f $manifest ]] || { echo "missing scenario manifest: $manifest" >&2; exit 2; }
# Emit one tab-delimited header and step rows. The manifest is data, never
# sourced as shell; control characters make it fail closed before any terminal
# command is issued.
mapfile -t scenarios < <(python3 - "$manifest" "$selection" <<'PY'
import base64
import os
import re
import sys
import tomllib


def control(value):
    return any(character in value for character in "\t\n\r\x1f")


def encoded(value):
    # Never empty: bash's `read` joins the tabs around an empty field.
    return "=" + base64.b64encode(value.encode()).decode()


path, selected = sys.argv[1:]
with open(path, "rb") as handle:
    document = tomllib.load(handle)
if document.get("version") != 1 or set(document) - {"version", "scenario"}:
    raise SystemExit("unsupported or malformed Herdr scenario manifest")
values = document.get("scenario")
if not isinstance(values, list) or not values:
    raise SystemExit("Herdr scenario manifest contains no scenarios")
wanted = None if selected in {"all", "automated", "harness"} else set(selected.split(","))
for scenario in values:
    if set(scenario) - {"id", "kind", "description", "ready", "exit", "keymap", "step", "args", "profile", "size", "beside", "restored", "empty", "isolated", "motion", "needs", "min_size"}:
        raise SystemExit("Herdr scenario manifest contains an unknown scenario field")
    identifier = scenario.get("id")
    kind = scenario.get("kind")
    ready = scenario.get("ready")
    exit_marker = scenario.get("exit")
    steps = scenario.get("step")
    if not all(isinstance(value, str) and value for value in (identifier, kind, ready)):
        raise SystemExit("Herdr scenario manifest has invalid scenario metadata")
    if kind not in {"automated", "operator", "harness", "join"} or not isinstance(exit_marker, str):
        raise SystemExit("Herdr scenario manifest has an invalid scenario kind")
    # v0.17 `join`: an empty, isolated profile against the harness relay,
    # run by scripts/test-tui-tmux.py only (`empty`, `isolated`).
    if kind == "join":
        continue
    # v0.17 `empty` and `isolated` (a profile that is the whole home, with
    # no session bus): scripts/test-tui-tmux.py only. `min_size` is read by
    # it only too; here the pane is the size it is.
    if scenario.get("empty") or scenario.get("isolated"):
        if wanted is None or identifier in wanted:
            print(f"scenario {identifier} is run by scripts/test-tui-tmux.py only (empty or isolated profile)", file=sys.stderr)
        continue
    if not isinstance(steps, list) or not steps:
        raise SystemExit("Herdr scenario manifest has no steps")
    # v0.17 `mouse` steps: Herdr sends keys only; scripts/test-tui-tmux.py
    # runs these.
    if any(isinstance(step, dict) and "mouse" in step for step in steps):
        if wanted is None or identifier in wanted:
            print(f"scenario {identifier} is run by scripts/test-tui-tmux.py only (mouse steps)", file=sys.stderr)
        continue
    if wanted is not None and identifier not in wanted:
        continue
    if selected in {"automated", "harness"} and kind != selected:
        continue
    # v0.17 `needs`: scenarios earlier in the manifest that make what this
    # one uses. They run first in the manifest's order, and the first
    # failure stops the run; a selection that leaves one out is told.
    needs = scenario.get("needs", [])
    if not isinstance(needs, list) or not all(isinstance(need, str) for need in needs):
        raise SystemExit("Herdr scenario has invalid needs")
    if wanted is not None:
        for need in needs:
            if need not in wanted:
                print(f"scenario {identifier} needs {need}, which is not selected", file=sys.stderr)
    # `args`: run `bzz <args>` on the harness profile instead of the TUI.
    # `profile = "second"`: the fixture's second profile of the same owner.
    args = scenario.get("args", [])
    profile = scenario.get("profile", "first")
    if not isinstance(args, list) or not all(isinstance(arg, str) and arg for arg in args):
        raise SystemExit("Herdr scenario has invalid args")
    # v0.17: an automated scenario may name a sub-command (`bzz demo`).
    if profile not in {"first", "second"} or (kind != "harness" and profile != "first"):
        raise SystemExit("Herdr scenario has an invalid profile outside a harness scenario")
    # v0.16: `size` (WxH), `beside = "second"`, `restored` (checked by
    # scripts/test-tui-tmux.py only: Herdr does not show what bzz wrote).
    size = scenario.get("size", "-")
    beside = scenario.get("beside", "-")
    if not re.fullmatch(r"-|[0-9]{2,3}x[0-9]{2,3}", size) or beside not in {"-", "second", "other"}:
        raise SystemExit("Herdr scenario has an invalid size or beside")
    if not isinstance(scenario.get("restored", False), bool):
        raise SystemExit("Herdr scenario has an invalid restored flag")
    # v0.17 `motion = true`: bzz runs without BZZ_NO_MOTION.
    motion = scenario.get("motion", False)
    if not isinstance(motion, bool):
        raise SystemExit("Herdr scenario has an invalid motion flag")
    safe = [identifier, kind, ready, exit_marker, scenario.get("keymap", "")]
    if any(control(value) for value in safe[:-1] + args):
        raise SystemExit("Herdr scenario manifest contains unsupported control characters")
    print("\t".join(["H"] + safe[:4] + [encoded(safe[4]), profile, encoded("\x1f".join(args)), size, beside, "motion" if motion else "still"]))
    for step in steps:
        if not {"keys", "visible"} <= set(step) or set(step) - {"keys", "visible", "absent", "timeout", "regex", "pause", "size", "paste", "pane", "run"}:
            raise SystemExit("Herdr scenario step takes keys, visible, absent, timeout, regex, pause, size, paste, pane and run")
        # v0.16 M4: `run`, a command of this machine's shell before the
        # keys: what the owner does outside bzz (saving a file).
        run = step.get("run", "")
        if not isinstance(run, str) or any(c in run for c in "\t\r\x1f\n"):
            raise SystemExit("Herdr scenario step has an invalid run")
        step_size = step.get("size", "-")
        step_pane = step.get("pane", "first")
        paste = step.get("paste", "")
        if not re.fullmatch(r"-|[0-9]{2,3}x[0-9]{2,3}", step_size) or step_pane not in {"first", "second"}:
            raise SystemExit("Herdr scenario step has an invalid size or pane")
        if not isinstance(paste, str) or any(c in paste for c in "\t\r\x1b\x1f"):
            raise SystemExit("Herdr scenario step has an invalid paste")
        keys = step["keys"]
        visible = step["visible"]
        absent = step.get("absent", "")
        timeout = step.get("timeout", 15000)
        # `regex = true`: `visible` is a pattern, for text a pane may wrap
        # in more than one place.
        pattern = step.get("regex", False)
        if not isinstance(pattern, bool):
            raise SystemExit("Herdr scenario has an invalid regex flag")
        # `pause`: milliseconds to wait before the keys are sent, for what
        # the relay must send back first and no screen shows arriving.
        pause = step.get("pause", 0)
        if not isinstance(pause, int) or isinstance(pause, bool) or not 0 <= pause <= 30000:
            raise SystemExit("Herdr scenario has an invalid pause")
        # No keys: the step only waits for what a command prints next.
        if not isinstance(keys, list) or not all(isinstance(key, str) and key for key in keys):
            raise SystemExit("Herdr scenario has invalid keys")
        if not isinstance(visible, str) or not visible or any(control(key) for key in keys):
            raise SystemExit("Herdr scenario has invalid visible output")
        if control(visible) or not isinstance(absent, str) or control(absent):
            raise SystemExit("Herdr scenario has unsupported visible or absent output")
        if not isinstance(timeout, int) or isinstance(timeout, bool) or not 1000 <= timeout <= 300000:
            raise SystemExit("Herdr scenario has an invalid timeout")
        # `type:<text>` is typed one key event per character; `{fixture}` in
        # it is the harness fixture's folder.
        events = []
        for key in keys:
            if key.startswith("type:") and len(key) > 5:
                text = key[5:].replace("{fixture}", os.environ.get("BZZ_HERDR_FIXTURE", "{fixture}"))
                events.extend("space" if character == " " else character for character in text)
            else:
                events.append(key)
        print("\t".join(["S", encoded("\x1f".join(events)), visible, str(timeout), encoded(absent), "regex" if pattern else "match", str(pause), step_size, encoded(paste), step_pane, encoded(run)]))
PY
)

if [[ ${#scenarios[@]} -eq 0 ]]; then
    echo "no scenarios selected" >&2
    exit 2
fi
if [[ ${1:-} == --list ]]; then
    for row in "${scenarios[@]}"; do
        IFS=$'\t' read -r tag identifier kind ready _ <<<"$row"
        [[ $tag == H ]] && printf '%s\t%s\t%s\n' "$identifier" "$kind" "$ready"
    done
    exit 0
fi

[[ ${HERDR_ENV:-} == 1 ]] || {
    echo "test-tui-herdr.sh must run from a Herdr-managed pane (HERDR_ENV=1)" >&2
    exit 2
}
pane=${BZZ_HERDR_PANE:?set BZZ_HERDR_PANE to a disposable shell pane ID}
[[ $pane != *[[:space:]]* ]] || { echo "BZZ_HERDR_PANE must not contain whitespace" >&2; exit 2; }
binary=${BZZ_BIN:-"$repo_root/target/release/bzz"}
[[ $binary = /* && -x $binary ]] || {
    echo "BZZ_BIN must be an executable absolute release binary (build with cargo build --release --locked)" >&2
    exit 2
}
herdr pane process-info --pane "$pane" >/dev/null

# Every scenario starts from the pane's shell prompt. A TUI left running by an
# interrupted run would otherwise receive the next launch command as keys.
require_shell() {
    local state
    for _ in $(seq 1 50); do
        state=$(herdr pane process-info --pane "$pane" | python3 -c '
import json, sys
names = [p["name"] for p in json.load(sys.stdin)["result"]["process_info"]["foreground_processes"]]
shells = {"bash", "zsh", "sh", "fish", "dash"}
print("bzz" if "bzz" in names else "shell" if set(names) <= shells else "busy")
')
        [[ $state == shell ]] && return 0
        [[ $state == bzz ]] && break
        sleep 0.1 # a prompt helper (starship, git, ...) is still running
    done
    echo "the target pane is not at its shell prompt ($state); quit any bzz left running there first" >&2
    exit 2
}

root=$(mktemp -d "${TMPDIR:-/tmp}/bzz-herdr.XXXXXXXX")
sound=
cleanup() {
    if [[ -n $sound ]]; then "$repo_root/scripts/test-audio-containers.sh" stop "$sound" || true; fi
    rm -rf -- "$root"
}
trap cleanup EXIT INT TERM

# On Linux bzz's huddles speak to the sound server PULSE_SERVER names, else
# to the session's: a harness huddle joins a PulseAudio with a null sink in a
# disposable container (or BZZ_TEST_PULSE_SERVER), else none and says so,
# never this machine's own microphone.
if [[ -n ${BZZ_TEST_PULSE_SERVER:-} ]]; then
    pulse_server=$BZZ_TEST_PULSE_SERVER
elif command -v docker >/dev/null 2>&1 \
    && pulse_server=$("$repo_root/scripts/test-audio-containers.sh" serve "$root/sound"); then
    sound=$root/sound
else
    pulse_server=unix:$root/no-sound-server/native
fi

# A field the manifest reader wrote as `=<base64>`.
decoded() {
    printf '%s' "${1#=}" | base64 --decode
}

run_scenario() {
    local identifier=$1 kind=$2 ready=$3 exit_marker=$4 encoded_keymap=$5 profile=$6 encoded_args=$7
    if [[ $kind == harness ]]; then
        run_harness_scenario "$identifier" "$ready" "$profile" "$encoded_args"
        return
    fi
    if [[ $kind != automated ]]; then
        echo "operator scenario '$identifier' is intentionally manual; see docs/e2e-herdr.md" >&2
        return 2
    fi
    local scenario_root="$root/$identifier"
    local config="$scenario_root/config"
    local data="$scenario_root/data"
    local cache="$scenario_root/cache"
    mkdir -p -- "$config" "$data" "$cache"
    chmod 700 -- "$scenario_root" "$config" "$data" "$cache"
    if [[ $encoded_keymap != "=" ]]; then
        decoded "$encoded_keymap" >"$config/keymap.toml"
        chmod 600 -- "$config/keymap.toml"
    fi

    # All values are local paths or a validated executable. printf %q creates
    # one shell word per value for the disposable target pane; no credential or
    # fixture value is passed to the process. Every visual wait below is bounded
    # by Herdr, which is portable to the controlled target shell.
    local command word still=BZZ_NO_MOTION=1
    [[ $scenario_motion != motion ]] || still=
    printf -v command \
        'env %s BZZ_CONFIG_DIR=%q BZZ_DATA_DIR=%q BZZ_CACHE_DIR=%q PULSE_SERVER=%q %q' \
        "$still" "$config" "$data" "$cache" "$pulse_server" "$binary"
    # v0.17: an automated scenario's `args` (`bzz demo`).
    if [[ $encoded_args != "=" ]]; then
        local -a arguments
        mapfile -d $'\x1f' -t arguments < <(decoded "$encoded_args")
        for word in "${arguments[@]}"; do
            printf -v word ' %q' "$word"
            command+=$word
        done
    fi
    command+='; status=$?; printf "__BZZ_HERDR_EXIT__:%s\n" "$status"'
    # Clear only the disposable terminal surface, with its scrollback, so a
    # prior label cannot satisfy a later readiness predicate. Nothing of it
    # is saved.
    require_shell
    herdr pane run "$pane" "printf '\\033[3J\\033[2J\\033[H'" >/dev/null
    herdr pane run "$pane" "$command" >/dev/null
    herdr pane wait-output "$pane" --source visible --match "$ready" --timeout 15000 >/dev/null
    printf 'scenario %s ready\n' "$identifier"
}

# Harness scenarios run the disposable harness profile. The passphrase comes
# from its owner-only file through a descriptor, never an argument or variable,
# and ALSA's default device is the null device so no real microphone opens.
# With `args` the release binary runs that command instead of the TUI, and
# what it asks on the terminal is answered with keys. `{fixture}` in an
# argument or in typed text is the fixture's folder. The fixture's `env` file
# (v0.15: where the stand-in for `node` and Pi's empty folder are) is added
# to the environment; it holds paths, never a secret.
harness_command() {
    local profile=$1 encoded_args=$2
    local fixture=${BZZ_HERDR_FIXTURE:?harness scenarios need BZZ_HERDR_FIXTURE}
    [[ $fixture = /* && -f $fixture/passphrase && -d $fixture/config ]] || {
        echo "BZZ_HERDR_FIXTURE must be an absolute harness fixture directory" >&2
        exit 2
    }
    local home=$fixture
    if [[ $profile == second || $profile == other ]]; then
        home=$fixture/$profile
        [[ -d $home/config ]] || { echo "the harness fixture has no $profile profile" >&2; exit 2; }
    fi
    printf '%s\n' 'pcm.!default { type null }' >"$root/null-asound.conf"
    local command word
    local still=BZZ_NO_MOTION=1
    [[ $scenario_motion != motion ]] || still=
    printf -v command 'env %s BZZ_CONFIG_DIR=%q BZZ_DATA_DIR=%q BZZ_CACHE_DIR=%q BZZ_PASSPHRASE_FD=3 ALSA_CONFIG_PATH=%q PULSE_SERVER=%q' \
        "$still" "$home/config" "$home/data" "$home/cache" "$root/null-asound.conf" "$pulse_server"
    if [[ -f $fixture/env ]]; then
        local name value
        while IFS='=' read -r name value; do
            [[ $name =~ ^[A-Za-z_][A-Za-z0-9_]*$ && -n $value ]] || {
                echo "the harness fixture's env file has a line that is not NAME=value" >&2
                exit 2
            }
            printf -v word ' %s=%q' "$name" "$value"
            command+=$word
        done <"$fixture/env"
    fi
    printf -v word ' %q' "$binary"
    command+=$word
    if [[ $encoded_args != "=" ]]; then
        local -a arguments
        mapfile -d $'\x1f' -t arguments < <(decoded "$encoded_args")
        for word in "${arguments[@]}"; do
            printf -v word ' %q' "${word//\{fixture\}/$fixture}"
            command+=$word
        done
    fi
    printf -v word ' 3<%q; status=$?; printf "__BZZ_HERDR_EXIT__:%%s\\n" "$status"' "$fixture/passphrase"
    command+=$word
    printf '%s' "$command"
}

# Starts a command in a pane at its prompt: the screen cleared, the pane's
# terminal written down (for `size` steps), and the scenario's size set.
launch_in() {
    local target=$1 which=$2 command=$3
    herdr pane run "$target" "printf '\\033[3J\\033[2J\\033[H'" >/dev/null
    herdr pane run "$target" "tty > $(printf %q "$root/tty-$which")" >/dev/null
    if [[ $scenario_size != - ]]; then
        herdr pane run "$target" "stty cols ${scenario_size%x*} rows ${scenario_size#*x}" >/dev/null
    fi
    sleep 0.3
    herdr pane run "$target" "$command" >/dev/null
}

run_harness_scenario() {
    local identifier=$1 ready=$2 profile=$3 encoded_args=$4
    local fixture=$BZZ_HERDR_FIXTURE command
    command=$(harness_command "$profile" "$encoded_args")
    # A command's output scrolls: what it printed is read from the pane's
    # recent output, with wrapped lines joined. The TUI is read as shown.
    source=visible
    [[ $encoded_args == "=" ]] || source=recent-unwrapped
    require_shell
    launch_in "$pane" first "$command"
    if [[ $scenario_beside != - ]]; then
        second_pane=${BZZ_HERDR_SECOND_PANE:?a scenario with beside needs BZZ_HERDR_SECOND_PANE}
        launch_in "$second_pane" second "$(harness_command "$scenario_beside" "=")"
    fi
    herdr pane wait-output "$pane" --source "$source" --lines 400 --match "$ready" --timeout 30000 >/dev/null || {
        echo "scenario '$identifier': \"$ready\" did not show. The pane shows:" >&2
        herdr pane read "$pane" --source "$source" --lines 400 >&2 || true
        exit 1
    }
    printf 'scenario %s ready\n' "$identifier"
}

source=visible
active_identifier=
active_exit=
scenario_size=-
scenario_beside=-
scenario_motion=still
second_pane=
for row in "${scenarios[@]}"; do
    IFS=$'\t' read -r tag first second third fourth fifth sixth seventh eighth ninth tenth eleventh <<<"$row"
    case $tag in
        H)
            [[ -z $active_identifier ]] || {
                echo "malformed manifest stream: scenario '$active_identifier' has no completion" >&2
                exit 2
            }
            source=visible
            scenario_size=$eighth
            scenario_beside=$ninth
            scenario_motion=$tenth
            run_scenario "$first" "$second" "$third" "$fourth" "$fifth" "$sixth" "$seventh"
            active_identifier=$first
            active_exit=$fourth
            ;;
        S)
            [[ -n $active_identifier ]] || { echo "malformed manifest step" >&2; exit 2; }
            mapfile -d $'\x1f' -t keys < <(decoded "$first")
            fixture_text=${BZZ_HERDR_FIXTURE:-}
            if ((sixth > 0)); then
                sleep "$(printf '%d.%03d' $((sixth / 1000)) $((sixth % 1000)))"
            fi
            # v0.16: `pane = "second"`, a `size` set on the pane's terminal
            # (its foreground program gets SIGWINCH), a bracketed paste.
            target=$pane
            [[ $tenth != second ]] || target=$second_pane
            if [[ $eighth != - ]]; then
                stty -F "$(cat "$root/tty-$tenth")" cols "${eighth%x*}" rows "${eighth#*x}"
            fi
            run=$(decoded "${eleventh:-}")
            if [[ -n $run ]]; then
                BZZ_BIN=$binary bash -c "${run//\{fixture\}/$fixture_text}" || {
                    echo "scenario '$active_identifier': \`$run\` failed" >&2
                    exit 1
                }
            fi
            paste=$(decoded "$ninth")
            if [[ -n $paste ]]; then
                # The one use of `send-text`: a paste is text, not keys. The
                # brackets are what a terminal sends around a paste.
                herdr pane send-text "$target" $'\e[200~'"$paste"$'\e[201~' >/dev/null
            fi
            # Otherwise never `send-text`: this is key-event coverage.
            # Herdr takes a bounded number of keys at a time.
            for ((index = 0; index < ${#keys[@]}; index += 40)); do
                ((${#keys[@]} > 0)) || break
                herdr pane send-keys "$target" "${keys[@]:index:40}" >/dev/null
            done
            [[ $fifth == regex ]] || second=${second//\{fixture\}/$fixture_text}
            herdr pane wait-output "$target" --source "$source" --lines 400 "--$fifth" "$second" --timeout "$third" >/dev/null || {
                echo "scenario '$active_identifier': \"$second\" did not show within $third ms. The pane shows:" >&2
                herdr pane read "$target" --source "$source" --lines 400 >&2 || true
                exit 1
            }
            # What must not be on the screen, such as a secret typed at a
            # prompt that does not echo.
            absent=$(decoded "$fourth")
            if [[ -n $absent ]]; then
                # Read first, then look: a `grep -q` on the pipe ends at the
                # first match, and under `pipefail` that made the match a
                # failure of the reader, so the text was never found. A TUI
                # may take a moment to redraw (a pane that closes), so the
                # text gets five seconds to go; what a command printed stays
                # in the pane's recent output and never goes.
                for attempt in {1..10}; do
                    shown=$(herdr pane read "$target" --source visible; herdr pane read "$target" --source recent-unwrapped --lines 200)
                    [[ $shown == *"$absent"* ]] || break
                    if ((attempt == 10)); then
                        echo "scenario '$active_identifier': \"$absent\" is on the screen and must not be" >&2
                        exit 1
                    fi
                    sleep 0.5
                done
            fi
            printf 'scenario %s observed %s\n' "$active_identifier" "$second"
            if [[ $second == "$active_exit" ]]; then
                active_identifier=
                active_exit=
            fi
            ;;
        *)
            echo "malformed manifest row" >&2
            exit 2
            ;;
    esac
done
[[ -z $active_identifier ]] || {
    echo "scenario '$active_identifier' did not produce its expected exit marker" >&2
    exit 1
}
printf 'Herdr %s acceptance scenarios passed.\n' "$selection"
