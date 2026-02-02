use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::tui::App;

#[derive(Debug, Parser)]
#[command(name = "gba", about = "Claude Agent powered repo automation")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start an interactive TUI session for a repository
    Run {
        /// Path to the target repository (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,

        /// Model to use
        #[arg(short, long, default_value = "claude-sonnet-4-20250514")]
        model: String,
    },

    /// Execute a one-shot prompt against a repository
    Exec {
        /// Path to the target repository
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,

        /// The prompt to execute
        #[arg(short, long)]
        prompt: String,
    },
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Run { repo, model } => {
                let _ = (repo, model, App::new());
                todo!("launch TUI session")
            }
            Commands::Exec { repo, prompt } => {
                let _ = (repo, prompt);
                todo!("execute one-shot prompt")
            }
        }
    }
}
