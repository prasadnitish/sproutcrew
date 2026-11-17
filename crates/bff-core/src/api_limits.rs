//! Why an API provider refused a request for its rate or its credit, in one sentence, and whether waiting and
//! trying again is sensible. The rules follow OpenRouter's documentation, read Nov 10, 2026: a limit of its own
//! comes with `X-RateLimit-Limit` and `X-RateLimit-Reset` headers (free models allow 20 requests a minute, and 50
//! or 1,000 a day); a provider that is rate limited or at capacity is named by `error.metadata.provider_code`;
//! `Retry-After` comes when every provider tried gave a retry hint; 402 means the account is out of credit.
//! A key with a credit limit of its own answers 403 with "Key limit exceeded" once it is spent (OpenRouter's
//! guide to capping keys, read Nov 16, 2026); any other 403 is a refused key and is not a limit.
//! Anything that cannot be told apart is `Other` and is never retried, because a retry on a free model uses up
//! the day's allowance.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What the provider's answer said about limits, read from its headers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hints {
    /// Seconds to wait, from `Retry-After`.
    pub retry_after: Option<u64>,
    /// The size of the limit that was hit, from `X-RateLimit-Limit`.
    pub limit: Option<u64>,
    /// When the limit resets, in milliseconds since 1970, from `X-RateLimit-Reset`.
    pub reset_ms: Option<u64>,
}

impl Hints {
    /// Reads the headers this module uses. Header names are matched without regard to case.
    pub fn from_headers<'a>(headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> Hints {
        let mut h = Hints::default();
        for (name, value) in headers {
            let v = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "retry-after" => h.retry_after = v.parse().ok(),
                "x-ratelimit-limit" => h.limit = v.parse().ok(),
                "x-ratelimit-reset" => h.reset_ms = v.parse().ok(),
                _ => {}
            }
        }
        h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    PerMinute,
    DailyCap,
    UpstreamBusy,
    OutOfCredits,
    KeyLimit,
    Other,
}

impl Kind {
    /// Only a short wait can clear these, so the app may wait and try again on its own.
    pub fn retryable(self) -> bool {
        matches!(self, Kind::PerMinute | Kind::UpstreamBusy)
    }

    pub fn word(self) -> &'static str {
        match self {
            Kind::PerMinute => "per_minute",
            Kind::DailyCap => "daily_cap",
            Kind::UpstreamBusy => "upstream_busy",
            Kind::OutOfCredits => "out_of_credits",
            Kind::KeyLimit => "key_limit",
            Kind::Other => "other",
        }
    }
}

