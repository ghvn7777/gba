//! Run workflow implementation.
//!
//! Implements the automated phase-by-phase execution pipeline for a feature.
//! The workflow loads a feature spec (`phases.yaml`), ensures a git worktree
//! exists, executes each phase via the coding agent, runs precommit hooks,
//! performs code review and verification, and finally creates a pull request.
//!
//! Progress is reported through [`RunEvent`] on a channel consumed by the CLI
//! via [`RunStream`].
//!
//! # Edge cases
//!
//! - **Empty phases list**: if `phases.yaml` has no phases, execution skips
//!   directly to the review/verification steps.
//! - **Missing verification commands**: verification is skipped when no test
//!   commands are defined.
//! - **Missing design spec**: a warning is logged and an empty string is used
//!   so the coding agent still receives valid context.
//! - **Resume support**: completed phases are detected and skipped automatically.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use claude_agent_sdk_rs::{ContentBlock, Message};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, error, info, instrument, warn};

use crate::agent::AgentRunner;
use crate::config::{HooksConfig, ReviewConfig, VerificationConfig};
use crate::engine::Engine;
use crate::error::CoreError;
use crate::events::{Issue, RunEvent, RunStream, Severity};
use crate::git::GitOps;
use crate::hooks::HookRunner;
use crate::spec::{
    Execution, FeatureSpec, PhaseResult, ReviewResult, StepStatus, VerificationResult,
    load_design_spec, load_feature_spec, save_feature_spec,
};

/// Channel buffer size for run events.
const EVENT_CHANNEL_SIZE: usize = 64;

/// Context passed to the background execution task.
///
/// Contains all the owned/cloned components the background task needs,
/// since `Engine` itself cannot be moved into a `tokio::spawn`.
#[derive(Debug)]
struct RunContext {
    /// Agent runner, shared via Arc since it is not Clone.
    agent_runner: Arc<AgentRunner>,
    /// Git operations helper (cloned from engine).
    git: GitOps,
    /// Hooks configuration.
    hooks_config: HooksConfig,
    /// Review configuration.
    review_config: ReviewConfig,
    /// Verification configuration.
    verification_config: VerificationConfig,
    /// Path to the `.gba` directory.
    gba_dir: PathBuf,
    /// Path to the repository root.
    repo_path: PathBuf,
    /// Base branch name.
    base_branch: String,
    /// Auto-commit setting.
    auto_commit: bool,
}

/// Start the run execution workflow.
///
/// Verifies the repository is initialized, loads the feature spec, sets up
/// channels, and spawns a background task that executes all phases.
///
/// # Errors
///
/// Returns `CoreError::NotInitialized` if `.gba/` does not exist.
/// Returns `CoreError::FeatureNotFound` if the feature spec does not exist.
#[instrument(skip(engine))]
pub(crate) async fn run_execution(engine: &Engine, slug: &str) -> Result<RunStream, CoreError> {
    // Verify initialized
    let gba_dir = engine.gba_dir();
    if !gba_dir.exists() {
        return Err(CoreError::NotInitialized);
    }

    // Load feature spec
    let spec = load_feature_spec(&gba_dir, slug)?;

    // Load design spec for agent context (warn but continue if missing)
    let design_spec = match load_design_spec(&gba_dir, slug) {
        Ok(content) => content,
        Err(CoreError::FeatureNotFound(msg)) => {
            warn!(slug, reason = %msg, "design spec missing, continuing with empty context");
            String::new()
        }
        Err(e) => return Err(e),
    };

    // Ensure worktree exists
    let worktree_path = engine.git().ensure_worktree(slug).await?;
    info!(worktree = %worktree_path.display(), "worktree ready");

    // Create event channels
    let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
    let stream = RunStream::new(event_rx);

    // Build the context for the background task
    let project_config = engine.project_config().clone();
    let ctx = RunContext {
        agent_runner: engine.agent_runner_arc(),
        git: engine.git().clone(),
        hooks_config: project_config.hooks.clone(),
        review_config: project_config.review.clone(),
        verification_config: project_config.verification.clone(),
        gba_dir: gba_dir.clone(),
        repo_path: engine.config().repo_path().clone(),
        base_branch: project_config.git.base_branch.clone(),
        auto_commit: project_config.git.auto_commit,
    };

    let slug_owned = slug.to_owned();

    // Spawn background execution task
    tokio::spawn(async move {
        execute_phases(ctx, slug_owned, spec, design_spec, event_tx).await;
    });

    Ok(stream)
}

