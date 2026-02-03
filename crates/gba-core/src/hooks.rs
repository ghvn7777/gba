//! Precommit hooks module (internal).
//!
//! Runs configured shell commands (build, fmt, clippy, etc.) in a worktree
//! after each phase's code is written, before committing. Each hook's stdout
//! and stderr are captured for display and for the agent to use when fixing
//! failures.

use std::path::Path;

use tracing::{debug, error, instrument, warn};

use crate::config::{Hook, HooksConfig};
use crate::error::CoreError;

/// Runs precommit hooks in sequence and reports results.
///
/// Each hook is a named shell command. If any hook fails, the output is
/// captured so the coding agent can attempt to fix the issue. The caller
/// controls the retry loop.
#[derive(Debug)]
pub(crate) struct HookRunner {
    /// Hook definitions to run.
    hooks: Vec<Hook>,
    /// Maximum hook-fix-retry cycles (informational, caller enforces).
    max_retries: u32,
}

/// Output from running a single hook.
#[derive(Debug, Clone)]
pub(crate) struct HookOutput {
    /// Hook name.
    pub name: String,
    /// Shell command that was executed.
    pub command: String,
    /// Whether the hook passed (exit code 0).
    pub passed: bool,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
}

impl HookRunner {
    /// Create a new hook runner from the hooks configuration.
    pub(crate) fn new(config: &HooksConfig) -> Self {
        Self {
            hooks: config.pre_commit.clone(),
            max_retries: config.max_retries,
        }
    }

    /// Returns the maximum retry count for the hook-fix cycle.
    pub(crate) fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// Returns whether there are any hooks configured.
    pub(crate) fn has_hooks(&self) -> bool {
        !self.hooks.is_empty()
    }

    /// Run all configured hooks in sequence.
    ///
    /// Executes each hook command in the given working directory. All hooks
    /// run regardless of whether earlier hooks fail -- the caller gets a
    /// complete picture of what passed and what failed.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Io` if a hook command cannot be spawned.
    #[instrument(skip(self))]
    pub(crate) async fn run_all(&self, cwd: &Path) -> Result<Vec<HookOutput>, CoreError> {
        let mut results = Vec::with_capacity(self.hooks.len());

        for hook in &self.hooks {
            debug!(hook = %hook.name, command = %hook.command, "running hook");

            let output = match tokio::process::Command::new("sh")
                .args(["-c", &hook.command])
                .current_dir(cwd)
                .output()
                .await
            {
                Ok(output) => output,
                Err(e) => {
                    error!(
                        hook = %hook.name,
                        command = %hook.command,
                        error = %e,
                        "failed to spawn hook command"
                    );
                    return Err(CoreError::Io(e));
                }
            };

            let passed = output.status.success();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if passed {
                debug!(hook = %hook.name, "hook passed");
            } else {
                warn!(
                    hook = %hook.name,
                    exit_code = ?output.status.code(),
                    "hook failed"
                );
            }

            results.push(HookOutput {
                name: hook.name.clone(),
                command: hook.command.clone(),
                passed,
                stdout,
                stderr,
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::HooksConfig;

    use super::*;

    fn test_hooks_config(hooks: Vec<Hook>) -> HooksConfig {
        HooksConfig {
            pre_commit: hooks,
            max_retries: 3,
        }
    }

    #[test]
    fn test_should_create_hook_runner() {
        let config = test_hooks_config(vec![Hook {
            name: "build".to_owned(),
            command: "echo build".to_owned(),
        }]);

        let runner = HookRunner::new(&config);
        assert!(runner.has_hooks());
        assert_eq!(runner.max_retries(), 3);
    }

    #[test]
    fn test_should_report_no_hooks_when_empty() {
        let config = test_hooks_config(vec![]);
        let runner = HookRunner::new(&config);
        assert!(!runner.has_hooks());
    }

    #[tokio::test]
    async fn test_should_run_passing_hook() {
        let config = test_hooks_config(vec![Hook {
            name: "echo".to_owned(),
            command: "echo hello".to_owned(),
        }]);

        let runner = HookRunner::new(&config);
        let results = runner
            .run_all(Path::new("/tmp"))
            .await
            .expect("should run hooks");

        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
        assert_eq!(results[0].name, "echo");
        assert!(results[0].stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_should_run_failing_hook() {
        let config = test_hooks_config(vec![Hook {
            name: "fail".to_owned(),
            command: "exit 1".to_owned(),
        }]);

        let runner = HookRunner::new(&config);
        let results = runner
            .run_all(Path::new("/tmp"))
            .await
            .expect("should run hooks");

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
    }

    #[tokio::test]
    async fn test_should_run_all_hooks_even_when_one_fails() {
        let config = test_hooks_config(vec![
            Hook {
                name: "pass".to_owned(),
                command: "echo pass".to_owned(),
            },
            Hook {
                name: "fail".to_owned(),
                command: "exit 1".to_owned(),
            },
            Hook {
                name: "also_pass".to_owned(),
                command: "echo also_pass".to_owned(),
            },
        ]);

        let runner = HookRunner::new(&config);
        let results = runner
            .run_all(Path::new("/tmp"))
            .await
            .expect("should run hooks");

        assert_eq!(results.len(), 3);
        assert!(results[0].passed);
        assert!(!results[1].passed);
        assert!(results[2].passed);
    }

    #[tokio::test]
    async fn test_should_capture_stderr_from_failing_hook() {
        let config = test_hooks_config(vec![Hook {
            name: "stderr_test".to_owned(),
            command: "echo error_msg >&2 && exit 1".to_owned(),
        }]);

        let runner = HookRunner::new(&config);
        let results = runner
            .run_all(Path::new("/tmp"))
            .await
            .expect("should run hooks");

        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(results[0].stderr.contains("error_msg"));
    }
}
