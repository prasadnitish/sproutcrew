//! Sleep and wake. The wall clock keeps running while a Mac sleeps and the monotonic clock does not, so a
//! wall-clock jump the monotonic clock did not see means the machine slept. Anything keyed to the wall clock
//! (the timebox) stays correct without help. This tells the app that a sleep happened, so it can check
//! tasks that were running across it.

pub const THRESHOLD_SECS: u64 = 30;

#[derive(Debug, PartialEq, Eq)]
pub struct Slept {
    pub secs: u64,
}

pub struct ClockWatch {
    wall: u64,
    mono: u64,
}

impl ClockWatch {
    pub fn new(wall_secs: u64, mono_secs: u64) -> ClockWatch {
        ClockWatch {
            wall: wall_secs,
            mono: mono_secs,
        }
    }

    /// Call periodically with both clocks. Reports a sleep once and then measures from the new reading.
    pub fn observe(&mut self, wall_secs: u64, mono_secs: u64) -> Option<Slept> {
        let dw = wall_secs.saturating_sub(self.wall);
        let dm = mono_secs.saturating_sub(self.mono);
        self.wall = wall_secs;
        self.mono = mono_secs;
        let gap = dw.saturating_sub(dm);
        (gap >= THRESHOLD_SECS).then_some(Slept { secs: gap })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timebox::{Phase, Timebox};

    #[test]
    fn a_wall_jump_the_monotonic_clock_missed_is_a_sleep() {
        let mut w = ClockWatch::new(1000, 50);
        assert_eq!(w.observe(1010, 60), None, "ordinary time");
        assert_eq!(w.observe(1015, 65), None);
        assert_eq!(
            w.observe(1015 + 1800 + 5, 70),
            Some(Slept { secs: 1800 + 5 - 5 }),
            "half an hour asleep"
        );
        assert_eq!(w.observe(1015 + 1800 + 15, 80), None, "only once");
    }

    #[test]
    fn small_drift_and_a_clock_set_backwards_are_not_sleeps() {
        let mut w = ClockWatch::new(1000, 0);
        assert_eq!(w.observe(1020, 0), None, "below the threshold");
        assert_eq!(w.observe(900, 10), None, "the wall clock went backwards");
        assert_eq!(w.observe(901, 11), None);
    }

    #[test]
    fn the_timebox_stays_correct_across_a_sleep_because_it_uses_the_wall_clock() {
        let t = Timebox::new(0, 5400);
        assert_eq!(t.phase(1000), Phase::Normal);
        // The Mac sleeps for 80 minutes. On wake the wall clock says so, and the timebox is over.
        assert_eq!(t.phase(1000 + 4800), Phase::Over);
        assert_eq!(t.seconds_left(1000 + 4800), 0);
        // A shorter sleep lands in the last ten minutes.
        assert_eq!(t.phase(1000 + 4000), Phase::RequiredOnly);
    }
}
