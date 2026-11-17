//! Budget reservations. A task reserves its worst case before it dispatches, so two tasks that run at the
//! same time cannot both pass a gate that has room for one. Each meter has its own budget and the meters
//! are never added together. Usage that is unknown is charged at the reserved amount and flagged, because
//! unknown is never read as zero.

use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The budget has no room for this reservation.
    Insufficient {
        meter: String,
        wanted: u64,
        available: u64,
    },
    /// The budget is exhausted or was paused. Nothing dispatches until it is raised.
    Paused {
        meter: String,
    },
    UnknownMeter(String),
    AlreadyReserved(String),
    NotReserved(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Charge {
    Known(u64),
    /// The provider gave no usage. The reservation is kept as the charge.
    Unknown,
}

#[derive(Debug, Default)]
struct Budget {
    limit: u64,
    spent: u64,
    unknown_charges: u32,
    /// task key to reserved amount
    held: BTreeMap<String, u64>,
    /// task keys already settled, so a retry or resume cannot charge twice
    settled: BTreeMap<String, u64>,
    paused: bool,
}

impl Budget {
    fn reserved(&self) -> u64 {
        self.held.values().sum()
    }
    fn available(&self) -> u64 {
        self.limit.saturating_sub(self.spent + self.reserved())
    }
}

#[derive(Default)]
pub struct Reservations {
    meters: BTreeMap<String, Budget>,
}

impl Reservations {
    fn budget(&mut self, meter: &str) -> Result<&mut Budget, Refusal> {
        self.meters
            .get_mut(meter)
            .ok_or_else(|| Refusal::UnknownMeter(meter.to_string()))
    }

    fn view(&self, meter: &str) -> Result<&Budget, Refusal> {
        self.meters
            .get(meter)
            .ok_or_else(|| Refusal::UnknownMeter(meter.to_string()))
    }

    pub fn set_budget(&mut self, meter: &str, limit: u64) {
        self.meters.entry(meter.to_string()).or_default().limit = limit;
    }

    pub fn reserve(&mut self, meter: &str, task: &str, amount: u64) -> Result<(), Refusal> {
        let b = self.budget(meter)?;
        if b.paused {
            return Err(Refusal::Paused {
                meter: meter.to_string(),
            });
        }
        if b.held.contains_key(task) || b.settled.contains_key(task) {
            return Err(Refusal::AlreadyReserved(task.to_string()));
        }
        let available = b.available();
        if amount > available {
            return Err(Refusal::Insufficient {
                meter: meter.to_string(),
                wanted: amount,
                available,
            });
        }
        b.held.insert(task.to_string(), amount);
        Ok(())
    }

    /// Settles a reserved task. Settling the same task again changes nothing.
    pub fn settle(&mut self, meter: &str, task: &str, charge: Charge) -> Result<u64, Refusal> {
        let b = self.budget(meter)?;
        let Some(reserved) = b.held.remove(task) else {
            return b
                .settled
                .get(task)
                .copied()
                .ok_or_else(|| Refusal::NotReserved(task.to_string()));
        };
        let charged = match charge {
            Charge::Known(n) => n,
            Charge::Unknown => {
                b.unknown_charges += 1;
                reserved
            }
        };
        b.spent += charged;
        b.settled.insert(task.to_string(), charged);
        if b.spent >= b.limit {
            b.paused = true;
        }
        Ok(charged)
    }

    /// Releases a reservation without a charge, for a task that never started.
    pub fn release(&mut self, meter: &str, task: &str) -> Result<(), Refusal> {
        self.budget(meter)?
            .held
            .remove(task)
            .map(|_| ())
            .ok_or_else(|| Refusal::NotReserved(task.to_string()))
    }

    pub fn available(&self, meter: &str) -> Result<u64, Refusal> {
        Ok(self.view(meter)?.available())
    }

    pub fn spent(&self, meter: &str) -> Result<u64, Refusal> {
        Ok(self.view(meter)?.spent)
    }

    pub fn unknown_charges(&self, meter: &str) -> Result<u32, Refusal> {
        Ok(self.view(meter)?.unknown_charges)
    }

    pub fn is_paused(&self, meter: &str) -> Result<bool, Refusal> {
        Ok(self.view(meter)?.paused)
    }

    /// Counts usage that was spent before this session, such as from the journal. Pauses at the limit.
    pub fn seed_spent(&mut self, meter: &str, amount: u64) -> Result<(), Refusal> {
        let b = self.budget(meter)?;
        b.spent += amount;
        if b.spent >= b.limit {
            b.paused = true;
        }
        Ok(())
    }

    pub fn limit(&self, meter: &str) -> Result<u64, Refusal> {
        Ok(self.view(meter)?.limit)
    }

    /// Nitish raises the limit and clears the pause, unless the new limit is still spent.
    pub fn raise_and_resume(&mut self, meter: &str, new_limit: u64) -> Result<(), Refusal> {
        let b = self.budget(meter)?;
        b.limit = new_limit;
        b.paused = b.spent >= b.limit;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(limit: u64) -> Reservations {
        let mut r = Reservations::default();
        r.set_budget("tokens", limit);
        r
    }

    #[test]
    fn two_parallel_tasks_cannot_both_pass_a_gate_that_fits_one() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 60).unwrap();
        let err = r.reserve("tokens", "b", 60).unwrap_err();
        assert_eq!(
            err,
            Refusal::Insufficient {
                meter: "tokens".into(),
                wanted: 60,
                available: 40
            }
        );
        // Once the first is released, the second fits.
        r.release("tokens", "a").unwrap();
        r.reserve("tokens", "b", 60).unwrap();
    }

    #[test]
    fn settling_frees_the_unused_part_of_a_reservation() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 60).unwrap();
        assert_eq!(r.settle("tokens", "a", Charge::Known(20)).unwrap(), 20);
        assert_eq!(r.spent("tokens").unwrap(), 20);
        assert_eq!(r.available("tokens").unwrap(), 80);
        r.reserve("tokens", "b", 80).unwrap();
    }

    #[test]
    fn settling_twice_charges_once_so_a_retry_or_resume_cannot_double_count() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 50).unwrap();
        r.settle("tokens", "a", Charge::Known(30)).unwrap();
        // The retry reports the same task again, even with a different number.
        assert_eq!(r.settle("tokens", "a", Charge::Known(30)).unwrap(), 30);
        assert_eq!(r.settle("tokens", "a", Charge::Known(99)).unwrap(), 30);
        assert_eq!(r.spent("tokens").unwrap(), 30);
    }

    #[test]
    fn unknown_usage_is_charged_at_the_reserved_amount_and_flagged() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 40).unwrap();
        assert_eq!(r.settle("tokens", "a", Charge::Unknown).unwrap(), 40);
        assert_eq!(r.spent("tokens").unwrap(), 40);
        assert_eq!(r.unknown_charges("tokens").unwrap(), 1);
    }

    #[test]
    fn an_overrun_is_recorded_and_pauses_the_budget() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 60).unwrap();
        r.settle("tokens", "a", Charge::Known(130)).unwrap();
        assert_eq!(r.spent("tokens").unwrap(), 130);
        assert!(r.is_paused("tokens").unwrap());
        assert_eq!(
            r.reserve("tokens", "b", 1),
            Err(Refusal::Paused {
                meter: "tokens".into()
            })
        );
    }

    #[test]
    fn spending_exactly_the_limit_pauses_and_raising_it_resumes() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 100).unwrap();
        r.settle("tokens", "a", Charge::Known(100)).unwrap();
        assert!(r.is_paused("tokens").unwrap());
        assert!(r.reserve("tokens", "b", 1).is_err());
        r.raise_and_resume("tokens", 150).unwrap();
        assert!(!r.is_paused("tokens").unwrap());
        r.reserve("tokens", "b", 50).unwrap();
        assert!(matches!(
            r.reserve("tokens", "c", 1),
            Err(Refusal::Insufficient { .. })
        ));
    }

    #[test]
    fn raising_to_a_limit_that_is_still_spent_leaves_it_paused() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 100).unwrap();
        r.settle("tokens", "a", Charge::Known(100)).unwrap();
        r.raise_and_resume("tokens", 100).unwrap();
        assert!(r.is_paused("tokens").unwrap(), "no room was added");
    }

    #[test]
    fn meters_have_separate_budgets_and_are_never_summed() {
        let mut r = tokens(100);
        r.set_budget("plan_window", 50);
        r.reserve("tokens", "a", 100).unwrap();
        // A full tokens budget does not touch the plan window.
        r.reserve("plan_window", "a", 50).unwrap();
        assert_eq!(r.available("tokens").unwrap(), 0);
        assert_eq!(r.available("plan_window").unwrap(), 0);
        r.settle("tokens", "a", Charge::Known(10)).unwrap();
        assert_eq!(r.spent("plan_window").unwrap(), 0);
    }

    #[test]
    fn the_same_task_cannot_reserve_twice_and_unreserved_tasks_cannot_settle() {
        let mut r = tokens(100);
        r.reserve("tokens", "a", 10).unwrap();
        assert_eq!(
            r.reserve("tokens", "a", 10),
            Err(Refusal::AlreadyReserved("a".into()))
        );
        assert_eq!(
            r.settle("tokens", "ghost", Charge::Known(1)),
            Err(Refusal::NotReserved("ghost".into()))
        );
        assert_eq!(
            r.release("tokens", "ghost"),
            Err(Refusal::NotReserved("ghost".into()))
        );
        assert!(matches!(
            r.reserve("nope", "a", 1),
            Err(Refusal::UnknownMeter(_))
        ));
    }

    #[test]
    fn earlier_spending_counts_against_the_budget_and_can_pause_it_at_once() {
        let mut r = tokens(100);
        r.seed_spent("tokens", 30).unwrap();
        assert_eq!(r.spent("tokens").unwrap(), 30);
        assert_eq!(r.available("tokens").unwrap(), 70);
        assert_eq!(r.limit("tokens").unwrap(), 100);
        assert!(!r.is_paused("tokens").unwrap());
        r.seed_spent("tokens", 70).unwrap();
        assert!(
            r.is_paused("tokens").unwrap(),
            "seeded spending reached the limit"
        );
        assert!(matches!(
            r.seed_spent("nope", 1),
            Err(Refusal::UnknownMeter(_))
        ));
    }
}
