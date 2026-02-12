use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use shephard::{apply, config, report, workflow};

use shephard::cli::{Cli, Command, RunArgs};
use shephard::config::ResolvedRepositoryConfig;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("Error: {err:#}");
            2
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<i32> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run(RunArgs::default())) {
        Command::Run(args) => run_sync(&args),
        Command::Apply(args) => {
            let cfg = config::load()?;
            apply::run(&args, &cfg)?;
            Ok(0)
        }
    }
}

fn run_sync(args: &RunArgs) -> Result<i32> {
    let cfg = config::load()?;
    let base_run_cfg = config::resolve_run_config(&cfg, args)?;

    let enabled_repositories = config::enabled_repositories(&cfg);
    let selected_repositories =
        resolve_configured_targets(args, &enabled_repositories, &cfg.repositories);

    if selected_repositories.is_empty() {
        println!("No repositories selected.");
        return Ok(0);
    }

    let mut run_targets = Vec::new();
    for repo in selected_repositories {
        if !is_git_repo(&repo.path) {
            eprintln!(
                "Skipping {} because it is not a git repository",
                repo.path.display()
            );
            continue;
        }

        let run_cfg = config::resolve_repo_run_config(&base_run_cfg, args, &repo);
        run_targets.push((repo.path.clone(), run_cfg));
    }

    if run_targets.is_empty() {
        println!("No repositories selected.");
        return Ok(0);
    }

    let results = workflow::run_with_repo_configs(&run_targets);
    report::print_run_summary(&results);

    Ok(report::exit_code(&results))
}

fn resolve_configured_targets(
    args: &RunArgs,
    enabled_repositories: &[ResolvedRepositoryConfig],
    all_repositories: &[ResolvedRepositoryConfig],
) -> Vec<ResolvedRepositoryConfig> {
    if args.repos.is_empty() {
        return enabled_repositories.to_vec();
    }

    let configured_keys: BTreeSet<String> = all_repositories
        .iter()
        .map(|repo| config::canonical_repo_key(&repo.path))
        .collect();
    let enabled_by_key: BTreeMap<String, ResolvedRepositoryConfig> = enabled_repositories
        .iter()
        .cloned()
        .map(|repo| (config::canonical_repo_key(&repo.path), repo))
        .collect();

    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();

    for path in &args.repos {
        let key = config::canonical_repo_key(path);
        if !seen.insert(key.clone()) {
            continue;
        }

        if let Some(repo) = enabled_by_key.get(&key) {
            selected.push(repo.clone());
            continue;
        }

        if configured_keys.contains(&key) {
            eprintln!(
                "Skipping {} because it is disabled in config",
                path.display()
            );
        } else {
            eprintln!("Skipping {} because it is not configured", path.display());
        }
    }

    selected
}

fn is_git_repo(path: &Path) -> bool {
    let git_marker = path.join(".git");
    git_marker.is_dir() || git_marker.is_file()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;
    use shephard::config::ResolvedRepositorySideChannelConfig;

    #[test]
    fn resolve_targets_defaults_to_enabled_repositories() {
        let args = RunArgs::default();
        let all = vec![
            repo_config("/tmp/repo-a", true),
            repo_config("/tmp/repo-b", false),
            repo_config("/tmp/repo-c", true),
        ];
        let enabled = all
            .iter()
            .filter(|repo| repo.enabled)
            .cloned()
            .collect::<Vec<_>>();

        let selected = resolve_configured_targets(&args, &enabled, &all);
        let selected_paths = selected
            .into_iter()
            .map(|repo| repo.path)
            .collect::<Vec<PathBuf>>();

        assert_eq!(
            selected_paths,
            vec![PathBuf::from("/tmp/repo-a"), PathBuf::from("/tmp/repo-c")]
        );
    }

    #[test]
    fn resolve_targets_filters_to_matching_enabled_repositories() {
        let temp = tempfile::tempdir().expect("tempdir should work");
        let repo_path = temp.path().join("repo");
        std::fs::create_dir_all(&repo_path).expect("repo directory should be created");

        let args = RunArgs {
            repos: vec![repo_path.clone()],
            ..RunArgs::default()
        };
        let all = vec![repo_config(&repo_path.to_string_lossy(), true)];
        let enabled = all.clone();

        let selected = resolve_configured_targets(&args, &enabled, &all);
        let selected_paths = selected
            .into_iter()
            .map(|repo| repo.path)
            .collect::<Vec<PathBuf>>();

        assert_eq!(selected_paths, vec![repo_path]);
    }

    fn repo_config(path: &str, enabled: bool) -> ResolvedRepositoryConfig {
        ResolvedRepositoryConfig {
            path: PathBuf::from(path),
            enabled,
            include_untracked: None,
            side_channel: ResolvedRepositorySideChannelConfig::default(),
        }
    }
}
