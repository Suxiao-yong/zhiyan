use super::error::AgentError;
use super::model::{RunEvent, RunStatus};

pub fn transition(from: RunStatus, event: RunEvent) -> Result<RunStatus, AgentError> {
    use RunEvent::*;
    use RunStatus::*;

    let next = match (from, event) {
        (Queued, Start) | (Interrupted, Resume) => Running,
        (Running, RequestApproval) => WaitingApproval,
        (WaitingApproval, Approve) => Running,
        (WaitingApproval, Reject) => Cancelled,
        (Queued, Cancel)
        | (Running, Cancel)
        | (WaitingApproval, Cancel)
        | (Interrupted, Cancel) => Cancelled,
        (Running, Complete) => Completed,
        (Running, Fail) | (WaitingApproval, Fail) => Failed,
        (Running, Interrupt) => Interrupted,
        _ => {
            return Err(AgentError::InvalidTransition {
                from: from.to_string(),
                event: event.to_string(),
            });
        }
    };

    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_transitions_reach_the_expected_status() {
        let cases = [
            (RunStatus::Queued, RunEvent::Start, RunStatus::Running),
            (
                RunStatus::Running,
                RunEvent::RequestApproval,
                RunStatus::WaitingApproval,
            ),
            (
                RunStatus::WaitingApproval,
                RunEvent::Approve,
                RunStatus::Running,
            ),
            (RunStatus::Interrupted, RunEvent::Resume, RunStatus::Running),
            (
                RunStatus::WaitingApproval,
                RunEvent::Reject,
                RunStatus::Cancelled,
            ),
            (RunStatus::Queued, RunEvent::Cancel, RunStatus::Cancelled),
            (RunStatus::Running, RunEvent::Cancel, RunStatus::Cancelled),
            (
                RunStatus::WaitingApproval,
                RunEvent::Cancel,
                RunStatus::Cancelled,
            ),
            (
                RunStatus::Interrupted,
                RunEvent::Cancel,
                RunStatus::Cancelled,
            ),
            (RunStatus::Running, RunEvent::Complete, RunStatus::Completed),
            (RunStatus::Running, RunEvent::Fail, RunStatus::Failed),
            (
                RunStatus::WaitingApproval,
                RunEvent::Fail,
                RunStatus::Failed,
            ),
            (
                RunStatus::Running,
                RunEvent::Interrupt,
                RunStatus::Interrupted,
            ),
        ];

        for (from, event, expected) in cases {
            assert_eq!(transition(from, event).unwrap(), expected);
        }
    }

    #[test]
    fn invalid_transitions_return_stable_diagnostics() {
        let cases = [
            (RunStatus::Completed, RunEvent::Start, "start"),
            (RunStatus::WaitingApproval, RunEvent::Interrupt, "interrupt"),
            (RunStatus::Queued, RunEvent::Complete, "complete"),
            (RunStatus::Failed, RunEvent::Resume, "resume"),
        ];

        for (from, event, expected_event) in cases {
            let error = transition(from, event).unwrap_err();
            assert_eq!(error.code(), "invalid_transition");
            assert_eq!(
                error,
                AgentError::InvalidTransition {
                    from: from.to_string(),
                    event: expected_event.to_owned(),
                }
            );
        }
    }
}
