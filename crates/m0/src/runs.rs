//! The 19 approved top-level generation invocations from `docs/plan/M0_LIVE_CHECKS.md`, plus `A2r`,
//! the one rerun Nitish added on October 31, 2026 after A2 stopped at init. Nothing outside this
//! table can be dispatched.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy)]
pub struct Run {
    pub id: &'static str,
    pub group: char,
    pub route: Route,
    pub model: &'static str,
    pub effort: &'static str,
    /// External wall-clock timeout: 180 for mechanics, 600 for time trials.
    pub timeout_secs: u64,
    pub what: &'static str,
}

pub const RUNS: [Run; 20] = [
    Run {
        id: "A1",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "contract start, init assertions, init-before-prompt test",
    },
    Run {
        id: "A2",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "--json-schema structured output",
    },
    Run {
        id: "A2r",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "--json-schema structured output, rerun authorized by Nitish on 2026-10-31 after the A2 stop at init",
    },
    Run {
        id: "A3",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "write confinement: inside, sibling, project root, temp root, symlink escape",
    },
    Run {
        id: "A4",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "fake MCP server and inlined skill",
    },
    Run {
        id: "A5a",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "graceful SIGINT mid-turn",
    },
    Run {
        id: "A5b",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "active host disconnect",
    },
    Run {
        id: "A6a",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "resume: first run",
    },
    Run {
        id: "A6b",
        group: 'A',
        route: Route::Claude,
        model: "claude-haiku-5-5",
        effort: "low",
        timeout_secs: 180,
        what: "resume: --resume run",
    },
    Run {
        id: "B1",
        group: 'B',
        route: Route::Codex,
        model: "gpt-6-luna",
        effort: "low",
        timeout_secs: 180,
        what: "account and provider check, bounded negative tool check",
    },
    Run {
        id: "B2",
        group: 'B',
        route: Route::Codex,
        model: "gpt-6-luna",
        effort: "low",
        timeout_secs: 180,
        what: "outputSchema on a turn",
    },
    Run {
        id: "B3",
        group: 'B',
        route: Route::Codex,
        model: "gpt-6-luna",
        effort: "low",
        timeout_secs: 180,
        what: "write confinement under workspaceWrite",
    },
    Run {
        id: "B4",
        group: 'B',
        route: Route::Codex,
        model: "gpt-6-luna",
        effort: "low",
        timeout_secs: 180,
        what: "turn/interrupt",
    },
    Run {
        id: "B5",
        group: 'B',
        route: Route::Codex,
        model: "gpt-6-luna",
        effort: "low",
        timeout_secs: 180,
        what: "thread/resume then one short turn",
    },
    Run {
        id: "C1a",
        group: 'C',
        route: Route::Codex,
        model: "gpt-6.1-sol",
        effort: "high",
        timeout_secs: 180,
        what: "peer exchange: Codex writes",
    },
    Run {
        id: "C1b",
        group: 'C',
        route: Route::Claude,
        model: "claude-sonnet-5-5",
        effort: "high",
        timeout_secs: 180,
        what: "peer exchange: Claude reviews",
    },
    Run {
        id: "C1c",
        group: 'C',
        route: Route::Codex,
        model: "gpt-6.1-sol",
        effort: "high",
        timeout_secs: 180,
        what: "peer exchange: Codex applies one finding",
    },
    Run {
        id: "D1",
        group: 'D',
        route: Route::Codex,
        model: "gpt-6-astra",
        effort: "high",
        timeout_secs: 600,
        what: "time trial",
    },
    Run {
        id: "D2",
        group: 'D',
        route: Route::Codex,
        model: "gpt-6-astra",
        effort: "low",
        timeout_secs: 600,
        what: "time trial",
    },
    Run {
        id: "D3",
        group: 'D',
        route: Route::Codex,
        model: "gpt-6.1-sol",
        effort: "high",
        timeout_secs: 600,
        what: "time trial",
    },
];

pub fn find(id: &str) -> Option<&'static Run> {
    RUNS.iter().find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_twenty_unique_approved_runs_the_nineteen_plus_one_authorized_rerun() {
        assert_eq!(RUNS.len(), 20);
        let mut ids: Vec<_> = RUNS.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 20);
    }

    #[test]
    fn counts_per_check_match_the_live_checks_document() {
        let n = |prefix: &str| RUNS.iter().filter(|r| r.id.starts_with(prefix)).count();
        // A2 has one authorized rerun, A2r, added by Nitish on October 31, 2026 after the A2 stop.
        assert_eq!(
            [n("A1"), n("A2"), n("A3"), n("A4"), n("A5"), n("A6")],
            [1, 2, 1, 1, 2, 2]
        );
        assert_eq!(
            [n("B1"), n("B2"), n("B3"), n("B4"), n("B5")],
            [1, 1, 1, 1, 1]
        );
        assert_eq!(n("C1"), 3);
        assert_eq!([n("D1"), n("D2"), n("D3")], [1, 1, 1]);
    }

    #[test]
    fn the_rerun_sits_directly_after_the_run_it_replaces_and_uses_the_same_settings() {
        let ids: Vec<&str> = RUNS.iter().map(|r| r.id).collect();
        let i = ids.iter().position(|x| *x == "A2").unwrap();
        assert_eq!(ids[i + 1], "A2r");
        let (a, b) = (find("A2").unwrap(), find("A2r").unwrap());
        assert_eq!(
            (a.route, a.model, a.effort, a.timeout_secs),
            (b.route, b.model, b.effort, b.timeout_secs)
        );
    }

    #[test]
    fn unknown_ids_are_not_found() {
        for id in ["", "A7", "a1", "A1 ", "A5", "E1", "C2", "D4", "../A1", "B6"] {
            assert!(find(id).is_none(), "{id:?}");
        }
        assert!(find("A1").is_some() && find("A5b").is_some() && find("C1c").is_some());
    }

    #[test]
    fn models_and_efforts_match_the_document() {
        for r in RUNS.iter().filter(|r| r.group == 'A') {
            assert_eq!(
                (r.route, r.model, r.effort),
                (Route::Claude, "claude-haiku-5-5", "low"),
                "{}",
                r.id
            );
        }
        for r in RUNS.iter().filter(|r| r.group == 'B') {
            assert_eq!(
                (r.route, r.model, r.effort),
                (Route::Codex, "gpt-6-luna", "low"),
                "{}",
                r.id
            );
        }
        let c = |id| find(id).map(|r| (r.route, r.model, r.effort)).unwrap();
        assert_eq!(c("C1a"), (Route::Codex, "gpt-6.1-sol", "high"));
        assert_eq!(c("C1b"), (Route::Claude, "claude-sonnet-5-5", "high"));
        assert_eq!(c("C1c"), (Route::Codex, "gpt-6.1-sol", "high"));
        assert_eq!(c("D1"), (Route::Codex, "gpt-6-astra", "high"));
        assert_eq!(c("D2"), (Route::Codex, "gpt-6-astra", "low"));
        assert_eq!(c("D3"), (Route::Codex, "gpt-6.1-sol", "high"));
    }

    #[test]
    fn timeouts_are_three_minutes_for_mechanics_and_ten_for_time_trials() {
        for r in RUNS.iter() {
            assert_eq!(
                r.timeout_secs,
                if r.group == 'D' { 600 } else { 180 },
                "{}",
                r.id
            );
        }
    }

    #[test]
    fn groups_are_in_document_order() {
        let groups: Vec<char> = RUNS.iter().map(|r| r.group).collect();
        let mut sorted = groups.clone();
        sorted.sort();
        assert_eq!(groups, sorted);
    }
}
