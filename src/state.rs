use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub selected_repos: BTreeMap<String, bool>,
}

pub fn load() -> Result<State> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(State::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed reading state file at {}", path.display()))?;
    let state: State = serde_json::from_str(&raw)
        .with_context(|| format!("failed parsing state file at {}", path.display()))?;
    Ok(state)
}

pub fn save(state: &State) -> Result<()> {
    let path = state_path()?;
    let parent = path
        .parent()
        .context("unable to determine parent directory for state file")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed creating state directory {}", parent.display()))?;
    let raw = serde_json::to_string_pretty(state).context("failed serializing state")?;
    fs::write(&path, raw).with_context(|| format!("failed writing {}", path.display()))?;
    Ok(())
}

pub fn canonical_repo_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn state_path() -> Result<PathBuf> {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
        .context("unable to resolve XDG state directory")?;
    Ok(base.join("shephard").join("state.json"))
}
