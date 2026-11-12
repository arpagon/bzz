---
name: experiment-mode
description: Use for disposable experiments, spikes, prototypes, reports, and assets under gitignored runs/experiments/ with local Git checkpoints per experiment.
---

# Experiment Mode

## Core Rules

- Use for disposable exploration before a codebase implementation plan exists.
- New experiment artifacts live under `runs/experiments/<product-or-app>/<slug>`.
- Put experiment code, markdown, reports, assets, caches, outputs, screenshots, and videos in that run folder.
- Before creating experiment artifacts, verify that the parent repository ignores `/runs/` with `git check-ignore runs/experiments/.ignore-check`. Add `/runs/` to the parent `.gitignore` if needed. Keep experiment artifacts under that ignored root until promoted.
- Each experiment folder must also be its own local Git repository so disposable work has checkpoints without entering the parent repository.
- Experiment plans may live in `docs/planning/<YYYY-MM-DD>/<slug>.md`; important final reports may also be promoted there when the user wants durable tracked documentation.
- If results imply codebase changes, write a tracked implementation plan before touching product code.
- For product code, use the promotion path: experiment → plan → issue → branch/worktree → PR.
- Keep provisional findings in the experiment folder. Before promoting any result into tracked product code or documentation, summarize it with references, dates, uncertainties, and a clear distinction between evidence and hypotheses, and obtain the user's approval. Agree on the destination with the user.
- Keep each run self-contained with commands, inputs, outputs, and conclusions.

## Host-Local Heavy Artifact Storage

`runs/experiments/<product-or-app>/<slug>` is the logical experiment root, but heavy artifacts should not always live on the same filesystem as the repository. On machines with a fast-but-small workspace and a slower high-capacity disk, use a host-local storage policy so large bytes land on cold storage while symlinks preserve the normal experiment layout.

Before downloading or generating heavy artifacts, check for this optional host-local config file:

```bash
$HOME/.config/experiments/storage.env
```

If it exists, source it before preparing experiment directories:

```bash
if [ -f "$HOME/.config/experiments/storage.env" ]; then
  set -a
  . "$HOME/.config/experiments/storage.env"
  set +a
fi
```

Supported variables:

```bash
EXPERIMENT_STORAGE_ENABLED=1
EXPERIMENT_HOST_ID=<host-short-name>
EXPERIMENT_REPO_ID=<repo-name>
EXPERIMENT_FAST_ROOT=/absolute/path/to/runs/experiments
EXPERIMENT_COLD_ROOT=/absolute/path/to/large/local/archive/experiment-artifacts/<host>/<repo>
EXPERIMENT_HEAVY_THRESHOLD_MB=256
EXPERIMENT_SYMLINK_HEAVY=1
```

Do not hardcode host-specific absolute paths in this tracked skill. Each machine defines its own `EXPERIMENT_COLD_ROOT` in the local config file. If the config file is absent or `EXPERIMENT_STORAGE_ENABLED` is not `1`, keep using the normal `runs/experiments/...` layout.

When host-local storage is enabled, keep lightweight experiment state in the fast workspace and put heavy paths under cold storage:

```bash
PRODUCT="<product-or-app>"
SLUG="example-run"
EXP="runs/experiments/${PRODUCT}/${SLUG}"

mkdir -p "$EXP"

if [ "${EXPERIMENT_STORAGE_ENABLED:-0}" = "1" ]; then
  COLD="${EXPERIMENT_COLD_ROOT}/${PRODUCT}/${SLUG}"
  mkdir -p "$COLD/cache/videos" "$COLD/frames" "$COLD/models" "$EXP/cache"
  ln -s "$COLD/cache/videos" "$EXP/cache/videos"
  ln -s "$COLD/frames" "$EXP/frames"
  ln -s "$COLD/models" "$EXP/models"
fi
```

Treat these as heavy by default:

- Videos: `.mp4`, `.mov`, `.mkv`, `.avi`, `.webm`, `.m4v`, `.m2ts`, `.mpg`, `.mpeg`.
- Model and checkpoint files: `.ckpt`, `.pth`, `.pt`, `.onnx`, `.safetensors`.
- Large local databases: `.db`, `.sqlite`, `.sqlite3`, `.duckdb`.
- Large generated datasets, frame dumps, debug renders, and inference outputs.
- Directories named `video/`, `videos/`, `frames/`, `datasets/`, `models/`, `checkpoints/`, `weights/`, `cache/videos/`, `cache/l1_segments/`, or `outputs/heavy/`.

Keep these in the fast workspace unless they become large: source code, scripts, README files, command notes, manifests, small CSV/JSON summaries, final markdown reports, and intentionally small sample images.