/// Execute all phases, review, verification, and PR creation in the background.
///
/// Sends [`RunEvent`]s on the channel as each step completes. If any step
/// fails, sends a [`RunEvent::Error`] and returns. The spec is saved after
/// each phase so that a resume picks up where execution left off.
#[instrument(skip_all, fields(slug = %slug, total_phases = spec.phases.len()))]
async fn execute_phases(
    ctx: RunContext,
    slug: String,
    mut spec: FeatureSpec,
    design_spec: String,
    event_tx: mpsc::Sender<RunEvent>,
) {
    let total_phases = spec.phases.len();

    // Send Started event
    if send_event(
        &event_tx,
        RunEvent::Started {
            feature: spec.feature.clone(),
            total_phases,
        },
    )
    .await
    .is_err()
    {
        return;
    }

    let worktree_path = ctx.git.worktree_path(&slug);
    let mut total_turns: u32 = 0;

    // Handle empty phases list -- skip directly to review/verification
    if total_phases == 0 {
        info!(slug = %slug, "no phases defined, skipping to review/verification");
    }

    // ── Phase Execution ──────────────────────────────────────────
    let completed_phases = collect_completed_phases(&spec);

    for index in 0..total_phases {
        // Skip already completed phases (resume support)
        if let Some(ref result) = spec.phases[index].result
            && result.status == StepStatus::Completed
        {
            debug!(index, name = %spec.phases[index].name, "skipping completed phase");
            continue;
        }

        let phase_name = spec.phases[index].name.clone();

        if send_event(
            &event_tx,
            RunEvent::PhaseStarted {
                index,
                name: phase_name.clone(),
            },
        )
        .await
        .is_err()
        {
            return;
        }

        // Run coding agent for this phase
        let phase_ctx = PhaseContext {
            slug: &slug,
            design_spec: &design_spec,
            phase: &spec.phases[index],
            index,
            total_phases,
            completed_phases: &completed_phases,
            worktree_path: &worktree_path,
        };
        let agent_result = run_coding_phase(&ctx, &phase_ctx).await;

        let turns = match agent_result {
            Ok(t) => t,
            Err(e) => {
                // Save spec on failure so resume picks up here
                spec.phases[index].result = Some(PhaseResult {
                    status: StepStatus::Failed,
                    turns: 0,
                    commit: None,
                });
                if let Err(save_err) = save_feature_spec(&ctx.gba_dir, &slug, &spec) {
                    warn!(error = %save_err, "failed to save spec after phase failure");
                }
                let _ = send_event(&event_tx, RunEvent::Error(e)).await;
                return;
            }
        };
        total_turns = total_turns.saturating_add(turns);

        // Run precommit hooks if configured
        let hook_result = run_hooks_cycle(&ctx, &slug, &worktree_path, &event_tx).await;
        if let Err(e) = hook_result {
            let _ = send_event(&event_tx, RunEvent::Error(e)).await;
            return;
        }

        // Commit if auto_commit is enabled
        let commit_hash = if ctx.auto_commit {
            let commit_msg = format!("feat({}): phase {} - {}", slug, index + 1, phase_name);
            match ctx.git.commit(&worktree_path, &commit_msg).await {
                Ok(hash) => {
                    info!(hash = %hash, phase = index + 1, "committed phase");
                    Some(hash)
                }
                Err(CoreError::Git(msg)) if msg.contains("nothing to commit") => {
                    debug!(phase = index + 1, "no changes to commit for phase");
                    None
                }
                Err(e) => {
                    let _ = send_event(&event_tx, RunEvent::Error(e)).await;
                    return;
                }
            }
        } else {
            None
        };

        // Update phase result
        spec.phases[index].result = Some(PhaseResult {
            status: StepStatus::Completed,
            turns,
            commit: commit_hash.clone(),
        });

        // Persist spec after each phase
        if let Err(e) = save_feature_spec(&ctx.gba_dir, &slug, &spec) {
            let _ = send_event(&event_tx, RunEvent::Error(e)).await;
            return;
        }

        if send_event(
            &event_tx,
            RunEvent::PhaseCommitted {
                index,
                commit_hash: commit_hash.unwrap_or_else(|| "(no changes)".to_owned()),
            },
        )
        .await
        .is_err()
        {
            return;
        }
    }

    // ── Code Review ──────────────────────────────────────────────
    let review_result = if ctx.review_config.enabled {
        if send_event(&event_tx, RunEvent::ReviewStarted)
            .await
            .is_err()
        {
            return;
        }

        match run_review_cycle(&ctx, &slug, &spec, &design_spec, &worktree_path).await {
            Ok(result) => {
                total_turns = total_turns.saturating_add(result.turns);
                let issues_count = result.issues_found;
                if send_event(
                    &event_tx,
                    RunEvent::ReviewCompleted {
                        issues: Vec::new(), // summary only, details in spec
                    },
                )
                .await
                .is_err()
                {
                    return;
                }
                debug!(issues_found = issues_count, "review completed");
                result
            }
            Err(e) => {
                let _ = send_event(&event_tx, RunEvent::Error(e)).await;
                return;
            }
        }
    } else {
        ReviewResult {
            turns: 0,
            issues_found: 0,
            issues_fixed: 0,
        }
    };

    // ── Verification ─────────────────────────────────────────────
    // Skip verification when no test commands are defined
    let skip_verification =
        spec.verification.test_commands.is_empty() && spec.verification.criteria.is_empty();
    if skip_verification {
        debug!("no verification commands or criteria defined, skipping verification step");
    }

    let verification_result = if ctx.verification_config.enabled && !skip_verification {
        if send_event(&event_tx, RunEvent::VerificationStarted)
            .await
            .is_err()
        {
            return;
        }

        match run_verification_cycle(&ctx, &slug, &spec, &design_spec, &worktree_path).await {
            Ok(result) => {
                total_turns = total_turns.saturating_add(result.turns);
                let passed = result.passed;
                let details = if passed {
                    "all criteria passed".to_owned()
                } else {
                    "some criteria failed".to_owned()
                };
                if send_event(
                    &event_tx,
                    RunEvent::VerificationCompleted { passed, details },
                )
                .await
                .is_err()
                {
                    return;
                }
                result
            }
            Err(e) => {
                let _ = send_event(&event_tx, RunEvent::Error(e)).await;
                return;
            }
        }
    } else {
        VerificationResult {
            turns: 0,
            passed: true,
        }
    };

    // ── PR Creation ──────────────────────────────────────────────
    let pr_url = match create_pr(&ctx, &slug, &spec, &review_result, &verification_result).await {
        Ok(url) => {
            if send_event(&event_tx, RunEvent::PrCreated { url: url.clone() })
                .await
                .is_err()
            {
                return;
            }
            Some(url)
        }
        Err(e) => {
            warn!(error = %e, "PR creation failed, continuing");
            let _ = send_event(
                &event_tx,
                RunEvent::Error(CoreError::Agent(format!("PR creation failed: {e}"))),
            )
            .await;
            None
        }
    };

    // ── Update Execution Summary ─────────────────────────────────
    spec.execution = Some(Execution {
        status: StepStatus::Completed,
        total_turns,
        review: review_result,
        verification: verification_result,
        pr: pr_url,
    });

    if let Err(e) = save_feature_spec(&ctx.gba_dir, &slug, &spec) {
        let _ = send_event(&event_tx, RunEvent::Error(e)).await;
        return;
    }

    let _ = send_event(&event_tx, RunEvent::Finished).await;
    info!(slug = %slug, total_turns, "run execution finished");
}

