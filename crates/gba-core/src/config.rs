//! Configuration types for gba-core.
//!
//! This module defines [`EngineConfig`] (CLI-level overrides), [`ProjectConfig`]
//! (from `.gba/config.yaml`), and all sub-configuration types. During engine
//! initialization, CLI flags in `EngineConfig` take precedence over values read
//! from `ProjectConfig`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

// ── Engine Configuration (CLI-level) ─────────────────────────

/// Engine configuration provided by the CLI layer.
///
/// Contains repository path and optional overrides for model and token limits.
/// When the engine starts, these values are merged with [`ProjectConfig`] from
/// `.gba/config.yaml`, with `EngineConfig` values taking precedence.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use gba_core::EngineConfig;
///
/// let config = EngineConfig::builder()
///     .repo_path(PathBuf::from("/tmp/my-repo"))
///     .model("claude-opus-4")
///     .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
pub struct EngineConfig {
    /// Path to the target repository.
    repo_path: PathBuf,

    /// Override Claude model (takes precedence over config.yaml).
    #[builder(default, setter(strip_option, into))]
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,

    /// Override max tokens per agent response (takes precedence over config.yaml).
    #[builder(default, setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

impl EngineConfig {
    /// Returns the repository path.
    pub fn repo_path(&self) -> &PathBuf {
        &self.repo_path
    }

    /// Returns the model override, if set.
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Returns the max tokens override, if set.
    pub fn max_tokens(&self) -> Option<u32> {
        self.max_tokens
    }

    /// Returns the `.gba` directory path for this repository.
    pub fn gba_dir(&self) -> PathBuf {
        self.repo_path.join(".gba")
    }

    /// Returns the `.trees` directory path for this repository.
    pub fn trees_dir(&self) -> PathBuf {
        self.repo_path.join(".trees")
    }

    /// Returns the path to `config.yaml` inside the `.gba` directory.
    pub fn config_path(&self) -> PathBuf {
        self.gba_dir().join("config.yaml")
    }
}

// ── Project Configuration (.gba/config.yaml) ────────────────

/// Project-level GBA configuration, deserialized from `.gba/config.yaml`.
///
/// All fields have serde defaults so that missing keys in the YAML file
/// produce valid configuration with sensible defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    /// Agent-level settings (model, tokens, permission mode).
    #[serde(default)]
    pub agent: AgentProjectConfig,

    /// Prompt template search paths.
    #[serde(default)]
    pub prompts: PromptsConfig,

    /// Git workflow settings (branching, auto-commit).
    #[serde(default)]
    pub git: GitConfig,

    /// Code review settings.
    #[serde(default)]
    pub review: ReviewConfig,

    /// Verification settings.
    #[serde(default)]
    pub verification: VerificationConfig,

    /// Precommit hook settings.
    #[serde(default)]
    pub hooks: HooksConfig,
}

// ── Sub-configuration types ──────────────────────────────────

/// Agent configuration from the project config file.
///
/// Controls which model, token limit, and permission mode the engine uses
/// by default. CLI overrides in [`EngineConfig`] take precedence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProjectConfig {
    /// Claude model to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Max tokens per agent response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Permission mode for agent tool use.
    #[serde(default)]
    pub permission_mode: PermissionMode,
}

/// Permission mode for agent tool invocations.
///
/// Controls whether the agent runs tools automatically or requires
/// user approval for each invocation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    /// Agent runs tools without asking (default).
    #[default]
    Auto,
    /// Agent asks the user before each tool invocation.
    Manual,
    /// Agent cannot use tools (prompt-only mode).
    None,
}

/// Prompt template configuration.
///
/// Specifies additional directories to search for prompt template overrides.
/// Templates found in these directories replace built-in templates with the
/// same name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptsConfig {
    /// Additional template directories to search (in order).
    #[serde(default)]
    pub include: Vec<PathBuf>,
}

