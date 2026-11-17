//! A visible progress line at least every 30 seconds while a stage runs, so no stage goes more than two
//! minutes without something to read.

pub const INTERVAL_SECS: u64 = 30;

pub struct Heartbeat {
    what: String,
    start: u64,
    last: Option<u64>,
}

pub fn elapsed(secs: u64) -> String {
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s}s"),
        (m, s) => format!("{m}m {s:02}s"),
    }
}

impl Heartbeat {
    pub fn new(what: &str, start: u64) -> Heartbeat {
        Heartbeat {
            what: what.to_string(),
            start,
            last: None,
        }
    }

    /// The line to show at `now`, or `None` if one was shown less than 30 seconds ago.
    pub fn line(&mut self, now: u64) -> Option<String> {
        let since = self.last.map(|l| now.saturating_sub(l));
        if since.is_some_and(|s| s < INTERVAL_SECS) {
            return None;
        }
        self.last = Some(now);
        Some(match since {
            None => format!("Started: {}", self.what),
            Some(_) => format!(
                "Still working: {} ({} so far)",
                self.what,
                elapsed(now.saturating_sub(self.start))
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_call_announces_the_start_then_a_line_every_thirty_seconds() {
        let mut h = Heartbeat::new("draft on Codex", 100);
        assert_eq!(h.line(100).as_deref(), Some("Started: draft on Codex"));
        assert_eq!(h.line(110), None);
        assert_eq!(h.line(129), None);
        assert_eq!(
            h.line(130).as_deref(),
            Some("Still working: draft on Codex (30s so far)")
        );
        assert_eq!(
            h.line(160).as_deref(),
            Some("Still working: draft on Codex (1m 00s so far)")
        );
        assert_eq!(h.line(165), None);
    }

    #[test]
    fn ticking_every_second_never_leaves_a_gap_over_thirty_seconds() {
        let mut h = Heartbeat::new("x", 0);
        let mut shown = Vec::new();
        for t in 0..=900 {
            if h.line(t).is_some() {
                shown.push(t);
            }
        }
        let worst = shown.windows(2).map(|w| w[1] - w[0]).max().unwrap();
        assert!(worst <= INTERVAL_SECS, "worst gap {worst}");
        assert!(900 - shown.last().unwrap() <= INTERVAL_SECS);
    }

    #[test]
    fn a_late_tick_after_a_long_pause_shows_a_line_at_once() {
        let mut h = Heartbeat::new("x", 0);
        h.line(0);
        assert!(h.line(500).unwrap().contains("8m 20s"));
    }

    #[test]
    fn elapsed_formats_seconds_and_minutes() {
        assert_eq!(elapsed(5), "5s");
        assert_eq!(elapsed(60), "1m 00s");
        assert_eq!(elapsed(125), "2m 05s");
    }
}