// ── Phase Helpers ────────────────────────────────────────────

/// Collect information about completed phases for resume context.
fn collect_completed_phases(spec: &FeatureSpec) -> Vec<serde_json::Value> {
    spec.phases
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let result = p.result.as_ref()?;
            if result.status != StepStatus::Completed {
                return None;
            }
            Some(json!({
                "index": i + 1,
                "name": p.name,
                "commit": result.commit.as_deref().unwrap_or("unknown"),
            }))
        })
        .collect()
}

/// Context for a single coding phase execution.
#[derive(Debug)]
struct PhaseContext<'a> {
    /// Feature slug.
    slug: &'a str,
    /// Design specification content.
    design_spec: &'a str,
    /// The phase to execute.
    phase: &'a crate::spec::Phase,
    /// Zero-based phase index.
    index: usize,
    /// Total number of phases.
    total_phases: usize,
    /// Previously completed phases (for resume context).
    completed_phases: &'a [serde_json::Value],
    /// Path to the worktree.
    worktree_path: &'a Path,
}

/// Run the coding agent for a single phase.
///
/// If there are completed phases, uses the resume template; otherwise uses
/// the fresh task template. Returns the number of turns consumed.
#[instrument(skip_all, fields(index = phase_ctx.index, slug = phase_ctx.slug))]
async fn run_coding_phase(
    ctx: &RunContext,
    phase_ctx: &PhaseContext<'_>,
) -> Result<u32, CoreError> {
    let phase_json = serde_json::to_value(phase_ctx.phase)
        .map_err(|e| CoreError::Agent(format!("failed to serialize phase: {e}")))?;

    let system_context = json!({
        "repo_path": ctx.repo_path.display().to_string(),
        "feature_slug": phase_ctx.slug,
        "design_spec": phase_ctx.design_spec,
    });

    let task_template = if phase_ctx.completed_phases.is_empty() {
        "code/task"
    } else {
        "code/resume"
    };

    let task_context = json!({
        "phase": phase_json,
        "phase_index": phase_ctx.index + 1,
        "total_phases": phase_ctx.total_phases,
        "completed_phases": phase_ctx.completed_phases,
    });

    // Merge system and task context for the agent
    let mut full_context = system_context;
    if let serde_json::Value::Object(task_map) = task_context
        && let serde_json::Value::Object(ref mut full_map) = full_context
    {
        full_map.extend(task_map);
    }

    let messages = ctx
        .agent_runner
        .run_agent(
            "code",
            task_template,
            &full_context,
            Some(phase_ctx.worktree_path),
        )
        .await?;

    let turns = extract_turn_count(&messages);
    debug!(turns, phase = phase_ctx.index + 1, "coding phase completed");
    Ok(turns)
}