/// Git workflow configuration.
///
/// Controls branch naming, auto-commit behavior, and the base branch
/// used when creating feature worktrees.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfig {
    /// Automatically commit after each phase completes.
    #[serde(default = "default_true")]
    pub auto_commit: bool,

    /// Branch naming pattern. Variables: `{id}`, `{slug}`.
    #[serde(default = "default_branch_pattern")]
    pub branch_pattern: String,

    /// Base branch to create worktrees from.
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            auto_commit: true,
            branch_pattern: default_branch_pattern(),
            base_branch: default_base_branch(),
        }
    }
}

/// Code review configuration.
///
/// Controls whether the code review step runs after all phases complete
/// and how many review-fix iterations are allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewConfig {
    /// Enable the code review step.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum review-fix iterations before proceeding.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: default_max_iterations(),
        }
    }
}

/// Verification configuration.
///
/// Controls whether the verification step runs after code review
/// and how many verify-fix iterations are allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationConfig {
    /// Enable the verification step.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum verify-fix iterations before failing.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: default_max_iterations(),
        }
    }
}

/// Precommit hooks configuration.
///
/// Defines the hooks that run after each phase's code is written (before commit)
/// and the maximum number of hook-fix-retry cycles per phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksConfig {
    /// Hooks to run before committing each phase.
    #[serde(default)]
    pub pre_commit: Vec<Hook>,

    /// Maximum hook-fix-retry cycles per phase.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            pre_commit: Vec::new(),
            max_retries: default_max_retries(),
        }
    }
}

/// A single precommit hook definition.
///
/// Each hook is a named shell command executed in the worktree root.
/// If the command exits with a non-zero status, the agent attempts to fix
/// the issues and re-run the hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    /// Human-readable hook name (e.g., "build", "fmt", "lint").
    pub name: String,

    /// Shell command to execute (e.g., "cargo build").
    pub command: String,
}

// ── Default value functions for serde ────────────────────────

fn default_true() -> bool {
    true
}

fn default_branch_pattern() -> String {
    "feat/{id}-{slug}".to_owned()
}

fn default_base_branch() -> String {
    "main".to_owned()
}

fn default_max_iterations() -> u32 {
    3
}

fn default_max_retries() -> u32 {
    5
}

// ── Config loading ───────────────────────────────────────────

