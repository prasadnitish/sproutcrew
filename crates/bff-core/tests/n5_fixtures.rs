//! M0 check N5: recorded-style streams replayed through the parsers, ledgers, and stop rules,
//! before any live data exists. One test per case named in `M0_LIVE_CHECKS.md`.

use bff_core::claude::{self, Event};
use bff_core::codex::{self, Message, Notification};
use bff_core::codex_types::GetAccountRateLimitsResponse;
use bff_core::ledger::{
    Baseline, ClaudeLedger, ClaudeReport, ClaudeTokens, CodexTokenLedger, Status,
};
use bff_core::ratelimit::{Block, RateLimitState};
use bff_core::stop::{StopReason, Stopper};
use std::collections::HashSet;

fn fixture(name: &str) -> Vec<String> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .map(str::to_string)
        .collect()
}

// ------------------------------------------------------------ Codex replay

struct Codex {
    state: RateLimitState,
    tokens: CodexTokenLedger,
    stopper: Stopper,
    violations: Vec<codex::Violation>,
    started: HashSet<String>,
    stopped_after_line: Vec<bool>,
}

fn replay_codex(file: &str, now: u64) -> Codex {
    let mut c = Codex {
        state: RateLimitState::new(),
        tokens: CodexTokenLedger::default(),
        stopper: Stopper::new(),
        violations: vec![],
        started: HashSet::new(),
        stopped_after_line: vec![],
    };
    for line in fixture(file) {
        match codex::parse_message(&line).unwrap().unwrap() {
            Message::Response { result, .. } => {
                let resp: GetAccountRateLimitsResponse = serde_json::from_value(result).unwrap();
                c.state.apply_read(&resp, now);
                let _ = c.state.freeze_baseline(); // only the first read can establish a baseline
            }
            Message::Notification { method, params } => {
                let n = codex::parse_notification(&method, &params).unwrap();
                match &n {
                    Notification::RateLimits(u) => c.state.apply_update(&u.rate_limits, now),
                    Notification::TurnStarted { thread_id, .. } => {
                        c.started.insert(thread_id.clone());
                    }
                    Notification::TokenUsage(u) if c.started.contains(&u.thread_id) => {
                        c.tokens.on_update(&u.thread_id, &u.token_usage.total)
                    }
                    // Before the turn starts it is the replay of what the thread already carried.
                    Notification::TokenUsage(u) => {
                        c.tokens.set_baseline(&u.thread_id, &u.token_usage.total)
                    }
                    Notification::Rerouted(r) => {
                        c.tokens
                            .record_reroute(&r.thread_id, &r.from_model, &r.to_model)
                    }
                    _ => {}
                }
                for v in codex::check_notification(&n) {
                    if let codex::Violation::AccountNotChatgpt(m) = &v {
                        c.stopper.trip(StopReason::BillingRoute(m.clone()));
                    }
                    c.violations.push(v);
                }
            }
            other => panic!("unexpected {other:?}"),
        }
        c.stopper.check_codex(&c.state);
        c.stopped_after_line.push(c.stopper.reason().is_some());
    }
    c
}

#[test]
fn rounded_fifty_percent_after_sparse_updates_stops_only_at_fifty() {
    let c = replay_codex("codex_rounded_sparse.jsonl", 1000);
    assert_eq!(c.stopped_after_line, vec![false, false, false, true]);
    assert!(matches!(
        c.stopper.reason(),
        Some(StopReason::PlanWindow { route: "codex", .. })
    ));
}

#[test]
fn sparse_update_with_null_credits_does_not_clear_the_balance_or_stop() {
    let c = replay_codex("codex_rounded_sparse.jsonl", 1000);
    // Line 3 carries `credits: null`. Had it cleared the balance, the stop would have fired there.
    assert!(!c.stopped_after_line[2]);
}

