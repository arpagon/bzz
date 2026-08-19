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
  BZZ_HERDR_SCENARIOS=automated|all|ID[,ID...]
  BZZ_HERDR_MANIFEST=<path>

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
import sys
import tomllib

path, selected = sys.argv[1:]
with open(path, "rb") as handle:
    document = tomllib.load(handle)
if document.get("version") != 1 or set(document) - {"version", "scenario"}:
    raise SystemExit("unsupported or malformed Herdr scenario manifest")
values = document.get("scenario")
if not isinstance(values, list) or not values:
    raise SystemExit("Herdr scenario manifest contains no scenarios")
wanted = None if selected in {"all", "automated"} else set(selected.split(","))
for scenario in values:
    if set(scenario) - {"id", "kind", "description", "ready", "exit", "keymap", "step"}:
        raise SystemExit("Herdr scenario manifest contains an unknown scenario field")
    identifier = scenario.get("id")
    kind = scenario.get("kind")
    ready = scenario.get("ready")
    exit_marker = scenario.get("exit")
    steps = scenario.get("step")
    if not all(isinstance(value, str) and value for value in (identifier, kind, ready)):
        raise SystemExit("Herdr scenario manifest has invalid scenario metadata")
    if kind not in {"automated", "operator"} or not isinstance(exit_marker, str):
        raise SystemExit("Herdr scenario manifest has an invalid scenario kind")
    if not isinstance(steps, list) or not steps:
        raise SystemExit("Herdr scenario manifest has no steps")
    if wanted is not None and identifier not in wanted:
        continue
    if selected == "automated" and kind != "automated":
        continue
    safe = [identifier, kind, ready, exit_marker, scenario.get("keymap", "")]
    if any("\t" in value or "\n" in value or "\r" in value for value in safe[:-1]):
        raise SystemExit("Herdr scenario manifest contains unsupported control characters")
    import base64
    print("H\t" + "\t".join(safe[:4]) + "\t" + base64.b64encode(safe[4].encode()).decode())
    for step in steps:
        if set(step) != {"keys", "visible"}:
            raise SystemExit("Herdr scenario step must contain only keys and visible")
        keys = step["keys"]
        visible = step["visible"]
        if not isinstance(keys, list) or not keys or not all(isinstance(key, str) and key for key in keys):
            raise SystemExit("Herdr scenario has invalid keys")
        if not isinstance(visible, str) or not visible or any("\t" in key or "\n" in key or "\r" in key for key in keys):
            raise SystemExit("Herdr scenario has invalid visible output")
        if "\t" in visible or "\n" in visible or "\r" in visible:
            raise SystemExit("Herdr scenario has unsupported visible output")
        print("S\t" + ",".join(keys) + "\t" + visible)
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

root=$(mktemp -d "${TMPDIR:-/tmp}/bzz-herdr.XXXXXXXX")
cleanup() {
    rm -rf -- "$root"
}
trap cleanup EXIT INT TERM

run_scenario() {
    local identifier=$1 kind=$2 ready=$3 exit_marker=$4 encoded_keymap=$5
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
    if [[ -n $encoded_keymap ]]; then
        printf '%s' "$encoded_keymap" | base64 --decode >"$config/keymap.toml"
        chmod 600 -- "$config/keymap.toml"
    fi

    # All values are local paths or a validated executable. printf %q creates
    # one shell word per value for the disposable target pane; no credential or
    # fixture value is passed to the process. Every visual wait below is bounded
    # by Herdr, which is portable to the controlled target shell.
    local command
    printf -v command \
        'env BZZ_CONFIG_DIR=%q BZZ_DATA_DIR=%q BZZ_CACHE_DIR=%q %q; status=$?; printf "__BZZ_HERDR_EXIT__:%%s\\n" "$status"' \
        "$config" "$data" "$cache" "$binary"
    # Clear only the disposable terminal surface so a prior visible label
    # cannot satisfy a later readiness predicate. Scrollback is never saved.
    herdr pane run "$pane" "printf '\\033[2J\\033[H'" >/dev/null
    herdr pane run "$pane" "$command" >/dev/null
    herdr pane wait-output "$pane" --source visible --match "$ready" --timeout 15000 >/dev/null
    printf 'scenario %s ready\n' "$identifier"
}

active_identifier=
active_exit=
for row in "${scenarios[@]}"; do
    IFS=$'\t' read -r tag first second third fourth fifth <<<"$row"
    case $tag in
        H)
            [[ -z $active_identifier ]] || {
                echo "malformed manifest stream: scenario '$active_identifier' has no completion" >&2
                exit 2
            }
            run_scenario "$first" "$second" "$third" "$fourth" "$fifth"
            active_identifier=$first
            active_exit=$fourth
            ;;
        S)
            [[ -n $active_identifier ]] || { echo "malformed manifest step" >&2; exit 2; }
            IFS=, read -r -a keys <<<"$first"
            # Deliberately never use `send-text`: this is key-event coverage.
            herdr pane send-keys "$pane" "${keys[@]}" >/dev/null
            herdr pane wait-output "$pane" --source visible --match "$second" --timeout 15000 >/dev/null
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
printf 'Herdr automated acceptance scenarios passed.\n'