/// Load [`ProjectConfig`] from the `.gba/config.yaml` file.
///
/// If the file does not exist, returns the default configuration.
///
/// # Errors
///
/// Returns `CoreError::Io` if the file exists but cannot be read.
/// Returns `CoreError::Yaml` if the file contains invalid YAML.
pub fn load_project_config(
    config_path: &std::path::Path,
) -> Result<ProjectConfig, crate::CoreError> {
    if !config_path.exists() {
        return Ok(ProjectConfig::default());
    }
    let content = std::fs::read_to_string(config_path)?;
    let config: ProjectConfig = serde_yaml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    #[test]
    fn test_should_build_engine_config_with_defaults() {
        let config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/repo"))
            .build();

        assert_eq!(config.repo_path(), &PathBuf::from("/tmp/repo"));
        assert!(config.model().is_none());
        assert!(config.max_tokens().is_none());
    }

    #[test]
    fn test_should_build_engine_config_with_overrides() {
        let config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/repo"))
            .model("claude-opus-4")
            .max_tokens(16384_u32)
            .build();

        assert_eq!(config.model(), Some("claude-opus-4"));
        assert_eq!(config.max_tokens(), Some(16384));
    }

    #[test]
    fn test_should_compute_gba_dir_path() {
        let config = EngineConfig::builder()
            .repo_path(PathBuf::from("/home/user/project"))
            .build();

        assert_eq!(config.gba_dir(), PathBuf::from("/home/user/project/.gba"));
        assert_eq!(
            config.trees_dir(),
            PathBuf::from("/home/user/project/.trees")
        );
        assert_eq!(
            config.config_path(),
            PathBuf::from("/home/user/project/.gba/config.yaml")
        );
    }

    #[test]
    fn test_should_deserialize_default_project_config() {
        let yaml = "";
        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap_or_default();

        assert!(config.agent.model.is_none());
        assert_eq!(config.agent.permission_mode, PermissionMode::Auto);
        assert!(config.git.auto_commit);
        assert_eq!(config.git.branch_pattern, "feat/{id}-{slug}");
        assert_eq!(config.git.base_branch, "main");
        assert!(config.review.enabled);
        assert_eq!(config.review.max_iterations, 3);
        assert!(config.verification.enabled);
        assert_eq!(config.verification.max_iterations, 3);
        assert!(config.hooks.pre_commit.is_empty());
        assert_eq!(config.hooks.max_retries, 5);
    }

    #[test]
    fn test_should_deserialize_full_project_config() {
        let yaml = r#"
agent:
  model: claude-sonnet-4-20250514
  maxTokens: 16384
  permissionMode: auto
prompts:
  include:
    - ~/.config/gba/prompts
git:
  autoCommit: true
  branchPattern: "feat/{id}-{slug}"
  baseBranch: main
review:
  enabled: true
  maxIterations: 3
verification:
  enabled: true
  maxIterations: 3
hooks:
  preCommit:
    - name: build
      command: cargo build
    - name: fmt
      command: cargo +nightly fmt --check
    - name: lint
      command: cargo clippy -- -D warnings
  maxRetries: 5
"#;

        let config: ProjectConfig = serde_yaml::from_str(yaml).expect("should parse YAML");

        assert_eq!(
            config.agent.model.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
        assert_eq!(config.agent.max_tokens, Some(16384));
        assert_eq!(config.agent.permission_mode, PermissionMode::Auto);
        assert_eq!(config.prompts.include.len(), 1);
        assert!(config.git.auto_commit);
        assert_eq!(config.hooks.pre_commit.len(), 3);
        assert_eq!(config.hooks.pre_commit[0].name, "build");
        assert_eq!(config.hooks.pre_commit[0].command, "cargo build");
        assert_eq!(config.hooks.max_retries, 5);
    }

    #[test]
    fn test_should_serialize_engine_config_to_json() {
        let config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/repo"))
            .model("claude-opus-4")
            .build();

        let value = serde_json::to_value(&config).expect("should serialize");
        assert_eq!(value["repo_path"], json!("/tmp/repo"));
        assert_eq!(value["model"], json!("claude-opus-4"));
        // max_tokens should be absent (skip_serializing_if)
        assert!(value.get("max_tokens").is_none());
    }

    #[test]
    fn test_should_deserialize_permission_mode_variants() {
        let auto: PermissionMode = serde_yaml::from_str("auto").expect("should parse auto");
        assert_eq!(auto, PermissionMode::Auto);

        let manual: PermissionMode = serde_yaml::from_str("manual").expect("should parse manual");
        assert_eq!(manual, PermissionMode::Manual);

        let none: PermissionMode = serde_yaml::from_str("none").expect("should parse none");
        assert_eq!(none, PermissionMode::None);
    }

    #[test]
    fn test_should_load_default_when_config_file_missing() {
        let path = PathBuf::from("/nonexistent/config.yaml");
        let config = load_project_config(&path).expect("should return default");
        assert!(config.agent.model.is_none());
        assert!(config.git.auto_commit);
    }

    #[test]
    fn test_should_load_config_from_tempfile() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let config_path = dir.path().join("config.yaml");
        std::fs::write(
            &config_path,
            "agent:\n  model: test-model\ngit:\n  baseBranch: develop\n",
        )
        .expect("should write config");

        let config = load_project_config(&config_path).expect("should load config");
        assert_eq!(config.agent.model.as_deref(), Some("test-model"));
        assert_eq!(config.git.base_branch, "develop");
        // Defaults should still apply for unspecified fields
        assert!(config.git.auto_commit);
    }

    #[test]
    fn test_should_serialize_hook() {
        let hook = Hook {
            name: "build".to_owned(),
            command: "cargo build".to_owned(),
        };
        let value = serde_json::to_value(&hook).expect("should serialize");
        assert_eq!(value["name"], "build");
        assert_eq!(value["command"], "cargo build");
    }
}
