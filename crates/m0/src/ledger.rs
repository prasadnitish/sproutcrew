//! Which approved runs have been used. A started run counts as used even if the harness dies,
//! and a failed or skipped run is never retried or replaced.

use crate::runs::{find, Run, RUNS};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Started,
    Done,
    Failed,
    Skipped,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub status: Status,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotApproved(String),
    Stopped(String),
    PinsNotVerified,
    AlreadyUsed(String, Status),
    OutOfOrder {
        wanted: String,
        expected: &'static str,
    },
    NothingToClear,
    NoAuthorization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopRecord {
    pub reason: String,
    pub cleared_at: u64,
    pub authorization: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    entries: Vec<Entry>,
    stop: Option<String>,
    #[serde(default)]
    stop_history: Vec<StopRecord>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(path: &Path) -> io::Result<Ledger> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Ledger::new()),
            Err(e) => Err(e),
        }
    }

    /// Writes through a temp file so a crash never leaves half a ledger.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
        std::fs::rename(tmp, path)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn stop_reason(&self) -> Option<&str> {
        self.stop.as_deref()
    }

    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The only run that may go next.
    pub fn next_id(&self) -> Option<&'static str> {
        RUNS.iter()
            .map(|r| r.id)
            .find(|id| self.entry(id).is_none())
    }

    /// Every dispatch passes through here. Order of checks matters: an unknown ID is refused first.
    pub fn gate(&self, id: &str, pins_ok: bool) -> Result<&'static Run, Refusal> {
        let run = find(id).ok_or_else(|| Refusal::NotApproved(id.to_string()))?;
        if let Some(reason) = &self.stop {
            return Err(Refusal::Stopped(reason.clone()));
        }
        if !pins_ok {
            return Err(Refusal::PinsNotVerified);
        }
        if let Some(e) = self.entry(id) {
            return Err(Refusal::AlreadyUsed(id.to_string(), e.status));
        }
        match self.next_id() {
            Some(next) if next == id => Ok(run),
            Some(next) => Err(Refusal::OutOfOrder {
                wanted: id.to_string(),
                expected: next,
            }),
            None => Err(Refusal::AlreadyUsed(id.to_string(), Status::Done)),
        }
    }

    pub fn begin(&mut self, id: &str, now: u64) {
        if self.entry(id).is_none() {
            self.entries.push(Entry {
                id: id.to_string(),
                status: Status::Started,
                started_at: Some(now),
                ended_at: None,
                note: String::new(),
            });
        }
    }

    pub fn finish(&mut self, id: &str, status: Status, note: &str, now: u64) {
        self.begin(id, now);
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.status = status;
            e.ended_at = Some(now);
            e.note = note.to_string();
        }
    }

    pub fn skip(&mut self, id: &str, reason: &str, now: u64) -> Result<(), Refusal> {
        find(id).ok_or_else(|| Refusal::NotApproved(id.to_string()))?;
        if let Some(e) = self.entry(id) {
            return Err(Refusal::AlreadyUsed(id.to_string(), e.status));
        }
        self.entries.push(Entry {
            id: id.to_string(),
            status: Status::Skipped,
            started_at: None,
            ended_at: Some(now),
            note: reason.to_string(),
        });
        Ok(())
    }

    /// Clears a latched stop on Nitish's say-so. The authorization text is required and kept with the
    /// reason it cleared, and a stop that was never latched cannot be "cleared".
    pub fn clear_stop(&mut self, authorization: &str, now: u64) -> Result<String, Refusal> {
        let reason = self.stop.clone().ok_or(Refusal::NothingToClear)?;
        if authorization.trim().is_empty() {
            return Err(Refusal::NoAuthorization);
        }
        self.stop_history.push(StopRecord {
            reason: reason.clone(),
            cleared_at: now,
            authorization: authorization.trim().to_string(),
        });
        self.stop = None;
        Ok(reason)
    }

    pub fn stop_history(&self) -> &[StopRecord] {
        &self.stop_history
    }

    /// Latches. Nothing dispatches after this. The first reason wins.
    pub fn latch_stop(&mut self, reason: &str) {
        self.stop.get_or_insert_with(|| reason.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_in_order(l: &mut Ledger, upto: &str) {
        for r in RUNS.iter() {
            l.gate(r.id, true).unwrap();
            l.begin(r.id, 1);
            l.finish(r.id, Status::Done, "", 2);
            if r.id == upto {
                break;
            }
        }
    }

    #[test]
    fn first_run_is_a1_and_needs_verified_pins() {
        let l = Ledger::new();
        assert_eq!(l.next_id(), Some("A1"));
        assert_eq!(l.gate("A1", true).unwrap().id, "A1");
        assert_eq!(l.gate("A1", false).unwrap_err(), Refusal::PinsNotVerified);
    }

    #[test]
    fn ids_outside_the_approved_list_are_refused_before_anything_else() {
        let mut l = Ledger::new();
        l.latch_stop("x");
        assert_eq!(
            l.gate("Z9", false).unwrap_err(),
            Refusal::NotApproved("Z9".into())
        );
        assert_eq!(
            l.gate("", true).unwrap_err(),
            Refusal::NotApproved("".into())
        );
    }

    #[test]
    fn runs_must_go_in_order() {
        let l = Ledger::new();
        assert_eq!(
            l.gate("B1", true).unwrap_err(),
            Refusal::OutOfOrder {
                wanted: "B1".into(),
                expected: "A1"
            }
        );
    }

    #[test]
    fn a_used_run_never_runs_again_whatever_its_outcome() {
        for status in [
            Status::Done,
            Status::Failed,
            Status::Stopped,
            Status::Skipped,
        ] {
            let mut l = Ledger::new();
            l.begin("A1", 1);
            l.finish("A1", status, "", 2);
            assert_eq!(
                l.gate("A1", true).unwrap_err(),
                Refusal::AlreadyUsed("A1".into(), status)
            );
            assert_eq!(l.next_id(), Some("A2"));
        }
    }

    #[test]
    fn a_started_run_without_a_finish_counts_as_used() {
        let mut l = Ledger::new();
        l.begin("A1", 1);
        assert_eq!(
            l.gate("A1", true).unwrap_err(),
            Refusal::AlreadyUsed("A1".into(), Status::Started)
        );
        assert_eq!(l.gate("A2", true).unwrap().id, "A2");
    }

    #[test]
    fn skipping_advances_the_order_and_is_not_a_retry() {
        let mut l = Ledger::new();
        l.skip("A1", "headroom", 1).unwrap();
        assert_eq!(l.next_id(), Some("A2"));
        assert_eq!(
            l.gate("A1", true).unwrap_err(),
            Refusal::AlreadyUsed("A1".into(), Status::Skipped)
        );
        assert!(l.skip("Q1", "x", 1).is_err());
        assert!(l.skip("A1", "again", 1).is_err());
    }

    #[test]
    fn the_stop_latch_blocks_everything_and_survives_a_reload() {
        let mut l = Ledger::new();
        l.latch_stop("plan window at 50%");
        assert_eq!(
            l.gate("A1", true).unwrap_err(),
            Refusal::Stopped("plan window at 50%".into())
        );
        let path = std::env::temp_dir().join(format!("bff-ledger-{}.json", std::process::id()));
        l.save(&path).unwrap();
        let back = Ledger::load(&path).unwrap();
        assert_eq!(back.stop_reason(), Some("plan window at 50%"));
        assert!(back.gate("A1", true).is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn clearing_a_stop_needs_authorization_and_keeps_the_history() {
        let mut l = Ledger::new();
        assert!(l.clear_stop("go ahead", 9).is_err(), "nothing is latched");
        l.latch_stop("tool outside allowlist");
        assert!(
            l.clear_stop("   ", 9).is_err(),
            "blank authorization is refused"
        );
        assert_eq!(l.stop_reason(), Some("tool outside allowlist"));
        assert_eq!(
            l.clear_stop("Nitish: allow StructuredOutput and continue", 9)
                .unwrap(),
            "tool outside allowlist"
        );
        assert_eq!(l.stop_reason(), None);
        assert!(l.gate("A1", true).is_ok());
        assert_eq!(
            l.stop_history(),
            [StopRecord {
                reason: "tool outside allowlist".into(),
                cleared_at: 9,
                authorization: "Nitish: allow StructuredOutput and continue".into()
            }]
        );
    }

    #[test]
    fn a_cleared_stop_can_latch_again_and_history_survives_a_reload() {
        let mut l = Ledger::new();
        l.latch_stop("one");
        l.clear_stop("ok", 1).unwrap();
        l.latch_stop("two");
        assert_eq!(l.stop_reason(), Some("two"));
        let p = std::env::temp_dir().join(format!("bff-ledger-hist-{}.json", std::process::id()));
        l.save(&p).unwrap();
        let back = Ledger::load(&p).unwrap();
        assert_eq!(back.stop_history().len(), 1);
        assert_eq!(back.stop_reason(), Some("two"));
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn first_stop_reason_wins() {
        let mut l = Ledger::new();
        l.latch_stop("first");
        l.latch_stop("second");
        assert_eq!(l.stop_reason(), Some("first"));
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let l = Ledger::load(Path::new("/nonexistent/bff/ledger.json")).unwrap();
        assert!(l.entries().is_empty());
    }

    #[test]
    fn all_twenty_can_be_used_once_and_then_nothing_remains() {
        let mut l = Ledger::new();
        run_in_order(&mut l, "D3");
        assert_eq!(l.entries().len(), 20);
        assert_eq!(l.next_id(), None);
        assert!(matches!(l.gate("A1", true), Err(Refusal::AlreadyUsed(..))));
        assert!(find("D3").is_some());
    }

    #[test]
    fn round_trips_through_json() {
        let mut l = Ledger::new();
        l.begin("A1", 5);
        l.finish("A1", Status::Failed, "init mismatch", 9);
        let path = std::env::temp_dir().join(format!("bff-ledger-rt-{}.json", std::process::id()));
        l.save(&path).unwrap();
        let back = Ledger::load(&path).unwrap();
        assert_eq!(back.entries(), l.entries());
        std::fs::remove_file(&path).unwrap();
    }
}