#[test]
fn an_unknown_bucket_is_guarded() {
    let c = replay_codex("codex_unknown_bucket_null_credits.jsonl", 1000);
    match c.stopper.reason() {
        Some(StopReason::PlanWindow { detail, .. }) => {
            assert!(detail.contains("codex_new_pool"), "{detail}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn null_credits_on_an_authoritative_read_after_a_reading_stops() {
    let c = replay_codex("codex_null_credits_authoritative.jsonl", 1000);
    assert_eq!(c.stopped_after_line, vec![false, true]);
    assert!(matches!(
        c.stopper.reason(),
        Some(StopReason::CreditBalance(_))
    ));
}

#[test]
fn an_account_change_between_reads_stops() {
    let c = replay_codex("codex_account_change.jsonl", 1000);
    assert!(matches!(
        c.stopper.reason(),
        Some(StopReason::BillingRoute(_))
    ));
}

#[test]
fn an_account_update_to_a_non_chatgpt_mode_stops() {
    let c = replay_codex("codex_account_switch_notification.jsonl", 1000);
    assert_eq!(c.stopped_after_line, vec![false, true]);
    assert_eq!(
        c.violations,
        vec![codex::Violation::AccountNotChatgpt("apikey".into())]
    );
}

#[test]
fn stale_data_blocks_the_next_dispatch_but_not_active_work() {
    let mut s = RateLimitState::new();
    let first = fixture("codex_rounded_sparse.jsonl")[0].clone();
    let Message::Response { result, .. } = codex::parse_message(&first).unwrap().unwrap() else {
        panic!("first line is the read");
    };
    s.apply_read(&serde_json::from_value(result).unwrap(), 1000);
    s.freeze_baseline().unwrap();
    assert_eq!(s.dispatch_gate(1060), Ok(()));
    assert!(matches!(s.dispatch_gate(1500), Err(Block::Stale { .. })));
    assert_eq!(s.stop_condition(), None);
}

#[test]
fn resume_replay_duplicates_and_reroute_in_the_token_ledger() {
    let c = replay_codex("codex_resume_duplicates_reroute.jsonl", 1000);
    let d = c.tokens.delta("t1").unwrap();
    // 1250 now, 1100 replayed at resume, the duplicate snapshot not counted twice.
    assert_eq!(
        (
            d.input,
            d.cached_input,
            d.output,
            d.reasoning_output,
            d.total
        ),
        (100, 30, 50, 15, 150)
    );
    assert_eq!(d.uncached_input(), 70);
    assert!(c.tokens.model_split_uncertain("t1"));
}

// ----------------------------------------------------------- Claude replay

fn replay_claude(
    file: &str,
    baseline: Baseline,
    drop_result: bool,
) -> (ClaudeReport, Vec<claude::Violation>, Vec<Event>) {
    let policy = claude::Policy {
        tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
        mcp_servers: vec![],
        ..Default::default()
    };
    let mut ledger = ClaudeLedger::new(baseline);
    let (mut violations, mut events) = (vec![], vec![]);
    for line in fixture(file) {
        let Some(ev) = claude::parse_line(&line).unwrap() else {
            continue;
        };
        violations.extend(match &ev {
            Event::Init(i) => claude::check_init(i, &policy),
            other => claude::check_event(other, &policy),
        });
        match &ev {
            Event::Assistant {
                message_id, usage, ..
            } => ledger.on_assistant(message_id, usage),
            Event::Result(v) if !drop_result => ledger.on_result(v),
            _ => {}
        }
        events.push(ev);
    }
    (ledger.report(), violations, events)
}

#[test]
fn repeated_message_ids_count_once_and_the_result_wins_when_trusted() {
    let (r, v, _) = replay_claude("claude_parallel_tools.jsonl", Baseline::default(), false);
    assert!(v.is_empty(), "{v:?}");
    let t = r.tokens.unwrap();
    assert_eq!(
        (t.input, t.output, t.cache_creation, t.cache_read),
        (150, 25, 10, 110)
    );
    assert_eq!(r.tokens_status, Status::Reported);
    assert_eq!(r.cost_usd, Some(0.004));
}

#[test]
fn without_a_result_streamed_usage_counts_each_message_id_once() {
    let (r, _, _) = replay_claude("claude_parallel_tools.jsonl", Baseline::default(), true);
    let t = r.tokens.unwrap();
    // msg_A appears twice and counts once: 100 + 50.
    assert_eq!((t.input, t.cache_creation, t.cache_read), (150, 10, 110));
    assert_eq!(
        (r.tokens_status, r.cost_usd, r.cost_status),
        (Status::Estimated, None, Status::Unknown)
    );
}

#[test]
fn a_zeroed_crash_result_is_distrusted_in_favour_of_streamed_usage() {
    let (r, _, _) = replay_claude("claude_crash_zeroed.jsonl", Baseline::default(), false);
    assert!(r.result_distrusted);
    assert_eq!(r.tokens.unwrap().input, 120);
    assert_eq!((r.tokens_status, r.cost_usd), (Status::Estimated, None));
}

#[test]
fn a_killed_run_with_no_result_keeps_its_streamed_usage() {
    let (r, _, _) = replay_claude("claude_killed_no_result.jsonl", Baseline::default(), false);
    assert_eq!(r.tokens.unwrap().input, 90);
    assert_eq!(
        (r.tokens_status, r.cost_status),
        (Status::Estimated, Status::Unknown)
    );
}

#[test]
fn absent_optional_fields_parse_and_usage_stays_unknown_not_zero() {
    let (r, v, events) = replay_claude("claude_absent_optional.jsonl", Baseline::default(), false);
    assert_eq!(v, vec![claude::Violation::ApiKeySourceMissing]);
    assert!(
        matches!(&events[1], Event::RateLimit(i) if i.utilization.is_none() && i.resets_at.is_none())
    );
    assert_eq!((r.tokens, r.tokens_status), (None, Status::Unknown));
    assert_eq!((r.cost_usd, r.cost_status), (None, Status::Unknown));
}

#[test]
fn cumulative_resume_totals_are_reported_as_a_delta() {
    let base = Baseline {
        cost_usd: Some(0.02),
        tokens: Some(ClaudeTokens {
            input: 100,
            output: 50,
            cache_creation: 0,
            cache_read: 10,
        }),
    };
    let (r, _, _) = replay_claude("claude_resume_second_run.jsonl", base, false);
    let t = r.tokens.unwrap();
    assert_eq!((t.input, t.output, t.cache_read), (30, 30, 5));
    assert!((r.cost_usd.unwrap() - 0.03).abs() < 1e-9);
}
