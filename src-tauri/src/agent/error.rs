use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("invalid transition from {from} using {event}")]
    InvalidTransition { from: String, event: String },
    #[error("agent record not found: {0}")]
    NotFound(String),
    #[error("agent state changed before the operation completed")]
    Conflict,
    #[error("agent persistence failed: {0}")]
    Persistence(String),
}

impl AgentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::NotFound(_) => "not_found",
            Self::Conflict => "conflict",
            Self::Persistence(_) => "persistence_error",
        }
    }
}
