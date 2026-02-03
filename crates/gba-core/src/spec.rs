//! Feature specification types and file operations.
//!
//! This module defines the [`FeatureSpec`] data model (serialized as `phases.yaml`)
//! and functions to load and save feature specifications from the `.gba/features/`
//! directory. The plan command writes the spec fields; the run command fills in
//! the result fields as it executes.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::error::CoreError;

// ── Feature Spec ─────────────────────────────────────────────

/// Feature specification and execution record.
///
/// Serialized as `phases.yaml`. Plan fields are written by `gba plan`;
/// result fields are written by `gba run` as execution progresses.
///
/// # Examples
///
/// ```
/// use gba_core::FeatureSpec;
///
/// let yaml = r#"
/// feature: "Add login page"
/// phases:
///   - name: "Phase 1: Components"
///     description: "Build UI components"
///     tasks:
///       - "Create LoginForm component"
/// verification:
///   criteria:
///     - "All tests pass"
///   testCommands:
///     - "cargo test"
/// "#;
///
/// let spec: FeatureSpec = serde_yaml::from_str(yaml).expect("valid yaml");
/// assert_eq!(spec.feature, "Add login page");
/// assert_eq!(spec.phases.len(), 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureSpec {
    /// Human-readable feature description.
    pub feature: String,

    /// Ordered development phases.
    pub phases: Vec<Phase>,

    /// Verification criteria and commands.
    pub verification: VerificationPlan,

    /// Execution summary, filled by `gba run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Execution>,
}

/// A single development phase within a feature spec.
///
/// Each phase represents a logical unit of work (e.g., "Core data structures",
/// "Business logic") that the coding agent implements in one session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// Phase name (e.g., "Phase 1: Core data structures").
    pub name: String,

    /// Detailed description of what this phase implements.
    pub description: String,

    /// Concrete tasks the agent should complete.
    pub tasks: Vec<String>,

    /// Execution result for this phase, filled by `gba run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<PhaseResult>,
}

/// Execution result for a single phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseResult {
    /// Current status of this phase.
    pub status: StepStatus,

    /// Number of agent API round-trips consumed.
    pub turns: u32,

    /// Commit hash after phase completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// Status of a phase or the overall execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    /// Not yet started.
    #[default]
    Pending,
    /// Currently executing.
    InProgress,
    /// Finished successfully.
    Completed,
    /// Failed with an error.
    Failed,
}

/// Verification criteria and test commands from the feature spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationPlan {
    /// Human-readable acceptance criteria.
    pub criteria: Vec<String>,

    /// Shell commands to run for verification (e.g., `cargo test`).
    pub test_commands: Vec<String>,
}

/// Overall execution summary, written to `phases.yaml` by `gba run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    /// Overall execution status.
    pub status: StepStatus,

    /// Total agent turns across all phases, review, and verification.
    pub total_turns: u32,

    /// Code review summary.
    pub review: ReviewResult,

    /// Verification summary.
    pub verification: VerificationResult,

    /// PR URL, set after the PR is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<String>,
}

/// Summary of the code review step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    /// Number of agent turns consumed during review.
    pub turns: u32,

    /// Number of issues found by the review agent.
    pub issues_found: u32,

    /// Number of issues successfully fixed.
    pub issues_fixed: u32,
}

/// Summary of the verification step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    /// Number of agent turns consumed during verification.
    pub turns: u32,

    /// Whether all verification criteria passed.
    pub passed: bool,
}

// ── File operations ──────────────────────────────────────────

/// Load a [`FeatureSpec`] from `phases.yaml` for the given feature slug.
///
/// Reads from `.gba/features/<slug>/phases.yaml` relative to `gba_dir`.
///
/// # Errors
///
/// Returns `CoreError::FeatureNotFound` if the feature directory does not exist.
/// Returns `CoreError::InvalidSpec` if the YAML content cannot be parsed.
/// Returns `CoreError::Io` if the file cannot be read.
#[instrument(skip(gba_dir))]
pub(crate) fn load_feature_spec(gba_dir: &Path, slug: &str) -> Result<FeatureSpec, CoreError> {
    let phases_path = gba_dir.join("features").join(slug).join("phases.yaml");
    if !phases_path.exists() {
        return Err(CoreError::FeatureNotFound(slug.to_owned()));
    }
    let content = fs::read_to_string(&phases_path)?;
    let spec: FeatureSpec = serde_yaml::from_str(&content)
        .map_err(|e| CoreError::InvalidSpec(format!("{}: {e}", phases_path.display())))?;
    Ok(spec)
}

