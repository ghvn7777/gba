use crate::error::CoreError;
use crate::session::Session;

/// Core execution engine that drives the Claude agent loop.
pub struct Engine {
    session: Session,
}

impl Engine {
    /// Create a new engine with the given session configuration.
    pub fn new(session: Session) -> Self {
        Self { session }
    }

    /// Run the agent loop to completion.
    pub async fn run(&self) -> Result<(), CoreError> {
        let _ = &self.session;
        todo!()
    }
}
