use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use shephard::{apply, config, discovery, report, state, tui, workflow};

use shephard::cli::{Cli, Command, RunArgs};

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
    let mut state = state::load().unwrap_or_default();
    let run_cfg = config::resolve_run_config(&cfg, args)?;

    let discovered =
        discovery::discover_repositories(&run_cfg.workspace_roots, run_cfg.descend_hidden_dirs)?;
    if discovered.is_empty() {
        println!("No git repositories found in configured workspace roots.");
        return Ok(0);
    }

    let (selected_repos, effective_run_cfg, should_persist_selection) = if args.non_interactive {
        let selected = resolve_non_interactive_targets(args, &discovered, &state.selected_repos);
        (selected, run_cfg, false)
    } else {
        let chosen = tui::select_and_configure_run(
            &discovered,
            &mut state,
            &run_cfg,
            cfg.tui.persist_selection,
        )?;
        let Some(chosen) = chosen else {
            println!("Cancelled interactive run.");
            return Ok(0);
        };
        (
            chosen.selected_repos,
            chosen.run_config,
            cfg.tui.persist_selection,
        )
    };

    if selected_repos.is_empty() {
        println!("No repositories selected.");
        if should_persist_selection {
            state::save(&state)?;
        }
        return Ok(0);
    }

    let results = workflow::run(&selected_repos, &effective_run_cfg);
    report::print_run_summary(&results);

    if should_persist_selection {
        state::save(&state)?;
    }

    Ok(report::exit_code(&results))
}

fn resolve_non_interactive_targets(
    args: &RunArgs,
    discovered: &[discovery::Repo],
    persisted_selection: &std::collections::BTreeMap<String, bool>,
) -> Vec<PathBuf> {
    if !args.repos.is_empty() {
        let mut seen = BTreeSet::new();
        let mut selected = Vec::new();

        for path in &args.repos {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let key = canonical.to_string_lossy().to_string();
            if !seen.insert(key) {
                continue;
            }

            if is_git_repo(&canonical) {
                selected.push(canonical);
            } else {
                eprintln!(
                    "Skipping {} because it is not a git repository",
                    path.display()
                );
            }
        }

        return selected;
    }

    let persisted: Vec<PathBuf> = discovered
        .iter()
        .filter_map(|repo| {
            let key = state::canonical_repo_key(&repo.path);
            if persisted_selection.get(&key).copied().unwrap_or(false) {
                Some(repo.path.clone())
            } else {
                None
            }
        })
        .collect();

    // In non-interactive mode with no explicit --repos, prefer the persisted
    // selection set; if there is none, fall back to all discovered repos.
    if !persisted.is_empty() {
        persisted
    } else {
        discovered.iter().map(|repo| repo.path.clone()).collect()
    }
}

fn is_git_repo(path: &Path) -> bool {
    let git_marker = path.join(".git");
    git_marker.is_dir() || git_marker.is_file()
}
