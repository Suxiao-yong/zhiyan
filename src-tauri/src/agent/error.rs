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
    #[error("plan precondition changed since the preview was generated")]
    PreconditionChanged,
    #[error("tool timed out")]
    ToolTimeout,
    #[error("idempotency key is required")]
    IdempotencyRequired,
    #[error("idempotency key is already being resolved; retry")]
    IdempotencyConflict,
    #[error("tool ownership is unavailable")]
    OwnershipUnavailable,
    #[error("tool input references data outside the run's exam scope")]
    ToolScopeViolation,
    #[error("llm provider is unavailable")]
    ProviderUnavailable,
    #[error("llm provider request failed")]
    ProviderRequestFailed,
    #[error("llm provider authentication failed")]
    ProviderAuthFailed,
    #[error("llm provider is rate limited")]
    ProviderRateLimited,
    #[error("llm provider request timed out")]
    ProviderTimeout,
    #[error("llm provider protocol error")]
    ProviderProtocolError,
    #[error("cloud llm data consent is required")]
    ConsentRequired,
    #[error("llm token budget exhausted")]
    BudgetExhausted,
    #[error("planner reached the maximum tool iterations")]
    MaxIterations,
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
            Self::PreconditionChanged => "precondition_changed",
            Self::ToolTimeout => "tool_timeout",
            Self::IdempotencyRequired => "idempotency_required",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::OwnershipUnavailable => "ownership_unavailable",
            Self::ToolScopeViolation => "tool_scope_violation",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderRequestFailed => "provider_request_failed",
            Self::ProviderAuthFailed => "provider_auth_failed",
            Self::ProviderRateLimited => "provider_rate_limited",
            Self::ProviderTimeout => "provider_timeout",
            Self::ProviderProtocolError => "provider_protocol_error",
            Self::ConsentRequired => "consent_required",
            Self::BudgetExhausted => "budget_exhausted",
            Self::MaxIterations => "max_iterations",
        }
    }
}
