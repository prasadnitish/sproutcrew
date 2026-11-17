//! A session clock. At ten minutes before the end, optional work stops and what was left out is labelled,
//! so a short package says what it is missing instead of looking complete.

pub const OPTIONAL_STOP_SECS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// More than ten minutes left.
    Normal,
    /// Ten minutes or less: required work only.
    RequiredOnly,
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omission {
    pub item: String,
    pub label: String,
}

pub struct Timebox {
    start: u64,
    total_secs: u64,
    omitted: Vec<Omission>,
}

impl Timebox {
    pub fn new(start: u64, total_secs: u64) -> Timebox {
        Timebox {
            start,
            total_secs,
            omitted: Vec::new(),
        }
    }

    fn left(&self, now: u64) -> u64 {
        (self.start + self.total_secs).saturating_sub(now)
    }

    pub fn seconds_left(&self, now: u64) -> u64 {
        self.left(now)
    }

    pub fn phase(&self, now: u64) -> Phase {
        match self.left(now) {
            0 => Phase::Over,
            l if l <= OPTIONAL_STOP_SECS => Phase::RequiredOnly,
            _ => Phase::Normal,
        }
    }

    /// Whether a piece of work may start now. Optional work stops at T minus 10 and is recorded as left
    /// out. Required work runs until the end.
    pub fn may_start(&mut self, item: &str, optional: bool, now: u64) -> bool {
        let ok = match self.phase(now) {
            Phase::Normal => true,
            Phase::RequiredOnly => !optional,
            Phase::Over => false,
        };
        if !ok {
            let why = if self.phase(now) == Phase::Over {
                "the time ran out"
            } else {
                "optional work stopped ten minutes before the end"
            };
            self.omitted.push(Omission {
                item: item.to_string(),
                label: format!("Omitted: {why}."),
            });
        }
        ok
    }

    pub fn omissions(&self) -> &[Omission] {
        &self.omitted
    }

    /// A Markdown section for the package that names every omission.
    pub fn omissions_markdown(&self) -> String {
        if self.omitted.is_empty() {
            return "No work was left out for time.\n".into();
        }
        let mut s = String::from("## Left out for time\n\n");
        for o in &self.omitted {
            s.push_str(&format!("- {}: {}\n", o.item, o.label));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phase_changes_at_ten_minutes_and_at_the_end() {
        let t = Timebox::new(1000, 5400);
        assert_eq!(t.phase(1000), Phase::Normal);
        assert_eq!(t.phase(1000 + 5400 - 601), Phase::Normal);
        assert_eq!(
            t.phase(1000 + 5400 - 600),
            Phase::RequiredOnly,
            "exactly T minus 10"
        );
        assert_eq!(t.phase(1000 + 5400 - 1), Phase::RequiredOnly);
        assert_eq!(t.phase(1000 + 5400), Phase::Over);
        assert_eq!(t.phase(1000 + 9999), Phase::Over);
        assert_eq!(t.seconds_left(1000 + 5400 + 5), 0, "no underflow");
    }

    #[test]
    fn optional_work_stops_at_t_minus_10_and_is_labelled_while_required_work_continues() {
        let mut t = Timebox::new(0, 3600);
        assert!(t.may_start("extra diagram", true, 100));
        let late = 3600 - 300;
        assert!(!t.may_start("competitor table", true, late));
        assert!(
            t.may_start("recommendation", false, late),
            "required work still runs"
        );
        assert!(
            !t.may_start("polish", false, 3600),
            "nothing starts after the end"
        );
        let o = t.omissions();
        assert_eq!(o.len(), 2);
        assert!(o[0].label.contains("ten minutes"));
        assert!(o[1].label.contains("ran out"));
        let md = t.omissions_markdown();
        assert!(
            md.contains("competitor table")
                && md.contains("polish")
                && !md.contains("extra diagram")
        );
    }

    #[test]
    fn a_session_with_nothing_omitted_says_so() {
        let t = Timebox::new(0, 100);
        assert!(t.omissions_markdown().contains("No work was left out"));
    }

    #[test]
    fn a_short_session_is_required_only_from_the_start() {
        let mut t = Timebox::new(0, 400);
        assert_eq!(t.phase(0), Phase::RequiredOnly);
        assert!(!t.may_start("optional", true, 0));
    }
}
