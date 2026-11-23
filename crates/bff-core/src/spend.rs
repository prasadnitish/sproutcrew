//! The dollar cap on API spending, kept across runs and projects. Every API request reserves its worst case
//! here before it is sent, so spending can never pass the cap by more than nothing, and a request that does
//! not fit is refused unsent. The cap rises only when a person approves a higher one. Usage that was never
//! reported is charged at the reserved amount, and a request that was sent but never settled (a crash) is
//! charged at its reservation the next time the book is opened.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Five dollars, in micro-dollars.
pub const DEFAULT_CAP_MICRO: u64 = 5_000_000;
/// The starting cap when tokens are counted instead of dollars.
pub const DEFAULT_TOKEN_CAP: u64 = 500_000;

/// What a book counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Micro-dollars.
    Dollars,
    Tokens,
}

impl Unit {
    pub fn default_cap(self) -> u64 {
        match self {
            Unit::Dollars => DEFAULT_CAP_MICRO,
            Unit::Tokens => DEFAULT_TOKEN_CAP,
        }
    }

    /// An amount in this unit, in words.
    pub fn show(self, n: u64) -> String {
        match self {
            Unit::Dollars => usd(n),
            Unit::Tokens => {
                let digits = n.to_string();
                let mut out = String::new();
                for (i, c) in digits.chars().enumerate() {
                    if i > 0 && (digits.len() - i).is_multiple_of(3) {
                        out.push(',');
                    }
                    out.push(c);
                }
                format!("{out} {}", if n == 1 { "token" } else { "tokens" })
            }
        }
    }
}

pub fn usd(micro: u64) -> String {
    format!("${:.2}", micro as f64 / 1_000_000.0)
}

/// The file in a project's folder that keeps its own dollar limit and what it has spent.
pub const PROJECT_FILE: &str = "spend.json";

static PROJECT_BOOKS: std::sync::LazyLock<
    std::sync::Mutex<BTreeMap<PathBuf, std::sync::Arc<std::sync::Mutex<SpendBook>>>>,
> = std::sync::LazyLock::new(Default::default);

/// The one dollar book for the project at `root`, shared by every step running in it. A record that cannot be
/// read locks the project's spending until it is fixed.
pub fn project_book(root: &Path) -> std::sync::Arc<std::sync::Mutex<SpendBook>> {
    let mut all = PROJECT_BOOKS.lock().expect("project books");
    all.entry(root.to_path_buf())
        .or_insert_with(|| {
            let book = SpendBook::open_unit(&root.join(PROJECT_FILE), Unit::Dollars)
                .unwrap_or_else(|why| SpendBook::unreadable(Unit::Dollars, &why));
            std::sync::Arc::new(std::sync::Mutex::new(book))
        })
        .clone()
}

/// A key no other hold in this run of the app has used.
pub fn hold_key(step: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "{step}-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub cap: u64,
    pub spent: u64,
    pub held: u64,
    pub available: u64,
    pub unknown_charges: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    cap_micro: Option<u64>,
    spent_micro: u64,
    unknown_charges: u32,
    held: BTreeMap<String, u64>,
}

pub struct SpendBook {
    unit: Unit,
    /// Set when the saved record could not be read: nothing is spent and the cap cannot be raised.
    locked: Option<String>,
    path: Option<PathBuf>,
    cap: u64,
    spent: u64,
    unknown_charges: u32,
    held: BTreeMap<String, u64>,
}

impl SpendBook {
    /// A book that is not saved, for tests.
    pub fn memory(cap: u64) -> SpendBook {
        SpendBook::memory_unit(cap, Unit::Dollars)
    }

    pub fn memory_unit(cap: u64, unit: Unit) -> SpendBook {
        SpendBook {
            unit,
            locked: None,
            path: None,
            cap,
            spent: 0,
            unknown_charges: 0,
            held: BTreeMap::new(),
        }
    }

    /// A book for a saved record that could not be read. The history that would justify any spending is gone, so
    /// nothing may be spent until the person fixes or moves the file. The file itself is left untouched.
    pub fn unreadable(unit: Unit, why: &str) -> SpendBook {
        SpendBook {
            locked: Some(why.to_string()),
            ..SpendBook::memory_unit(0, unit)
        }
    }

    /// Why spending is locked, if it is.
    pub fn locked(&self) -> Option<&str> {
        self.locked.as_deref()
    }

    fn lock_message(why: &str) -> String {
        format!("stopped: the saved spending record could not be read ({why}). Nothing is spent until it is fixed or moved aside, then SproutCrew is reopened")
    }

