use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use gba_core::{Engine, EngineConfig};

/// CLI entry point for GBA -- Claude Agent powered repo automation.
#[derive(Debug, Parser)]
#[command(name = "gba", about = "Claude Agent powered repo automation")]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Initialize current repo for GBA
    Init {
        /// Path to the target repository (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
    },
    /// Start interactive planning session
    Plan {
        /// Feature slug
        slug: String,
        /// Path to the target repository
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Execute feature plan phase by phase
    Run {
        /// Feature slug
        slug: String,
        /// Path to the target repository
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
    },
}

impl Cli {
    /// Execute the selected CLI command.
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Init { repo } => {
                let config = EngineConfig::builder().repo_path(repo).build();
                let engine = Engine::new(config)
                    .await
                    .context("failed to create engine")?;
                engine.init().await.context("init failed")?;
                info!("Repository initialized for GBA.");
                println!("Repository initialized for GBA.");
                Ok(())
            }
            Commands::Plan { slug, repo, model } => {
                let config = build_engine_config(repo, model);
                let engine = Engine::new(config)
                    .await
                    .context("failed to create engine")?;
                let _session = engine
                    .plan(&slug)
                    .await
                    .context("failed to start plan session")?;
                // Plan workflow will be implemented in Phase 4.
                println!("Plan session started for '{slug}'. (Phase 4 implementation pending)");
                Ok(())
            }
            Commands::Run { slug, repo, model } => {
                let config = build_engine_config(repo, model);
                let engine = Engine::new(config)
                    .await
                    .context("failed to create engine")?;
                let _stream = engine
                    .run(&slug)
                    .await
                    .context("failed to start run stream")?;
                // Run workflow will be implemented in Phase 5.
                println!("Run started for '{slug}'. (Phase 5 implementation pending)");
                Ok(())
            }
        }
    }
}

/// Build an [`EngineConfig`] from CLI arguments.
///
/// The typed-builder pattern changes the type on each setter call, so
/// conditional model setting must be handled by building different configs.
fn build_engine_config(repo: PathBuf, model: Option<String>) -> EngineConfig {
    match model {
        Some(m) => EngineConfig::builder().repo_path(repo).model(m).build(),
        None => EngineConfig::builder().repo_path(repo).build(),
    }
}