/// Save a [`FeatureSpec`] to `phases.yaml` for the given feature slug.
///
/// Writes to `.gba/features/<slug>/phases.yaml`, creating parent directories
/// as needed. This function is called after each phase completes to ensure
/// that resume information is persisted even if a later step fails.
///
/// # Errors
///
/// Returns `CoreError::Io` if directories cannot be created or the file cannot
/// be written.
/// Returns `CoreError::Yaml` if the spec cannot be serialized.
#[instrument(skip(gba_dir, spec))]
pub(crate) fn save_feature_spec(
    gba_dir: &Path,
    slug: &str,
    spec: &FeatureSpec,
) -> Result<(), CoreError> {
    let feature_dir = gba_dir.join("features").join(slug);
    fs::create_dir_all(&feature_dir)?;

    let phases_path = feature_dir.join("phases.yaml");
    let yaml = serde_yaml::to_string(spec)?;
    fs::write(&phases_path, yaml)?;
    debug!(path = %phases_path.display(), "saved feature spec");
    Ok(())
}

/// Load the design specification markdown for a feature.
///
/// Reads from `.gba/features/<slug>/specs/design.md`.
///
/// # Errors
///
/// Returns `CoreError::FeatureNotFound` if the file does not exist.
/// Returns `CoreError::Io` if the file cannot be read.
#[instrument(skip(gba_dir))]
pub(crate) fn load_design_spec(gba_dir: &Path, slug: &str) -> Result<String, CoreError> {
    let path = gba_dir
        .join("features")
        .join(slug)
        .join("specs")
        .join("design.md");
    if !path.exists() {
        return Err(CoreError::FeatureNotFound(format!(
            "design spec not found for {slug}"
        )));
    }
    let content = fs::read_to_string(&path)?;
    Ok(content)
}