// ── Hook Helpers ─────────────────────────────────────────────

/// Run precommit hooks and retry with agent fixes if any fail.
///
/// Iterates up to `max_retries` times. On each failure, sends the hook output
/// to the coding agent with the `code/hook_fix` template, then re-runs hooks.
#[instrument(skip(ctx, worktree_path, event_tx))]
async fn run_hooks_cycle(
    ctx: &RunContext,
    slug: &str,
    worktree_path: &Path,
    event_tx: &mpsc::Sender<RunEvent>,
) -> Result<(), CoreError> {
    let runner = HookRunner::new(&ctx.hooks_config);
    if !runner.has_hooks() {
        return Ok(());
    }

    let max_retries = runner.max_retries();

    for attempt in 0..=max_retries {
        let results = runner.run_all(worktree_path).await?;

        // Report each hook result
        for result in &results {
            let _ = send_event(
                event_tx,
                RunEvent::HookResult {
                    hook: result.name.clone(),
                    passed: result.passed,
                },
            )
            .await;
        }

        // Check if all hooks passed
        let all_passed = results.iter().all(|r| r.passed);
        if all_passed {
            return Ok(());
        }

        // If we've exhausted retries, fail
        if attempt >= max_retries {
            let failed_hooks: Vec<&str> = results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| r.name.as_str())
                .collect();
            error!(
                failed_hooks = ?failed_hooks,
                max_retries,
                "hooks failed after exhausting retries"
            );
            return Err(CoreError::Hook(format!(
                "hooks failed after {} retries: {}",
                max_retries,
                failed_hooks.join(", ")
            )));
        }

        // Run coding agent with hook_fix template for each failed hook
        for result in &results {
            if result.passed {
                continue;
            }

            debug!(hook = %result.name, attempt, "running hook fix agent");
            let hook_output = format!("{}\n{}", result.stdout, result.stderr);
            let context = json!({
                "repo_path": ctx.repo_path.display().to_string(),
                "feature_slug": slug,
                "design_spec": "",
                "hook_name": result.name,
                "hook_command": result.command,
                "hook_output": hook_output,
            });

            ctx.agent_runner
                .run_agent("code", "code/hook_fix", &context, Some(worktree_path))
                .await?;
        }
    }

    Ok(())
}

// ── Review Helpers ───────────────────────────────────────────

