# shephard

`shephard` is a Rust CLI/TUI for syncing many Git repositories in one run.

It scans configured roots for repos, lets you pick targets interactively (or pass them non-interactively), and runs a simple workflow:

1. `git pull --ff-only`
2. optional commit
3. optional push

## Status

Current version: `0.1.2`

The project is functional, test-covered, and packaged for Arch via `dist/arch/PKGBUILD`.

## Features

- Recursive repository discovery from configured roots
- Interactive `ratatui` multi-select flow (persisted repo selection)
- Non-interactive mode for scripts/automation
- Per-run overrides via CLI or interactive prompts
- Tracked-only or include-untracked commit scope
- Side-channel sync mode that avoids polluting the current branch
- Manual side-channel apply (`merge`, `cherry-pick`, `squash`)
- Per-repo failure isolation with final summary

## Install / Build

```bash
cargo build --release
```

Run directly:

```bash
cargo run -- run
```

## CLI

Top-level commands:

- `shephard run`
- `shephard apply`

Run flags:

- `--non-interactive`
- `--repos <PATH>...`
- `--pull-only`
- `--push`
- `--include-untracked`
- `--tracked-only`
- `--side-channel`
- `--no-side-channel`
- `--roots <PATH>...`

Apply flags:

- `--repo <PATH>`
- `--method merge|cherry-pick|squash`

## Configuration

Config file path:

- `~/.config/shephard/config.toml`

If no config exists, shephard uses built-in defaults.

All keys are optional. Example:

```toml
workspace_roots = ["/home/you/projects", "/home/you/code"]
default_mode = "sync_all" # or "pull_only"
push_enabled = true
include_untracked = false
descend_hidden_dirs = false
failure_policy = "continue"

[side_channel]
enabled = false
remote_name = "shephard"
branch_name = "shephard/sync"

[commit]
message_template = "shephard sync: {timestamp} {hostname} [{scope}]"

[tui]
persist_selection = true
```

Resolution order:

1. built-in defaults
2. config file values
3. current run overrides (CLI/TUI)

## Side-channel mode

Side-channel mode lets shephard capture your local changes and push them to a dedicated remote branch without creating commits on your current branch.

### Why it exists

It solves the "sync my in-progress work to another machine/back-up location, but don't pollute my current branch history yet" use case.

### Exact sync flow (`shephard run` with side-channel enabled)

For each selected repo, shephard does this:

1. Runs `git pull --ff-only` first (same as normal mode).
2. Verifies the side-channel remote exists, then fetches it with `--prune`.
3. Creates a temporary Git index file and sets `GIT_INDEX_FILE` to it.
4. Loads `HEAD` into that temporary index with `git read-tree HEAD`.
5. Stages into the temporary index from your working tree:
6. Uses `git add -u` when `include_untracked = false`.
7. Uses `git add -A` when `include_untracked = true`.
8. Checks `git diff --cached --quiet` (against the temporary index). If nothing changed, it reports no-op.
9. Writes the tree with `git write-tree`.
10. Resolves current side-branch tip (`<remote>/<branch>`) as parent if it exists.
11. Creates a commit object with `git commit-tree` (without moving local `HEAD`).
12. Pushes that commit hash directly to `<remote>:<branch>`.

### What side-channel mode changes vs does not change

1. Changes:
2. The side-channel remote branch advances with a new commit when there are local changes.
3. Does not change:
4. Your current branch `HEAD`.
5. Your real Git index.
6. Your working tree files.

### Applying side-channel commits later (`shephard apply`)

`shephard apply` fetches the side-channel branch, then applies it to your current branch using one method:

1. `merge`: `git merge --ff-only <remote>/<branch>`
2. `cherry-pick`: cherry-picks the side branch tip commit
3. `squash`: `git merge --squash <remote>/<branch>` (staged changes, no commit yet)

### Common failure cases

1. Missing side-channel remote (`side_channel.remote_name`) in a repo.
2. Non-fast-forward pull failure before sync (`git pull --ff-only`).
3. Push rejection if side branch advanced concurrently and your local computed parent is stale.

## Exit codes

- `0`: all selected repos succeeded or no-op
- `1`: at least one selected repo failed
- `2`: startup/config/usage failure

## Testing

```bash
cargo test
```

Integration tests create temporary repos/remotes under `/tmp` and validate:

- discovery
- ff-only pull behavior
- tracked/untracked commit scope
- no-op commit path
- continue-on-failure
- side-channel behavior
- apply merge/cherry-pick/squash behavior

## Arch packaging (`dist/arch/PKGBUILD`)

Arch packaging files live under `dist/arch/`.

Build from that directory (or from your CI runner's clean staging directory), not from repo root:

```bash
cd dist/arch
makepkg -Ccf
```

## Source map

- `src/main.rs`: app entrypoint + command routing
- `src/cli.rs`: clap CLI definitions
- `src/config.rs`: config/defaults/validation + run-time resolution
- `src/discovery.rs`: recursive Git repo discovery
- `src/tui.rs`: interactive ratatui selection + per-run prompts
- `src/workflow.rs`: per-repo sync orchestration
- `src/git.rs`: git subprocess operations
- `src/apply.rs`: side-channel apply flow
- `src/state.rs`: persisted selection state
- `src/report.rs`: run summary + exit code mapping
- `tests/integration_behaviors.rs`: integration coverage across git workflows
