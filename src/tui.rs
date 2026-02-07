use std::path::PathBuf;

use anyhow::Result;
use dialoguer::{Confirm, MultiSelect, Select, theme::ColorfulTheme};

use crate::config::ResolvedRunConfig;
use crate::discovery::Repo;
use crate::state::{State, canonical_repo_key};

pub struct InteractiveSelection {
    pub selected_repos: Vec<PathBuf>,
    pub run_config: ResolvedRunConfig,
}

pub fn select_and_configure_run(
    repos: &[Repo],
    state: &mut State,
    base_run_config: &ResolvedRunConfig,
    persist_selection: bool,
) -> Result<InteractiveSelection> {
    let theme = ColorfulTheme::default();

    let labels: Vec<String> = repos.iter().map(|r| r.path.display().to_string()).collect();
    let defaults: Vec<bool> = repos
        .iter()
        .map(|r| {
            state
                .selected_repos
                .get(&canonical_repo_key(&r.path))
                .copied()
                .unwrap_or(true)
        })
        .collect();

    let selected_indexes = MultiSelect::with_theme(&theme)
        .with_prompt("Select repositories")
        .items(&labels)
        .defaults(&defaults)
        .interact()?;

    let selected_repos: Vec<PathBuf> = selected_indexes
        .iter()
        .map(|idx| repos[*idx].path.clone())
        .collect();

    if persist_selection {
        for repo in repos {
            let key = canonical_repo_key(&repo.path);
            let selected = selected_repos.iter().any(|r| r == &repo.path);
            state.selected_repos.insert(key, selected);
        }
    }

    let sync_all = Select::with_theme(&theme)
        .with_prompt("Run mode")
        .items(&["Sync All (pull + commit + push)", "Pull only"])
        .default(if base_run_config.push_enabled { 0 } else { 1 })
        .interact()?;

    let push_enabled = sync_all == 0;

    let mut include_untracked = base_run_config.include_untracked;
    if push_enabled {
        include_untracked = Confirm::with_theme(&theme)
            .with_prompt("Include untracked files?")
            .default(base_run_config.include_untracked)
            .interact()?;
    }

    let side_channel_enabled = if push_enabled {
        Confirm::with_theme(&theme)
            .with_prompt("Use side-channel remote/branch?")
            .default(base_run_config.side_channel.enabled)
            .interact()?
    } else {
        false
    };

    let mut run_config = base_run_config.clone();
    run_config.push_enabled = push_enabled;
    run_config.include_untracked = include_untracked;
    run_config.side_channel.enabled = side_channel_enabled;

    Ok(InteractiveSelection {
        selected_repos,
        run_config,
    })
}
