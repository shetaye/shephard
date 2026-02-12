# shephard

`shephard` is a Rust CLI for syncing many Git repositories in one run.

Repositories are configured declaratively in `~/.config/shephard/config.toml`.
`shephard run` uses that config and runs a simple workflow:

1. `git pull --ff-only`
2. optional commit
3. optional push

## Status

Current version: `0.1.4`

The project is functional, test-covered, and packaged for Arch via `dist/arch/PKGBUILD`.

## Features

- TOML-driven repository selection (`[[repositories]]`)
- Non-interactive execution suitable for scripts/automation
- Per-run CLI overrides
- Per-repository overrides for untracked scope and side-channel settings
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

View help as a man page (when installed by package manager):

```bash
man shephard
```

## CLI

Top-level commands:

- `shephard run`
- `shephard apply`

Run flags:

- `--non-interactive` (accepted for compatibility; no effect)
- `--repos <PATH>...` (filter configured repositories)
- `--pull-only`
- `--push`
- `--include-untracked`
- `--tracked-only`
- `--side-channel`
- `--no-side-channel`

Apply flags:

- `--repo <PATH>`
- `--method merge|cherry-pick|squash`

## Configuration

Config file path:

- `~/.config/shephard/config.toml`

If no config exists, shephard uses built-in defaults.

All keys are optional. Example:

```toml
default_mode = "sync_all" # or "pull_only"
push_enabled = true
include_untracked = false
failure_policy = "continue"

[side_channel]
enabled = false
remote_name = "shephard"
branch_name = "shephard/sync"

[commit]
message_template = "shephard sync: {timestamp} {hostname} [{scope}]"

[[repositories]]
path = "/home/you/projects/repo-a"
enabled = true
include_untracked = false

[repositories.side_channel]
enabled = true
remote_name = "shephard"
branch_name = "shephard/sync"

[[repositories]]
path = "/home/you/code/repo-b"
enabled = true
```

Resolution order:

1. built-in defaults
2. global config values
3. per-repository config values
4. current run CLI overrides

Notes:

- `shephard run` operates only on configured repositories.
- Without `--repos`, all configured `enabled = true` repositories are processed.
- With `--repos`, only matching configured repositories are processed; unknown paths are skipped.

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
5. Stages into the temporary index from your working tree.
6. Uses `git add -u` when `include_untracked = false`.
7. Uses `git add -A` when `include_untracked = true`.
8. Checks `git diff --cached --quiet` (against the temporary index). If nothing changed, it reports no-op.
9. Writes the local snapshot tree with `git write-tree`.
10. If a side-branch tip exists and is not already contained in local `HEAD`, performs a virtual 3-way apply (`git merge-tree --write-tree`).
11. If virtual apply has conflicts, sync fails and reports conflicting paths.
12. Creates a commit object with `git commit-tree` (without moving local `HEAD`), using side tip as parent when present.
13. Pushes that commit hash directly to `<remote>:<branch>`.
14. If push is rejected non-fast-forward, fetches side channel, recomputes once, and retries push.

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

## Exit codes

- `0`: all selected repos succeeded or no-op
- `1`: at least one selected repo failed
- `2`: startup/config/usage failure

## Testing

```bash
cargo test
```

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
- `src/discovery.rs`: repository discovery utilities and tests
- `src/workflow.rs`: per-repo sync orchestration
- `src/git.rs`: git subprocess operations
- `src/apply.rs`: side-channel apply flow
- `src/report.rs`: run summary + exit code mapping
- `tests/integration_behaviors.rs`: integration coverage across git workflows
- `docs/man/shephard.1`: manual page (`man shephard`)
