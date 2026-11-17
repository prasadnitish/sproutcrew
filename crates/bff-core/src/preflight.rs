//! Checks before a run: the two CLIs are in the range this build was verified against, the plan windows have
//! room before the 50 percent stop, the credit balance has not moved, and the tools a deliverable needs are
//! there. A failed check blocks the run. A warning does not. Readings are passed in, so the checks are the
//! same whether they came from a live Mac or a test.

use crate::claude_usage::STOP_PERCENT;

/// The builds the plan was verified against. A newer version of the same major warns. Anything else fails.
pub const CLAUDE_VERIFIED: (u32, u32, u32) = (2, 1, 316);
pub const CODEX_VERIFIED: (u32, u32, u32) = (0, 166, 0);
/// Warn when a window is within this many points of the stop.
pub const LOW_HEADROOM_POINTS: f64 = 15.0;
pub const MIN_FREE_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Default, Clone)]
pub struct Reading {
    pub claude_version: Option<String>,
    pub codex_version: Option<String>,
    /// `(label, used percent)`, as read.
    pub claude_windows: Option<Vec<(String, f64)>>,
    pub codex_windows: Option<Vec<(String, f64)>>,
    pub codex_balance: Option<String>,
    pub codex_baseline_balance: Option<String>,
    pub typst: Option<String>,
    pub free_bytes: Option<u64>,
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    s.split_whitespace().find_map(|w| {
        let mut it = w.split('.');
        let (a, b, c) = (it.next()?, it.next()?, it.next()?);
        if it.next().is_some() {
            return None;
        }
        Some((a.parse().ok()?, b.parse().ok()?, c.parse().ok()?))
    })
}

fn version_check(name: &'static str, found: &Option<String>, verified: (u32, u32, u32)) -> Check {
    let want = format!("{}.{}.{}", verified.0, verified.1, verified.2);
    let (status, detail) = match found.as_deref().and_then(parse_version) {
        None => (
            Status::Fail,
            format!("the version could not be read. This build was verified against {want}"),
        ),
        Some(v) if v == verified => (Status::Pass, format!("{want}, as verified")),
        // Nitish's call on November 2, 2026 (docs/issues.md 89): the tools update themselves, so a newer version of
        // the same major runs with a warning. The event checks still stop tool use, hooks, and logins they do not
        // allow, but an event kind this build has never seen is kept as "other" and not stopped.
        Some(v) if v.0 == verified.0 && (v.1, v.2) > (verified.1, verified.2) => (
            Status::Warn,
            format!(
                "{}.{}.{} is newer than the verified {want}. Runs are allowed, and its new kinds of events are not checked",
                v.0, v.1, v.2
            ),
        ),
        Some(v) => (
            Status::Fail,
            format!(
                "{}.{}.{} is outside the range verified against {want}",
                v.0, v.1, v.2
            ),
        ),
    };
    Check {
        name,
        status,
        detail,
    }
}

fn windows_check(name: &'static str, windows: &Option<Vec<(String, f64)>>) -> Check {
    let Some(w) = windows.as_ref().filter(|w| !w.is_empty()) else {
        return Check {
            name,
            status: Status::Fail,
            detail: "the plan windows could not be read".into(),
        };
    };
    if let Some((label, pct)) = w.iter().find(|(_, p)| p.round() >= STOP_PERCENT) {
        return Check {
            name,
            status: Status::Fail,
            detail: format!("{label} is at {pct}%, at or over the {STOP_PERCENT}% stop"),
        };
    }
    if let Some((label, pct)) = w
        .iter()
        .find(|(_, p)| *p >= STOP_PERCENT - LOW_HEADROOM_POINTS)
    {
        return Check { name, status: Status::Warn, detail: format!("{label} is at {pct}%, within {LOW_HEADROOM_POINTS} points of the {STOP_PERCENT}% stop") };
    }
    let top = w.iter().map(|(_, p)| *p).fold(0.0, f64::max);
    Check {
        name,
        status: Status::Pass,
        detail: format!("highest window {top}%"),
    }
}

