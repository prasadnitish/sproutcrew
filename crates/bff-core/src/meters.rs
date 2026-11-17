//! The three meters (plan section 9): API dollars, plan window, and tokens. They are never summed
//! together, each entry says whether it is reported, estimated, reconciled, or unknown, and unknown is
//! never shown as zero. The ledger is a projection of journal events of kind `usage`.

use crate::journal::{Event, Journal};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Meter {
    ApiDollars,
    PlanWindow,
    Tokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Reported,
    Estimated,
    Reconciled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub step: String,
    pub meter: Meter,
    /// `None` is unknown. For the plan window, the percent after the step.
    pub value: Option<f64>,
    pub status: Status,
    /// The bucket for a plan window, or the model for tokens.
    pub detail: String,
}

#[derive(Debug, PartialEq)]
pub struct Aggregate {
    pub total: Option<f64>,
    pub known: usize,
    pub unknown: usize,
}

/// Records one usage entry in the journal.
pub fn record(j: &Journal, e: &Entry, at: u64) -> Result<i64, String> {
    j.append(
        "usage",
        &json!({"step": e.step, "meter": meter_name(e.meter), "value": e.value, "status": status_name(e.status), "detail": e.detail}),
        at,
    )
}

fn meter_name(m: Meter) -> &'static str {
    match m {
        Meter::ApiDollars => "api_dollars",
        Meter::PlanWindow => "plan_window",
        Meter::Tokens => "tokens",
    }
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Reported => "reported",
        Status::Estimated => "estimated",
        Status::Reconciled => "reconciled",
        Status::Unknown => "unknown",
    }
}

fn parse(v: &Value) -> Option<Entry> {
    let s = |k: &str| v.get(k).and_then(Value::as_str);
    let meter = match s("meter")? {
        "api_dollars" => Meter::ApiDollars,
        "plan_window" => Meter::PlanWindow,
        "tokens" => Meter::Tokens,
        _ => return None,
    };
    let status = match s("status")? {
        "reported" => Status::Reported,
        "estimated" => Status::Estimated,
        "reconciled" => Status::Reconciled,
        "unknown" => Status::Unknown,
        _ => return None,
    };
    let value = v.get("value")?.as_f64();
    // A figure with the status unknown, or a missing figure with another status, is not coherent.
    if value.is_none() != (status == Status::Unknown) {
        return None;
    }
    Some(Entry {
        step: s("step")?.to_string(),
        meter,
        value,
        status,
        detail: s("detail")?.to_string(),
    })
}

