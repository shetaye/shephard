use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::Local;

use crate::config::SideChannelConfig;

pub enum SideChannelSyncResult {
    Pushed,
    NoChanges,
}

enum SideChannelPushResult {
    Pushed,
    NonFastForward,
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

    // Use a temporary index file so side-channel commits are produced from a
    // detached index snapshot instead of mutating/staging in the real worktree.
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

    let local_tree = run_git_with_env(repo, &["write-tree"], &env)?
        .stdout
        .trim()
        .to_string();
    let local_head = rev_parse(repo, "HEAD")?.trim().to_string();
    let remote_ref = format!("{}/{}", side.remote_name, side.branch_name);
    let destination_ref = if side.branch_name.starts_with("refs/") {
        side.branch_name.clone()
    } else {
        format!("refs/heads/{}", side.branch_name)
    };
    let mut did_retry = false;
    loop {
        let side_tip = rev_parse_optional(repo, &remote_ref)?;
        let parent = if let Some(parent) = &side_tip {
            parent.clone()
        } else {
            local_head.clone()
        };
        let tree =
            merge_side_tip_into_snapshot(repo, &local_head, &local_tree, side_tip.as_deref())?;
        // Build a commit object directly from the temporary tree so HEAD stays put.
        let commit_hash = commit_tree(repo, &tree, Some(parent.as_str()), message)?;

        match push_side_channel_commit(repo, side, &destination_ref, &commit_hash)? {
            SideChannelPushResult::Pushed => return Ok(SideChannelSyncResult::Pushed),
            SideChannelPushResult::NonFastForward if !did_retry => {
                fetch_side_channel(repo, side)?;
                did_retry = true;
            }
            SideChannelPushResult::NonFastForward => {
                bail!("side-channel push rejected after retry because branch advanced concurrently")
            }
        }
    }
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

fn merge_side_tip_into_snapshot(
    repo: &Path,
    local_head: &str,
    local_tree: &str,
    side_tip: Option<&str>,
) -> Result<String> {
    let Some(side_tip) = side_tip else {
        return Ok(local_tree.to_string());
    };

    if is_ancestor(repo, side_tip, local_head)? {
        return Ok(local_tree.to_string());
    }

    let base = merge_base(repo, local_head, side_tip)?;
    let local_commit = commit_tree(
        repo,
        local_tree,
        Some(local_head),
        "shephard side-channel local snapshot",
    )?;

    let output = Command::new("git")
        .args([
            "merge-tree",
            "--write-tree",
            "--merge-base",
            &base,
            &local_commit,
            side_tip,
        ])
        .current_dir(repo)
        .output()
        .with_context(|| format!("failed running git merge-tree in {}", repo.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let conflicts = conflict_paths_from_merge_tree_output(&stdout);
        if !conflicts.is_empty() {
            bail!(
                "side-channel merge conflict while combining local changes with remote tip {}: {}",
                side_tip,
                conflicts.join(", ")
            );
        }
        bail!(
            "git merge-tree failed in {} while combining local changes with remote tip {}: {} {}",
            repo.display(),
            side_tip,
            stderr.trim(),
            stdout.trim()
        );
    }

    match stdout.lines().next().map(str::trim) {
        Some(tree) if !tree.is_empty() => Ok(tree.to_string()),
        _ => bail!(
            "git merge-tree returned no tree for remote tip {} in {}",
            side_tip,
            repo.display()
        ),
    }
}

fn merge_base(repo: &Path, left: &str, right: &str) -> Result<String> {
    Ok(run_git(repo, &["merge-base", left, right])?
        .stdout
        .trim()
        .to_string())
}

fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(repo)
        .output()
        .with_context(|| format!("failed running git merge-base in {}", repo.display()))?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "git merge-base --is-ancestor failed in {}: {}",
                repo.display(),
                stderr.trim()
            )
        }
    }
}

fn conflict_paths_from_merge_tree_output(output: &str) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for line in output.lines() {
        if let Some((_, path)) = line.split_once('\t') {
            paths.insert(path.to_string());
        }
    }
    paths.into_iter().collect()
}

fn push_side_channel_commit(
    repo: &Path,
    side: &SideChannelConfig,
    destination_ref: &str,
    commit_hash: &str,
) -> Result<SideChannelPushResult> {
    let output = Command::new("git")
        .args([
            "push",
            &side.remote_name,
            &format!("{commit_hash}:{destination_ref}"),
        ])
        .current_dir(repo)
        .output()
        .with_context(|| format!("failed running git push in {}", repo.display()))?;

    if output.status.success() {
        return Ok(SideChannelPushResult::Pushed);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stderr}\n{stdout}");
    if combined.contains("non-fast-forward") || combined.contains("[rejected]") {
        return Ok(SideChannelPushResult::NonFastForward);
    }

    bail!("git push failed in {}: {}", repo.display(), combined.trim())
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
