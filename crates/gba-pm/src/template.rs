//! Template and agent configuration types used by the prompt manager.
//!
//! Defines [`PromptTemplate`] for representing template sources and
//! [`AgentConfig`] for agent-level settings parsed from `config.yml` files.

use serde::{Deserialize, Serialize};

/// Metadata about a prompt template, including its name and source content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// Template name used for lookup (e.g., `init/system`).
    pub name: String,

    /// Raw Jinja2 template source.
    pub source: String,
}

/// Configuration for an agent, loaded from `config.yml` in an agent directory.
///
/// Controls whether the agent uses the Claude Code preset (built-in tools)
/// or runs as a plain text-analysis agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AgentConfig {
    /// Whether the agent uses the Claude Code preset.
    /// `true` means the agent gets built-in tools; `false` means pure text analysis.
    pub preset: bool,

    /// Restrict to specific tools. Empty means all tools are available.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Disallow specific tools. Empty means nothing is disallowed.
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
}