fn num(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

#[derive(Debug, Default)]
pub struct Ledger {
    entries: Vec<Entry>,
}

impl Ledger {
    /// Rebuilds the ledger from journal events. Other kinds are ignored, and malformed usage is an error.
    pub fn from_events(events: &[Event]) -> Result<Ledger, String> {
        let mut entries = Vec::new();
        for ev in events.iter().filter(|e| e.kind == "usage") {
            entries.push(
                parse(&ev.payload).ok_or_else(|| format!("malformed usage event {}", ev.id))?,
            );
        }
        Ok(Ledger { entries })
    }

    pub fn for_step(&self, step: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.step == step).collect()
    }

    /// Sums known values of an additive meter. For the plan window use [`Ledger::latest_windows`].
    pub fn aggregate(&self, meter: Meter) -> Aggregate {
        let of: Vec<&Entry> = self.entries.iter().filter(|e| e.meter == meter).collect();
        let known: Vec<f64> = of.iter().filter_map(|e| e.value).collect();
        Aggregate {
            total: (!known.is_empty()).then(|| known.iter().sum()),
            known: known.len(),
            unknown: of.len() - known.len(),
        }
    }

    /// The most recent reading per bucket. Windows are levels, so they are never summed.
    pub fn latest_windows(&self) -> BTreeMap<String, Option<f64>> {
        let mut out = BTreeMap::new();
        for e in self.entries.iter().filter(|e| e.meter == Meter::PlanWindow) {
            out.insert(e.detail.clone(), e.value);
        }
        out
    }

    /// One row per step, in the order the steps first appeared: tokens and plan window as display text.
    pub fn step_rows(&self) -> Vec<(String, String, String)> {
        let mut steps: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !steps.contains(&e.step.as_str()) {
                steps.push(&e.step);
            }
        }
        steps
            .into_iter()
            .map(|step| {
                let of = |m: Meter| {
                    self.entries
                        .iter()
                        .filter(move |e| e.step == step && e.meter == m)
                };
                let known: Vec<f64> = of(Meter::Tokens).filter_map(|e| e.value).collect();
                let tokens = if known.is_empty() {
                    "unknown".to_string()
                } else {
                    num(known.iter().sum())
                };
                let mut windows: BTreeMap<&str, Option<f64>> = BTreeMap::new();
                for e in of(Meter::PlanWindow) {
                    windows.insert(&e.detail, e.value);
                }
                let window = if windows.is_empty() {
                    "unknown".to_string()
                } else {
                    windows
                        .iter()
                        .map(|(k, v)| {
                            format!(
                                "{k} {}",
                                v.map_or("unknown".to_string(), |p| format!("{}%", num(p)))
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                (step.to_string(), tokens, window)
            })
            .collect()
    }

    /// What to show for a meter's aggregate: a figure, a figure with how much is unknown, or `unknown`.
    pub fn display(&self, meter: Meter) -> String {
        if meter == Meter::PlanWindow {
            let w = self.latest_windows();
            if w.is_empty() {
                return "unknown".into();
            }
            return w
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{k} {}",
                        v.map_or("unknown".to_string(), |p| format!("{}%", num(p)))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
        }
        let a = self.aggregate(meter);
        let value = |t: f64| {
            if meter != Meter::ApiDollars {
                return num(t);
            }
            if t > 0.0 && t < 0.000001 {
                return "<$0.000001".into();
            }
            if t == 0.0 {
                return "$0.00".into();
            }
            format!(
                "${}",
                format!("{t:.6}")
                    .trim_end_matches('0')
                    .trim_end_matches('.')
            )
        };
        match a.total {
            None => "unknown".into(),
            Some(t) if a.unknown == 0 => value(t),
            Some(t) => format!(
                "{} ({} {} unknown)",
                value(t),
                a.unknown,
                if a.unknown == 1 { "entry" } else { "entries" }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollar_totals_keep_micro_precision_without_float_noise_or_hidden_unknowns() {
        let l = ledger(&[
            e(
                "a",
                Meter::ApiDollars,
                Some(0.002764),
                Status::Reported,
                "api",
            ),
            e(
                "b",
                Meter::ApiDollars,
                Some(0.001617),
                Status::Reported,
                "api",
            ),
            e("a", Meter::ApiDollars, None, Status::Unknown, "api"),
        ]);
        assert_eq!(l.display(Meter::ApiDollars), "$0.004381 (1 entry unknown)");
        let tiny = ledger(&[e(
            "a",
            Meter::ApiDollars,
            Some(0.0000001),
            Status::Reported,
            "api",
        )]);
        assert_eq!(tiny.display(Meter::ApiDollars), "<$0.000001");
        assert_eq!(
            ledger(&[e(
                "a",
                Meter::ApiDollars,
                Some(0.0),
                Status::Reported,
                "api"
            )])
            .display(Meter::ApiDollars),
            "$0.00"
        );
    }

    fn e(step: &str, meter: Meter, value: Option<f64>, status: Status, detail: &str) -> Entry {
        Entry {
            step: step.into(),
            meter,
            value,
            status,
            detail: detail.into(),
        }
    }

    fn ledger(entries: &[Entry]) -> Ledger {
        let j = Journal::open_in_memory().unwrap();
        for (i, x) in entries.iter().enumerate() {
            record(&j, x, i as u64).unwrap();
        }
        j.append("plan.approved", &json!({}), 99).unwrap(); // other kinds are ignored
        Ledger::from_events(&j.events().unwrap()).unwrap()
    }

    #[test]
    fn entries_round_trip_through_the_journal() {
        let x = e(
            "s1",
            Meter::Tokens,
            Some(1234.0),
            Status::Reported,
            "gpt-6-luna",
        );
        let l = ledger(std::slice::from_ref(&x));
        assert_eq!(l.for_step("s1"), vec![&x]);
        assert!(l.for_step("nope").is_empty());
    }

    #[test]
    fn unknown_is_stored_as_unknown_and_never_becomes_zero() {
        let l = ledger(&[e("s1", Meter::Tokens, None, Status::Unknown, "m")]);
        assert_eq!(l.for_step("s1")[0].value, None);
        assert_eq!(
            l.aggregate(Meter::Tokens),
            Aggregate {
                total: None,
                known: 0,
                unknown: 1
            }
        );
        assert_eq!(l.display(Meter::Tokens), "unknown");
    }

    #[test]
    fn a_partly_unknown_total_says_so() {
        let l = ledger(&[
            e("s1", Meter::Tokens, Some(1000.0), Status::Reported, "m"),
            e("s2", Meter::Tokens, Some(234.0), Status::Estimated, "m"),
            e("s3", Meter::Tokens, None, Status::Unknown, "m"),
        ]);
        assert_eq!(
            l.aggregate(Meter::Tokens),
            Aggregate {
                total: Some(1234.0),
                known: 2,
                unknown: 1
            }
        );
        assert_eq!(l.display(Meter::Tokens), "1234 (1 entry unknown)");
    }

    #[test]
    fn meters_are_never_added_together() {
        let l = ledger(&[
            e("s1", Meter::Tokens, Some(100.0), Status::Reported, "m"),
            e("s1", Meter::ApiDollars, Some(0.5), Status::Estimated, "m"),
        ]);
        assert_eq!(l.aggregate(Meter::Tokens).total, Some(100.0));
        assert_eq!(l.aggregate(Meter::ApiDollars).total, Some(0.5));
    }

    #[test]
    fn plan_windows_keep_the_latest_level_per_bucket_and_are_not_summed() {
        let l = ledger(&[
            e(
                "s1",
                Meter::PlanWindow,
                Some(2.0),
                Status::Reported,
                "codex",
            ),
            e(
                "s2",
                Meter::PlanWindow,
                Some(3.0),
                Status::Reported,
                "codex",
            ),
            e(
                "s2",
                Meter::PlanWindow,
                Some(10.0),
                Status::Reported,
                "claude week",
            ),
            e(
                "s3",
                Meter::PlanWindow,
                None,
                Status::Unknown,
                "claude week",
            ),
        ]);
        let w = l.latest_windows();
        assert_eq!(w["codex"], Some(3.0));
        assert_eq!(
            w["claude week"], None,
            "a later unknown replaces the older figure"
        );
    }

    #[test]
    fn an_empty_ledger_shows_unknown_for_every_meter() {
        let l = Ledger::default();
        for m in [Meter::ApiDollars, Meter::PlanWindow, Meter::Tokens] {
            assert_eq!(l.display(m), "unknown", "{m:?}");
        }
    }

    #[test]
    fn each_step_shows_its_own_tokens_and_window_with_unknown_read_as_unknown() {
        let l = ledger(&[
            e(
                "plan",
                Meter::Tokens,
                Some(1200.0),
                Status::Reported,
                "gpt-6.1-terra",
            ),
            e(
                "plan",
                Meter::PlanWindow,
                Some(2.0),
                Status::Reported,
                "codex",
            ),
            e("t1", Meter::Tokens, None, Status::Unknown, "gpt-6.1-sol"),
            e(
                "t1",
                Meter::PlanWindow,
                Some(3.0),
                Status::Reported,
                "codex",
            ),
            e(
                "t1",
                Meter::Tokens,
                Some(500.0),
                Status::Reported,
                "gpt-6.1-sol",
            ),
            e("t2", Meter::ApiDollars, Some(0.5), Status::Estimated, "m"),
        ]);
        let rows = l.step_rows();
        assert_eq!(
            rows[0],
            (
                "plan".to_string(),
                "1200".to_string(),
                "codex 2%".to_string()
            )
        );
        assert_eq!(rows[1].0, "t1");
        assert_eq!(
            rows[1].1, "500",
            "known tokens for the step are summed and the unknown entry is not shown as zero"
        );
        assert_eq!(rows[1].2, "codex 3%");
        assert_eq!(
            rows[2],
            (
                "t2".to_string(),
                "unknown".to_string(),
                "unknown".to_string()
            )
        );
        assert_eq!(Ledger::default().step_rows(), vec![]);
    }

    #[test]
    fn a_step_whose_only_tokens_entry_is_unknown_reads_unknown() {
        let l = ledger(&[
            e("t1", Meter::Tokens, None, Status::Unknown, "m"),
            e("t1", Meter::PlanWindow, None, Status::Unknown, "codex"),
        ]);
        assert_eq!(
            l.step_rows(),
            vec![(
                "t1".to_string(),
                "unknown".to_string(),
                "codex unknown".to_string()
            )]
        );
    }

    #[test]
    fn malformed_usage_events_are_an_error_not_a_silent_skip() {
        let j = Journal::open_in_memory().unwrap();
        j.append("usage", &json!({"step": "s1"}), 1).unwrap();
        assert!(Ledger::from_events(&j.events().unwrap()).is_err());
    }
}
