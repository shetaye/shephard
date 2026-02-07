use std::path::{Path, PathBuf};

use crate::config::{FailurePolicy, ResolvedRunConfig};
use crate::git;

#[derive(Debug, Clone)]
pub enum RepoStatus {
    Success,
    NoOp,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RepoResult {
    pub repo: PathBuf,
    pub status: RepoStatus,
    pub message: String,
}

pub fn run(repos: &[PathBuf], cfg: &ResolvedRunConfig) -> Vec<RepoResult> {
    let mut results = Vec::new();

    for repo in repos {
        let outcome = run_repo(repo, cfg);
        let failed = matches!(outcome.status, RepoStatus::Failed);
        results.push(outcome);

        if failed && !matches!(cfg.failure_policy, FailurePolicy::Continue) {
            break;
        }
    }

    results
}

fn run_repo(repo: &Path, cfg: &ResolvedRunConfig) -> RepoResult {
    if let Err(err) = git::pull_ff_only(repo) {
        return RepoResult {
            repo: repo.to_path_buf(),
            status: RepoStatus::Failed,
            message: format!("pull failed: {err:#}"),
        };
    }

    if !cfg.push_enabled {
        return RepoResult {
            repo: repo.to_path_buf(),
            status: RepoStatus::Success,
            message: "pull ok".to_string(),
        };
    }

    if cfg.side_channel.enabled {
        if let Err(err) = git::side_channel_preflight(repo, &cfg.side_channel) {
            return RepoResult {
                repo: repo.to_path_buf(),
                status: RepoStatus::Failed,
                message: format!("side-channel setup failed: {err:#}"),
            };
        }

        let message = git::generate_commit_message(&cfg.commit_template, cfg.include_untracked);
        return match git::side_channel_sync(
            repo,
            &cfg.side_channel,
            cfg.include_untracked,
            &message,
        ) {
            Ok(git::SideChannelSyncResult::Pushed) => RepoResult {
                repo: repo.to_path_buf(),
                status: RepoStatus::Success,
                message: "pull ok, side-channel commit pushed".to_string(),
            },
            Ok(git::SideChannelSyncResult::NoChanges) => RepoResult {
                repo: repo.to_path_buf(),
                status: RepoStatus::NoOp,
                message: "pull ok, no local changes to commit".to_string(),
            },
            Err(err) => RepoResult {
                repo: repo.to_path_buf(),
                status: RepoStatus::Failed,
                message: format!("side-channel sync failed: {err:#}"),
            },
        };
    }

    if let Err(err) = git::stage_changes(repo, cfg.include_untracked) {
        return RepoResult {
            repo: repo.to_path_buf(),
            status: RepoStatus::Failed,
            message: format!("stage failed: {err:#}"),
        };
    }

    let has_changes = match git::has_staged_changes(repo) {
        Ok(value) => value,
        Err(err) => {
            return RepoResult {
                repo: repo.to_path_buf(),
                status: RepoStatus::Failed,
                message: format!("failed to inspect staged diff: {err:#}"),
            };
        }
    };

    if has_changes {
        let message = git::generate_commit_message(&cfg.commit_template, cfg.include_untracked);
        if let Err(err) = git::commit(repo, &message) {
            return RepoResult {
                repo: repo.to_path_buf(),
                status: RepoStatus::Failed,
                message: format!("commit failed: {err:#}"),
            };
        }
    }

    let push_result = git::push(repo);

    if let Err(err) = push_result {
        return RepoResult {
            repo: repo.to_path_buf(),
            status: RepoStatus::Failed,
            message: format!("push failed: {err:#}"),
        };
    }

    if has_changes {
        RepoResult {
            repo: repo.to_path_buf(),
            status: RepoStatus::Success,
            message: "pull ok, committed, pushed".to_string(),
        }
    } else {
        RepoResult {
            repo: repo.to_path_buf(),
            status: RepoStatus::NoOp,
            message: "pull ok, no local changes to commit".to_string(),
        }
    }
}
