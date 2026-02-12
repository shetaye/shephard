use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::{ApplyArgs, ApplyMethodArg};
use crate::config::{self, ResolvedConfig};
use crate::git;

pub fn run(args: &ApplyArgs, config: &ResolvedConfig) -> Result<()> {
    let repo = match &args.repo {
        Some(path) => path.clone(),
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };

    let repo = canonical_repo(&repo)?;
    let side = config::resolve_apply_side_channel(config, &repo);

    git::fetch_side_channel(&repo, &side).with_context(|| {
        format!(
            "failed to fetch side-channel branch {}/{} for {}",
            side.remote_name,
            side.branch_name,
            repo.display()
        )
    })?;

    match args.method {
        ApplyMethodArg::Merge => git::merge_side_channel_ff(&repo, &side)
            .with_context(|| format!("failed to ff-merge into {}", repo.display()))?,
        ApplyMethodArg::CherryPick => git::cherry_pick_side_channel_tip(&repo, &side)
            .with_context(|| format!("failed to cherry-pick into {}", repo.display()))?,
        ApplyMethodArg::Squash => git::squash_merge_side_channel(&repo, &side)
            .with_context(|| format!("failed to squash-merge into {}", repo.display()))?,
    }

    println!(
        "Applied side-channel changes to {} using {:?}",
        repo.display(),
        args.method
    );
    Ok(())
}

fn canonical_repo(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))
}
