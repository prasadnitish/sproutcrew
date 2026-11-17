//! Claude plan usage from `claude -p "/usage"`, which costs no tokens (M0 check N3).
//! Required windows: the session window and the all-models weekly window. Any other
//! `Current ...` window the output shows is guarded too.

pub const STOP_PERCENT: f64 = 50.0;
pub const MAX_AGE_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindow {
    /// Without the leading "Current ": `session`, `week (all models)`, `week (Fable)`.
    pub label: String,
    pub percent: f64,
    pub resets: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    pub windows: Vec<UsageWindow>,
    pub observed_at: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UsageBlock {
    MissingWindow(&'static str),
    Stale { age_secs: u64 },
    AtOrOverThreshold { window: String, percent_tenths: i64 },
}

/// `None` when no `Current <label>: N% used` line is found. Only the first line per label counts.
pub fn parse_usage(text: &str, now: u64) -> Option<UsageReading> {
    let mut windows: Vec<UsageWindow> = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("Current ") else {
            continue;
        };
        let Some((label, tail)) = rest.split_once(": ") else {
            continue;
        };
        let Some((num, after)) = tail.split_once('%') else {
            continue;
        };
        let (Ok(percent), true) = (
            num.trim().parse::<f64>(),
            after.trim_start().starts_with("used"),
        ) else {
            continue;
        };
        if windows.iter().any(|w| w.label == label) {
            continue;
        }
        let resets = after
            .split_once(" · resets ")
            .map(|(_, r)| r.trim().to_string());
        windows.push(UsageWindow {
            label: label.to_string(),
            percent,
            resets,
        });
    }
    (!windows.is_empty()).then_some(UsageReading {
        windows,
        observed_at: now,
    })
}

impl UsageReading {
    pub fn gate(&self, now: u64) -> Result<(), UsageBlock> {
        for required in ["session", "week (all models)"] {
            if !self.windows.iter().any(|w| w.label == required) {
                return Err(UsageBlock::MissingWindow(required));
            }
        }
        if let Some(w) = self
            .windows
            .iter()
            .find(|w| w.percent.round() >= STOP_PERCENT)
        {
            return Err(UsageBlock::AtOrOverThreshold {
                window: w.label.clone(),
                percent_tenths: (w.percent * 10.0).round() as i64,
            });
        }
        let age = now.saturating_sub(self.observed_at);
        if age > MAX_AGE_SECS {
            return Err(UsageBlock::Stale { age_secs: age });
        }
        Ok(())
    }

    pub fn max_percent(&self) -> f64 {
        self.windows.iter().map(|w| w.percent).fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "You are currently using your subscription to power your Claude Code usage\n\nCurrent session: 0% used\nCurrent week (all models): 19% used · resets Nov 3 at 5:59am (America/Los_Angeles)\nCurrent week (Fable): 0% used · resets Nov 3 at 6am (America/Los_Angeles)\n\nWhat's contributing to your usage?\nApproximate · sessions on this machine only · shares overlap\n\nLast 24h · 404 requests · 3 sessions\n  93%  Start fresh when you switch tasks · run /clear\n  Top skills: /ponytail 1%\n";

    fn text(session: &str, week: &str) -> String {
        format!("Current session: {session}% used\nCurrent week (all models): {week}% used · resets Nov 3 at 5:59am (America/Los_Angeles)\n")
    }

    #[test]
    fn parses_the_real_output_and_ignores_the_breakdown_below_it() {
        let r = parse_usage(REAL, 100).unwrap();
        assert_eq!(r.observed_at, 100);
        assert_eq!(r.windows.len(), 3);
        assert_eq!(
            r.windows[0],
            UsageWindow {
                label: "session".into(),
                percent: 0.0,
                resets: None
            }
        );
        assert_eq!(r.windows[1].label, "week (all models)");
        assert_eq!(r.windows[1].percent, 19.0);
        assert_eq!(
            r.windows[1].resets.as_deref(),
            Some("Nov 3 at 5:59am (America/Los_Angeles)")
        );
        assert_eq!(r.windows[2].label, "week (Fable)");
        assert_eq!(r.max_percent(), 19.0);
    }

    #[test]
    fn decimals_parse() {
        let r = parse_usage(&text("3.5", "12"), 0).unwrap();
        assert_eq!(r.windows[0].percent, 3.5);
    }

    #[test]
    fn text_with_no_windows_is_not_a_reading() {
        assert!(parse_usage("", 0).is_none());
        assert!(parse_usage("Something else entirely\n98% of nothing", 0).is_none());
        assert!(parse_usage("Current session: lots used", 0).is_none());
    }

    #[test]
    fn a_clear_fresh_reading_passes() {
        assert_eq!(
            parse_usage(&text("10", "49"), 1000).unwrap().gate(1010),
            Ok(())
        );
    }

    #[test]
    fn fifty_and_rounded_fifty_block_on_any_window() {
        let blocked = |t: String| parse_usage(&t, 1000).unwrap().gate(1001);
        assert!(matches!(
            blocked(text("50", "10")),
            Err(UsageBlock::AtOrOverThreshold { .. })
        ));
        assert!(matches!(
            blocked(text("10", "49.5")),
            Err(UsageBlock::AtOrOverThreshold { .. })
        ));
        assert_eq!(blocked(text("10", "49.4")), Ok(()));
        let extra = format!("{}Current week (Sonnet): 77% used\n", text("1", "1"));
        assert!(matches!(
            blocked(extra),
            Err(UsageBlock::AtOrOverThreshold { window, .. }) if window == "week (Sonnet)"
        ));
    }

    #[test]
    fn both_required_windows_must_be_present() {
        let only_week = parse_usage("Current week (all models): 5% used\n", 0).unwrap();
        assert_eq!(only_week.gate(1), Err(UsageBlock::MissingWindow("session")));
        let only_session = parse_usage("Current session: 5% used\n", 0).unwrap();
        assert_eq!(
            only_session.gate(1),
            Err(UsageBlock::MissingWindow("week (all models)"))
        );
    }

    #[test]
    fn an_old_reading_is_stale() {
        let r = parse_usage(&text("1", "1"), 1000).unwrap();
        assert_eq!(r.gate(1000 + MAX_AGE_SECS), Ok(()));
        assert_eq!(
            r.gate(1000 + MAX_AGE_SECS + 1),
            Err(UsageBlock::Stale {
                age_secs: MAX_AGE_SECS + 1
            })
        );
    }
}
