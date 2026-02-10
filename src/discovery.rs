use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone)]
pub struct Repo {
    pub path: PathBuf,
}

pub fn discover_repositories(roots: &[PathBuf], descend_hidden_dirs: bool) -> Result<Vec<Repo>> {
    let mut found = BTreeSet::new();

    for root in roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| should_descend(entry, descend_hidden_dirs))
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

fn should_descend(entry: &DirEntry, descend_hidden_dirs: bool) -> bool {
    if entry.file_name() == ".git" {
        return false;
    }

    if descend_hidden_dirs || entry.depth() == 0 {
        return true;
    }

    !is_hidden(entry)
}

fn is_hidden(entry: &DirEntry) -> bool {
    if let Some(name) = entry.file_name().to_str() {
        return name.starts_with('.');
    }
    false
}

fn is_git_repository(path: &Path) -> bool {
    let git_dir = path.join(".git");
    git_dir.is_dir() || git_dir.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    #[test]
    fn hidden_directories_are_skipped_when_disabled() {
        let temp = tempfile::tempdir().expect("tempdir should work");
        let root = temp.path();

        let visible_repo = root.join("visible");
        let hidden_repo = root.join(".hidden").join("repo");

        init_fake_repo(&visible_repo);
        init_fake_repo(&hidden_repo);

        let discovered =
            discover_repositories(&[root.to_path_buf()], false).expect("discovery should work");
        let discovered_paths: Vec<PathBuf> = discovered.into_iter().map(|repo| repo.path).collect();
        let expected = vec![
            visible_repo
                .canonicalize()
                .expect("visible canonical path should exist"),
        ];

        assert_eq!(discovered_paths, expected);
    }

    #[test]
    fn hidden_directories_are_descended_when_enabled() {
        let temp = tempfile::tempdir().expect("tempdir should work");
        let root = temp.path();

        let visible_repo = root.join("visible");
        let hidden_repo = root.join(".hidden").join("repo");

        init_fake_repo(&visible_repo);
        init_fake_repo(&hidden_repo);

        let discovered =
            discover_repositories(&[root.to_path_buf()], true).expect("discovery should work");
        let discovered_paths: Vec<PathBuf> = discovered.into_iter().map(|repo| repo.path).collect();
        let expected = vec![
            hidden_repo
                .canonicalize()
                .expect("hidden canonical path should exist"),
            visible_repo
                .canonicalize()
                .expect("visible canonical path should exist"),
        ];

        assert_eq!(discovered_paths, expected);
    }

    fn init_fake_repo(path: &Path) {
        fs::create_dir_all(path.join(".git")).expect("repo marker creation should work");
    }
}
