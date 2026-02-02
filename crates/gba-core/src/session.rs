use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// Represents a single agent session bound to a repository.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
pub struct Session {
    /// Path to the target repository
    pub repo_path: PathBuf,

    /// System prompt to use for the session
    #[builder(default)]
    pub system_prompt: Option<String>,

    /// Model identifier (e.g. "claude-sonnet-4-20250514")
    #[builder(default = String::from("claude-sonnet-4-20250514"))]
    pub model: String,

    /// Maximum tokens for agent responses
    #[builder(default = 8192)]
    pub max_tokens: u32,
}
