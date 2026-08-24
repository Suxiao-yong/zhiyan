//! 错题间隔重复调度（SM-2 简化版）。纯函数，无 IO。

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewState {
    pub interval_days: f64,
    pub ease_factor: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewOutcome {
    pub interval_days: f64,
    pub ease_factor: f64,
    pub due_in_days: f64,
}

impl ReviewOutcome {
    pub fn into_state(self) -> ReviewState {
        ReviewState { interval_days: self.interval_days, ease_factor: self.ease_factor }
    }
}

const EASE_FLOOR: f64 = 1.3;
const INTERVAL_CAP: f64 = 365.0;

/// quality: 0–5（0–2 未记住；3 勉强；4 良好；5 轻松）。经典 SM-2 简化：
/// 失败归零间隔、次日重见；成功按 1 → 6 → ×ease 爬梯。
pub fn schedule(state: ReviewState, quality: u8) -> ReviewOutcome {
    let q = f64::from(quality.min(5));
    let delta = 0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02);
    let ease = (state.ease_factor + delta).max(EASE_FLOOR);

    if q < 3.0 {
        return ReviewOutcome { interval_days: 0.0, ease_factor: ease, due_in_days: 1.0 };
    }
    let interval = if state.interval_days <= 0.0 {
        1.0
    } else if state.interval_days < 6.0 {
        6.0
    } else {
        (state.interval_days * ease).min(INTERVAL_CAP)
    };
    ReviewOutcome { interval_days: interval, ease_factor: ease, due_in_days: interval }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_success_schedules_one_day() {
        let out = schedule(ReviewState { interval_days: 0.0, ease_factor: 2.5 }, 4);
        assert_eq!(out.due_in_days, 1.0);
        assert_eq!(out.interval_days, 1.0);
    }

    #[test]
    fn second_success_jumps_to_six_days() {
        let out = schedule(ReviewState { interval_days: 1.0, ease_factor: 2.5 }, 4);
        assert_eq!(out.interval_days, 6.0);
    }

    #[test]
    fn later_success_multiplies_by_ease() {
        let out = schedule(ReviewState { interval_days: 6.0, ease_factor: 2.5 }, 4);
        assert!((out.interval_days - 15.0).abs() < 1e-9); // 6 * 2.5
    }

    #[test]
    fn failure_resets_interval_but_ease_stays_above_floor() {
        let out = schedule(ReviewState { interval_days: 30.0, ease_factor: 2.5 }, 1);
        assert_eq!(out.interval_days, 0.0);
        assert_eq!(out.due_in_days, 1.0);
        assert!(out.ease_factor < 2.5 && out.ease_factor >= 1.3);
    }

    #[test]
    fn ease_floor_is_130() {
        let mut st = ReviewState { interval_days: 100.0, ease_factor: 1.3 };
        for _ in 0..5 {
            st = schedule(st, 3).into_state();
        }
        assert!(st.ease_factor >= 1.3);
    }

    #[test]
    fn interval_cap_365_days() {
        let out = schedule(ReviewState { interval_days: 400.0, ease_factor: 2.8 }, 5);
        assert!(out.interval_days <= 365.0);
    }
}
