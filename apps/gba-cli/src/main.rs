//! GBA CLI binary entry point.
//!
//! Initializes the tracing subscriber, parses command-line arguments with
//! clap, and dispatches to the selected subcommand via [`Cli::run`].

mod cli;
mod logging;
mod tui;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Extract slug and repo_path before consuming cli.
    let (repo_path, slug) = cli.log_context();

    // Clean old logs (best-effort, before tracing is initialized).
    logging::cleanup_old_logs(&repo_path);

    // Initialize tracing with optional file layer.
    let _guard = logging::init_tracing(&repo_path, slug.as_deref())?;

    cli.run().await
}
