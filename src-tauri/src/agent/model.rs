use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    WaitingApproval,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEvent {
    Start,
    RequestApproval,
    Approve,
    Reject,
    Complete,
    Fail,
    Cancel,
    Interrupt,
    Resume,
}

impl std::fmt::Display for RunEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Start => "start",
            Self::RequestApproval => "request_approval",
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Complete => "complete",
            Self::Fail => "fail",
            Self::Cancel => "cancel",
            Self::Interrupt => "interrupt",
            Self::Resume => "resume",
        };
        f.write_str(value)
    }
}

#[cfg(test)]
mod tests {
    use super::RunEvent;

    #[test]
    fn run_event_display_uses_stable_snake_case_values() {
        let cases = [
            (RunEvent::Start, "start"),
            (RunEvent::RequestApproval, "request_approval"),
            (RunEvent::Approve, "approve"),
            (RunEvent::Reject, "reject"),
            (RunEvent::Complete, "complete"),
            (RunEvent::Fail, "fail"),
            (RunEvent::Cancel, "cancel"),
            (RunEvent::Interrupt, "interrupt"),
            (RunEvent::Resume, "resume"),
        ];

        for (event, expected) in cases {
            assert_eq!(event.to_string(), expected);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentSession {
    pub id: String,
    pub exam_id: Option<String>,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// One conversation message (M5, spec §7.1). `content_json` holds structured
/// payloads when present; token usage and model are recorded per assistant
/// turn so the UI can show cost and provenance.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentMessage {
    pub id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub role: String,
    pub text: String,
    pub content_json: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub model: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentRun {
    pub id: String,
    pub session_id: String,
    pub goal: String,
    pub status: RunStatus,
    pub trigger_source: String,
    pub current_step: i64,
    pub error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRequest {
    pub run_id: String,
    pub step_index: i64,
    pub tool_name: String,
    pub tool_version: String,
    pub input: Value,
    pub idempotency_key: Option<String>,
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolCallResponse {
    Completed {
        step_id: String,
        output: Value,
        replayed: bool,
        undo_available: bool,
    },
    WaitingApproval {
        step_id: String,
        approval_id: String,
        preview: Value,
        expires_at: String,
    },
    SummaryRequired {
        step_id: String,
        preview: Value,
    },
    NavigationRequired {
        route: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalRecord {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub risk: i64,
    pub preview: Value,
    pub precondition_hash: String,
    pub status: String,
    pub expires_at: String,
    pub decided_at: Option<String>,
    pub created_at: String,
}
