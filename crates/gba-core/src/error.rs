//! Error types for the gba-core crate.
//!
//! Defines [`CoreError`], the unified error type used across all core engine
//! operations including init, plan, and run workflows.

use thiserror::Error;

/// Core engine errors.
///
/// All fallible operations in gba-core return `Result<T, CoreError>`. Each variant
/// represents a distinct failure domain to enable targeted error handling by callers.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The repository has not been initialized with `gba init`.
    #[error("not initialized: run `gba init` first")]
    NotInitialized,

    /// The repository has already been initialized.
    #[error("already initialized")]
    AlreadyInitialized,

    /// The requested feature was not found in `.gba/features/`.
    #[error("feature not found: {0}")]
    FeatureNotFound(String),

    /// The feature spec (phases.yaml) is missing or contains invalid data.
    #[error("feature spec missing or invalid: {0}")]
    InvalidSpec(String),

    /// An error occurred while communicating with the Claude agent.
    #[error("agent error: {0}")]
    Agent(String),

    /// A git operation (worktree, commit, branch, diff) failed.
    #[error("git operation failed: {0}")]
    Git(String),

    /// Configuration file is missing or contains invalid data.
    #[error("configuration error: {0}")]
    Config(String),

    /// A precommit hook failed after exhausting retries.
    #[error("hook failed: {0}")]
    Hook(String),

    /// An error from the prompt manager crate.
    #[error("prompt error")]
    Prompt(#[from] gba_pm::PmError),

    /// A YAML serialization or deserialization error.
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// An I/O error from the filesystem or process execution.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A catch-all for unexpected errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
