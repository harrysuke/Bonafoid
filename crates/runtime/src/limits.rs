//! Run budgets.
//!
//! Every limit is checked *before* work is dispatched, never after, so a run
//! cannot exceed a budget by one node. The defaults come from the plan §7.5.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Ceilings for one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunLimits {
    /// Node executions, including retries. Guards a cyclic graph.
    pub max_steps: usize,
    pub max_wall_clock: Duration,
    /// Executors in flight at once.
    pub max_parallel: usize,
    pub max_tokens: u64,
    pub max_cost_usd: f64,
    pub max_tool_calls: usize,
    /// Attempts per node, including the first. `1` means no retry.
    ///
    /// Added to §7.5's struct because §7.4's resume table ("error, retries
    /// left → re-run") needs a number to compare against, and a node's own
    /// `RetryConfig` belongs with the executors that can honour it (Phase 36).
    pub max_attempts: u32,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_steps: 500,
            max_wall_clock: Duration::from_secs(30 * 60),
            max_parallel: 4,
            max_tokens: 500_000,
            max_cost_usd: 5.00,
            max_tool_calls: 200,
            max_attempts: 1,
        }
    }
}

/// Which ceiling a run hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Steps,
    WallClock,
    Tokens,
    Cost,
    ToolCalls,
}

impl Limit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::WallClock => "wall_clock",
            Self::Tokens => "tokens",
            Self::Cost => "cost_usd",
            Self::ToolCalls => "tool_calls",
        }
    }
}

/// Usage accumulated so far, checked against [`RunLimits`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Budget {
    pub steps: usize,
    pub tokens: u64,
    pub cost_usd: f64,
    pub tool_calls: usize,
}

impl Budget {
    /// Count one node execution. Returns the limit that was hit, if any.
    pub fn charge_step(&mut self, limits: &RunLimits) -> Option<Limit> {
        self.steps += 1;
        (self.steps > limits.max_steps).then_some(Limit::Steps)
    }

    pub fn charge_usage(&mut self, tokens: u64, cost_usd: f64, tool_calls: usize) {
        self.tokens = self.tokens.saturating_add(tokens);
        self.cost_usd += cost_usd;
        self.tool_calls = self.tool_calls.saturating_add(tool_calls);
    }

    /// Check the limits that are not charged per step.
    pub fn exceeded(&self, limits: &RunLimits) -> Option<Limit> {
        if self.tokens > limits.max_tokens {
            return Some(Limit::Tokens);
        }
        if self.cost_usd > limits.max_cost_usd {
            return Some(Limit::Cost);
        }
        if self.tool_calls > limits.max_tool_calls {
            return Some(Limit::ToolCalls);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_plan() {
        let limits = RunLimits::default();
        assert_eq!(limits.max_steps, 500);
        assert_eq!(limits.max_wall_clock, Duration::from_secs(30 * 60));
        assert_eq!(limits.max_parallel, 4);
        assert_eq!(limits.max_tokens, 500_000);
        assert_eq!(limits.max_tool_calls, 200);
        assert_eq!(limits.max_attempts, 1, "no silent retries by default");
    }

    #[test]
    fn a_step_is_charged_before_it_runs() {
        let limits = RunLimits {
            max_steps: 2,
            ..RunLimits::default()
        };
        let mut budget = Budget::default();
        assert_eq!(budget.charge_step(&limits), None);
        assert_eq!(budget.charge_step(&limits), None);
        // The third step is refused, so exactly two nodes can run.
        assert_eq!(budget.charge_step(&limits), Some(Limit::Steps));
    }

    #[test]
    fn token_and_cost_ceilings_are_reported_separately() {
        let limits = RunLimits {
            max_tokens: 100,
            ..RunLimits::default()
        };
        let mut budget = Budget::default();
        budget.charge_usage(101, 0.0, 0);
        assert_eq!(budget.exceeded(&limits), Some(Limit::Tokens));

        let limits = RunLimits {
            max_cost_usd: 1.0,
            ..RunLimits::default()
        };
        let mut budget = Budget::default();
        budget.charge_usage(0, 1.5, 0);
        assert_eq!(budget.exceeded(&limits), Some(Limit::Cost));
    }
}
