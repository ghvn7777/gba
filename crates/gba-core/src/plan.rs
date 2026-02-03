//! Plan workflow implementation.
//!
//! Implements the interactive planning session that produces feature specs.
//! The workflow creates a [`PlanSession`] for bidirectional communication
//! between the CLI and a Claude planning agent. The agent asks questions,
//! the user responds, and eventually the agent generates `design.md`,
//! `verification.md`, and `phases.yaml` under `.gba/features/<slug>/`.

use std::path::PathBuf;

use claude_agent_sdk_rs::{ClaudeClient, ContentBlock, Message};
use futures::StreamExt as _;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, error, info, instrument, warn};

use crate::engine::Engine;
use crate::error::CoreError;
use crate::events::{PlanEvent, PlanSession};

/// Run the plan workflow.
///
/// Creates feature directories, sets up a git worktree, and spawns a
/// background task that manages a bidirectional `ClaudeClient` session.
/// Returns a [`PlanSession`] handle for the CLI to drive the conversation.
///
/// # Workflow
///
/// 1. Verify the repository is initialized (`.gba/` exists)
/// 2. Create feature directory `.gba/features/<slug>/specs/`
/// 3. Create a git worktree for the feature branch
/// 4. Build agent options and render the task prompt
/// 5. Spawn a background task with a `ClaudeClient` for bidirectional streaming
/// 6. Return the session handle
///
/// # Errors
///
/// Returns `CoreError::NotInitialized` if `.gba/` does not exist.
/// Returns `CoreError::Io` if directory creation fails.
/// Returns `CoreError::Git` if worktree creation fails.
/// Returns `CoreError::Agent` if agent options cannot be built.
#[instrument(skip(engine))]
pub(crate) async fn run_plan(engine: &Engine, slug: &str) -> Result<PlanSession, CoreError> {
    // Step 1: Verify initialized
    let gba_dir = engine.gba_dir();
    if !gba_dir.exists() {
        return Err(CoreError::NotInitialized);
    }

    // Step 2: Create feature directory
    let feature_dir = gba_dir.join("features").join(slug);
    let specs_dir = feature_dir.join("specs");
    std::fs::create_dir_all(&specs_dir)?;
    info!(feature = slug, "created feature directory");

    // Step 3: Create worktree (tolerate if it already exists for resume)
    match engine.git().create_worktree(slug).await {
        Ok(path) => info!(worktree = %path.display(), "created worktree"),
        Err(CoreError::Git(msg)) if msg.contains("already") => {
            info!("worktree already exists, continuing");
        }
        Err(e) => return Err(e),
    }

    // Step 4: Build agent options and task prompt
    let repo_path = engine.config().repo_path().to_path_buf();
    let context = json!({
        "repo_path": repo_path.display().to_string(),
        "feature_slug": slug,
    });

    let options = engine
        .agent_runner()
        .build_agent_options("plan", &context, Some(&repo_path))?;

    let task_prompt = engine
        .agent_runner()
        .render_template("plan/task", &context)?;

    debug!(slug, "built agent options and task prompt for plan session");

    // Step 5: Create channels for bidirectional communication
    let (event_tx, event_rx) = mpsc::channel(32);
    let (input_tx, input_rx) = mpsc::channel(32);
    let session = PlanSession::new(event_rx, input_tx);

    // Step 6: Spawn background task to drive the ClaudeClient session
    let feature_dir_for_task = feature_dir.clone();
    tokio::spawn(async move {
        run_plan_session(
            options,
            task_prompt,
            event_tx,
            input_rx,
            feature_dir_for_task,
        )
        .await;
    });

    Ok(session)
}