Separate canonical source data from derived experiment artifacts:

- Canonical source data that should be retained (original recordings, datasets) belongs in its permanent archive; the experiment symlinks to it.
- Derived scratch outputs such as trims, reencodes, debug clips, temporary frame datasets, and model training outputs belong under `experiment-artifacts/<host>/<repo>/...` in the configured cold root.

For existing experiment artifacts, do not move or delete anything immediately. First create a dry-run manifest, review it with the user, then move approved batches and replace the original paths with symlinks. The manifest should include source path, destination path, size, category, whether the destination already exists, and proposed action.

## Local Git Repository per Experiment

When creating a new experiment, initialize Git inside the experiment directory:

```bash
mkdir -p runs/experiments/<product-or-app>/<slug>
git -C runs/experiments/<product-or-app>/<slug> init
```

When taking over an existing experiment, check whether it already has a local repo:

```bash
git -C runs/experiments/<product-or-app>/<slug> rev-parse --show-toplevel
```

If that command fails, initialize the local repo before making further changes. Do not initialize Git at `runs/experiments/` unless the user explicitly asks for one shared experiment repository.

## Default Experiment `.gitignore`

Create an experiment-local `.gitignore` before the first commit. Start with this template and adjust only when the experiment needs to version a normally ignored small artifact:

```gitignore
# Dependencies and build outputs
node_modules/
.next/
dist/
build/

# Python environments and bytecode
.venv/
__pycache__/
*.py[cod]

# Local config and secrets
.env
*.env
.secrets/

# Logs, caches, and temporary files
*.log
.cache/
tmp/
temp/

# Heavy artifact directories, usually symlinked to cold storage
video/
videos/
frames/
images/
datasets/
models/
checkpoints/
weights/
cache/videos/
cache/l1_segments/
outputs/heavy/

# Large generated outputs, model artifacts, and local databases
*.mp4
*.mov
*.mkv
*.avi
*.webm
*.m4v
*.m2ts
*.mpg
*.mpeg
*.pt
*.pth
*.ckpt
*.onnx
*.safetensors
*.db
*.sqlite
*.sqlite3
*.duckdb

# Tool-specific caches
.pytest_cache/
.mypy_cache/
.ruff_cache/
```

Commit lightweight source, scripts, READMEs, command notes, configs, and small summary outputs. Keep large videos, caches, model weights, generated databases, and bulky intermediate outputs untracked by default; document their paths in the experiment README instead.

If a normally ignored artifact is intentionally small and important, force-add it only inside the experiment repo:

```bash
git -C runs/experiments/<product-or-app>/<slug> add -f relative/path/to/artifact
```

Never force-add experiment artifacts from the parent repository.

## Checkpointing Workflow

Use explicit `git -C` commands for experiment Git operations so logs make the repository boundary obvious:

```bash
EXP=runs/experiments/<product-or-app>/<slug>
git -C "$EXP" status --short
git -C "$EXP" add README.md scripts/ src/
git -C "$EXP" commit -m "checkpoint: describe the experiment state"
```

Before any experiment commit, verify the Git top-level is the experiment folder, not the parent repository:

```bash
git -C "$EXP" rev-parse --show-toplevel
```

The printed path must end with `runs/experiments/<product-or-app>/<slug>`.

For experiments that copy code from tracked apps or services, commit the unmodified copy first:

```bash
git -C "$EXP" add .
git -C "$EXP" commit -m "chore: add baseline copied sources"
```

Then make experiment-specific changes in later commits. This keeps diffs readable and makes it easy to separate vanilla source from prototype changes.

## Parent Repository Safety

- `git status` at the project root should not show experiment files because `runs/` is ignored.
- Do not run `git add -f runs/...` from the parent repository for experiment artifacts.
- Do not add experiment folders as submodules of the parent repository.
- Do not add a remote to an experiment repo unless the user explicitly asks for backup or sharing.
- If an experiment repo already has a remote, tell the user before pushing.

## Backup and Handoff

For a portable local backup without creating a remote, create a Git bundle inside or near the experiment:

```bash
git -C runs/experiments/<product-or-app>/<slug> bundle create <slug>.bundle --all
```

The bundle can be copied elsewhere and later cloned with:

```bash
git clone <slug>.bundle <slug>-restored
```

## Promotion to Product Code

Do not merge an experiment repo directly into the parent repository. If an experiment produces a useful product change, use the normal promotion path:

1. Summarize the experiment result in the experiment README.
2. Write a tracked implementation plan.
3. Create an issue, branch/worktree, commits, PR, and merge.
4. Use the experiment repo's commits and diffs as reference material, not as the production branch itself.
