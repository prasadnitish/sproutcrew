//! Stop rules from `M0_LIVE_CHECKS.md`. The first reason latches. After that nothing dispatches.

use crate::ratelimit::{Block, RateLimitState};
use std::sync::{Arc, Mutex};

pub const STOP_PERCENT: f64 = 50.0;

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// A plan window displays 50 percent or above.
    PlanWindow { route: &'static str, detail: String },
    /// The Codex credit balance changed or became unavailable. Reported as an unattributed account change.
    CreditBalance(String),
    /// `account/read` is not `chatgpt`, or `claude auth status` is not `claude.ai`, or the account changed.
    BillingRoute(String),
    /// A cloud, gateway, custom-provider, or helper route appeared.
    ForeignRoute(String),
    /// Delegation, a tool outside the allowlist, or a hook fired.
    CuratedRouteFailed(String),
    /// A write landed outside the allowed folder.
    WriteOutsideFolder(String),
    /// A run passed its timeout.
    Timeout { run: String, secs: u64 },
    /// Required telemetry is stale, missing, or unparseable.
    Telemetry(String),
}

/// One stop for the whole app. The first safety stop seen on any route is kept here, and every route checks it
/// before it sends anything, so a stop on one provider holds back the others too. It stays until BFF restarts.
#[derive(Debug, Clone, Default)]
pub struct StopLatch(
    Arc<Mutex<Option<String>>>,
    Arc<Mutex<Option<std::time::Instant>>>,
);

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

    /// True for the one caller that should read the plan windows now: none has been read for `every`. Everyone else
    /// gets false until that time passes again, so several steps starting at once read them once.
    pub fn poll_due(&self, every: std::time::Duration) -> bool {
        let mut last = self.1.lock().expect("stop latch poll");
        let due = last.is_none_or(|t| t.elapsed() >= every);
        if due {
            *last = Some(std::time::Instant::now());
        }
        due
    }

    /// An error while a stop is held.
    pub fn check(&self) -> Result<(), String> {
        match self.reason() {
            Some(why) => Err(format!(
                "stopped: AI work is on hold. {} Check your usage with the provider, then quit and reopen SproutCrew to go on.",
                held_reason(&why)
            )),
            None => Ok(()),
        }
    }
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

#[derive(Debug, Default)]
pub struct Stopper {
    reason: Option<StopReason>,
}