/// Load the verification specification markdown for a feature.
///
/// Reads from `.gba/features/<slug>/specs/verification.md`.
///
/// # Errors
///
/// Returns `CoreError::FeatureNotFound` if the file does not exist.
/// Returns `CoreError::Io` if the file cannot be read.
#[allow(dead_code)] // Will be used when verification reads its own spec
#[instrument(skip(gba_dir))]
pub(crate) fn load_verification_spec(gba_dir: &Path, slug: &str) -> Result<String, CoreError> {
    let path = gba_dir
        .join("features")
        .join(slug)
        .join("specs")
        .join("verification.md");
    if !path.exists() {
        return Err(CoreError::FeatureNotFound(format!(
            "verification spec not found for {slug}"
        )));
    }
    let content = fs::read_to_string(&path)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_roundtrip_feature_spec_yaml() {
        let spec = FeatureSpec {
            feature: "Add login page".to_owned(),
            phases: vec![Phase {
                name: "Phase 1: Components".to_owned(),
                description: "Build UI components".to_owned(),
                tasks: vec!["Create LoginForm component".to_owned()],
                result: None,
            }],
            verification: VerificationPlan {
                criteria: vec!["All tests pass".to_owned()],
                test_commands: vec!["cargo test".to_owned()],
            },
            execution: None,
        };

        let yaml = serde_yaml::to_string(&spec).expect("should serialize");
        let parsed: FeatureSpec = serde_yaml::from_str(&yaml).expect("should deserialize");

        assert_eq!(parsed.feature, "Add login page");
        assert_eq!(parsed.phases.len(), 1);
        assert_eq!(parsed.phases[0].name, "Phase 1: Components");
        assert!(parsed.phases[0].result.is_none());
        assert!(parsed.execution.is_none());
    }

    #[test]
    fn test_should_roundtrip_feature_spec_with_results() {
        let spec = FeatureSpec {
            feature: "Completed feature".to_owned(),
            phases: vec![Phase {
                name: "Phase 1".to_owned(),
                description: "Done".to_owned(),
                tasks: vec!["Task A".to_owned()],
                result: Some(PhaseResult {
                    status: StepStatus::Completed,
                    turns: 12,
                    commit: Some("a1b2c3d".to_owned()),
                }),
            }],
            verification: VerificationPlan {
                criteria: vec!["Tests pass".to_owned()],
                test_commands: vec!["cargo test".to_owned()],
            },
            execution: Some(Execution {
                status: StepStatus::Completed,
                total_turns: 34,
                review: ReviewResult {
                    turns: 8,
                    issues_found: 2,
                    issues_fixed: 2,
                },
                verification: VerificationResult {
                    turns: 6,
                    passed: true,
                },
                pr: Some("https://github.com/org/repo/pull/42".to_owned()),
            }),
        };

        let yaml = serde_yaml::to_string(&spec).expect("should serialize");
        let parsed: FeatureSpec = serde_yaml::from_str(&yaml).expect("should deserialize");

        let phase_result = parsed.phases[0]
            .result
            .as_ref()
            .expect("should have result");
        assert_eq!(phase_result.status, StepStatus::Completed);
        assert_eq!(phase_result.turns, 12);
        assert_eq!(phase_result.commit.as_deref(), Some("a1b2c3d"));

        let exec = parsed.execution.as_ref().expect("should have execution");
        assert_eq!(exec.status, StepStatus::Completed);
        assert_eq!(exec.total_turns, 34);
        assert_eq!(exec.review.issues_found, 2);
        assert!(exec.verification.passed);
        assert_eq!(
            exec.pr.as_deref(),
            Some("https://github.com/org/repo/pull/42")
        );
    }

    #[test]
    fn test_should_default_step_status_to_pending() {
        let status = StepStatus::default();
        assert_eq!(status, StepStatus::Pending);
    }

    #[test]
    fn test_should_save_and_load_feature_spec() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path();

        let spec = FeatureSpec {
            feature: "Test feature".to_owned(),
            phases: vec![Phase {
                name: "Phase 1".to_owned(),
                description: "Test".to_owned(),
                tasks: vec!["Do something".to_owned()],
                result: None,
            }],
            verification: VerificationPlan {
                criteria: vec!["Passes".to_owned()],
                test_commands: vec!["echo ok".to_owned()],
            },
            execution: None,
        };

        save_feature_spec(gba_dir, "test_feature", &spec).expect("should save");
        let loaded = load_feature_spec(gba_dir, "test_feature").expect("should load");

        assert_eq!(loaded.feature, "Test feature");
        assert_eq!(loaded.phases.len(), 1);
    }

    #[test]
    fn test_should_return_feature_not_found_for_missing_spec() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let result = load_feature_spec(dir.path(), "nonexistent");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::FeatureNotFound(_)));
    }

    #[test]
    fn test_should_return_feature_not_found_for_missing_design_spec() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let result = load_design_spec(dir.path(), "nonexistent");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::FeatureNotFound(_)));
    }

    #[test]
    fn test_should_return_feature_not_found_for_missing_verification_spec() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let result = load_verification_spec(dir.path(), "nonexistent");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::FeatureNotFound(_)));
    }

    #[test]
    fn test_should_load_design_spec_from_file() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let specs_dir = dir.path().join("features").join("test_feat").join("specs");
        std::fs::create_dir_all(&specs_dir).expect("should create dirs");
        std::fs::write(specs_dir.join("design.md"), "# Design\n\nDetails here.")
            .expect("should write");

        let content = load_design_spec(dir.path(), "test_feat").expect("should load");
        assert!(content.contains("# Design"));
    }

    #[test]
    fn test_should_load_verification_spec_from_file() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let specs_dir = dir.path().join("features").join("test_feat").join("specs");
        std::fs::create_dir_all(&specs_dir).expect("should create dirs");
        std::fs::write(
            specs_dir.join("verification.md"),
            "# Verification\n\nCriteria.",
        )
        .expect("should write");

        let content = load_verification_spec(dir.path(), "test_feat").expect("should load");
        assert!(content.contains("# Verification"));
    }

    #[test]
    fn test_should_deserialize_step_status_variants() {
        assert_eq!(
            serde_yaml::from_str::<StepStatus>("pending").expect("should parse"),
            StepStatus::Pending
        );
        assert_eq!(
            serde_yaml::from_str::<StepStatus>("inProgress").expect("should parse"),
            StepStatus::InProgress
        );
        assert_eq!(
            serde_yaml::from_str::<StepStatus>("completed").expect("should parse"),
            StepStatus::Completed
        );
        assert_eq!(
            serde_yaml::from_str::<StepStatus>("failed").expect("should parse"),
            StepStatus::Failed
        );
    }
}
