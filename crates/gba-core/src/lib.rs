//! Core execution engine for GBA -- Claude Agent SDK based automation.
//!
//! This crate provides the [`Engine`] which orchestrates three workflows:
//!
//! - **Init**: Initialize a repository for GBA usage
//! - **Plan**: Interactive planning session to produce feature specs
//! - **Run**: Automated phase-by-phase execution of the plan
//!
//! The CLI layer (`gba-cli`) constructs an [`EngineConfig`], creates an
//! [`Engine`], and drives it using the event stream APIs ([`PlanSession`],
//! [`RunStream`]).

// ── Module declarations ──────────────────────────────────────

mod config;
mod engine;
mod error;
#[allow(dead_code)]
mod events;
#[allow(dead_code)]
mod spec;

// Internal modules (not re-exported).
// Allow dead_code: these are foundational infrastructure used in Phases 3-5.
#[allow(dead_code)]
mod agent;
#[allow(dead_code)]
mod git;
#[allow(dead_code)]
mod hooks;

// ── Public re-exports ────────────────────────────────────────

pub use config::{
    AgentProjectConfig, EngineConfig, GitConfig, Hook, HooksConfig, PermissionMode, ProjectConfig,
    PromptsConfig, ReviewConfig, VerificationConfig,
};
pub use engine::Engine;
pub use error::CoreError;
pub use events::{Issue, PlanEvent, PlanSession, RunEvent, RunStream, Severity};
pub use spec::{
    Execution, FeatureSpec, Phase, PhaseResult, ReviewResult, StepStatus, VerificationPlan,
    VerificationResult,
};
