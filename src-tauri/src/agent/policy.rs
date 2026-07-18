use chrono::{DateTime, Utc};

use crate::agent::{error::AgentError, tools::RiskLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Execute,
    ExecuteWithUndo,
    PresentSummary,
    AwaitApproval,
    NavigateOnly,
}

#[derive(Debug, Clone, Copy)]
pub struct ApprovalGrant<'a> {
    pub approval_id: &'a str,
    pub step_id: &'a str,
    pub expected_step_id: &'a str,
    pub status: &'a str,
    pub expires_at: DateTime<Utc>,
    pub now: DateTime<Utc>,
    pub precondition_hash: &'a str,
    pub current_precondition_hash: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct PolicyContext<'a> {
    pub risk: RiskLevel,
    pub user_allows_r2: bool,
    pub approval: Option<ApprovalGrant<'a>>,
}

pub fn decide(context: PolicyContext<'_>) -> Result<PolicyDecision, AgentError> {
    match context.risk {
        RiskLevel::R0 => Ok(PolicyDecision::Execute),
        RiskLevel::R1 => Ok(PolicyDecision::ExecuteWithUndo),
        RiskLevel::R2 if context.user_allows_r2 => Ok(PolicyDecision::Execute),
        RiskLevel::R2 => Ok(PolicyDecision::PresentSummary),
        RiskLevel::R3 => match context.approval {
            None => Ok(PolicyDecision::AwaitApproval),
            Some(grant)
                if grant.status == "approved"
                    && grant.step_id == grant.expected_step_id
                    && grant.expires_at > grant.now
                    && grant.precondition_hash == grant.current_precondition_hash =>
            {
                Ok(PolicyDecision::Execute)
            }
            Some(_) => Err(AgentError::ApprovalInvalid),
        },
        RiskLevel::R4 => Ok(PolicyDecision::NavigateOnly),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, Utc};

    use super::{decide, ApprovalGrant, PolicyContext, PolicyDecision};
    use crate::agent::tools::RiskLevel;

    fn now() -> DateTime<Utc> {
        "2026-07-18T08:00:00Z".parse().unwrap()
    }

    fn valid_grant<'a>(now: DateTime<Utc>) -> ApprovalGrant<'a> {
        ApprovalGrant {
            approval_id: "approval-1",
            step_id: "step-1",
            expected_step_id: "step-1",
            status: "approved",
            expires_at: now + Duration::minutes(5),
            now,
            precondition_hash: "hash-1",
            current_precondition_hash: "hash-1",
        }
    }

    #[test]
    fn r0_executes_without_approval_or_setting() {
        assert_eq!(
            decide(PolicyContext {
                risk: RiskLevel::R0,
                user_allows_r2: false,
                approval: None,
            })
            .unwrap(),
            PolicyDecision::Execute
        );
    }

    #[test]
    fn r1_executes_with_undo() {
        assert_eq!(
            decide(PolicyContext {
                risk: RiskLevel::R1,
                user_allows_r2: false,
                approval: None,
            })
            .unwrap(),
            PolicyDecision::ExecuteWithUndo
        );
    }

    #[test]
    fn r2_requires_summary_unless_user_setting_allows_execution() {
        let denied = decide(PolicyContext {
            risk: RiskLevel::R2,
            user_allows_r2: false,
            approval: None,
        })
        .unwrap();
        let allowed = decide(PolicyContext {
            risk: RiskLevel::R2,
            user_allows_r2: true,
            approval: None,
        })
        .unwrap();

        assert_eq!(denied, PolicyDecision::PresentSummary);
        assert_eq!(allowed, PolicyDecision::Execute);
    }

    #[test]
    fn r3_without_grant_awaits_approval() {
        assert_eq!(
            decide(PolicyContext {
                risk: RiskLevel::R3,
                user_allows_r2: true,
                approval: None,
            })
            .unwrap(),
            PolicyDecision::AwaitApproval
        );
    }

    #[test]
    fn r4_is_navigation_only_regardless_of_setting() {
        for user_allows_r2 in [false, true] {
            assert_eq!(
                decide(PolicyContext {
                    risk: RiskLevel::R4,
                    user_allows_r2,
                    approval: None,
                })
                .unwrap(),
                PolicyDecision::NavigateOnly
            );
        }
    }

    #[test]
    fn no_unapproved_r3_can_execute() {
        let now = now();

        let mut expired = valid_grant(now);
        expired.expires_at = now - Duration::seconds(1);
        let mut expires_now = valid_grant(now);
        expires_now.expires_at = now;
        let mut wrong_step = valid_grant(now);
        wrong_step.step_id = "other-step";
        let mut wrong_precondition = valid_grant(now);
        wrong_precondition.precondition_hash = "old-hash";
        let mut rejected = valid_grant(now);
        rejected.status = "rejected";

        for approval in [
            expired,
            expires_now,
            wrong_step,
            wrong_precondition,
            rejected,
        ] {
            let error = decide(PolicyContext {
                risk: RiskLevel::R3,
                user_allows_r2: true,
                approval: Some(approval),
            })
            .unwrap_err();
            assert_eq!(error.code(), "approval_invalid");
        }
    }

    #[test]
    fn valid_r3_grant_executes_even_when_r2_setting_is_disabled() {
        let now = now();
        assert_eq!(
            decide(PolicyContext {
                risk: RiskLevel::R3,
                user_allows_r2: false,
                approval: Some(valid_grant(now)),
            })
            .unwrap(),
            PolicyDecision::Execute
        );
    }
}