/// What a 402, a 429, or a 403 for a key at its own limit means. Any other answer is not a limit and gives `None`.
pub fn classify(status: u16, hints: &Hints, body: &Value, now_ms: u64) -> Option<Kind> {
    match status {
        402 => return Some(Kind::OutOfCredits),
        403 => {
            let said = body["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            return said
                .contains("key limit exceeded")
                .then_some(Kind::KeyLimit);
        }
        429 => {}
        _ => return None,
    }
    let meta = &body["error"]["metadata"];
    if meta["provider_code"].is_string() || meta["provider_name"].is_string() {
        return Some(Kind::UpstreamBusy);
    }
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if message.contains("per-day") || message.contains("per day") || message.contains("daily") {
        return Some(Kind::DailyCap);
    }
    if message.contains("per-min") || message.contains("per min") {
        return Some(Kind::PerMinute);
    }
    match hints.limit {
        Some(20) => return Some(Kind::PerMinute),
        Some(50) | Some(1000) => return Some(Kind::DailyCap),
        _ => {}
    }
    // A limit that resets within two minutes is a per-minute limit; one that resets later is a daily one.
    if let Some(reset) = hints.reset_ms {
        let wait = reset.saturating_sub(now_ms) / 1000;
        return Some(if wait <= 120 {
            Kind::PerMinute
        } else {
            Kind::DailyCap
        });
    }
    Some(Kind::Other)
}

/// The one-sentence reason shown to the person, ending in a single full stop.
pub fn sentence(kind: Kind, openrouter: bool, free_model: bool) -> String {
    let who = if openrouter {
        "OpenRouter"
    } else {
        "The provider"
    };
    match kind {
        Kind::PerMinute if free_model => {
            "OpenRouter's free models allow 20 requests a minute, and this step hit that limit.".into()
        }
        Kind::PerMinute => format!("{who} is limiting how many requests can be sent each minute."),
        Kind::DailyCap if free_model => {
            "Today's free allowance on OpenRouter is used up: 50 requests a day, or 1,000 once the account has bought $10 of credit.".into()
        }
        Kind::DailyCap => format!("{who} has a daily limit on this account, and it is used up."),
        Kind::UpstreamBusy => "The service running this model is busy right now.".into(),
        Kind::OutOfCredits => format!(
            "Your {} account is out of credit.",
            if openrouter { "OpenRouter" } else { "provider" }
        ),
        Kind::KeyLimit => "This API key has reached the spending limit set for it.".into(),
        Kind::Other => format!("{who} refused the request because of a rate limit."),
    }
}

/// Which limit a failure note is about, from the fixed sentence the engine wrote. A note this module did not
/// write gives `None`.
pub fn kind_of(note: &str) -> Option<Kind> {
    let note = note.strip_prefix("paused: ").unwrap_or(note);
    [
        Kind::PerMinute,
        Kind::DailyCap,
        Kind::UpstreamBusy,
        Kind::OutOfCredits,
        Kind::KeyLimit,
        Kind::Other,
    ]
    .into_iter()
    .find(|&k| {
        [(true, true), (true, false), (false, false)]
            .iter()
            .any(|&(o, f)| note.starts_with(&sentence(k, o, f)))
    })
}

pub const MAX_ATTEMPTS: u32 = 3;

/// Seconds to wait before attempt `next` (2 or 3). The provider's own hint wins; otherwise 2, then 4 seconds.
/// Waits are capped at a minute so a step never sits silently for long.
pub fn wait_secs(hints: &Hints, next: u32, now_ms: u64) -> u64 {
    let hinted = hints.retry_after.or_else(|| {
        hints
            .reset_ms
            .map(|r| r.saturating_sub(now_ms).div_ceil(1000))
    });
    hinted
        .unwrap_or(2u64.pow(next.saturating_sub(1)))
        .clamp(1, 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: u64 = 1_794_355_200_000;

    fn platform() -> Value {
        json!({"error": {"code": 429, "message": "Rate limit exceeded", "metadata": {"error_type": "rate_limit_exceeded"}}})
    }

    #[test]
    fn a_free_model_limit_of_twenty_is_per_minute_and_may_be_retried() {
        let h = Hints {
            limit: Some(20),
            reset_ms: Some(NOW + 30_000),
            retry_after: Some(7),
        };
        let k = classify(429, &h, &platform(), NOW).unwrap();
        assert_eq!(k, Kind::PerMinute);
        assert!(k.retryable());
        assert_eq!(wait_secs(&h, 2, NOW), 7, "Retry-After wins");
    }

    #[test]
    fn a_limit_of_fifty_or_a_thousand_is_the_daily_cap_and_is_never_retried() {
        for limit in [50, 1000] {
            let h = Hints {
                limit: Some(limit),
                reset_ms: Some(NOW + 9 * 3_600_000),
                retry_after: None,
            };
            let k = classify(429, &h, &platform(), NOW).unwrap();
            assert_eq!(k, Kind::DailyCap);
            assert!(!k.retryable());
        }
        let worded =
            json!({"error": {"code": 429, "message": "Rate limit exceeded: free-models-per-day"}});
        assert_eq!(
            classify(429, &Hints::default(), &worded, NOW),
            Some(Kind::DailyCap)
        );
    }

    #[test]
    fn a_busy_upstream_provider_is_named_by_its_own_code_and_may_be_retried() {
        let body = json!({"error": {"code": 429, "message": "Rate limit exceeded",
            "metadata": {"error_type": "rate_limit_exceeded", "provider_code": "rate_limited"}}});
        let k = classify(429, &Hints::default(), &body, NOW).unwrap();
        assert_eq!(k, Kind::UpstreamBusy);
        assert!(k.retryable());
    }

    #[test]
    fn out_of_credit_is_its_own_case_and_is_never_retried() {
        let body = json!({"error": {"code": 402, "message": "Insufficient credits. Add more using https://openrouter.ai/credits"}});
        let k = classify(402, &Hints::default(), &body, NOW).unwrap();
        assert_eq!(k, Kind::OutOfCredits);
        assert!(!k.retryable());
    }

    #[test]
    fn a_key_at_its_own_spending_limit_is_said_plainly_and_is_never_retried() {
        let body = json!({"error": {"code": 403, "message": "Key limit exceeded. Manage it using https://openrouter.ai/settings/keys"}});
        let k = classify(403, &Hints::default(), &body, NOW).unwrap();
        assert_eq!(k, Kind::KeyLimit);
        assert!(!k.retryable());
        let said = sentence(k, true, false);
        assert_eq!(
            said,
            "This API key has reached the spending limit set for it."
        );
        assert_eq!(kind_of(&said), Some(Kind::KeyLimit));
        // Any other 403 is still a refused key, which is not a limit.
        let refused = json!({"error": {"code": 403, "message": "Forbidden"}});
        assert_eq!(classify(403, &Hints::default(), &refused, NOW), None);
        assert_eq!(
            classify(403, &Hints::default(), &json!("denied"), NOW),
            None
        );
    }

    #[test]
    fn the_reset_time_decides_when_the_limit_size_is_missing() {
        let soon = Hints {
            reset_ms: Some(NOW + 45_000),
            ..Hints::default()
        };
        assert_eq!(
            classify(429, &soon, &platform(), NOW),
            Some(Kind::PerMinute)
        );
        let later = Hints {
            reset_ms: Some(NOW + 3_600_000),
            ..Hints::default()
        };
        assert_eq!(
            classify(429, &later, &platform(), NOW),
            Some(Kind::DailyCap)
        );
        assert_eq!(wait_secs(&soon, 2, NOW), 45);
    }

    #[test]
    fn a_429_that_cannot_be_told_apart_is_other_and_is_not_retried() {
        let k = classify(429, &Hints::default(), &Value::Null, NOW).unwrap();
        assert_eq!(k, Kind::Other);
        assert!(!k.retryable());
        assert_eq!(classify(500, &Hints::default(), &platform(), NOW), None);
        assert_eq!(classify(200, &Hints::default(), &platform(), NOW), None);
    }

    #[test]
    fn waits_double_without_a_hint_and_never_exceed_a_minute() {
        let none = Hints::default();
        assert_eq!(wait_secs(&none, 2, NOW), 2);
        assert_eq!(wait_secs(&none, 3, NOW), 4);
        let long = Hints {
            retry_after: Some(3600),
            ..Hints::default()
        };
        assert_eq!(wait_secs(&long, 2, NOW), 60);
    }

    #[test]
    fn headers_are_read_without_regard_to_case() {
        let h = Hints::from_headers([
            ("Retry-After", " 12 "),
            ("x-ratelimit-limit", "20"),
            ("X-RateLimit-Reset", "1794355230000"),
            ("X-Other", "1"),
        ]);
        assert_eq!(
            h,
            Hints {
                retry_after: Some(12),
                limit: Some(20),
                reset_ms: Some(1_794_355_230_000)
            }
        );
    }

    #[test]
    fn a_note_written_from_a_sentence_gives_its_kind_back() {
        for kind in [
            Kind::PerMinute,
            Kind::DailyCap,
            Kind::UpstreamBusy,
            Kind::OutOfCredits,
            Kind::Other,
        ] {
            for (openrouter, free) in [(true, true), (true, false), (false, false)] {
                let note = format!("{} Try again later.", sentence(kind, openrouter, free));
                assert_eq!(kind_of(&note), Some(kind), "{note}");
            }
        }
        assert_eq!(kind_of("timed out"), None);
        assert_eq!(kind_of("rate limit reached (status 429)"), None);
    }

    #[test]
    fn every_sentence_is_one_sentence_with_one_full_stop() {
        for kind in [
            Kind::PerMinute,
            Kind::DailyCap,
            Kind::UpstreamBusy,
            Kind::OutOfCredits,
            Kind::Other,
        ] {
            for (openrouter, free) in [(true, true), (true, false), (false, false)] {
                let s = sentence(kind, openrouter, free);
                assert!(s.ends_with('.') && !s.ends_with(".."), "{s}");
                assert!(!s.to_lowercase().contains("status"), "{s}");
            }
        }
    }
}
