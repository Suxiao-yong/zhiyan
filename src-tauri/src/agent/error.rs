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
    #[error("tool not found")]
    ToolNotFound,
    #[error("tool version mismatch")]
    ToolVersionMismatch,
    #[error("tool input or output schema is invalid")]
    ToolSchemaInvalid,
    #[error("tool is not rust-owned")]
    OwnershipNotRust,
    #[error("approval required")]
    ApprovalRequired,
    #[error("approval is invalid")]
    ApprovalInvalid,
    #[error("tool timed out")]
    ToolTimeout,
    #[error("idempotency key is required")]
    IdempotencyRequired,
    #[error("idempotency key is already being resolved; retry")]
    IdempotencyConflict,
    #[error("tool ownership is unavailable")]
    OwnershipUnavailable,
}

impl AgentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::NotFound(_) => "not_found",
            Self::Conflict => "conflict",
            Self::Persistence(_) => "persistence_error",
            Self::ToolNotFound => "tool_not_found",
            Self::ToolVersionMismatch => "tool_version_mismatch",
            Self::ToolSchemaInvalid => "tool_schema_invalid",
            Self::OwnershipNotRust => "ownership_not_rust",
            Self::ApprovalRequired => "approval_required",
            Self::ApprovalInvalid => "approval_invalid",
            Self::ToolTimeout => "tool_timeout",
            Self::IdempotencyRequired => "idempotency_required",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::OwnershipUnavailable => "ownership_unavailable",
        }
    }
}
