use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::Local;

use crate::config::SideChannelConfig;

pub enum SideChannelSyncResult {
    Pushed,
    NoChanges,
}

pub fn pull_ff_only(repo: &Path) -> Result<()> {
    run_git(repo, &["pull", "--ff-only"]).map(|_| ())
}

pub fn side_channel_preflight(repo: &Path, side: &SideChannelConfig) -> Result<()> {
    ensure_remote_exists(repo, &side.remote_name)?;
    run_git(repo, &["fetch", &side.remote_name, "--prune"]).map(|_| ())
}

pub fn stage_changes(repo: &Path, include_untracked: bool) -> Result<()> {
    if include_untracked {
        run_git(repo, &["add", "-A"]).map(|_| ())
    } else {
        run_git(repo, &["add", "-u"]).map(|_| ())
    }
}

pub fn has_staged_changes(repo: &Path) -> Result<bool> {
    has_staged_changes_with_env(repo, &[])
}

pub fn commit(repo: &Path, message: &str) -> Result<()> {
    run_git(repo, &["commit", "-m", message]).map(|_| ())
}

pub fn push(repo: &Path) -> Result<()> {
    run_git(repo, &["push"]).map(|_| ())
}

pub fn side_channel_sync(
    repo: &Path,
    side: &SideChannelConfig,
    include_untracked: bool,
    message: &str,
) -> Result<SideChannelSyncResult> {
    ensure_remote_exists(repo, &side.remote_name)?;

    let temp_index = tempfile::NamedTempFile::new().context("failed to allocate temp git index")?;
    let index_path = temp_index.path().to_string_lossy().to_string();
    let env = [("GIT_INDEX_FILE", index_path.as_str())];

    run_git_with_env(repo, &["read-tree", "HEAD"], &env)?;
    if include_untracked {
        run_git_with_env(repo, &["add", "-A"], &env)?;
    } else {
        run_git_with_env(repo, &["add", "-u"], &env)?;
    }

    if !has_staged_changes_with_env(repo, &env)? {
        return Ok(SideChannelSyncResult::NoChanges);
    }

    let tree = run_git_with_env(repo, &["write-tree"], &env)?
        .stdout
        .trim()
        .to_string();
    let remote_ref = format!("{}/{}", side.remote_name, side.branch_name);
    let parent = rev_parse_optional(repo, &remote_ref)?;
    let commit_hash = commit_tree(repo, &tree, parent.as_deref(), message)?;

    run_git(
        repo,
        &[
            "push",
            &side.remote_name,
            &format!("{}:{}", commit_hash, side.branch_name),
        ],
    )
    .map(|_| ())?;

    Ok(SideChannelSyncResult::Pushed)
}

pub fn ensure_remote_exists(repo: &Path, remote_name: &str) -> Result<()> {
    run_git(repo, &["remote", "get-url", remote_name])
        .with_context(|| format!("missing side-channel remote '{remote_name}'"))
        .map(|_| ())
}

pub fn generate_commit_message(template: &str, include_untracked: bool) -> String {
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S %z").to_string();
    let host = hostname::get()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let scope = if include_untracked { "all" } else { "tracked" };

    template
        .replace("{timestamp}", &ts)
        .replace("{hostname}", &host)
        .replace("{scope}", scope)
}

pub fn fetch_side_channel(repo: &Path, side: &SideChannelConfig) -> Result<()> {
    ensure_remote_exists(repo, &side.remote_name)?;
    run_git(repo, &["fetch", &side.remote_name, &side.branch_name]).map(|_| ())
}

pub fn merge_side_channel_ff(repo: &Path, side: &SideChannelConfig) -> Result<()> {
    run_git(
        repo,
        &[
            "merge",
            "--ff-only",
            &format!("{}/{}", side.remote_name, side.branch_name),
        ],
    )
    .map(|_| ())
}

pub fn cherry_pick_side_channel_tip(repo: &Path, side: &SideChannelConfig) -> Result<()> {
    let commit = rev_parse(repo, &format!("{}/{}", side.remote_name, side.branch_name))?;
    run_git(repo, &["cherry-pick", commit.trim()]).map(|_| ())
}

pub fn squash_merge_side_channel(repo: &Path, side: &SideChannelConfig) -> Result<()> {
    run_git(
        repo,
        &[
            "merge",
            "--squash",
            &format!("{}/{}", side.remote_name, side.branch_name),
        ],
    )
    .map(|_| ())
}

fn rev_parse(repo: &Path, rev: &str) -> Result<String> {
    let out = run_git(repo, &["rev-parse", rev])?;
    Ok(out.stdout)
}

fn rev_parse_optional(repo: &Path, rev: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", rev])
        .current_dir(repo)
        .output()
        .with_context(|| format!("failed running git rev-parse in {}", repo.display()))?;

    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else {
        Ok(None)
    }
}

fn commit_tree(repo: &Path, tree: &str, parent: Option<&str>, message: &str) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo);
    cmd.arg("commit-tree").arg(tree).arg("-m").arg(message);
    if let Some(parent) = parent {
        cmd.arg("-p").arg(parent);
    }

    let output = cmd
        .output()
        .with_context(|| format!("failed running git commit-tree in {}", repo.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git commit-tree failed in {}: {}",
            repo.display(),
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn has_staged_changes_with_env(repo: &Path, env: &[(&str, &str)]) -> Result<bool> {
    let mut cmd = Command::new("git");
    cmd.args(["diff", "--cached", "--quiet"]).current_dir(repo);
    for (key, value) in env {
        cmd.env(key, value);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed running git diff in {}", repo.display()))?;

    match status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => bail!("git diff --cached --quiet failed in {}", repo.display()),
    }
}

pub struct GitOutput {
    pub stdout: String,
}

fn run_git(repo: &Path, args: &[&str]) -> Result<GitOutput> {
    run_git_with_env(repo, args, &[])
}

fn run_git_with_env(repo: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<GitOutput> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(repo);
    for (key, value) in env {
        cmd.env(key, value);
    }

    let output = cmd
        .output()
        .with_context(|| format!("failed running git {:?} in {}", args, repo.display()))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {:?} failed in {}: {}{}",
            args,
            repo.display(),
            stderr.trim(),
            if stdout.trim().is_empty() {
                "".to_string()
            } else {
                format!(" | {}", stdout.trim())
            }
        );
    }

    Ok(GitOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
    })
}