/// Drive the bidirectional ClaudeClient session in a background task.
///
/// Connects to the Claude agent, sends the initial task prompt, and enters
/// a loop: receive agent messages, emit events, wait for user input, and
/// send the next query. The loop terminates when the user closes the input
/// channel or the agent signals completion.
#[instrument(skip_all)]
async fn run_plan_session(
    options: claude_agent_sdk_rs::ClaudeAgentOptions,
    task_prompt: String,
    event_tx: mpsc::Sender<PlanEvent>,
    mut input_rx: mpsc::Receiver<String>,
    feature_dir: PathBuf,
) {
    // Connect the ClaudeClient
    let mut client = ClaudeClient::new(options);
    if let Err(e) = client.connect().await {
        error!(error = %e, "failed to connect plan agent");
        let _ = event_tx
            .send(PlanEvent::Error(CoreError::Agent(format!(
                "failed to connect plan agent: {e}. Check your network connection and API credentials."
            ))))
            .await;
        return;
    }
    debug!("plan agent connected");

    // Send the initial task prompt
    if let Err(e) = client.query(&task_prompt).await {
        error!(error = %e, "failed to send initial query to plan agent");
        let _ = event_tx
            .send(PlanEvent::Error(CoreError::Agent(format!(
                "failed to send initial query: {e}"
            ))))
            .await;
        let _ = client.disconnect().await;
        return;
    }
    debug!("sent initial task prompt to plan agent");

    // Main conversation loop
    loop {
        // Receive messages for one agent turn
        let turn_result = receive_turn(&client, &event_tx, &feature_dir).await;

        match turn_result {
            TurnOutcome::WaitingForInput => {
                // Agent finished its turn, notify the CLI
                if event_tx.send(PlanEvent::WaitingForInput).await.is_err() {
                    debug!("event channel closed, ending plan session");
                    break;
                }

                // Wait for user input
                match input_rx.recv().await {
                    Some(input) => {
                        debug!("received user input, sending to agent");
                        if let Err(e) = client.query(&input).await {
                            let _ = event_tx
                                .send(PlanEvent::Error(CoreError::Agent(format!(
                                    "failed to send user input: {e}"
                                ))))
                                .await;
                            break;
                        }
                    }
                    None => {
                        // Input channel closed, user ended the session
                        debug!("input channel closed, ending plan session");
                        let _ = event_tx.send(PlanEvent::Completed).await;
                        break;
                    }
                }
            }
            TurnOutcome::Completed => {
                let _ = event_tx.send(PlanEvent::Completed).await;
                break;
            }
            TurnOutcome::Error(err) => {
                let _ = event_tx.send(PlanEvent::Error(err)).await;
                break;
            }
            TurnOutcome::StreamEnded => {
                // Stream ended unexpectedly (e.g., process exited)
                warn!("plan agent stream ended unexpectedly");
                let _ = event_tx.send(PlanEvent::Completed).await;
                break;
            }
        }
    }

    // Clean up
    if let Err(e) = client.disconnect().await {
        warn!("failed to disconnect plan agent cleanly: {e}");
    }
    debug!("plan session ended");
}

/// Outcome of receiving a single agent turn.
#[derive(Debug)]
enum TurnOutcome {
    /// Agent finished speaking and is waiting for user input.
    WaitingForInput,
    /// The conversation completed successfully.
    #[allow(dead_code)] // Variant exists for protocol completeness
    Completed,
    /// The message stream ended (connection closed).
    StreamEnded,
    /// An error occurred.
    Error(CoreError),
}

/// Receive messages for one agent turn.
///
/// Consumes messages from the agent until a `Result` message is received
/// (indicating the turn is done). Emits `PlanEvent::Message` for text
/// content and `PlanEvent::SpecGenerated` for detected spec file writes.
async fn receive_turn(
    client: &ClaudeClient,
    event_tx: &mpsc::Sender<PlanEvent>,
    feature_dir: &PathBuf,
) -> TurnOutcome {
    let mut stream = client.receive_messages();
    let mut turn_text = String::new();

    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                return TurnOutcome::Error(CoreError::Agent(format!(
                    "plan agent stream error: {e}"
                )));
            }
        };

        match msg {
            Message::Assistant(ref assistant) => {
                for block in &assistant.message.content {
                    if let ContentBlock::Text(text_block) = block {
                        turn_text.push_str(&text_block.text);
                    }
                    // Detect tool use for spec file generation
                    if let ContentBlock::ToolUse(tool_use) = block {
                        check_spec_file_written(
                            &tool_use.name,
                            &tool_use.input,
                            feature_dir,
                            event_tx,
                        )
                        .await;
                    }
                }
            }
            Message::Result(ref result) => {
                // Turn is complete -- send accumulated text
                if !turn_text.is_empty()
                    && event_tx
                        .send(PlanEvent::Message(turn_text.clone()))
                        .await
                        .is_err()
                {
                    return TurnOutcome::Error(CoreError::Agent("event channel closed".to_owned()));
                }

                if result.is_error {
                    return TurnOutcome::Error(CoreError::Agent(format!(
                        "plan agent returned error result: {}",
                        result.result.as_deref().unwrap_or("unknown error")
                    )));
                }

                // A successful result means the turn finished
                return TurnOutcome::WaitingForInput;
            }
            // System messages, stream events, user messages -- skip
            _ => {}
        }
    }

    // Stream ended without a Result message
    if !turn_text.is_empty() {
        let _ = event_tx.send(PlanEvent::Message(turn_text)).await;
    }
    TurnOutcome::StreamEnded
}

