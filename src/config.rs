use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::cli::RunArgs;

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    SyncAll,
    PullOnly,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    Continue,
}

#[derive(Debug, Clone)]
pub struct SideChannelConfig {
    pub enabled: bool,
    pub remote_name: String,
    pub branch_name: String,
}

#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub persist_selection: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub workspace_roots: Vec<PathBuf>,
    pub default_mode: RunMode,
    pub push_enabled: bool,
    pub include_untracked: bool,
    pub side_channel: SideChannelConfig,
    pub commit_template: String,
    pub failure_policy: FailurePolicy,
    pub tui: TuiConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedRunConfig {
    pub workspace_roots: Vec<PathBuf>,
    pub push_enabled: bool,
    pub include_untracked: bool,
    pub side_channel: SideChannelConfig,
    pub commit_template: String,
    pub failure_policy: FailurePolicy,
}

#[derive(Debug, Deserialize, Default)]
struct PartialConfig {
    workspace_roots: Option<Vec<PathBuf>>,
    default_mode: Option<RunMode>,
    push_enabled: Option<bool>,
    include_untracked: Option<bool>,
    side_channel: Option<PartialSideChannelConfig>,
    commit: Option<PartialCommitConfig>,
    failure_policy: Option<FailurePolicy>,
    tui: Option<PartialTuiConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct PartialSideChannelConfig {
    enabled: Option<bool>,
    remote_name: Option<String>,
    branch_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PartialCommitConfig {
    message_template: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PartialTuiConfig {
    persist_selection: Option<bool>,
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("unable to resolve XDG config directory")?;
    Ok(base.join("shephard").join("config.toml"))
}

pub fn load() -> Result<ResolvedConfig> {
    let mut cfg = defaults();
    let path = config_path()?;
    if !path.exists() {
        return Ok(cfg);
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed reading config file at {}", path.display()))?;
    let parsed: PartialConfig = toml::from_str(&raw)
        .with_context(|| format!("failed parsing config file at {}", path.display()))?;

    if let Some(roots) = parsed.workspace_roots {
        cfg.workspace_roots = roots;
    }
    if let Some(mode) = parsed.default_mode {
        cfg.default_mode = mode;
    }
    if let Some(enabled) = parsed.push_enabled {
        cfg.push_enabled = enabled;
    }
    if let Some(include_untracked) = parsed.include_untracked {
        cfg.include_untracked = include_untracked;
    }
    if let Some(side_channel) = parsed.side_channel {
        if let Some(enabled) = side_channel.enabled {
            cfg.side_channel.enabled = enabled;
        }
        if let Some(remote_name) = side_channel.remote_name {
            cfg.side_channel.remote_name = remote_name;
        }
        if let Some(branch_name) = side_channel.branch_name {
            cfg.side_channel.branch_name = branch_name;
        }
    }
    if let Some(commit) = parsed.commit {
        if let Some(template) = commit.message_template {
            cfg.commit_template = template;
        }
    }
    if let Some(policy) = parsed.failure_policy {
        cfg.failure_policy = policy;
    }
    if let Some(tui) = parsed.tui {
        if let Some(persist_selection) = tui.persist_selection {
            cfg.tui.persist_selection = persist_selection;
        }
    }

    validate(&cfg)?;
    Ok(cfg)
}

pub fn resolve_run_config(base: &ResolvedConfig, args: &RunArgs) -> Result<ResolvedRunConfig> {
    if args.pull_only && args.push {
        bail!("--pull-only and --push cannot be used together");
    }
    if args.include_untracked && args.tracked_only {
        bail!("--include-untracked and --tracked-only cannot be used together");
    }
    if args.side_channel && args.no_side_channel {
        bail!("--side-channel and --no-side-channel cannot be used together");
    }

    let mut mode = base.default_mode;
    if args.pull_only {
        mode = RunMode::PullOnly;
    }
    if args.push {
        mode = RunMode::SyncAll;
    }

    let mut include_untracked = base.include_untracked;
    if args.include_untracked {
        include_untracked = true;
    }
    if args.tracked_only {
        include_untracked = false;
    }

    let mut side_channel = base.side_channel.clone();
    if args.side_channel {
        side_channel.enabled = true;
    }
    if args.no_side_channel {
        side_channel.enabled = false;
    }

    let mut workspace_roots = base.workspace_roots.clone();
    if !args.roots.is_empty() {
        workspace_roots = args.roots.clone();
    }

    let push_enabled = match mode {
        RunMode::PullOnly => false,
        RunMode::SyncAll => base.push_enabled,
    };

    Ok(ResolvedRunConfig {
        workspace_roots,
        push_enabled,
        include_untracked,
        side_channel,
        commit_template: base.commit_template.clone(),
        failure_policy: base.failure_policy,
    })
}

fn defaults() -> ResolvedConfig {
    ResolvedConfig {
        workspace_roots: default_workspace_roots(),
        default_mode: RunMode::SyncAll,
        push_enabled: true,
        include_untracked: false,
        side_channel: SideChannelConfig {
            enabled: false,
            remote_name: "shephard".to_string(),
            branch_name: "shephard/sync".to_string(),
        },
        commit_template: "shephard sync: {timestamp} {hostname} [{scope}]".to_string(),
        failure_policy: FailurePolicy::Continue,
        tui: TuiConfig {
            persist_selection: true,
        },
    }
}

fn default_workspace_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("projects"));
        roots.push(home.join("code"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    roots
}

fn validate(cfg: &ResolvedConfig) -> Result<()> {
    if cfg.workspace_roots.is_empty() {
        bail!("workspace_roots cannot be empty");
    }
    if cfg.side_channel.remote_name.trim().is_empty() {
        bail!("side_channel.remote_name cannot be empty");
    }
    if cfg.side_channel.branch_name.trim().is_empty() {
        bail!("side_channel.branch_name cannot be empty");
    }
    if cfg.commit_template.trim().is_empty() {
        bail!("commit.message_template cannot be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_only_override_disables_push() {
        let base = defaults();
        let args = RunArgs {
            pull_only: true,
            ..RunArgs::default()
        };

        let resolved = resolve_run_config(&base, &args).expect("resolve should succeed");
        assert!(!resolved.push_enabled);
    }

    #[test]
    fn conflicting_untracked_flags_fail() {
        let base = defaults();
        let args = RunArgs {
            include_untracked: true,
            tracked_only: true,
            ..RunArgs::default()
        };

        let err = resolve_run_config(&base, &args).expect_err("resolve should fail");
        assert!(
            err.to_string()
                .contains("--include-untracked and --tracked-only")
        );
    }

    #[test]
    fn roots_override_wins() {
        let base = defaults();
        let args = RunArgs {
            roots: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            ..RunArgs::default()
        };

        let resolved = resolve_run_config(&base, &args).expect("resolve should succeed");
        assert_eq!(resolved.workspace_roots, args.roots);
    }
}
