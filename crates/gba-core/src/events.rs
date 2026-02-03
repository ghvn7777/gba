//! Event types for CLI consumption.
//!
//! This module defines the event streams used by the CLI layer to display
//! progress during plan and run operations. [`PlanSession`] provides a
//! bidirectional conversation handle for interactive planning, while
//! [`RunStream`] provides a unidirectional event stream for run progress.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

// ── Plan Session ─────────────────────────────────────────────

/// Handle for an interactive planning session.
///
/// The CLI drives the conversation by calling [`next()`](PlanSession::next)
/// to receive agent events and [`respond()`](PlanSession::respond) to send
/// user input back to the planning agent.
#[derive(Debug)]
pub struct PlanSession {
    /// Receiver for plan events from the agent.
    event_rx: tokio::sync::mpsc::Receiver<PlanEvent>,

    /// Sender for user input back to the agent.
    input_tx: tokio::sync::mpsc::Sender<String>,
}

impl PlanSession {
    /// Create a new plan session with the given channels.
    pub(crate) fn new(
        event_rx: tokio::sync::mpsc::Receiver<PlanEvent>,
        input_tx: tokio::sync::mpsc::Sender<String>,
    ) -> Self {
        Self { event_rx, input_tx }
    }

    /// Get the next event from the planning agent.
    ///
    /// Returns `None` when the session is complete and the event channel
    /// has been closed.
    pub async fn next(&mut self) -> Option<PlanEvent> {
        self.event_rx.recv().await
    }

    /// Send user input to the planning agent.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Agent` if the agent has already finished and
    /// the input channel is closed.
    pub async fn respond(&mut self, input: &str) -> Result<(), CoreError> {
        self.input_tx
            .send(input.to_owned())
            .await
            .map_err(|e| CoreError::Agent(format!("failed to send user input: {e}")))
    }
}

/// Events emitted during a planning session.
#[derive(Debug)]
pub enum PlanEvent {
    /// Agent produced a text message to display.
    Message(String),

    /// Agent is waiting for user input.
    WaitingForInput,

    /// Agent generated a spec file.
    SpecGenerated {
        /// Path where the spec file was written.
        path: PathBuf,
        /// Content of the generated file.
        content: String,
    },

    /// Planning session completed successfully.
    Completed,

    /// An error occurred during planning.
    Error(CoreError),
}

// ── Run Stream ───────────────────────────────────────────────

/// Handle for consuming run execution progress.
///
/// The CLI reads events from this stream to update the progress display
/// during phased feature execution.
#[derive(Debug)]
pub struct RunStream {
    /// Receiver for run events.
    event_rx: tokio::sync::mpsc::Receiver<RunEvent>,
}

impl RunStream {
    /// Create a new run stream with the given channel.
    pub(crate) fn new(event_rx: tokio::sync::mpsc::Receiver<RunEvent>) -> Self {
        Self { event_rx }
    }

    /// Get the next event from the run execution.
    ///
    /// Returns `None` when execution is complete and the event channel
    /// has been closed.
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.event_rx.recv().await
    }
}

/// Events emitted during feature execution.
#[derive(Debug)]
pub enum RunEvent {
    /// Execution started.
    Started {
        /// Feature description.
        feature: String,
        /// Total number of phases.
        total_phases: usize,
    },

    /// A development phase started.
    PhaseStarted {
        /// Zero-based phase index.
        index: usize,
        /// Phase name.
        name: String,
    },

    /// Coding agent is producing output.
    CodingOutput(String),

    /// Precommit hook result.
    HookResult {
        /// Hook name.
        hook: String,
        /// Whether the hook passed.
        passed: bool,
    },

    /// A phase was committed.
    PhaseCommitted {
        /// Zero-based phase index.
        index: usize,
        /// Git commit hash.
        commit_hash: String,
    },

    /// Code review started.
    ReviewStarted,

    /// Code review completed.
    ReviewCompleted {
        /// Issues found during review.
        issues: Vec<Issue>,
    },

    /// Verification started.
    VerificationStarted,

    /// Verification completed.
    VerificationCompleted {
        /// Whether all criteria passed.
        passed: bool,
        /// Human-readable details about verification outcome.
        details: String,
    },

    /// Pull request created.
    PrCreated {
        /// PR URL.
        url: String,
    },

    /// Execution finished successfully.
    Finished,

    /// An error occurred during execution.
    Error(CoreError),
}

// ── Code Review Types ────────────────────────────────────────

/// A code review issue found by the review agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Severity level of the issue.
    pub severity: Severity,

    /// File path where the issue was found.
    pub file: PathBuf,

    /// Human-readable description of the issue.
    pub description: String,
}

/// Severity level for a code review issue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// A critical issue that must be fixed.
    Error,
    /// A potential problem that should be addressed.
    Warning,
    /// A style or improvement suggestion.
    Suggestion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_serialize_issue_to_json() {
        let issue = Issue {
            severity: Severity::Error,
            file: PathBuf::from("src/main.rs"),
            description: "Unused import".to_owned(),
        };

        let json = serde_json::to_value(&issue).expect("should serialize");
        assert_eq!(json["severity"], "error");
        assert_eq!(json["file"], "src/main.rs");
        assert_eq!(json["description"], "Unused import");
    }

    #[test]
    fn test_should_deserialize_issue_from_json() {
        let json = serde_json::json!({
            "severity": "warning",
            "file": "lib.rs",
            "description": "Consider using match"
        });

        let issue: Issue = serde_json::from_value(json).expect("should deserialize");
        assert_eq!(issue.severity, Severity::Warning);
        assert_eq!(issue.file, PathBuf::from("lib.rs"));
    }

    #[test]
    fn test_should_serialize_all_severity_variants() {
        let error_json = serde_json::to_value(Severity::Error).expect("should serialize");
        assert_eq!(error_json, "error");

        let warning_json = serde_json::to_value(Severity::Warning).expect("should serialize");
        assert_eq!(warning_json, "warning");

        let suggestion_json = serde_json::to_value(Severity::Suggestion).expect("should serialize");
        assert_eq!(suggestion_json, "suggestion");
    }

    #[tokio::test]
    async fn test_should_create_and_recv_plan_session_events() {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(16);

        let mut session = PlanSession::new(event_rx, input_tx);

        // Simulate agent sending a message
        event_tx
            .send(PlanEvent::Message("Hello".to_owned()))
            .await
            .expect("should send");

        let event = session.next().await;
        assert!(matches!(event, Some(PlanEvent::Message(_))));

        // Simulate user responding
        session.respond("User reply").await.expect("should respond");

        let input = input_rx.recv().await;
        assert_eq!(input.as_deref(), Some("User reply"));
    }

    #[tokio::test]
    async fn test_should_create_and_recv_run_stream_events() {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);

        let mut stream = RunStream::new(event_rx);

        event_tx
            .send(RunEvent::Started {
                feature: "test".to_owned(),
                total_phases: 3,
            })
            .await
            .expect("should send");

        let event = stream.next().await;
        assert!(matches!(event, Some(RunEvent::Started { .. })));

        // Drop sender to close the channel
        drop(event_tx);
        let event = stream.next().await;
        assert!(event.is_none());
    }
}
