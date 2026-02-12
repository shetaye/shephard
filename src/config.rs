use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SideChannelConfig {
    pub enabled: bool,
    pub remote_name: String,
    pub branch_name: String,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ResolvedRepositorySideChannelConfig {
    pub enabled: Option<bool>,
    pub remote_name: Option<String>,
    pub branch_name: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedRepositoryConfig {
    pub path: PathBuf,
    pub enabled: bool,
    pub include_untracked: Option<bool>,
    pub side_channel: ResolvedRepositorySideChannelConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub default_mode: RunMode,
    pub push_enabled: bool,
    pub include_untracked: bool,
    pub side_channel: SideChannelConfig,
    pub commit_template: String,
    pub failure_policy: FailurePolicy,
    pub repositories: Vec<ResolvedRepositoryConfig>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedRunConfig {
    pub push_enabled: bool,
    pub include_untracked: bool,
    pub side_channel: SideChannelConfig,
    pub commit_template: String,
    pub failure_policy: FailurePolicy,
}

#[derive(Debug, Deserialize, Default)]
struct PartialConfig {
    default_mode: Option<RunMode>,
    push_enabled: Option<bool>,
    include_untracked: Option<bool>,
    side_channel: Option<PartialSideChannelConfig>,
    commit: Option<PartialCommitConfig>,
    failure_policy: Option<FailurePolicy>,
    repositories: Option<Vec<PartialRepositoryConfig>>,
}

#[derive(Debug, Deserialize, Default)]
struct PartialRepositoryConfig {
    path: PathBuf,
    enabled: Option<bool>,
    include_untracked: Option<bool>,
    side_channel: Option<PartialSideChannelConfig>,
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
    if let Some(template) = parsed.commit.and_then(|commit| commit.message_template) {
        cfg.commit_template = template;
    }
    if let Some(policy) = parsed.failure_policy {
        cfg.failure_policy = policy;
    }
    if let Some(repositories) = parsed.repositories {
        let config_dir = path
            .parent()
            .context("unable to determine parent directory for config file")?;
        cfg.repositories = resolve_repositories(repositories, config_dir)?;
    }

    validate(&cfg)?;
    Ok(cfg)
}

pub fn resolve_run_config(base: &ResolvedConfig, args: &RunArgs) -> Result<ResolvedRunConfig> {
    validate_run_args(args)?;

    let mut mode = base.default_mode;
    if args.pull_only {
        mode = RunMode::PullOnly;
    }
    if args.push {
        mode = RunMode::SyncAll;
    }

    let push_enabled = match mode {
        RunMode::PullOnly => false,
        RunMode::SyncAll => base.push_enabled,
    };

    let mut resolved = ResolvedRunConfig {
        push_enabled,
        include_untracked: base.include_untracked,
        side_channel: base.side_channel.clone(),
        commit_template: base.commit_template.clone(),
        failure_policy: base.failure_policy,
    };
    apply_cli_overrides(&mut resolved, args);

    Ok(resolved)
}

pub fn resolve_repo_run_config(
    base: &ResolvedRunConfig,
    args: &RunArgs,
    repo: &ResolvedRepositoryConfig,
) -> ResolvedRunConfig {
    let mut resolved = base.clone();
    apply_repo_overrides(&mut resolved, repo);
    apply_cli_overrides(&mut resolved, args);
    resolved
}

pub fn enabled_repositories(config: &ResolvedConfig) -> Vec<ResolvedRepositoryConfig> {
    config
        .repositories
        .iter()
        .filter(|repo| repo.enabled)
        .cloned()
        .collect()
}

pub fn resolve_apply_side_channel(config: &ResolvedConfig, repo: &Path) -> SideChannelConfig {
    let repo_key = canonical_repo_key(repo);

    for configured in &config.repositories {
        if canonical_repo_key(&configured.path) == repo_key {
            let mut side_channel = config.side_channel.clone();
            apply_repo_side_channel_overrides(&mut side_channel, &configured.side_channel);
            return side_channel;
        }
    }

    config.side_channel.clone()
}

pub fn canonical_repo_key(path: &Path) -> String {
    canonicalize_repo_path(path).to_string_lossy().to_string()
}

fn validate_run_args(args: &RunArgs) -> Result<()> {
    if args.pull_only && args.push {
        bail!("--pull-only and --push cannot be used together");
    }
    if args.include_untracked && args.tracked_only {
        bail!("--include-untracked and --tracked-only cannot be used together");
    }
    if args.side_channel && args.no_side_channel {
        bail!("--side-channel and --no-side-channel cannot be used together");
    }
    Ok(())
}

fn apply_repo_overrides(config: &mut ResolvedRunConfig, repo: &ResolvedRepositoryConfig) {
    if let Some(include_untracked) = repo.include_untracked {
        config.include_untracked = include_untracked;
    }
    apply_repo_side_channel_overrides(&mut config.side_channel, &repo.side_channel);
}

fn apply_repo_side_channel_overrides(
    side_channel: &mut SideChannelConfig,
    overrides: &ResolvedRepositorySideChannelConfig,
) {
    if let Some(enabled) = overrides.enabled {
        side_channel.enabled = enabled;
    }
    if let Some(remote_name) = &overrides.remote_name {
        side_channel.remote_name = remote_name.clone();
    }
    if let Some(branch_name) = &overrides.branch_name {
        side_channel.branch_name = branch_name.clone();
    }
}

fn apply_cli_overrides(config: &mut ResolvedRunConfig, args: &RunArgs) {
    if args.include_untracked {
        config.include_untracked = true;
    }
    if args.tracked_only {
        config.include_untracked = false;
    }
    if args.side_channel {
        config.side_channel.enabled = true;
    }
    if args.no_side_channel {
        config.side_channel.enabled = false;
    }
}

fn resolve_repositories(
    partials: Vec<PartialRepositoryConfig>,
    config_dir: &Path,
) -> Result<Vec<ResolvedRepositoryConfig>> {
    let mut resolved = Vec::new();
    let mut seen_keys = BTreeSet::new();

    for (idx, partial) in partials.into_iter().enumerate() {
        if partial.path.as_os_str().is_empty() {
            bail!("repositories[{idx}].path cannot be empty");
        }

        let resolved_path = if partial.path.is_absolute() {
            partial.path.clone()
        } else {
            config_dir.join(&partial.path)
        };
        let canonical_path = canonicalize_repo_path(&resolved_path);
        let key = canonical_repo_key(&canonical_path);
        if !seen_keys.insert(key) {
            bail!(
                "repositories[{idx}] duplicates repository path {}",
                partial.path.display()
            );
        }

        let side_channel = if let Some(repo_side_channel) = partial.side_channel {
            ResolvedRepositorySideChannelConfig {
                enabled: repo_side_channel.enabled,
                remote_name: repo_side_channel.remote_name,
                branch_name: repo_side_channel.branch_name,
            }
        } else {
            ResolvedRepositorySideChannelConfig::default()
        };

        resolved.push(ResolvedRepositoryConfig {
            path: canonical_path,
            enabled: partial.enabled.unwrap_or(true),
            include_untracked: partial.include_untracked,
            side_channel,
        });
    }

    Ok(resolved)
}

fn canonicalize_repo_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn defaults() -> ResolvedConfig {
    ResolvedConfig {
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
        repositories: Vec::new(),
    }
}

fn validate(cfg: &ResolvedConfig) -> Result<()> {
    if cfg.side_channel.remote_name.trim().is_empty() {
        bail!("side_channel.remote_name cannot be empty");
    }
    if cfg.side_channel.branch_name.trim().is_empty() {
        bail!("side_channel.branch_name cannot be empty");
    }
    if cfg.commit_template.trim().is_empty() {
        bail!("commit.message_template cannot be empty");
    }

    let mut seen_keys = BTreeSet::new();
    for (idx, repo) in cfg.repositories.iter().enumerate() {
        if repo.path.as_os_str().is_empty() {
            bail!("repositories[{idx}].path cannot be empty");
        }

        let key = canonical_repo_key(&repo.path);
        if !seen_keys.insert(key) {
            bail!(
                "repositories[{idx}] duplicates repository path {}",
                repo.path.display()
            );
        }

        if repo
            .side_channel
            .remote_name
            .as_ref()
            .is_some_and(|remote_name| remote_name.trim().is_empty())
        {
            bail!("repositories[{idx}].side_channel.remote_name cannot be empty");
        }
        if repo
            .side_channel
            .branch_name
            .as_ref()
            .is_some_and(|branch_name| branch_name.trim().is_empty())
        {
            bail!("repositories[{idx}].side_channel.branch_name cannot be empty");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn pull_only_override_disables_push() {
        let base = defaults();
        let args = RunArgs {
            pull_only: true,
            ..RunArgs::default()
        };

        let resolved = resolve_run_config(&base, &args).expect("resolve should succeed");
        assert_eq!(resolved.push_enabled, false);
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
    fn per_repo_overrides_apply_when_cli_flags_are_absent() {
        let base = defaults();
        let args = RunArgs::default();
        let global = resolve_run_config(&base, &args).expect("resolve should succeed");
        let repo = ResolvedRepositoryConfig {
            path: PathBuf::from("/tmp/repo"),
            enabled: true,
            include_untracked: Some(true),
            side_channel: ResolvedRepositorySideChannelConfig {
                enabled: Some(true),
                remote_name: Some("backup".to_string()),
                branch_name: Some("backup/sync".to_string()),
            },
        };

        let resolved = resolve_repo_run_config(&global, &args, &repo);

        assert_eq!(
            resolved,
            ResolvedRunConfig {
                push_enabled: true,
                include_untracked: true,
                side_channel: SideChannelConfig {
                    enabled: true,
                    remote_name: "backup".to_string(),
                    branch_name: "backup/sync".to_string(),
                },
                commit_template: "shephard sync: {timestamp} {hostname} [{scope}]".to_string(),
                failure_policy: FailurePolicy::Continue,
            }
        );
    }

    #[test]
    fn cli_flags_override_repo_overrides() {
        let base = defaults();
        let args = RunArgs {
            tracked_only: true,
            no_side_channel: true,
            ..RunArgs::default()
        };
        let global = resolve_run_config(&base, &args).expect("resolve should succeed");
        let repo = ResolvedRepositoryConfig {
            path: PathBuf::from("/tmp/repo"),
            enabled: true,
            include_untracked: Some(true),
            side_channel: ResolvedRepositorySideChannelConfig {
                enabled: Some(true),
                ..ResolvedRepositorySideChannelConfig::default()
            },
        };

        let resolved = resolve_repo_run_config(&global, &args, &repo);

        assert_eq!(resolved.include_untracked, false);
        assert_eq!(resolved.side_channel.enabled, false);
    }

    #[test]
    fn apply_side_channel_uses_repo_specific_override() {
        let mut cfg = defaults();
        cfg.repositories = vec![ResolvedRepositoryConfig {
            path: PathBuf::from("/tmp/repo"),
            enabled: true,
            include_untracked: None,
            side_channel: ResolvedRepositorySideChannelConfig {
                enabled: Some(true),
                remote_name: Some("backup".to_string()),
                branch_name: Some("backup/sync".to_string()),
            },
        }];

        let side_channel = resolve_apply_side_channel(&cfg, Path::new("/tmp/repo"));

        assert_eq!(
            side_channel,
            SideChannelConfig {
                enabled: true,
                remote_name: "backup".to_string(),
                branch_name: "backup/sync".to_string(),
            }
        );
    }
}
