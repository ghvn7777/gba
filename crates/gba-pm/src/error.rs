//! Error types for the prompt manager crate.

use thiserror::Error;

/// Prompt manager errors.
#[derive(Debug, Error)]
pub enum PmError {
    /// A requested template was not found by name.
    #[error("template not found: {0}")]
    TemplateNotFound(String),

    /// Rendering a template failed (e.g., syntax error or missing variable).
    #[error("render error: {0}")]
    RenderError(String),

    /// A template source is invalid and could not be compiled.
    #[error("invalid template: {0}")]
    InvalidTemplate(String),

    /// An I/O error occurred while reading template files from disk.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A YAML parsing error occurred while reading agent config files.
    #[error("config parse error: {0}")]
    ConfigParse(String),
}