pub fn run(r: &Reading) -> Vec<Check> {
    let balance = match (
        r.codex_balance
            .as_deref()
            .and_then(|b| b.parse::<f64>().ok()),
        r.codex_baseline_balance.as_deref(),
    ) {
        (None, _) => Check {
            name: "codex credit balance",
            status: Status::Fail,
            detail: "the balance could not be read".into(),
        },
        (Some(_), None) => Check {
            name: "codex credit balance",
            status: Status::Pass,
            detail: "no earlier balance to compare".into(),
        },
        (Some(now), Some(base)) => match base.parse::<f64>() {
            Ok(b) if (b - now).abs() < 1e-9 => Check {
                name: "codex credit balance",
                status: Status::Pass,
                detail: format!("unchanged at {now}"),
            },
            _ => Check {
                name: "codex credit balance",
                status: Status::Fail,
                detail: format!("the balance changed from {base} to {now}"),
            },
        },
    };
    let typst = match &r.typst {
        Some(v) => Check {
            name: "typst",
            status: Status::Pass,
            detail: v.clone(),
        },
        None => Check {
            name: "typst",
            status: Status::Warn,
            detail: "Typst is not installed. PDF export is unavailable".into(),
        },
    };
    let disk = match r.free_bytes {
        None => Check {
            name: "free disk",
            status: Status::Warn,
            detail: "free space could not be read".into(),
        },
        Some(b) if b < 100 << 20 => Check {
            name: "free disk",
            status: Status::Fail,
            detail: format!("{} MB free", b >> 20),
        },
        Some(b) if b < MIN_FREE_BYTES => Check {
            name: "free disk",
            status: Status::Warn,
            detail: format!("{} MB free", b >> 20),
        },
        Some(b) => Check {
            name: "free disk",
            status: Status::Pass,
            detail: format!("{} GB free", b >> 30),
        },
    };
    vec![
        version_check("claude version", &r.claude_version, CLAUDE_VERIFIED),
        version_check("codex version", &r.codex_version, CODEX_VERIFIED),
        windows_check("claude plan windows", &r.claude_windows),
        windows_check("codex plan windows", &r.codex_windows),
        balance,
        typst,
        disk,
    ]
}