impl Stopper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reason(&self) -> Option<&StopReason> {
        self.reason.as_ref()
    }

    pub fn trip(&mut self, r: StopReason) {
        self.reason.get_or_insert(r);
    }

    /// Codex window, balance, and account state. Staleness is not a stop condition here.
    pub fn check_codex(&mut self, state: &RateLimitState) {
        let Some(block) = state.stop_condition() else {
            return;
        };
        self.trip(match block {
            Block::AccountChanged => StopReason::BillingRoute("account changed".into()),
            Block::BalanceChanged { from, to } => {
                StopReason::CreditBalance(format!("{from} -> {to}"))
            }
            Block::BalanceUnavailable => {
                StopReason::CreditBalance("unavailable after a reading".into())
            }
            Block::AtOrOverThreshold {
                bucket,
                window,
                percent,
            } => StopReason::PlanWindow {
                route: "codex",
                detail: format!("{bucket}/{window} at {percent}%"),
            },
            other => StopReason::Telemetry(format!("{other:?}")),
        });
    }

    /// A Claude plan reading as a percentage. The displayed value is rounded, so 49.5 counts as 50.
    pub fn check_claude_percent(&mut self, percent: f64) {
        if percent.round() >= STOP_PERCENT {
            self.trip(StopReason::PlanWindow {
                route: "claude",
                detail: format!("at {percent}%"),
            });
        }
    }

    /// `authMethod` from `claude auth status`.
    pub fn check_claude_auth(&mut self, auth_method: &str) {
        if auth_method != "claude.ai" {
            self.trip(StopReason::BillingRoute(format!(
                "claude authMethod is {auth_method:?}"
            )));
        }
    }

    pub fn check_timeout(&mut self, run: &str, secs: u64) {
        self.trip(StopReason::Timeout {
            run: run.to_string(),
            secs,
        });
    }

    pub fn check_outside_write(&mut self, paths: &[String]) {
        if !paths.is_empty() {
            self.trip(StopReason::WriteOutsideFolder(paths.join(", ")));
        }
    }

    pub fn route_failed(&mut self, what: &str) {
        self.trip(StopReason::CuratedRouteFailed(what.to_string()));
    }

    pub fn foreign_route(&mut self, what: &str) {
        self.trip(StopReason::ForeignRoute(what.to_string()));
    }

    /// Nothing may dispatch once stopped.
    pub fn may_dispatch(&self) -> Result<(), &StopReason> {
        match &self.reason {
            Some(r) => Err(r),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_types::GetAccountRateLimitsResponse;
    use serde_json::{json, Value};

    fn state(pct: i32, balance: &str, account: &str) -> RateLimitState {
        let snap = json!({"limitId": "codex", "primary": {"usedPercent": pct}, "secondary": null,
                          "credits": {"hasCredits": true, "unlimited": false, "balance": balance}});
        let mut resp: Value = json!({"rateLimits": snap});
        resp["accountId"] = json!(account);
        let resp: GetAccountRateLimitsResponse = serde_json::from_value(resp).unwrap();
        let mut s = RateLimitState::new();
        s.apply_read(&resp, 1000);
        s
    }

    #[test]
    fn fresh_stopper_allows_dispatch() {
        assert!(Stopper::new().may_dispatch().is_ok());
    }

    #[test]
    fn codex_window_at_fifty_stops_and_below_does_not() {
        let mut s = Stopper::new();
        let mut ok = state(49, "10", "a");
        ok.freeze_baseline().unwrap();
        s.check_codex(&ok);
        assert!(s.may_dispatch().is_ok());
        let mut hot = state(50, "10", "a");
        hot.freeze_baseline().unwrap();
        s.check_codex(&hot);
        assert!(matches!(
            s.reason(),
            Some(StopReason::PlanWindow { route: "codex", .. })
        ));
    }

    #[test]
    fn codex_balance_change_stops_as_a_credit_event() {
        let mut st = state(10, "10", "a");
        st.freeze_baseline().unwrap();
        let moved = state(10, "9.5", "a");
        let mut merged = st;
        // A later authoritative read with a new balance.
        let snap = json!({"limitId": "codex", "primary": {"usedPercent": 10}, "credits": {"hasCredits": true, "unlimited": false, "balance": "9.5"}});
        let resp: GetAccountRateLimitsResponse =
            serde_json::from_value(json!({"rateLimits": snap, "accountId": "a"})).unwrap();
        merged.apply_read(&resp, 1010);
        let _ = moved;
        let mut s = Stopper::new();
        s.check_codex(&merged);
        assert!(matches!(s.reason(), Some(StopReason::CreditBalance(_))));
    }

    #[test]
    fn codex_account_change_stops_as_a_billing_route_event() {
        let mut st = state(10, "10", "a");
        st.freeze_baseline().unwrap();
        let snap = json!({"limitId": "codex", "primary": {"usedPercent": 10}, "credits": {"hasCredits": true, "unlimited": false, "balance": "10"}});
        let resp: GetAccountRateLimitsResponse =
            serde_json::from_value(json!({"rateLimits": snap, "accountId": "b"})).unwrap();
        st.apply_read(&resp, 1010);
        let mut s = Stopper::new();
        s.check_codex(&st);
        assert!(matches!(s.reason(), Some(StopReason::BillingRoute(_))));
    }

    #[test]
    fn claude_percent_uses_the_rounded_display() {
        let mut s = Stopper::new();
        s.check_claude_percent(49.4);
        assert!(s.may_dispatch().is_ok());
        s.check_claude_percent(49.5);
        assert!(matches!(
            s.reason(),
            Some(StopReason::PlanWindow {
                route: "claude",
                ..
            })
        ));
    }

    #[test]
    fn claude_auth_must_be_claude_ai() {
        let mut s = Stopper::new();
        s.check_claude_auth("claude.ai");
        assert!(s.may_dispatch().is_ok());
        for bad in [
            "api_key",
            "api_key_helper",
            "third_party",
            "oauth_token",
            "none",
            "",
        ] {
            let mut s = Stopper::new();
            s.check_claude_auth(bad);
            assert!(
                matches!(s.reason(), Some(StopReason::BillingRoute(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn first_reason_latches_and_blocks_all_dispatch() {
        let mut s = Stopper::new();
        s.check_timeout("A1", 180);
        s.check_claude_percent(99.0);
        assert_eq!(
            s.reason(),
            Some(&StopReason::Timeout {
                run: "A1".into(),
                secs: 180
            })
        );
        assert!(s.may_dispatch().is_err());
    }

    #[test]
    fn outside_writes_route_and_foreign_routes_all_trip() {
        let mut s = Stopper::new();
        s.check_outside_write(&[]);
        assert!(s.may_dispatch().is_ok());
        s.check_outside_write(&["sibling/x.txt".into()]);
        assert!(matches!(
            s.reason(),
            Some(StopReason::WriteOutsideFolder(_))
        ));
        let mut s = Stopper::new();
        s.foreign_route("ANTHROPIC_BASE_URL");
        assert!(matches!(s.reason(), Some(StopReason::ForeignRoute(_))));
        let mut s = Stopper::new();
        s.route_failed("hook fired");
        assert!(matches!(
            s.reason(),
            Some(StopReason::CuratedRouteFailed(_))
        ));
    }

    #[test]
    fn stale_or_missing_telemetry_is_not_a_stop_for_active_work_but_blocks_the_gate() {
        let mut st = state(10, "10", "a");
        st.freeze_baseline().unwrap();
        let mut s = Stopper::new();
        s.check_codex(&st);
        assert!(s.may_dispatch().is_ok());
        assert!(matches!(
            st.dispatch_gate(1_000_000),
            Err(Block::Stale { .. })
        ));
    }
}
