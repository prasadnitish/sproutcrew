//! Codex plan-window and credit state (plan section 9, `M0_LIVE_CHECKS.md` stop rules).
//!
//! Updates are sparse and keyed by `limitId`. Missing means unknown. `usedPercent` is a
//! rounded integer, so the guard fires at the threshold, not above it.

use crate::codex_types::{
    CreditsSnapshot, GetAccountRateLimitsResponse, RateLimitSnapshot, RateLimitWindow,
};
use crate::decimal::Decimal;
use std::collections::BTreeMap;

pub const STOP_PERCENT: i32 = 50;
/// A reading older than this cannot gate a dispatch.
pub const MAX_AGE_SECS: u64 = 120;
const DEFAULT_BUCKET: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub used_percent: i32,
    pub resets_at: Option<i64>,
    pub duration_mins: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bucket {
    pub primary: Option<Window>,
    pub secondary: Option<Window>,
    pub observed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Balance {
    Unknown,
    Unlimited,
    Known(Decimal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    AccountChanged,
    BalanceChanged {
        from: String,
        to: String,
    },
    BalanceUnavailable,
    NoBalanceBaseline,
    NoReading,
    Stale {
        bucket: String,
        age_secs: u64,
    },
    AtOrOverThreshold {
        bucket: String,
        window: &'static str,
        percent: i32,
    },
}

#[derive(Debug, Clone)]
pub struct RateLimitState {
    buckets: BTreeMap<String, Bucket>,
    balance: Balance,
    baseline: Option<Decimal>,
    account_id: Option<String>,
    account_changed: bool,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitState {
    pub fn new() -> Self {
        RateLimitState {
            buckets: BTreeMap::new(),
            balance: Balance::Unknown,
            baseline: None,
            account_id: None,
            account_changed: false,
        }
    }

    pub fn buckets(&self) -> &BTreeMap<String, Bucket> {
        &self.buckets
    }

    pub fn balance(&self) -> &Balance {
        &self.balance
    }

    /// An authoritative `account/rateLimits/read`. Absent credits mean the balance is unknown,
    /// and an absent window means the plan has no such window.
    pub fn apply_read(&mut self, resp: &GetAccountRateLimitsResponse, now: u64) {
        if let Some(id) = &resp.account_id {
            match &self.account_id {
                None => self.account_id = Some(id.clone()),
                Some(known) if known != id => self.account_changed = true,
                Some(_) => {}
            }
        }
        let by_id = resp
            .rate_limits_by_limit_id
            .iter()
            .flatten()
            .map(|(_, s)| s);
        for snap in std::iter::once(&resp.rate_limits).chain(by_id) {
            let b = self.buckets.entry(key_of(snap)).or_default();
            b.primary = snap.primary.as_ref().map(window_of);
            b.secondary = snap.secondary.as_ref().map(window_of);
            b.observed_at = now;
        }
        self.balance = resp
            .rate_limits
            .credits
            .as_ref()
            .map_or(Balance::Unknown, balance_of);
    }

    /// A sparse `account/rateLimits/updated`. Absent fields keep their known values.
    pub fn apply_update(&mut self, snap: &RateLimitSnapshot, now: u64) {
        let b = self.buckets.entry(key_of(snap)).or_default();
        if let Some(w) = &snap.primary {
            b.primary = Some(window_of(w));
        }
        if let Some(w) = &snap.secondary {
            b.secondary = Some(window_of(w));
        }
        b.observed_at = now;
        if let Some(c) = &snap.credits {
            self.balance = balance_of(c);
        }
    }

    /// Restores a baseline saved by an earlier invocation, so a change between runs is still seen.
    pub fn restore_baseline(&mut self, balance: Decimal, account_id: Option<String>) {
        self.baseline = Some(balance);
        if account_id.is_some() {
            self.account_id = account_id;
        }
    }

    pub fn baseline(&self) -> Option<&Decimal> {
        self.baseline.as_ref()
    }

    pub fn account_id(&self) -> Option<&str> {
        self.account_id.as_deref()
    }

    /// Freezes the credit balance for the live group. Fails if the balance is not a known decimal.
    pub fn freeze_baseline(&mut self) -> Result<(), Block> {
        match &self.balance {
            Balance::Known(d) => {
                self.baseline = Some(d.clone());
                Ok(())
            }
            _ => Err(Block::BalanceUnavailable),
        }
    }

    /// Conditions that stop active work at any time. Staleness is not one of them.
    pub fn stop_condition(&self) -> Option<Block> {
        if self.account_changed {
            return Some(Block::AccountChanged);
        }
        if let Some(base) = &self.baseline {
            match &self.balance {
                Balance::Known(now) if now == base => {}
                Balance::Known(now) => {
                    return Some(Block::BalanceChanged {
                        from: base.to_string(),
                        to: now.to_string(),
                    })
                }
                Balance::Unlimited => {
                    return Some(Block::BalanceChanged {
                        from: base.to_string(),
                        to: "unlimited".into(),
                    })
                }
                Balance::Unknown => return Some(Block::BalanceUnavailable),
            }
        }
        for (name, b) in &self.buckets {
            for (window, w) in [("primary", &b.primary), ("secondary", &b.secondary)] {
                if let Some(w) = w.as_ref().filter(|w| w.used_percent >= STOP_PERCENT) {
                    return Some(Block::AtOrOverThreshold {
                        bucket: name.clone(),
                        window,
                        percent: w.used_percent,
                    });
                }
            }
        }
        None
    }

    /// Everything that must hold before a new dispatch.
    pub fn dispatch_gate(&self, now: u64) -> Result<(), Block> {
        if let Some(stop) = self.stop_condition() {
            return Err(stop);
        }
        if self.buckets.is_empty()
            || self
                .buckets
                .values()
                .any(|b| b.primary.is_none() && b.secondary.is_none())
        {
            return Err(Block::NoReading);
        }
        if self.baseline.is_none() {
            return Err(Block::NoBalanceBaseline);
        }
        for (name, b) in &self.buckets {
            let age = now.saturating_sub(b.observed_at);
            if age > MAX_AGE_SECS {
                return Err(Block::Stale {
                    bucket: name.clone(),
                    age_secs: age,
                });
            }
        }
        Ok(())
    }
}

fn key_of(s: &RateLimitSnapshot) -> String {
    s.limit_id
        .clone()
        .unwrap_or_else(|| DEFAULT_BUCKET.to_string())
}

fn balance_of(c: &CreditsSnapshot) -> Balance {
    if c.unlimited {
        return Balance::Unlimited;
    }
    c.balance
        .as_deref()
        .and_then(Decimal::parse)
        .map_or(Balance::Unknown, Balance::Known)
}

fn window_of(w: &RateLimitWindow) -> Window {
    Window {
        used_percent: w.used_percent,
        resets_at: w.resets_at,
        duration_mins: w.window_duration_mins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn snap(v: Value) -> RateLimitSnapshot {
        serde_json::from_value(v).expect("snapshot json")
    }

    fn read(
        account: Option<&str>,
        main: Value,
        extra: &[(&str, Value)],
    ) -> GetAccountRateLimitsResponse {
        let mut by_id = serde_json::Map::new();
        for (k, v) in extra {
            by_id.insert((*k).to_string(), v.clone());
        }
        let mut v = json!({ "rateLimits": main });
        if let Some(a) = account {
            v["accountId"] = json!(a);
        }
        if !by_id.is_empty() {
            v["rateLimitsByLimitId"] = Value::Object(by_id);
        }
        serde_json::from_value(v).expect("response json")
    }

    fn bucket(
        id: &str,
        primary: Option<i32>,
        secondary: Option<i32>,
        balance: Option<&str>,
    ) -> Value {
        let w = |p: i32| json!({"usedPercent": p, "windowDurationMins": 300, "resetsAt": 1_790_000_000});
        let mut v = json!({ "limitId": id });
        v["primary"] = primary.map_or(Value::Null, w);
        v["secondary"] = secondary.map_or(Value::Null, w);
        v["credits"] = match balance {
            Some(b) => json!({"hasCredits": true, "unlimited": false, "balance": b}),
            None => Value::Null,
        };
        v
    }

    fn ready(pct: i32) -> RateLimitState {
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(pct), Some(5), Some("10.00")),
                &[],
            ),
            1000,
        );
        s.freeze_baseline().unwrap();
        s
    }

    #[test]
    fn clear_reading_passes_the_gate() {
        assert_eq!(ready(12).dispatch_gate(1010), Ok(()));
    }

    #[test]
    fn rounded_fifty_percent_blocks_and_forty_nine_does_not() {
        assert_eq!(ready(49).dispatch_gate(1010), Ok(()));
        assert!(matches!(
            ready(50).dispatch_gate(1010),
            Err(Block::AtOrOverThreshold { percent: 50, .. })
        ));
        assert!(matches!(
            ready(51).stop_condition(),
            Some(Block::AtOrOverThreshold { .. })
        ));
    }

    #[test]
    fn secondary_window_counts() {
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(10), Some(60), Some("10")),
                &[],
            ),
            1000,
        );
        s.freeze_baseline().unwrap();
        match s.stop_condition() {
            Some(Block::AtOrOverThreshold {
                window, percent, ..
            }) => {
                assert_eq!((window, percent), ("secondary", 60));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn absent_five_hour_window_is_not_an_error() {
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", None, Some(8), Some("10")),
                &[],
            ),
            1000,
        );
        s.freeze_baseline().unwrap();
        assert_eq!(s.dispatch_gate(1001), Ok(()));
    }

    #[test]
    fn unknown_bucket_is_guarded_too() {
        let mut s = ready(10);
        s.apply_update(&snap(bucket("codex_future_x", Some(70), None, None)), 1005);
        match s.stop_condition() {
            Some(Block::AtOrOverThreshold { bucket, .. }) => assert_eq!(bucket, "codex_future_x"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multi_bucket_read_tracks_each_bucket() {
        let mut s = RateLimitState::new();
        let main = bucket("codex", Some(10), Some(5), Some("10"));
        let other = bucket("codex_other", Some(55), None, None);
        s.apply_read(
            &read(
                Some("acct"),
                main.clone(),
                &[("codex", main), ("codex_other", other)],
            ),
            1000,
        );
        s.freeze_baseline().unwrap();
        assert_eq!(s.buckets().len(), 2);
        assert!(matches!(
            s.stop_condition(),
            Some(Block::AtOrOverThreshold { .. })
        ));
    }

    #[test]
    fn update_without_limit_id_uses_default_bucket() {
        let mut s = RateLimitState::new();
        s.apply_update(&snap(json!({"primary": {"usedPercent": 3}})), 1000);
        assert!(s.buckets().contains_key("default"));
    }

    #[test]
    fn null_credits_on_first_read_cannot_freeze_a_baseline() {
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(Some("acct"), bucket("codex", Some(5), None, None), &[]),
            1000,
        );
        assert_eq!(s.freeze_baseline(), Err(Block::BalanceUnavailable));
    }

    #[test]
    fn unparseable_balance_is_unknown() {
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(5), None, Some("lots")),
                &[],
            ),
            1000,
        );
        assert_eq!(*s.balance(), Balance::Unknown);
    }

    #[test]
    fn sparse_update_keeps_other_windows_and_credits() {
        let mut s = ready(10);
        s.apply_update(
            &snap(json!({"limitId": "codex", "primary": {"usedPercent": 20}})),
            1010,
        );
        let b = &s.buckets()["codex"];
        assert_eq!(b.primary.as_ref().unwrap().used_percent, 20);
        assert_eq!(b.secondary.as_ref().unwrap().used_percent, 5);
        assert_eq!(*s.balance(), Balance::Known(Decimal::parse("10").unwrap()));
        assert_eq!(s.stop_condition(), None);
    }

    #[test]
    fn sparse_update_with_null_credits_does_not_clear_the_balance() {
        let mut s = ready(10);
        s.apply_update(&snap(bucket("codex", Some(11), None, None)), 1010);
        assert_eq!(*s.balance(), Balance::Known(Decimal::parse("10").unwrap()));
        assert_eq!(s.stop_condition(), None);
    }

    #[test]
    fn authoritative_read_with_null_credits_after_a_reading_stops() {
        let mut s = ready(10);
        s.apply_read(
            &read(Some("acct"), bucket("codex", Some(10), Some(5), None), &[]),
            1020,
        );
        assert_eq!(s.stop_condition(), Some(Block::BalanceUnavailable));
    }

    #[test]
    fn balance_change_stops_but_a_respelled_balance_does_not() {
        let mut s = ready(10);
        s.apply_update(&snap(bucket("codex", Some(10), None, Some("10.0"))), 1010);
        assert_eq!(s.stop_condition(), None);
        s.apply_update(&snap(bucket("codex", Some(10), None, Some("9.99"))), 1020);
        assert_eq!(
            s.stop_condition(),
            Some(Block::BalanceChanged {
                from: "10".into(),
                to: "9.99".into()
            })
        );
    }

    #[test]
    fn stale_reading_blocks_dispatch_but_does_not_stop_active_work() {
        let s = ready(10);
        assert_eq!(s.stop_condition(), None);
        assert!(matches!(
            s.dispatch_gate(1000 + MAX_AGE_SECS + 1),
            Err(Block::Stale { age_secs, .. }) if age_secs == MAX_AGE_SECS + 1
        ));
        assert_eq!(s.dispatch_gate(1000 + MAX_AGE_SECS), Ok(()));
    }

    #[test]
    fn fresh_update_clears_staleness() {
        let mut s = ready(10);
        s.apply_update(&snap(bucket("codex", Some(10), None, None)), 5000);
        assert_eq!(s.dispatch_gate(5001), Ok(()));
    }

    #[test]
    fn duplicate_snapshots_change_nothing() {
        let mut s = ready(10);
        let before = s.buckets().clone();
        let dup = snap(bucket("codex", Some(10), Some(5), Some("10.00")));
        s.apply_update(&dup, 1000);
        s.apply_update(&dup, 1000);
        assert_eq!(*s.buckets(), before);
        assert_eq!(s.stop_condition(), None);
    }

    #[test]
    fn account_change_stops_and_is_sticky() {
        let mut s = ready(10);
        s.apply_read(
            &read(
                Some("other-acct"),
                bucket("codex", Some(10), Some(5), Some("10")),
                &[],
            ),
            1010,
        );
        assert_eq!(s.stop_condition(), Some(Block::AccountChanged));
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(10), Some(5), Some("10")),
                &[],
            ),
            1020,
        );
        assert_eq!(s.stop_condition(), Some(Block::AccountChanged));
    }

    #[test]
    fn a_restored_baseline_catches_a_change_since_the_last_invocation() {
        let mut s = RateLimitState::new();
        s.restore_baseline(Decimal::parse("1250").unwrap(), Some("acct".into()));
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(2), None, Some("1250.0000000000")),
                &[],
            ),
            1000,
        );
        assert_eq!(s.stop_condition(), None);
        assert_eq!(s.dispatch_gate(1001), Ok(()));
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(2), None, Some("1249.5")),
                &[],
            ),
            1010,
        );
        assert!(matches!(
            s.stop_condition(),
            Some(Block::BalanceChanged { .. })
        ));
    }

    #[test]
    fn a_restored_account_id_catches_an_account_change() {
        let mut s = RateLimitState::new();
        s.restore_baseline(Decimal::parse("10").unwrap(), Some("acct".into()));
        s.apply_read(
            &read(
                Some("someone-else"),
                bucket("codex", Some(2), None, Some("10")),
                &[],
            ),
            1000,
        );
        assert_eq!(s.stop_condition(), Some(Block::AccountChanged));
    }

    #[test]
    fn before_any_read_a_restored_baseline_leaves_the_balance_unavailable() {
        let mut s = RateLimitState::new();
        s.restore_baseline(Decimal::parse("10").unwrap(), None);
        assert_eq!(s.baseline().map(|d| d.to_string()), Some("10".into()));
        // The balance is still unknown until a read arrives, and unknown after a baseline is a stop.
        assert_eq!(s.stop_condition(), Some(Block::BalanceUnavailable));
    }

    #[test]
    fn no_reading_and_no_baseline_block_dispatch() {
        let s = RateLimitState::new();
        assert_eq!(s.dispatch_gate(0), Err(Block::NoReading));
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(
                Some("acct"),
                bucket("codex", Some(5), None, Some("10")),
                &[],
            ),
            1000,
        );
        assert_eq!(s.dispatch_gate(1001), Err(Block::NoBalanceBaseline));
    }

    #[test]
    fn bucket_with_no_windows_is_no_reading() {
        let mut s = RateLimitState::new();
        s.apply_read(
            &read(Some("acct"), bucket("codex", None, None, Some("10")), &[]),
            1000,
        );
        s.freeze_baseline().unwrap();
        assert_eq!(s.dispatch_gate(1001), Err(Block::NoReading));
    }
}