    /// Opens the book at `path`, or starts one at the default cap. Anything still held was sent and never
    /// settled, so it counts as spent.
    pub fn open(path: &Path) -> Result<SpendBook, String> {
        SpendBook::open_unit(path, Unit::Dollars)
    }

    pub fn open_unit(path: &Path, unit: Unit) -> Result<SpendBook, String> {
        let saved: Saved = match std::fs::read_to_string(path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let lost: u64 = saved.held.values().sum();
        let book = SpendBook {
            unit,
            locked: None,
            path: Some(path.to_path_buf()),
            cap: saved.cap_micro.unwrap_or(unit.default_cap()),
            spent: saved.spent_micro + lost,
            unknown_charges: saved.unknown_charges + saved.held.len() as u32,
            held: BTreeMap::new(),
        };
        if lost > 0 {
            book.save()?;
        }
        Ok(book)
    }

    pub fn unit(&self) -> Unit {
        self.unit
    }

    pub fn status(&self) -> Status {
        let held: u64 = self.held.values().sum();
        Status {
            cap: self.cap,
            spent: self.spent,
            held,
            available: self.cap.saturating_sub(self.spent + held),
            unknown_charges: self.unknown_charges,
        }
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let saved = Saved {
            cap_micro: Some(self.cap),
            spent_micro: self.spent,
            unknown_charges: self.unknown_charges,
            held: self.held.clone(),
        };
        let json = serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Holds `wanted` against the cap for the request `key`. Refused, in words, when it does not fit.
    pub fn reserve(&mut self, key: &str, wanted: u64) -> Result<(), String> {
        if let Some(why) = &self.locked {
            return Err(SpendBook::lock_message(why));
        }
        if self.held.contains_key(key) {
            return Err(format!("{key} already holds money"));
        }
        let s = self.status();
        if wanted > s.available {
            let u = self.unit;
            return Err(match u {
                Unit::Dollars => format!(
                    "stopped: the API spending cap of {} is reached ({} spent, {} held, and this request could cost up to {}). Approve a higher cap in Usage to continue",
                    u.show(s.cap),
                    u.show(s.spent),
                    u.show(s.held),
                    u.show(wanted)
                ),
                Unit::Tokens => format!(
                    "stopped: the API token cap of {} is reached ({} used, {} held, and this request could use up to {}). Approve a higher cap in Usage to continue",
                    u.show(s.cap),
                    u.show(s.spent),
                    u.show(s.held),
                    u.show(wanted)
                ),
            });
        }
        self.held.insert(key.to_string(), wanted);
        self.save()
    }

    /// [`SpendBook::reserve`] for a project's own dollar limit, refused in plain words. The refusal starts with
    /// `paused:`, so it holds back only this project and is not a safety stop.
    pub fn reserve_for_project(&mut self, key: &str, wanted: u64) -> Result<(), String> {
        if self.locked.is_some() {
            return self.reserve(key, wanted);
        }
        let s = self.status();
        if wanted > s.available {
            return Err(format!(
                "paused: this project has reached its spending limit of {} ({} spent, and this step could cost up to {}). Raise the limit in Usage & limits to go on",
                usd(s.cap),
                usd(s.spent + s.held),
                usd(wanted)
            ));
        }
        self.reserve(key, wanted)
    }

    /// Frees a hold without charging, for a request that was never sent or was refused.
    pub fn release(&mut self, key: &str) {
        if self.held.remove(key).is_some() {
            let _ = self.save();
        }
    }

    /// Ends a hold with what the request cost. `None` means the cost is unknown and the hold is the charge.
    pub fn settle(&mut self, key: &str, cost: Option<u64>) {
        let Some(held) = self.held.remove(key) else {
            return;
        };
        match cost {
            Some(c) => self.spent += c,
            None => {
                self.spent += held;
                self.unknown_charges += 1;
            }
        }
        let _ = self.save();
    }

    /// Sets a higher cap. Raising is the approval; there is no way to lower it below what is spent or held.
    pub fn raise_cap(&mut self, new_cap: u64) -> Result<(), String> {
        if let Some(why) = &self.locked {
            return Err(SpendBook::lock_message(why));
        }
        if new_cap <= self.cap {
            return Err(format!(
                "the new cap {} is not higher than the current cap {}",
                self.unit.show(new_cap),
                self.unit.show(self.cap)
            ));
        }
        self.cap = new_cap;
        self.save()
    }

    /// Sets the cap to any amount at or above what is spent and held. Used for a project's own dollar limit, which
    /// can go down as well as up. A request already held still settles: its hold is part of the floor.
    pub fn set_cap(&mut self, new_cap: u64) -> Result<(), String> {
        if let Some(why) = &self.locked {
            return Err(SpendBook::lock_message(why));
        }
        if new_cap == 0 {
            return Err("The limit has to be more than nothing.".into());
        }
        let floor = self.spent + self.held.values().sum::<u64>();
        if new_cap < floor {
            // Round up to whole cents so the amount shown is never below the real floor.
            let cents = floor.div_ceil(10_000);
            let shown = if cents >= 100 {
                usd(floor)
            } else {
                format!("{cents}¢")
            };
            return Err(format!(
                "You've already spent {shown} on this project, so the limit can't go below that."
            ));
        }
        self.cap = new_cap;
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-spend-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("spend.json")
    }

    #[test]
    fn a_new_book_has_a_five_dollar_cap_and_nothing_spent() {
        let s = SpendBook::open(&tmp("new")).unwrap().status();
        assert_eq!(
            (s.cap, s.spent, s.held, s.available),
            (5_000_000, 0, 0, 5_000_000)
        );
        assert_eq!(usd(4_800_000), "$4.80");
        assert_eq!(usd(5_000_000), "$5.00");
    }

    #[test]
    fn a_request_that_does_not_fit_under_the_cap_is_refused_in_words_and_holds_count() {
        let mut b = SpendBook::memory(1_000_000);
        b.reserve("a", 600_000).unwrap();
        // Held money counts: only 400,000 is left, so a second 500,000 is refused even though nothing is spent yet.
        let e = b.reserve("b", 500_000).unwrap_err();
        assert!(e.starts_with("stopped:"), "{e}");
        assert!(
            e.contains("$1.00")
                && e.contains("$0.60")
                && e.contains("$0.50")
                && e.contains("Approve"),
            "{e}"
        );
        b.reserve("b", 400_000).unwrap();
        assert_eq!(b.status().available, 0);
        assert!(b.reserve("a", 1).is_err(), "one key cannot hold twice");
    }

    #[test]
    fn settling_charges_the_real_cost_and_an_unknown_cost_is_charged_at_the_hold() {
        let mut b = SpendBook::memory(1_000_000);
        b.reserve("a", 300_000).unwrap();
        b.settle("a", Some(120_000));
        assert_eq!(
            (b.status().spent, b.status().held, b.status().available),
            (120_000, 0, 880_000)
        );
        b.reserve("b", 300_000).unwrap();
        b.settle("b", None);
        let s = b.status();
        assert_eq!((s.spent, s.unknown_charges), (420_000, 1));
        b.reserve("c", 300_000).unwrap();
        b.release("c");
        assert_eq!(b.status().spent, 420_000);
        assert_eq!(b.status().held, 0);
        b.settle("never-held", Some(5));
        assert_eq!(
            b.status().spent,
            420_000,
            "settling a key that holds nothing charges nothing"
        );
    }

    #[test]
    fn spending_is_remembered_and_a_request_that_was_never_settled_is_charged_when_reopened() {
        let p = tmp("saved");
        {
            let mut b = SpendBook::open(&p).unwrap();
            b.reserve("a", 200_000).unwrap();
            b.settle("a", Some(150_000));
            b.reserve("crash", 400_000).unwrap();
            // The program stops here, with a request sent and not settled.
        }
        let b = SpendBook::open(&p).unwrap();
        let s = b.status();
        assert_eq!((s.spent, s.held, s.unknown_charges), (550_000, 0, 1));
        assert_eq!(
            SpendBook::open(&p).unwrap().status().spent,
            550_000,
            "reopening again does not charge twice"
        );
    }

    #[test]
    fn raising_the_cap_is_the_approval_and_it_only_goes_up_and_survives_a_restart() {
        let p = tmp("raise");
        let mut b = SpendBook::open(&p).unwrap();
        b.reserve("a", 5_000_000).unwrap();
        b.settle("a", Some(4_900_000));
        assert!(b.reserve("b", 200_000).is_err());
        assert!(
            b.raise_cap(5_000_000).is_err(),
            "the same cap is not a raise"
        );
        b.raise_cap(10_000_000).unwrap();
        b.reserve("b", 200_000).unwrap();
        assert_eq!(SpendBook::open(&p).unwrap().status().cap, 10_000_000);
    }

    /// A book with 20,000 spent and 30,000 held, under a $5 cap.
    fn book_with_spent_and_held() -> SpendBook {
        let mut b = SpendBook::memory(5_000_000);
        b.reserve("a", 20_000).unwrap();
        b.settle("a", Some(20_000));
        b.reserve("b", 30_000).unwrap();
        b
    }

    #[test]
    fn a_lower_cap_above_what_is_spent_and_held_is_accepted() {
        let mut b = book_with_spent_and_held();
        b.set_cap(50_000).unwrap();
        assert_eq!(b.status().cap, 50_000);
    }

    #[test]
    fn a_cap_below_spent_plus_held_is_refused_in_plain_words() {
        let mut b = book_with_spent_and_held();
        assert_eq!(
            b.set_cap(49_999).unwrap_err(),
            "You've already spent 5¢ on this project, so the limit can't go below that."
        );
        assert_eq!(
            b.status().cap,
            5_000_000,
            "a refused change leaves the cap alone"
        );
        assert!(b.set_cap(0).is_err(), "zero is not a limit");
    }

    #[test]
    fn a_held_request_still_settles_after_the_cap_is_lowered() {
        let mut b = book_with_spent_and_held();
        b.set_cap(50_000).unwrap();
        b.settle("b", Some(30_000));
        let s = b.status();
        assert_eq!(
            (s.cap, s.spent, s.held, s.available),
            (50_000, 50_000, 0, 0)
        );
    }

    #[test]
    fn set_cap_moves_the_limit_either_way_and_survives_a_restart() {
        let p = tmp("set-cap");
        let mut b = SpendBook::open(&p).unwrap();
        b.set_cap(2_000_000).unwrap();
        assert_eq!(SpendBook::open(&p).unwrap().status().cap, 2_000_000);
        b.set_cap(8_000_000).unwrap();
        assert_eq!(SpendBook::open(&p).unwrap().status().cap, 8_000_000);
    }

    #[test]
    fn a_token_book_counts_tokens_has_its_own_default_cap_and_says_so_in_its_refusals() {
        let p = tmp("tokens");
        let mut b = SpendBook::open_unit(&p, Unit::Tokens).unwrap();
        let s = b.status();
        assert_eq!((s.cap, s.spent), (500_000, 0));
        assert_eq!(b.unit(), Unit::Tokens);
        assert_eq!(Unit::Tokens.show(1_234_567), "1,234,567 tokens");
        assert_eq!(Unit::Tokens.show(1), "1 token");
        assert_eq!(Unit::Dollars.show(1_500_000), "$1.50");
        b.reserve("a", 480_000).unwrap();
        b.settle("a", Some(470_000));
        let e = b.reserve("b", 40_000).unwrap_err();
        assert!(e.starts_with("stopped:"), "{e}");
        assert!(
            e.contains("API token cap of 500,000 tokens")
                && e.contains("470,000 tokens used")
                && e.contains("up to 40,000 tokens")
                && e.contains("Approve a higher cap"),
            "{e}"
        );
        assert!(!e.contains('$'), "{e}");
        // The book is saved as tokens, and a dollar book at the same path is not confused with it.
        assert_eq!(
            SpendBook::open_unit(&p, Unit::Tokens)
                .unwrap()
                .status()
                .spent,
            470_000
        );
        assert_eq!(
            SpendBook::open_unit(&tmp("tokens-other"), Unit::Dollars)
                .unwrap()
                .status()
                .cap,
            5_000_000
        );
        b.raise_cap(1_000_000).unwrap();
        b.reserve("b", 40_000).unwrap();
    }

    #[test]
    fn a_book_that_cannot_be_read_spends_nothing_and_cannot_be_raised() {
        let mut b = SpendBook::unreadable(Unit::Tokens, "spend.json: bad JSON");
        assert!(b.locked().is_some());
        let e = b.reserve("t1", 1).unwrap_err();
        assert!(
            e.contains("could not be read") && e.contains("bad JSON"),
            "{e}"
        );
        assert!(b
            .raise_cap(1_000_000)
            .unwrap_err()
            .contains("could not be read"));
        assert_eq!(b.status().available, 0);
    }

    #[test]
    fn a_locked_book_refuses_set_cap() {
        let mut b = SpendBook::unreadable(Unit::Dollars, "spend.json: bad JSON");
        assert!(b
            .set_cap(1_000_000)
            .unwrap_err()
            .contains("could not be read"));
    }
}
