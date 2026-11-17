//! What a scenario returns to the orchestrator.

use crate::common::Dirs;
use crate::runs::Run;
use crate::work::Work;
use bff_core::ratelimit::RateLimitState;
use bff_core::stop::StopReason;
use serde_json::Value;

pub struct Ctx<'a> {
    pub dirs: &'a Dirs,
    pub run: &'static Run,
    pub w: Work,
    /// The Codex plan state from the pre-dispatch reading, with its baseline, for in-run stop checks.
    pub codex_state: RateLimitState,
}

#[derive(Default)]
pub struct Outcome {
    /// `(name, passed, detail)`. Each is judged from events, hashes, or process state.
    pub checks: Vec<(String, bool, String)>,
    pub evidence: Value,
    /// Redacted files to write under `fixtures/redacted/runs/<ID>/`.
    pub fixtures: Vec<(String, Value)>,
    /// Relative paths of writes that landed outside the allowed folder.
    pub outside_writes: Vec<String>,
    /// A curated-route failure: delegation, a disallowed tool, a hook, or a wrong billing route.
    pub route_failure: Option<String>,
    /// The external timeout fired.
    pub timed_out: bool,
    /// A stop condition the scenario itself tripped while running.
    pub stop: Option<StopReason>,
}

impl Outcome {
    pub fn new() -> Self {
        Outcome {
            evidence: Value::Null,
            ..Default::default()
        }
    }

    pub fn check(&mut self, name: &str, ok: bool, detail: impl Into<String>) {
        self.checks.push((name.to_string(), ok, detail.into()));
    }

    pub fn all_ok(&self) -> bool {
        self.checks.iter().all(|(_, ok, _)| *ok)
    }

    pub fn failures(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|(_, ok, _)| !ok)
            .map(|(n, _, d)| format!("{n}: {d}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_is_ok_only_if_every_check_passed() {
        let mut o = Outcome::new();
        assert!(o.all_ok());
        o.check("a", true, "");
        o.check("b", false, "why");
        assert!(!o.all_ok());
        assert_eq!(o.failures(), vec!["b: why".to_string()]);
    }
}
