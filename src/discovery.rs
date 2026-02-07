use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone)]
pub struct Repo {
    pub path: PathBuf,
}

pub fn discover_repositories(roots: &[PathBuf]) -> Result<Vec<Repo>> {
    let mut found = BTreeSet::new();

    for root in roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(should_descend)
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_dir() {
                continue;
            }

            let candidate = entry.path();
            if is_git_repository(candidate) {
                let canonical = candidate
                    .canonicalize()
                    .unwrap_or_else(|_| candidate.to_path_buf());
                found.insert(canonical);
            }
        }
    }

    let repos = found.into_iter().map(|path| Repo { path }).collect();
    Ok(repos)
}

fn should_descend(entry: &DirEntry) -> bool {
    entry.file_name() != ".git"
}

fn is_git_repository(path: &Path) -> bool {
    let git_dir = path.join(".git");
    git_dir.is_dir() || git_dir.is_file()
}
