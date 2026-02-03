//! Core execution engine.
//!
//! The [`Engine`] is the main entry point for all gba-core operations.
//! It orchestrates agent sessions, git operations, and hook execution
//! for the init, plan, and run workflows.

use std::path::PathBuf;
use std::sync::Arc;

use tracing::{info, instrument};

use crate::agent::AgentRunner;
use crate::config::{EngineConfig, ProjectConfig, load_project_config};
use crate::error::CoreError;
use crate::events::{PlanSession, RunStream};
use crate::git::GitOps;

/// Core execution engine that drives all GBA workflows.
///
/// Created via [`Engine::new()`], which loads configuration, initializes the
/// prompt manager, and sets up git operations. The engine then provides
/// [`init()`](Engine::init), [`plan()`](Engine::plan), and
/// [`run()`](Engine::run) methods corresponding to the three CLI commands.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// use gba_core::{Engine, EngineConfig};
///
/// # async fn example() -> Result<(), gba_core::CoreError> {
/// let config = EngineConfig::builder()
///     .repo_path(PathBuf::from("."))
///     .build();
///
/// let engine = Engine::new(config).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Engine {
    /// CLI-level configuration.
    config: EngineConfig,
    /// Project-level configuration from `.gba/config.yaml`.
    project_config: ProjectConfig,
    /// Agent session runner (wraps Claude SDK).
    /// Wrapped in `Arc` so it can be shared with spawned background tasks
    /// (e.g., the run workflow's phase execution task).
    agent_runner: Arc<AgentRunner>,
    /// Git operations helper.
    git: GitOps,
}

impl Engine {
    /// Create a new engine with the given configuration.
    ///
    /// Loads the project configuration from `.gba/config.yaml` (if it exists),
    /// initializes the prompt manager with built-in and custom templates, and
    /// sets up the git operations helper.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Config` if the config file exists but is invalid.
    /// Returns `CoreError::Prompt` if prompt templates cannot be loaded.
    #[instrument(skip_all)]
    pub async fn new(config: EngineConfig) -> Result<Self, CoreError> {
        info!(repo = %config.repo_path().display(), "initializing engine");

        // Load project config (defaults if file doesn't exist)
        let project_config = load_project_config(&config.config_path())?;

        // Initialize agent runner with merged configuration
        let agent_runner = AgentRunner::new(&config, &project_config)?;

        // Set up git operations
        let git = GitOps::new(config.repo_path().clone(), project_config.git.clone());

        Ok(Self {
            config,
            project_config,
            agent_runner: Arc::new(agent_runner),
            git,
        })
    }

    /// Initialize the repository for GBA.
    ///
    /// Creates `.gba/` and `.trees/` directories, analyzes the repository
    /// structure, and generates context documents. This is the implementation
    /// behind `gba init`.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::AlreadyInitialized` if `.gba/` already exists.
    /// Returns `CoreError::Agent` if the init agent fails.
    /// Returns `CoreError::Io` if directory creation fails.
    #[instrument(skip(self))]
    pub async fn init(&self) -> Result<(), CoreError> {
        crate::init::run_init(self).await
    }

    /// Start an interactive planning session for a feature.
    ///
    /// Returns a [`PlanSession`] handle for bidirectional communication with
    /// the planning agent. The CLI drives the conversation by calling
    /// `next()` and `respond()` on the session.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::NotInitialized` if the repo is not initialized.
    /// Returns `CoreError::Io` if directory creation fails.
    /// Returns `CoreError::Git` if worktree creation fails.
    /// Returns `CoreError::Agent` if the planning agent cannot be started.
    #[instrument(skip(self))]
    pub async fn plan(&self, slug: &str) -> Result<PlanSession, CoreError> {
        crate::plan::run_plan(self, slug).await
    }

    /// Execute a feature's development plan phase by phase.
    ///
    /// Returns a [`RunStream`] handle for consuming progress events.
    /// The CLI reads events from the stream to update its progress display.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::NotInitialized` if the repo is not initialized.
    /// Returns `CoreError::FeatureNotFound` if the feature spec doesn't exist.
    #[instrument(skip(self))]
    pub async fn run(&self, slug: &str) -> Result<RunStream, CoreError> {
        crate::run::run_execution(self, slug).await
    }

    /// Returns a reference to the engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Returns a reference to the project configuration.
    pub fn project_config(&self) -> &ProjectConfig {
        &self.project_config
    }

    /// Returns a reference to the internal git operations helper.
    pub(crate) fn git(&self) -> &GitOps {
        &self.git
    }

    /// Returns a reference to the internal agent runner.
    pub(crate) fn agent_runner(&self) -> &AgentRunner {
        &self.agent_runner
    }

    /// Returns an Arc clone of the agent runner for sharing with spawned tasks.
    pub(crate) fn agent_runner_arc(&self) -> Arc<AgentRunner> {
        Arc::clone(&self.agent_runner)
    }

    /// Returns the `.gba` directory path.
    pub fn gba_dir(&self) -> PathBuf {
        self.config.gba_dir()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[tokio::test]
    async fn test_should_create_engine_with_defaults() {
        let config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/nonexistent-repo"))
            .build();

        let engine = Engine::new(config).await;
        assert!(
            engine.is_ok(),
            "engine creation should succeed: {:?}",
            engine.err()
        );

        let engine = engine.expect("engine should be ok");
        assert_eq!(
            engine.config().repo_path(),
            &PathBuf::from("/tmp/nonexistent-repo")
        );
    }

    #[tokio::test]
    async fn test_should_load_project_config_from_tempdir() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path().join(".gba");
        std::fs::create_dir_all(&gba_dir).expect("should create .gba dir");
        std::fs::write(
            gba_dir.join("config.yaml"),
            "agent:\n  model: test-model\ngit:\n  baseBranch: develop\n",
        )
        .expect("should write config");

        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        assert_eq!(
            engine.project_config().agent.model.as_deref(),
            Some("test-model")
        );
        assert_eq!(engine.project_config().git.base_branch, "develop");
    }

    #[tokio::test]
    async fn test_should_return_not_initialized_for_plan() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.plan("test_feature").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::NotInitialized));
    }

    #[tokio::test]
    async fn test_should_return_not_initialized_for_run() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.run("test_feature").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::NotInitialized));
    }

    #[tokio::test]
    async fn test_should_return_already_initialized_for_init() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path().join(".gba");
        std::fs::create_dir_all(&gba_dir).expect("should create .gba dir");

        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.init().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::AlreadyInitialized));
    }
}