/// Fills the plan-window and balance fields from the engines' `status()` JSON. A status that is missing
/// (the engine refused or was unreachable) leaves its fields unread, which the checks then fail.
pub fn apply_status(
    r: &mut Reading,
    claude: Option<&serde_json::Value>,
    codex: Option<&serde_json::Value>,
) {
    let windows = |v: &serde_json::Value, label: &str| -> Vec<(String, f64)> {
        v["windows"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|w| {
                        Some((w[label].as_str()?.to_string(), w["used_percent"].as_f64()?))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(c) = claude {
        r.claude_windows = Some(windows(c, "window"));
    }
    if let Some(c) = codex {
        r.codex_windows = Some(windows(c, "bucket"));
        r.codex_balance = c["balance"].as_str().map(str::to_string);
    }
}

/// No failed check.
pub fn can_start(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.status != Status::Fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Reading {
        Reading {
            claude_version: Some("2.1.316".into()),
            codex_version: Some("0.166.0".into()),
            claude_windows: Some(vec![
                ("session".into(), 10.0),
                ("week (all models)".into(), 20.0),
            ]),
            codex_windows: Some(vec![("codex".into(), 2.0)]),
            codex_balance: Some("1250".into()),
            codex_baseline_balance: Some("1250".into()),
            typst: Some("typst 0.13.1".into()),
            free_bytes: Some(50 << 30),
        }
    }

    fn of<'a>(c: &'a [Check], name: &str) -> &'a Check {
        c.iter()
            .find(|x| x.name == name)
            .unwrap_or_else(|| panic!("no check {name}: {c:?}"))
    }

    #[test]
    fn a_good_reading_passes_everything() {
        let c = run(&good());
        assert!(c.iter().all(|x| x.status == Status::Pass), "{c:?}");
        assert!(can_start(&c));
    }

    #[test]
    fn versions_parse_from_cli_output() {
        assert_eq!(parse_version("2.1.316 (Claude Code)"), Some((2, 1, 316)));
        assert_eq!(parse_version("codex-cli 0.166.0"), Some((0, 166, 0)));
        assert_eq!(parse_version("2.1"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn a_newer_cli_version_of_the_same_major_warns_and_anything_else_fails() {
        let mut r = good();
        r.claude_version = Some("2.2.0".into());
        let c = run(&r);
        assert_eq!(of(&c, "claude version").status, Status::Warn, "newer minor");
        assert!(of(&c, "claude version").detail.contains("2.1.316"));
        assert!(can_start(&c), "a warning does not block runs");
        r.claude_version = Some("3.0.0".into());
        let c = run(&r);
        assert_eq!(of(&c, "claude version").status, Status::Fail, "newer major");
        assert!(!can_start(&c));
        r.claude_version = Some("1.9.0".into());
        assert_eq!(
            of(&run(&r), "claude version").status,
            Status::Fail,
            "older major"
        );
        r.claude_version = Some("2.1.320".into());
        assert_eq!(
            of(&run(&r), "claude version").status,
            Status::Warn,
            "newer patch of the same minor"
        );
        r.claude_version = Some("2.1.300".into());
        assert_eq!(
            of(&run(&r), "claude version").status,
            Status::Fail,
            "older patch is not what was verified"
        );
        let mut r = good();
        r.codex_version = Some("0.167.0".into());
        assert_eq!(of(&run(&r), "codex version").status, Status::Warn);
        r.codex_version = Some("0.165.9".into());
        assert_eq!(of(&run(&r), "codex version").status, Status::Fail);
        r.codex_version = None;
        assert_eq!(
            of(&run(&r), "codex version").status,
            Status::Fail,
            "unreadable counts as failed"
        );
    }

    #[test]
    fn low_headroom_warns_and_the_stop_threshold_fails() {
        let mut r = good();
        r.claude_windows = Some(vec![
            ("session".into(), 36.0),
            ("week (all models)".into(), 20.0),
        ]);
        let c = run(&r);
        assert_eq!(of(&c, "claude plan windows").status, Status::Warn);
        assert!(of(&c, "claude plan windows").detail.contains("session"));
        assert!(can_start(&c), "a warning does not block");
        r.claude_windows = Some(vec![
            ("session".into(), 50.0),
            ("week (all models)".into(), 20.0),
        ]);
        assert_eq!(of(&run(&r), "claude plan windows").status, Status::Fail);
        r.claude_windows = Some(vec![("session".into(), 34.0)]);
        assert_eq!(
            of(&run(&r), "claude plan windows").status,
            Status::Pass,
            "15 points of room is enough"
        );
        r.claude_windows = None;
        assert_eq!(of(&run(&r), "claude plan windows").status, Status::Fail);
        let mut r = good();
        r.codex_windows = Some(vec![("codex".into(), 45.0)]);
        assert_eq!(of(&run(&r), "codex plan windows").status, Status::Warn);
        r.codex_windows = Some(vec![("codex".into(), 51.0)]);
        assert_eq!(of(&run(&r), "codex plan windows").status, Status::Fail);
        r.codex_windows = Some(vec![]);
        assert_eq!(
            of(&run(&r), "codex plan windows").status,
            Status::Fail,
            "no window at all is unreadable"
        );
    }

    #[test]
    fn a_moved_credit_balance_fails() {
        let mut r = good();
        r.codex_balance = Some("1249.5".into());
        let c = run(&r);
        assert_eq!(of(&c, "codex credit balance").status, Status::Fail);
        assert!(of(&c, "codex credit balance").detail.contains("changed"));
        r.codex_balance = None;
        assert_eq!(of(&run(&r), "codex credit balance").status, Status::Fail);
        r.codex_balance = Some("1250.0000000000".into());
        assert_eq!(
            of(&run(&r), "codex credit balance").status,
            Status::Pass,
            "same number, different spelling"
        );
    }

    #[test]
    fn missing_typst_and_low_disk_warn_or_fail_by_how_much_they_matter() {
        let mut r = good();
        r.typst = None;
        let c = run(&r);
        assert_eq!(of(&c, "typst").status, Status::Warn);
        assert!(of(&c, "typst").detail.contains("PDF"));
        r.free_bytes = Some(500 << 20);
        assert_eq!(of(&run(&r), "free disk").status, Status::Warn);
        r.free_bytes = Some(50 << 20);
        assert_eq!(of(&run(&r), "free disk").status, Status::Fail);
        r.free_bytes = None;
        assert_eq!(
            of(&run(&r), "free disk").status,
            Status::Warn,
            "unknown disk is a warning"
        );
    }

    #[test]
    fn engine_status_json_fills_the_readings_and_a_missing_status_leaves_them_unread() {
        let claude = serde_json::json!({"login": "claude.ai", "windows": [
            {"window": "session", "used_percent": 25.0}, {"window": "week (all models)", "used_percent": 22.0}]});
        let codex = serde_json::json!({"balance": "1250", "windows": [{"bucket": "codex", "used_percent": 2, "window_mins": 10080}]});
        let mut r = Reading::default();
        apply_status(&mut r, Some(&claude), Some(&codex));
        assert_eq!(
            r.claude_windows,
            Some(vec![
                ("session".to_string(), 25.0),
                ("week (all models)".to_string(), 22.0)
            ])
        );
        assert_eq!(r.codex_windows, Some(vec![("codex".to_string(), 2.0)]));
        assert_eq!(r.codex_balance.as_deref(), Some("1250"));
        let mut r = Reading::default();
        apply_status(&mut r, None, None);
        assert_eq!(
            (r.claude_windows, r.codex_windows, r.codex_balance),
            (None, None, None)
        );
        let unreadable =
            serde_json::json!({"windows": [{"bucket": "codex", "used_percent": null}]});
        let mut r = Reading::default();
        apply_status(&mut r, None, Some(&unreadable));
        assert_eq!(
            r.codex_windows,
            Some(vec![]),
            "an unreadable percent is dropped, so the check fails on an empty list"
        );
    }
}