/// Check if a tool use represents a spec file being written.
///
/// When the agent uses the `Write` tool to create files inside the feature
/// directory, emit a `PlanEvent::SpecGenerated` event so the CLI can
/// display which spec files were created.
async fn check_spec_file_written(
    tool_name: &str,
    input: &serde_json::Value,
    feature_dir: &PathBuf,
    event_tx: &mpsc::Sender<PlanEvent>,
) {
    // The Write tool has `file_path` and `content` fields
    if tool_name != "Write" {
        return;
    }

    let file_path_str = match input.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return,
    };

    let file_path = PathBuf::from(file_path_str);

    // Check if the file is inside the feature directory
    let is_spec_file = file_path.starts_with(feature_dir);
    if !is_spec_file {
        return;
    }

    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    info!(path = %file_path.display(), "spec file generated by plan agent");
    let _ = event_tx
        .send(PlanEvent::SpecGenerated {
            path: file_path,
            content,
        })
        .await;
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::config::EngineConfig;
    use crate::engine::Engine;
    use crate::error::CoreError;
    use crate::events::PlanEvent;

    #[tokio::test]
    async fn test_should_return_not_initialized() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        // Do NOT create .gba/ -- this should fail
        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.plan("test_feature").await;

        assert!(result.is_err());
        assert!(
            matches!(result.as_ref().unwrap_err(), CoreError::NotInitialized),
            "expected NotInitialized, got: {:?}",
            result.unwrap_err()
        );
    }

    #[tokio::test]
    async fn test_should_create_feature_directory() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path().join(".gba");
        std::fs::create_dir_all(&gba_dir).expect("should create .gba dir");

        // Write a minimal config so engine can load
        std::fs::write(gba_dir.join("config.yaml"), "").expect("should write config");

        // Create .trees/ directory
        let trees_dir = dir.path().join(".trees");
        std::fs::create_dir_all(&trees_dir).expect("should create .trees dir");

        // Initialize a git repo so worktree operations can work
        // (the worktree creation will fail since there's no git repo,
        //  but the feature directory should already be created by that point)
        let feature_dir = gba_dir.join("features").join("test_feature");
        let specs_dir = feature_dir.join("specs");

        // Call run_plan directly to test directory creation.
        // We expect it to fail at the worktree step, but the directories
        // should be created before that.
        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();
        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.plan("test_feature").await;

        // The plan call may fail (due to git worktree), but the feature
        // directory should have been created.
        if result.is_err() {
            // Verify the feature directory was created before the error
            assert!(
                feature_dir.exists(),
                "feature directory should exist even if worktree fails"
            );
            assert!(
                specs_dir.exists(),
                "specs directory should exist even if worktree fails"
            );
        }
    }

    #[tokio::test]
    async fn test_should_send_events_through_plan_session() {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(16);

        let mut session = crate::events::PlanSession::new(event_rx, input_tx);

        // Simulate agent sending a message
        event_tx
            .send(PlanEvent::Message("What feature do you want?".to_owned()))
            .await
            .expect("should send");

        let event = session.next().await;
        assert!(
            matches!(&event, Some(PlanEvent::Message(msg)) if msg.contains("feature")),
            "expected Message event, got: {:?}",
            event
        );

        // Simulate agent waiting for input
        event_tx
            .send(PlanEvent::WaitingForInput)
            .await
            .expect("should send");

        let event = session.next().await;
        assert!(
            matches!(event, Some(PlanEvent::WaitingForInput)),
            "expected WaitingForInput"
        );

        // User responds
        session
            .respond("A login page")
            .await
            .expect("should respond");
        let input = input_rx.recv().await;
        assert_eq!(input.as_deref(), Some("A login page"));

        // Simulate spec generated
        event_tx
            .send(PlanEvent::SpecGenerated {
                path: PathBuf::from("/tmp/.gba/features/login/specs/design.md"),
                content: "# Design".to_owned(),
            })
            .await
            .expect("should send");

        let event = session.next().await;
        assert!(
            matches!(event, Some(PlanEvent::SpecGenerated { .. })),
            "expected SpecGenerated"
        );

        // Simulate completion
        event_tx
            .send(PlanEvent::Completed)
            .await
            .expect("should send");

        let event = session.next().await;
        assert!(
            matches!(event, Some(PlanEvent::Completed)),
            "expected Completed"
        );
    }

    #[test]
    fn test_should_detect_spec_file_path() {
        let feature_dir = PathBuf::from("/repo/.gba/features/login");
        let spec_path = PathBuf::from("/repo/.gba/features/login/specs/design.md");
        let non_spec_path = PathBuf::from("/repo/src/main.rs");

        assert!(
            spec_path.starts_with(&feature_dir),
            "spec path should be inside feature dir"
        );
        assert!(
            !non_spec_path.starts_with(&feature_dir),
            "non-spec path should not be inside feature dir"
        );
    }
}
