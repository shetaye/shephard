use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "shephard", about = "Sync many git repositories from one place")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    Apply(ApplyArgs),
}

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
    #[arg(long)]
    pub non_interactive: bool,
    #[arg(long, value_name = "PATH")]
    pub repos: Vec<PathBuf>,
    #[arg(long)]
    pub pull_only: bool,
    #[arg(long)]
    pub push: bool,
    #[arg(long)]
    pub include_untracked: bool,
    #[arg(long)]
    pub tracked_only: bool,
    #[arg(long)]
    pub side_channel: bool,
    #[arg(long)]
    pub no_side_channel: bool,
    #[arg(long, value_name = "PATH")]
    pub roots: Vec<PathBuf>,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            non_interactive: false,
            repos: Vec::new(),
            pull_only: false,
            push: false,
            include_untracked: false,
            tracked_only: false,
            side_channel: false,
            no_side_channel: false,
            roots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Parser)]
pub struct ApplyArgs {
    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = ApplyMethodArg::Merge)]
    pub method: ApplyMethodArg,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ApplyMethodArg {
    Merge,
    CherryPick,
    Squash,
}
