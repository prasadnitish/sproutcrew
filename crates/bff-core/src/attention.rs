//! Attention is a hint to inspect the inbox, never authority to approve a decision.
use crate::{decision_inbox::Item, engine::sha256_hex, policy::Tier};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Tracker {
    seen: BTreeSet<String>,
    last_attempt: Option<u64>,
}
pub struct Notice {
    pub count: usize,
    pub tier: Tier,
}
impl Notice {
    pub fn body(&self) -> String {
        let risk = match self.tier {
            Tier::Destructive => " Includes a destructive decision.",
            Tier::Consequential => " Includes a consequential decision or stopped step.",
            Tier::Reversible => "",
        };
        format!(
            "{} new {} {} your input.{risk} Open the Decision inbox to review.",
            self.count,
            if self.count == 1 { "item" } else { "items" },
            if self.count == 1 { "needs" } else { "need" }
        )
    }
}
fn key(item: &Item) -> String {
    // Hash content in memory; neither project names nor model prose go into system notifications.
    sha256_hex(
        serde_json::to_string(item)
            .expect("serializable inbox item")
            .as_bytes(),
    )
}
impl Tracker {
    pub fn observe(&mut self, items: &[Item], foreground: bool, now: u64) -> Option<Notice> {
        let current: BTreeSet<_> = items.iter().map(key).collect();
        self.seen.retain(|k| current.contains(k));
        if foreground {
            self.seen = current;
            return None;
        }
        if self
            .last_attempt
            .is_some_and(|at| now.saturating_sub(at) < 30)
        {
            return None;
        }
        let new: Vec<_> = items
            .iter()
            .filter(|i| !self.seen.contains(&key(i)))
            .collect();
        Some(Notice {
            count: new.len(),
            tier: new.iter().map(|i| i.tier).min()?,
        })
    }
    pub fn attempted(&mut self, items: &[Item], now: u64, accepted: bool) {
        self.last_attempt = Some(now);
        if accepted {
            self.seen = items.iter().map(key).collect();
        }
    }
}

/// How a run ended, or that it is waiting, for a Mac notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Finished,
    Stopped,
    /// A step did not finish. Its kind, when known, is said in a few plain words.
    Failed,
    /// A step is waiting out a provider's rate limit before trying again.
    Waiting,
}

impl RunOutcome {
    pub fn from_word(w: &str) -> Option<RunOutcome> {
        Some(match w {
            "finished" => RunOutcome::Finished,
            "stopped" => RunOutcome::Stopped,
            "failed" => RunOutcome::Failed,
            "waiting" => RunOutcome::Waiting,
            _ => return None,
        })
    }
}

/// What a failure kind means, as a few plain words for a notification.
pub fn kind_phrase(kind: Option<&str>) -> Option<&'static str> {
    Some(match kind? {
        "per_minute" | "upstream_busy" | "daily_cap" => "it reached a provider's rate limit",
        "out_of_credits" => "the provider account is out of credit",
        "key_limit" => "the API key reached its spending limit",
        _ => return None,
    })
}

/// A run notification's title and body. No step text, result or file name is ever included, and the project's
/// name only when the person turned that on and passes it.
pub fn run_text(
    outcome: RunOutcome,
    kind: Option<&str>,
    project: Option<&str>,
) -> (String, String) {
    let of = project
        .map(|p| format!(" in {}", p.chars().take(60).collect::<String>()))
        .unwrap_or_default();
    match outcome {
        RunOutcome::Finished => (
            "SproutCrew run finished".into(),
            format!(
                "Every step that could run{of} has finished. Open SproutCrew to read the results."
            ),
        ),
        RunOutcome::Stopped => (
            "SproutCrew run stopped".into(),
            format!("The run{of} was stopped. Nothing more is running."),
        ),
        RunOutcome::Failed => (
            "A SproutCrew step did not finish".into(),
            match kind_phrase(kind) {
                Some(why) => {
                    format!("A step{of} stopped: {why}. Open SproutCrew to choose what to do.")
                }
                None => format!("A step{of} did not finish. Open SproutCrew to see why."),
            },
        ),
        RunOutcome::Waiting => (
            "SproutCrew is waiting".into(),
            format!(
                "A step{of} is waiting for a provider's rate limit and will try again shortly."
            ),
        ),
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;

    #[test]
    fn run_notifications_say_what_happened_in_plain_words_and_name_a_project_only_when_given() {
        let (t, b) = run_text(RunOutcome::Finished, None, None);
        assert_eq!(t, "SproutCrew run finished");
        assert!(!b.contains(" in "), "{b}");
        let (_, b) = run_text(
            RunOutcome::Failed,
            Some("out_of_credits"),
            Some("bakery-news"),
        );
        assert_eq!(b, "A step in bakery-news stopped: the provider account is out of credit. Open SproutCrew to choose what to do.");
        let (_, b) = run_text(RunOutcome::Failed, Some("other"), None);
        assert_eq!(b, "A step did not finish. Open SproutCrew to see why.");
        assert_eq!(RunOutcome::from_word("waiting"), Some(RunOutcome::Waiting));
        assert_eq!(RunOutcome::from_word("approve"), None);
    }
}
