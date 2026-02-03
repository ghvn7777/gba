//! Prompt manager for GBA -- template-based prompt generation with Jinja2.
//!
//! This crate provides `PromptManager`, which loads Jinja2 templates at compile
//! time from the built-in `agents/` directory and supports runtime overrides
//! from a custom directory on disk.

mod error;
mod manager;
mod template;

pub use error::PmError;
pub use manager::PromptManager;
pub use template::{AgentConfig, PromptTemplate};
