//! Git operations module (internal).
//!
//! Provides worktree creation, branch management, commit, and diff operations.
//! All git commands are executed asynchronously via `tokio::process::Command`.

use std::path::{Path, PathBuf};

use tracing::{debug, instrument};

use crate::config::GitConfig;
use crate::error::CoreError;

/// Manages git operations for feature worktrees.
///
/// Encapsulates the repository path and git configuration to provide
/// a consistent interface for worktree creation, branching, committing,
/// and diffing.
#[derive(Debug)]
pub(crate) struct GitOps {
    /// Path to the main repository.
    repo_path: PathBuf,
    /// Git configuration from the project config.
    git_config: GitConfig,
}

impl GitOps {
    /// Create a new `GitOps` instance.
    pub(crate) fn new(repo_path: PathBuf, config: GitConfig) -> Self {
        Self {
            repo_path,
            git_config: config,
        }
    }

    /// Compute the worktree path for a feature slug.
    ///
    /// Returns `<repo_path>/.trees/<slug>`.
    pub(crate) fn worktree_path(&self, slug: &str) -> PathBuf {
        self.repo_path.join(".trees").join(slug)
    }

    /// Compute the branch name for a feature slug.
    ///
    /// Applies the branch pattern from config, substituting `{slug}` and `{id}`
    /// (extracted from the slug prefix if present, e.g., "0001" from "0001_feature").
    pub(crate) fn branch_name(&self, slug: &str) -> String {
        let id = extract_id(slug);
        self.git_config
            .branch_pattern
            .replace("{slug}", slug)
            .replace("{id}", id)
    }

    /// Create a new git worktree for a feature.
    ///
    /// Creates a new branch from the base branch and sets up a worktree
    /// at `.trees/<slug>`. The branch name is derived from the config pattern.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Git` if the git command fails.
    #[instrument(skip(self))]
    pub(crate) async fn create_worktree(&self, slug: &str) -> Result<PathBuf, CoreError> {
        let worktree_path = self.worktree_path(slug);
        let branch = self.branch_name(slug);
        let base = &self.git_config.base_branch;

        debug!(
            slug,
            branch = %branch,
            path = %worktree_path.display(),
            "creating worktree"
        );

        let output = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", &branch])
            .arg(&worktree_path)
            .arg(base)
            .current_dir(&self.repo_path)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Git(format!(
                "failed to create worktree for {slug}: {stderr}"
            )));
        }

        Ok(worktree_path)
    }

    /// Ensure a worktree exists for the given feature slug.
    ///
    /// If the worktree directory already exists, returns its path without
    /// modification. Otherwise, creates a new worktree.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Git` if worktree creation fails.
    #[instrument(skip(self))]
    pub(crate) async fn ensure_worktree(&self, slug: &str) -> Result<PathBuf, CoreError> {
        let worktree_path = self.worktree_path(slug);
        if worktree_path.exists() {
            debug!(slug, path = %worktree_path.display(), "worktree already exists");
            return Ok(worktree_path);
        }
        self.create_worktree(slug).await
    }

    /// Commit all changes in a worktree with the given message.
    ///
    /// Stages all changes with `git add -A` and commits. Returns the
    /// short commit hash of the new commit.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Git` if staging or committing fails.
    #[instrument(skip(self, message))]
    pub(crate) async fn commit(&self, worktree: &Path, message: &str) -> Result<String, CoreError> {
        // Stage all changes
        let add_output = tokio::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree)
            .output()
            .await?;

        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Err(CoreError::Git(format!("git add failed: {stderr}")));
        }

        // Commit
        let commit_output = tokio::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(worktree)
            .output()
            .await?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            return Err(CoreError::Git(format!("git commit failed: {stderr}")));
        }

        // Get the short commit hash
        let hash_output = tokio::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(worktree)
            .output()
            .await?;

        if !hash_output.status.success() {
            let stderr = String::from_utf8_lossy(&hash_output.stderr);
            return Err(CoreError::Git(format!("git rev-parse failed: {stderr}")));
        }

        let hash = String::from_utf8_lossy(&hash_output.stdout)
            .trim()
            .to_owned();
        debug!(hash = %hash, "committed changes");
        Ok(hash)
    }

    /// Get the diff between the worktree and a base reference.
    ///
    /// Returns the unified diff output as a string.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Git` if the diff command fails.
    #[instrument(skip(self))]
    pub(crate) async fn get_diff(&self, worktree: &Path, base: &str) -> Result<String, CoreError> {
        let output = tokio::process::Command::new("git")
            .args(["diff", base])
            .current_dir(worktree)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Git(format!("git diff failed: {stderr}")));
        }

        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(diff)
    }

    /// Get the current branch name in a worktree.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Git` if the git command fails.
    #[instrument(skip(self))]
    pub(crate) async fn current_branch(&self, worktree: &Path) -> Result<String, CoreError> {
        let output = tokio::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(worktree)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::Git(format!(
                "git rev-parse --abbrev-ref failed: {stderr}"
            )));
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok(branch)
    }
}

/// Extract the numeric ID prefix from a feature slug.
///
/// For example, "0001_web_frontend" returns "0001".
/// If no numeric prefix is found, returns the full slug.
fn extract_id(slug: &str) -> &str {
    slug.split('_')
        .next()
        .filter(|part| part.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GitConfig {
        GitConfig {
            auto_commit: true,
            branch_pattern: "feat/{id}-{slug}".to_owned(),
            base_branch: "main".to_owned(),
        }
    }

    #[test]
    fn test_should_compute_worktree_path() {
        let ops = GitOps::new(PathBuf::from("/repo"), test_config());
        assert_eq!(
            ops.worktree_path("0001_feature"),
            PathBuf::from("/repo/.trees/0001_feature")
        );
    }

    #[test]
    fn test_should_compute_branch_name_with_id() {
        let ops = GitOps::new(PathBuf::from("/repo"), test_config());
        assert_eq!(
            ops.branch_name("0001_web_frontend"),
            "feat/0001-0001_web_frontend"
        );
    }

    #[test]
    fn test_should_compute_branch_name_without_numeric_prefix() {
        let ops = GitOps::new(PathBuf::from("/repo"), test_config());
        // When slug has no numeric prefix, both {id} and {slug} get the full slug
        assert_eq!(
            ops.branch_name("web_frontend"),
            "feat/web_frontend-web_frontend"
        );
    }

    #[test]
    fn test_should_extract_id_from_numbered_slug() {
        assert_eq!(extract_id("0001_feature"), "0001");
        assert_eq!(extract_id("0042_another"), "0042");
    }

    #[test]
    fn test_should_return_full_slug_when_no_numeric_prefix() {
        assert_eq!(extract_id("feature"), "feature");
        assert_eq!(extract_id("abc_123"), "abc_123");
    }

    #[test]
    fn test_should_use_custom_branch_pattern() {
        let config = GitConfig {
            auto_commit: true,
            branch_pattern: "feature/{slug}".to_owned(),
            base_branch: "develop".to_owned(),
        };
        let ops = GitOps::new(PathBuf::from("/repo"), config);
        assert_eq!(ops.branch_name("0001_login"), "feature/0001_login");
    }
}
