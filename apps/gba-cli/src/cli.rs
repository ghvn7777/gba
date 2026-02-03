use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use gba_core::{Engine, EngineConfig, RunEvent};

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
                let mut stream = engine
                    .run(&slug)
                    .await
                    .context("failed to start run stream")?;

                while let Some(event) = stream.next().await {
                    display_run_event(&event);
                }

                Ok(())
            }
        }
    }
}

/// Display a single run event to stdout.
///
/// Formats each event variant with a prefix indicator:
/// - `[~]` for in-progress steps
/// - `[x]` for completed steps
/// - `[!]` for warnings/failures
fn display_run_event(event: &RunEvent) {
    match event {
        RunEvent::Started {
            feature,
            total_phases,
        } => {
            println!("Running feature: {feature} ({total_phases} phases)");
        }
        RunEvent::PhaseStarted { index, name } => {
            println!("[~] Phase {}: {name}", index + 1);
        }
        RunEvent::CodingOutput(text) => {
            print!("{text}");
        }
        RunEvent::HookResult { hook, passed } => {
            let indicator = if *passed { "x" } else { "!" };
            println!("[{indicator}] Hook: {hook}");
        }
        RunEvent::PhaseCommitted { index, commit_hash } => {
            println!("[x] Phase {} committed: {commit_hash}", index + 1);
        }
        RunEvent::ReviewStarted => println!("[~] Code review..."),
        RunEvent::ReviewCompleted { issues } => {
            println!("[x] Code review completed ({} issues)", issues.len());
        }
        RunEvent::VerificationStarted => println!("[~] Verification..."),
        RunEvent::VerificationCompleted { passed, details } => {
            let indicator = if *passed { "x" } else { "!" };
            println!("[{indicator}] Verification: {details}");
        }
        RunEvent::PrCreated { url } => println!("[x] PR created: {url}"),
        RunEvent::Finished => println!("\nDone!"),
        RunEvent::Error(e) => eprintln!("[!] Error: {e}"),
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
