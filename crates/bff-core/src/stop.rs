//! The safety stop. The first reason latches. After that nothing dispatches.

use std::sync::{Arc, Mutex};

/// One stop for the whole app. The first safety stop seen on any route is kept here, and every route checks it
/// before it sends anything, so a stop on one provider holds back the others too. It stays until BFF restarts.
#[derive(Debug, Clone, Default)]
pub struct StopLatch(Arc<Mutex<Option<String>>>);

impl StopLatch {
    pub fn new() -> StopLatch {
        StopLatch::default()
    }

    /// Keeps the first reason. Later ones are ignored.
    pub fn trip(&self, why: &str) {
        let mut r = self.0.lock().expect("stop latch");
        if r.is_none() {
            *r = Some(why.chars().take(300).collect());
        }
    }

    pub fn reason(&self) -> Option<String> {
        self.0.lock().expect("stop latch").clone()
    }

    /// Clears a stop held because the app's own `kind` cap was reached (`token_cap` or `spending_cap`), once the
    /// person has approved a higher cap. That approval is the way on for that stop. A stop for any other reason
    /// stays until the app restarts. True when a stop was cleared.
    pub fn clear_cap(&self, kind: &str) -> bool {
        let mut r = self.0.lock().expect("stop latch");
        if r.as_deref().and_then(cap_kind) == Some(kind) {
            *r = None;
            return true;
        }
        false
    }

    /// An error while a stop is held.
    pub fn check(&self) -> Result<(), String> {
        match self.reason() {
            // The way on from a cap stop is a higher cap, which the reason already says.
            Some(why) if cap_kind(&why).is_some() => Err(format!(
                "stopped: AI work is on hold. {}",
                held_reason(&why)
            )),
            Some(why) => Err(format!(
                "stopped: AI work is on hold. {} Check your usage with the provider, then quit and reopen SproutCrew to go on.",
                held_reason(&why)
            )),
            None => Ok(()),
        }
    }
}

/// Which of the app's own caps refused a request, from the words the spend book refuses with: `token_cap` or
/// `spending_cap`.
pub fn cap_kind(why: &str) -> Option<&'static str> {
    let w = why.to_lowercase();
    if w.contains("the api token cap of") {
        Some("token_cap")
    } else if w.contains("the api spending cap of") {
        Some("spending_cap")
    } else {
        None
    }
}

/// A stop's reason without its `stopped:` label, for text that adds its own (Q2-B9: "stopped: stopped:").
pub fn without_label(why: &str) -> &str {
    why.strip_prefix("stopped:").map_or(why, str::trim_start)
}

/// The reason a stop was kept, as a sentence: without its `stopped:` label, and without the advice that the
/// check message adds again.
fn held_reason(why: &str) -> String {
    let w = why.strip_prefix("stopped:").unwrap_or(why).trim();
    let w = w
        .split(" It counted the most")
        .next()
        .unwrap_or(w)
        .trim_end_matches(['.', ' ']);
    let mut c = w.chars();
    match c.next() {
        Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: &str = "stopped: the API token cap of 500,000 tokens is reached (455,038 tokens used, 0 tokens held, and this request could use up to 51,149 tokens). Approve a higher cap in Usage to continue";

    #[test]
    fn a_cap_stop_is_named_by_its_cap_and_other_stops_are_not() {
        assert_eq!(cap_kind(CAP), Some("token_cap"));
        assert_eq!(
            cap_kind("stopped: the API spending cap of $5.00 is reached ($5.00 spent)"),
            Some("spending_cap")
        );
        assert_eq!(cap_kind("stopped: OpenRouter did not answer"), None);
        assert_eq!(
            cap_kind("stopped: the API dollar budget has room for 4"),
            None
        );
    }

    #[test]
    fn approving_a_higher_cap_clears_only_a_stop_held_for_that_cap() {
        // Q2-B10: the stop held for the token cap goes once a higher token cap is approved, with no restart.
        let l = StopLatch::new();
        l.trip(CAP);
        assert!(!l.clear_cap("spending_cap"), "another cap leaves it held");
        assert!(l.check().is_err());
        assert!(l.clear_cap("token_cap"));
        assert!(l.check().is_ok());
        // A stop for any other reason stays until the app is reopened.
        let other = StopLatch::new();
        other.trip("stopped: the provider cannot tell whether the request was charged");
        assert!(!other.clear_cap("token_cap"));
        assert!(other.check().is_err());
    }

    #[test]
    fn a_held_cap_stop_says_to_approve_a_higher_cap_and_not_to_reopen_the_app() {
        let l = StopLatch::new();
        l.trip(CAP);
        let m = l.check().unwrap_err();
        assert!(
            m.starts_with("stopped: AI work is on hold. The API token cap"),
            "{m}"
        );
        assert!(m.contains("Approve a higher cap in Usage"), "{m}");
        assert!(!m.contains("quit and reopen"), "{m}");
        assert_eq!(m.matches("stopped:").count(), 1, "{m}");
    }

    #[test]
    fn a_reason_loses_its_stopped_label_once() {
        assert_eq!(without_label(CAP).matches("stopped:").count(), 0);
        assert!(without_label(CAP).starts_with("the API token cap"));
        assert_eq!(without_label("no label"), "no label");
    }
}