/// Run the code review loop.
///
/// Gets the diff, runs the review agent, parses issues, and if issues are
/// found, runs the coding agent with fix instructions. Repeats up to
/// `max_iterations`.
#[instrument(skip(ctx, spec, design_spec, worktree_path))]
async fn run_review_cycle(
    ctx: &RunContext,
    slug: &str,
    spec: &FeatureSpec,
    design_spec: &str,
    worktree_path: &Path,
) -> Result<ReviewResult, CoreError> {
    let max_iterations = ctx.review_config.max_iterations;
    let mut total_turns: u32 = 0;
    let mut total_issues_found: u32 = 0;
    let mut total_issues_fixed: u32 = 0;

    for iteration in 0..max_iterations {
        // Get diff against base branch
        let diff = ctx
            .git
            .get_diff(worktree_path, &ctx.base_branch)
            .await
            .unwrap_or_default();

        if diff.is_empty() {
            debug!("no diff to review");
            break;
        }

        // Run review agent (non-preset, pure text analysis)
        let review_context = json!({
            "repo_path": ctx.repo_path.display().to_string(),
            "feature_slug": slug,
            "design_spec": design_spec,
            "verification_criteria": spec.verification.criteria,
            "diff": diff,
        });

        let messages = ctx
            .agent_runner
            .run_agent("review", "review/task", &review_context, None)
            .await?;

        let turns = extract_turn_count(&messages);
        total_turns = total_turns.saturating_add(turns);

        // Extract text output from review agent
        let review_output = extract_text_from_messages(&messages);
        let issues = parse_review_issues(&review_output);

        if issues.is_empty() {
            debug!(iteration, "review found no issues");
            break;
        }

        let issue_count = issues.len() as u32;
        total_issues_found = total_issues_found.saturating_add(issue_count);
        info!(iteration, issues = issue_count, "review found issues");

        // Run coding agent to fix issues
        let issues_json: Vec<serde_json::Value> = issues
            .iter()
            .map(|issue| {
                json!({
                    "severity": format!("{:?}", issue.severity).to_lowercase(),
                    "file": issue.file.display().to_string(),
                    "description": issue.description,
                })
            })
            .collect();

        let fix_context = json!({
            "repo_path": ctx.repo_path.display().to_string(),
            "feature_slug": slug,
            "design_spec": design_spec,
            "issues": issues_json,
        });

        let fix_messages = ctx
            .agent_runner
            .run_agent("code", "review/fix", &fix_context, Some(worktree_path))
            .await?;

        let fix_turns = extract_turn_count(&fix_messages);
        total_turns = total_turns.saturating_add(fix_turns);
        total_issues_fixed = total_issues_fixed.saturating_add(issue_count);

        // Commit review fixes
        if ctx.auto_commit {
            let commit_msg = format!("fix({}): review iteration {} fixes", slug, iteration + 1);
            match ctx.git.commit(worktree_path, &commit_msg).await {
                Ok(hash) => debug!(hash = %hash, "committed review fixes"),
                Err(CoreError::Git(msg)) if msg.contains("nothing to commit") => {
                    debug!("no review fix changes to commit");
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(ReviewResult {
        turns: total_turns,
        issues_found: total_issues_found,
        issues_fixed: total_issues_fixed,
    })
}

// ── Verification Helpers ─────────────────────────────────────

/// Run the verification loop.
///
/// Runs the verify agent with test commands, and if verification fails,
/// runs the coding agent to fix issues. Repeats up to `max_iterations`.
#[instrument(skip(ctx, spec, design_spec, worktree_path))]
async fn run_verification_cycle(
    ctx: &RunContext,
    slug: &str,
    spec: &FeatureSpec,
    design_spec: &str,
    worktree_path: &Path,
) -> Result<VerificationResult, CoreError> {
    let max_iterations = ctx.verification_config.max_iterations;
    let mut total_turns: u32 = 0;

    for iteration in 0..max_iterations {
        // Run verify agent
        let verify_context = json!({
            "repo_path": ctx.repo_path.display().to_string(),
            "feature_slug": slug,
            "design_spec": design_spec,
            "criteria": spec.verification.criteria,
            "test_commands": spec.verification.test_commands,
        });

        let messages = ctx
            .agent_runner
            .run_agent(
                "verify",
                "verify/task",
                &verify_context,
                Some(worktree_path),
            )
            .await?;

        let turns = extract_turn_count(&messages);
        total_turns = total_turns.saturating_add(turns);

        // Check result -- the verify agent's result message indicates pass/fail
        let verify_output = extract_text_from_messages(&messages);
        let passed = check_verification_passed(&messages, &verify_output);

        if passed {
            debug!(iteration, "verification passed");
            return Ok(VerificationResult {
                turns: total_turns,
                passed: true,
            });
        }

        info!(iteration, "verification failed, running fix agent");

        // If this is the last iteration, return failure
        if iteration + 1 >= max_iterations {
            break;
        }

        // Run coding agent to fix verification failures
        let fix_context = json!({
            "repo_path": ctx.repo_path.display().to_string(),
            "feature_slug": slug,
            "design_spec": design_spec,
            "failures": [],
            "output": verify_output,
        });

        let fix_messages = ctx
            .agent_runner
            .run_agent("code", "verify/fix", &fix_context, Some(worktree_path))
            .await?;

        let fix_turns = extract_turn_count(&fix_messages);
        total_turns = total_turns.saturating_add(fix_turns);

        // Commit verification fixes
        if ctx.auto_commit {
            let commit_msg = format!(
                "fix({}): verification iteration {} fixes",
                slug,
                iteration + 1
            );
            match ctx.git.commit(worktree_path, &commit_msg).await {
                Ok(hash) => debug!(hash = %hash, "committed verification fixes"),
                Err(CoreError::Git(msg)) if msg.contains("nothing to commit") => {
                    debug!("no verification fix changes to commit");
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(VerificationResult {
        turns: total_turns,
        passed: false,
    })
}

// ── PR Creation ──────────────────────────────────────────────

/// Create a pull request via the coding agent.
///
/// Renders the `code/pr` template and runs the code agent, which uses
/// the `gh` CLI to create the PR. Extracts the PR URL from the agent's
/// output.
#[instrument(skip(ctx, spec, review_result, verification_result))]
async fn create_pr(
    ctx: &RunContext,
    slug: &str,
    spec: &FeatureSpec,
    review_result: &ReviewResult,
    verification_result: &VerificationResult,
) -> Result<String, CoreError> {
    let branch = ctx.git.branch_name(slug);
    let worktree_path = ctx.git.worktree_path(slug);

    let phases_json: Vec<serde_json::Value> = spec
        .phases
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "result": p.result.as_ref().map(|r| json!({
                    "turns": r.turns,
                    "commit": r.commit.as_deref().unwrap_or("unknown"),
                })),
            })
        })
        .collect();

    let pr_context = json!({
        "repo_path": ctx.repo_path.display().to_string(),
        "feature_slug": slug,
        "design_spec": "",
        "feature_description": spec.feature,
        "branch": branch,
        "base_branch": ctx.base_branch,
        "phases": phases_json,
        "review": {
            "issues_found": review_result.issues_found,
            "issues_fixed": review_result.issues_fixed,
        },
        "verification": {
            "passed": verification_result.passed,
        },
    });

    let messages = ctx
        .agent_runner
        .run_agent("code", "code/pr", &pr_context, Some(&worktree_path))
        .await?;

    // Extract PR URL from agent output
    let output = extract_text_from_messages(&messages);
    let url = extract_pr_url(&output)
        .unwrap_or_else(|| format!("(PR URL not detected in agent output: {output})"));

    Ok(url)
}

// ── Text Extraction / Parsing ────────────────────────────────

/// Extract text content from a list of agent messages.
fn extract_text_from_messages(messages: &[Message]) -> String {
    let mut text = String::new();
    for msg in messages {
        if let Message::Assistant(assistant) = msg {
            for block in &assistant.message.content {
                if let ContentBlock::Text(text_block) = block {
                    text.push_str(&text_block.text);
                    text.push('\n');
                }
            }
        }
    }
    text
}

/// Extract the turn count from the Result message in a collected session.
fn extract_turn_count(messages: &[Message]) -> u32 {
    for msg in messages {
        if let Message::Result(result) = msg {
            return result.num_turns;
        }
    }
    1 // default to 1 if no result message found
}

/// Check whether verification passed based on agent output.
///
/// Looks at the Result message's `is_error` field and scans the text
/// output for failure indicators.
fn check_verification_passed(messages: &[Message], output: &str) -> bool {
    // Check if the result message indicates an error
    for msg in messages {
        if let Message::Result(result) = msg
            && result.is_error
        {
            return false;
        }
    }

    // Heuristic: check for failure keywords in the output
    let lower = output.to_lowercase();
    let has_fail = lower.contains("fail") || lower.contains("error");
    let has_pass = lower.contains("pass") || lower.contains("success");

    // If both or neither, default to checking if no explicit failure
    if has_fail && !has_pass {
        return false;
    }

    !has_fail || has_pass
}

/// Parse review issues from the review agent's text output.
///
/// Expects issues in the format:
/// ```text
/// - severity: error
///   file: src/main.rs
///   description: Missing error handling
/// ```
///
/// Also handles inline formats like:
/// ```text
/// - [error] src/main.rs: Missing error handling
/// ```
pub(crate) fn parse_review_issues(output: &str) -> Vec<Issue> {
    let mut issues = Vec::new();

    // Try block format first
    let block_issues = parse_block_format(output);
    if !block_issues.is_empty() {
        return block_issues;
    }

    // Try inline format: - [severity] file: description
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(issue) = parse_inline_issue(trimmed) {
            issues.push(issue);
        }
    }

    issues
}

/// Parse issues in the block format (YAML-like).
fn parse_block_format(output: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut current_severity: Option<Severity> = None;
    let mut current_file: Option<String> = None;
    let mut current_description: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();

        // Check for severity field
        if let Some(rest) = trimmed
            .strip_prefix("severity:")
            .or_else(|| trimmed.strip_prefix("- severity:").map(|s| s.trim_start()))
        {
            // Flush previous issue if any
            if let (Some(sev), Some(file), Some(desc)) =
                (&current_severity, &current_file, &current_description)
            {
                issues.push(Issue {
                    severity: sev.clone(),
                    file: PathBuf::from(file),
                    description: desc.clone(),
                });
            }
            current_severity = parse_severity(rest.trim());
            current_file = None;
            current_description = None;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("file:") {
            current_file = Some(rest.trim().to_owned());
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("description:") {
            current_description = Some(rest.trim().to_owned());
            continue;
        }
    }

    // Flush the last issue
    if let (Some(sev), Some(file), Some(desc)) =
        (current_severity, current_file, current_description)
    {
        issues.push(Issue {
            severity: sev,
            file: PathBuf::from(file),
            description: desc,
        });
    }

    issues
}

/// Parse a single inline issue in the format: `- [severity] file: description`
fn parse_inline_issue(line: &str) -> Option<Issue> {
    let content = line.strip_prefix('-')?.trim();

    // Match [severity]
    let content = content.strip_prefix('[')?;
    let bracket_end = content.find(']')?;
    let severity_str = &content[..bracket_end];
    let rest = content[bracket_end + 1..].trim();

    let severity = parse_severity(severity_str)?;

    // Match file: description
    let colon_pos = rest.find(':')?;
    let file = rest[..colon_pos].trim();
    let description = rest[colon_pos + 1..].trim();

    if file.is_empty() || description.is_empty() {
        return None;
    }

    Some(Issue {
        severity,
        file: PathBuf::from(file),
        description: description.to_owned(),
    })
}

/// Parse a severity string to a [`Severity`] enum variant.
fn parse_severity(s: &str) -> Option<Severity> {
    match s.to_lowercase().trim() {
        "error" => Some(Severity::Error),
        "warning" | "warn" => Some(Severity::Warning),
        "suggestion" | "info" | "note" => Some(Severity::Suggestion),
        _ => None,
    }
}

/// Extract a PR URL from agent text output.
///
/// Looks for common GitHub PR URL patterns in the output text.
fn extract_pr_url(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        // Look for GitHub PR URLs
        if let Some(start) = trimmed.find("https://github.com/") {
            let url_part = &trimmed[start..];
            // Find the end of the URL (whitespace, quote, or end of line)
            let end = url_part
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
                .unwrap_or(url_part.len());
            let url = &url_part[..end];
            if url.contains("/pull/") {
                return Some(url.to_owned());
            }
        }
    }
    None
}

/// Send an event on the channel, returning an error if the receiver is gone.
async fn send_event(tx: &mpsc::Sender<RunEvent>, event: RunEvent) -> Result<(), ()> {
    tx.send(event).await.map_err(|_| {
        debug!("run event channel closed");
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::EngineConfig;
    use crate::engine::Engine;
    use crate::spec::{FeatureSpec, Phase, PhaseResult, StepStatus, VerificationPlan};

    #[test]
    fn test_should_parse_review_issues_block_format() {
        let output = r"
Here are the issues found:

- severity: error
  file: src/main.rs
  description: Missing error handling for database connection

- severity: warning
  file: src/lib.rs
  description: Consider using a more descriptive variable name

- severity: suggestion
  file: tests/integration.rs
  description: Add more edge case tests
";

        let issues = parse_review_issues(output);

        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].file, PathBuf::from("src/main.rs"));
        assert!(issues[0].description.contains("Missing error handling"));

        assert_eq!(issues[1].severity, Severity::Warning);
        assert_eq!(issues[1].file, PathBuf::from("src/lib.rs"));

        assert_eq!(issues[2].severity, Severity::Suggestion);
        assert_eq!(issues[2].file, PathBuf::from("tests/integration.rs"));
    }

    #[test]
    fn test_should_parse_review_issues_inline_format() {
        let output = r"
Review complete. Issues:
- [error] src/main.rs: Missing error handling
- [warning] src/config.rs: Unused import
- [suggestion] src/lib.rs: Consider extracting this function
";

        let issues = parse_review_issues(output);

        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].file, PathBuf::from("src/main.rs"));
        assert_eq!(issues[0].description, "Missing error handling");

        assert_eq!(issues[1].severity, Severity::Warning);
        assert_eq!(issues[1].file, PathBuf::from("src/config.rs"));

        assert_eq!(issues[2].severity, Severity::Suggestion);
    }

    #[test]
    fn test_should_parse_no_issues() {
        let output = r"
Code review complete. No issues found. The implementation looks good
and follows all the project conventions.
";

        let issues = parse_review_issues(output);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_should_parse_empty_output() {
        let issues = parse_review_issues("");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_should_identify_completed_phases() {
        let spec = FeatureSpec {
            feature: "Test feature".to_owned(),
            phases: vec![
                Phase {
                    name: "Phase 1".to_owned(),
                    description: "First phase".to_owned(),
                    tasks: vec!["Task A".to_owned()],
                    result: Some(PhaseResult {
                        status: StepStatus::Completed,
                        turns: 5,
                        commit: Some("abc123".to_owned()),
                    }),
                },
                Phase {
                    name: "Phase 2".to_owned(),
                    description: "Second phase".to_owned(),
                    tasks: vec!["Task B".to_owned()],
                    result: None,
                },
                Phase {
                    name: "Phase 3".to_owned(),
                    description: "Third phase".to_owned(),
                    tasks: vec!["Task C".to_owned()],
                    result: Some(PhaseResult {
                        status: StepStatus::Failed,
                        turns: 2,
                        commit: None,
                    }),
                },
            ],
            verification: VerificationPlan {
                criteria: vec!["Tests pass".to_owned()],
                test_commands: vec!["cargo test".to_owned()],
            },
            execution: None,
        };

        let completed = collect_completed_phases(&spec);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0]["name"], "Phase 1");
        assert_eq!(completed[0]["commit"], "abc123");
        assert_eq!(completed[0]["index"], 1);
    }

    #[test]
    fn test_should_identify_no_completed_phases() {
        let spec = FeatureSpec {
            feature: "Test".to_owned(),
            phases: vec![Phase {
                name: "Phase 1".to_owned(),
                description: "First".to_owned(),
                tasks: vec!["Task".to_owned()],
                result: None,
            }],
            verification: VerificationPlan {
                criteria: vec![],
                test_commands: vec![],
            },
            execution: None,
        };

        let completed = collect_completed_phases(&spec);
        assert!(completed.is_empty());
    }

    #[tokio::test]
    async fn test_should_return_not_initialized() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.run("test_feature").await;

        assert!(result.is_err());
        assert!(
            matches!(result.as_ref().unwrap_err(), CoreError::NotInitialized),
            "expected NotInitialized, got: {:?}",
            result.unwrap_err()
        );
    }

    #[tokio::test]
    async fn test_should_return_feature_not_found() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path().join(".gba");
        std::fs::create_dir_all(&gba_dir).expect("should create .gba dir");

        let config = EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = Engine::new(config).await.expect("should create engine");
        let result = engine.run("nonexistent_feature").await;

        assert!(result.is_err());
        assert!(
            matches!(result.as_ref().unwrap_err(), CoreError::FeatureNotFound(_)),
            "expected FeatureNotFound, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn test_should_parse_severity_variants() {
        assert_eq!(parse_severity("error"), Some(Severity::Error));
        assert_eq!(parse_severity("Error"), Some(Severity::Error));
        assert_eq!(parse_severity("ERROR"), Some(Severity::Error));
        assert_eq!(parse_severity("warning"), Some(Severity::Warning));
        assert_eq!(parse_severity("warn"), Some(Severity::Warning));
        assert_eq!(parse_severity("suggestion"), Some(Severity::Suggestion));
        assert_eq!(parse_severity("info"), Some(Severity::Suggestion));
        assert_eq!(parse_severity("note"), Some(Severity::Suggestion));
        assert_eq!(parse_severity("unknown"), None);
    }

    #[test]
    fn test_should_extract_pr_url() {
        let output = r#"
PR created successfully!
https://github.com/org/repo/pull/42

Done.
"#;
        let url = extract_pr_url(output);
        assert_eq!(url.as_deref(), Some("https://github.com/org/repo/pull/42"));
    }

    #[test]
    fn test_should_extract_pr_url_with_surrounding_text() {
        let output = "Created PR: https://github.com/user/project/pull/123 successfully";
        let url = extract_pr_url(output);
        assert_eq!(
            url.as_deref(),
            Some("https://github.com/user/project/pull/123")
        );
    }

    #[test]
    fn test_should_return_none_for_no_pr_url() {
        let output = "No PR was created. Something went wrong.";
        let url = extract_pr_url(output);
        assert!(url.is_none());
    }

    #[test]
    fn test_should_return_none_for_non_pr_github_url() {
        let output = "See https://github.com/org/repo/issues/5 for details";
        let url = extract_pr_url(output);
        assert!(url.is_none());
    }

    #[test]
    fn test_should_check_verification_passed() {
        let pass_output = "All tests passed successfully. Verification complete.";
        assert!(check_verification_passed(&[], pass_output));

        let fail_output = "Test failed: expected 4 but got 5. Error in module X.";
        assert!(!check_verification_passed(&[], fail_output));

        let ambiguous_output = "Tests passed with some warnings.";
        assert!(check_verification_passed(&[], ambiguous_output));
    }

    #[test]
    fn test_should_parse_inline_issue_correctly() {
        let line = "- [error] src/main.rs: Missing error handling";
        let issue = parse_inline_issue(line.trim());
        assert!(issue.is_some());
        let issue = issue.expect("should parse");
        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(issue.file, PathBuf::from("src/main.rs"));
        assert_eq!(issue.description, "Missing error handling");
    }

    #[test]
    fn test_should_reject_malformed_inline_issue() {
        assert!(parse_inline_issue("not an issue").is_none());
        assert!(parse_inline_issue("- [error]").is_none());
        assert!(parse_inline_issue("- [error] :").is_none());
        assert!(parse_inline_issue("- [unknown] file: desc").is_none());
    }
}
