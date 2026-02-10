# shephard

`shephard` is a Rust CLI/TUI for syncing many Git repositories in one run.

It scans configured roots for repos, lets you pick targets interactively (or pass them non-interactively), and runs a simple workflow:

1. `git pull --ff-only`
2. optional commit
3. optional push

## Status

Current version: `0.1.1`

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

When side-channel mode is enabled, shephard writes sync commits to a configured side remote/branch, not your working branch.

This is implemented with an isolated temporary Git index and `git commit-tree`, so local branch `HEAD` is unchanged by side-channel sync commits.

Use `shephard apply` to pull those side-channel commits into your current branch manually.

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
