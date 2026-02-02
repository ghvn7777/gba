use thiserror::Error;

#[derive(Debug, Error)]
pub enum PmError {
    #[error("template not found: {0}")]
    TemplateNotFound(String),

    #[error("render failed: {0}")]
    RenderError(String),

    #[error("invalid template: {0}")]
    InvalidTemplate(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
