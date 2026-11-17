//! The Codex turn runner against scripted fake servers (`m0 __fake-codex turn*`).

use bff_core::codex::Violation;
use bff_core::ledger::CodexTokenLedger;
use bff_core::process::spawn;
use bff_core::ratelimit::RateLimitState;
use bff_core::stop::Stopper;
use m0::codex_run::{run_turn, TurnOpts};
use m0::session::CodexSession;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

fn session(mode: &str) -> CodexSession {
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let sup = spawn(
        Path::new(env!("CARGO_BIN_EXE_m0")),
        &["__fake-codex".into(), mode.into()],
        &env,
        Path::new("/private/tmp"),
    )
    .unwrap();
    CodexSession::new(sup)
}

struct Fx {
    state: RateLimitState,
    stopper: Stopper,
    ledger: CodexTokenLedger,
}

fn go(
    mode: &str,
    timeout_ms: u64,
    interrupt_when: Option<m0::codex_run::Trigger>,
) -> (m0::codex_run::Turn, Fx, CodexSession) {
    let mut s = session(mode);
    let mut fx = Fx {
        state: RateLimitState::new(),
        stopper: Stopper::new(),
        ledger: CodexTokenLedger::default(),
    };
    let opts = TurnOpts {
        timeout: Duration::from_millis(timeout_ms),
        grace: Duration::from_millis(500),
        interrupt_when,
        cancel_when: None,
        state: &mut fx.state,
        stopper: &mut fx.stopper,
        ledger: &mut fx.ledger,
    };
    let t = run_turn(&mut s, "t1", json!({"threadId": "t1", "input": []}), opts).unwrap();
    (t, fx, s)
}

#[test]
fn a_normal_turn_collects_items_usage_and_the_final_message() {
    let (t, fx, mut s) = go("turn", 5000, None);
    assert_eq!(t.status, "completed");
    assert!(t.violations.is_empty());
    let msg = t.items.iter().find(|i| i.kind == "agentMessage").unwrap();
    assert_eq!(msg.text.as_deref(), Some("hello"));
    assert_eq!(msg.phase.as_deref(), Some("final_answer"));
    assert_eq!(fx.ledger.delta("t1").unwrap().total, 150);
    assert_eq!(t.methods["thread/tokenUsage/updated"], 2);
    assert!(!t.interrupted_by_harness && !t.timed_out);
    s.shutdown();
}

#[test]
fn tool_activity_interrupts_the_turn_and_is_reported_as_a_violation() {
    let (t, fx, mut s) = go("turn-websearch", 5000, None);
    assert_eq!(t.violations, vec![Violation::WebSearch]);
    assert!(t.interrupted_by_harness);
    assert_eq!(t.status, "interrupted");
    assert!(t.interrupt_ack_ms.is_some() && t.terminal_ms.is_some());
    assert!(
        fx.stopper.reason().is_none(),
        "the orchestrator, not the runner, latches route failures"
    );
    s.shutdown();
}

#[test]
fn the_external_timeout_interrupts_a_hung_turn() {
    let (t, _, mut s) = go("turn-hang", 600, None);
    assert!(t.timed_out);
    assert_eq!(t.status, "interrupted");
    s.shutdown();
}

#[test]
fn the_caller_can_interrupt_on_a_notification_and_late_usage_is_kept() {
    let when = |m: &str, _: &Value| m == "item/agentMessage/delta";
    let (t, fx, mut s) = go("turn-long", 5000, Some(&when));
    assert!(t.interrupted_by_harness);
    assert_eq!(t.status, "interrupted");
    assert!(t.terminal_ms.unwrap() >= t.interrupt_ack_ms.unwrap());
    // The fake reports usage after the interrupt, as the real server may.
    assert_eq!(fx.ledger.delta("t1").unwrap().total, 77);
    s.shutdown();
}

#[test]
fn a_rate_limit_update_at_fifty_percent_interrupts_the_turn_and_trips_the_stopper() {
    let (t, fx, mut s) = go("turn-hot", 5000, None);
    assert!(t.interrupted_by_harness);
    assert!(matches!(
        fx.stopper.reason(),
        Some(bff_core::stop::StopReason::PlanWindow { route: "codex", .. })
    ));
    s.shutdown();
}

#[test]
fn unparseable_usage_for_a_known_method_is_a_telemetry_stop_not_a_silent_skip() {
    let (t, fx, mut s) = go("turn-badusage", 5000, None);
    assert!(t
        .parse_errors
        .contains(&"thread/tokenUsage/updated".to_string()));
    assert!(matches!(
        fx.stopper.reason(),
        Some(bff_core::stop::StopReason::Telemetry(_))
    ));
    s.shutdown();
}

#[test]
fn a_stop_request_ends_a_silent_turn_long_before_its_timeout() {
    let mut s = session("turn-hang");
    let mut fx = Fx {
        state: RateLimitState::new(),
        stopper: Stopper::new(),
        ledger: CodexTokenLedger::default(),
    };
    let started = std::time::Instant::now();
    let stop = || started.elapsed() > Duration::from_millis(300);
    let opts = TurnOpts {
        timeout: Duration::from_secs(30),
        grace: Duration::from_millis(500),
        interrupt_when: None,
        cancel_when: Some(&stop),
        state: &mut fx.state,
        stopper: &mut fx.stopper,
        ledger: &mut fx.ledger,
    };
    let t = run_turn(&mut s, "t1", json!({"threadId": "t1", "input": []}), opts).unwrap();
    assert!(t.interrupted_by_harness && !t.timed_out);
    assert_eq!(t.status, "interrupted");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
    s.shutdown();
}
