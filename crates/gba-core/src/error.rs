use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("session error: {0}")]
    Session(String),

    #[error("agent sdk error: {0}")]
    Agent(String),

    #[error("prompt error: {0}")]
    Prompt(#[from] gba_pm::PmError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
