//! Runs one Codex turn under supervision: usage into the ledger, rate limits into the state, an
//! interrupt on any stop, violation, or timeout, and the terminal state awaited after an interrupt.

use crate::codex::{check_notification, parse_notification, Notification, Violation};
use crate::codex_session::CodexSession;
use crate::ledger::CodexTokenLedger;
use crate::ratelimit::RateLimitState;
use crate::stop::{StopReason, Stopper};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub struct ItemRec {
    pub kind: String,
    pub id: String,
    pub status: Option<String>,
    pub phase: Option<String>,
    /// Message text. In memory only.
    pub text: Option<String>,
    pub command: Option<String>,
    pub exit_code: Option<i64>,
}

#[derive(Default)]
pub struct Turn {
    pub turn_id: String,
    pub status: String,
    pub items: Vec<ItemRec>,
    pub violations: Vec<Violation>,
    pub methods: BTreeMap<String, usize>,
    pub parse_errors: Vec<String>,
    pub interrupted_by_harness: bool,
    pub timed_out: bool,
    pub interrupt_ack_ms: Option<u128>,
    pub terminal_ms: Option<u128>,
    pub elapsed_ms: u128,
}

/// A trigger for a harness interrupt, tested against each notification.
pub type Trigger<'a> = &'a dyn Fn(&str, &Value) -> bool;

pub struct TurnOpts<'a> {
    pub timeout: Duration,
    pub grace: Duration,
    /// Interrupt (not a violation) the first time this returns true for a notification.
    pub interrupt_when: Option<Trigger<'a>>,
    /// Polled every 100 ms, also while the server is silent. When it returns true the turn is interrupted.
    pub cancel_when: Option<&'a dyn Fn() -> bool>,
    pub state: &'a mut RateLimitState,
    pub stopper: &'a mut Stopper,
    pub ledger: &'a mut CodexTokenLedger,
}

/// Methods whose params BFF depends on. A parse failure for one of these is a telemetry stop.
const DEPENDED_ON: &[&str] = &[
    "thread/tokenUsage/updated",
    "account/rateLimits/updated",
    "turn/completed",
    "turn/started",
    "model/rerouted",
    "item/started",
    "item/completed",
];

struct Acc<'a> {
    turn: Turn,
    opts: TurnOpts<'a>,
    done: bool,
    need_interrupt: bool,
}

fn rec(item: &Value) -> ItemRec {
    let s = |k: &str| item.get(k).and_then(Value::as_str).map(str::to_string);
    ItemRec {
        kind: s("type").unwrap_or_default(),
        id: s("id").unwrap_or_default(),
        status: s("status"),
        phase: s("phase"),
        text: s("text"),
        command: s("command"),
        exit_code: item.get("exitCode").and_then(Value::as_i64),
    }
}

fn upsert(items: &mut Vec<ItemRec>, new: ItemRec) {
    match items
        .iter_mut()
        .find(|i| !new.id.is_empty() && i.id == new.id)
    {
        Some(existing) => *existing = new,
        None => items.push(new),
    }
}

fn on_note(a: &mut Acc, method: &str, params: &Value) {
    *a.turn.methods.entry(method.to_string()).or_default() += 1;
    match parse_notification(method, params) {
        Err(_) => {
            if DEPENDED_ON.contains(&method) {
                a.turn.parse_errors.push(method.to_string());
                a.opts
                    .stopper
                    .trip(StopReason::Telemetry(format!("unparseable {method}")));
                a.need_interrupt = true;
            }
        }
        Ok(n) => {
            match &n {
                Notification::TokenUsage(u) => {
                    a.opts.ledger.on_update(&u.thread_id, &u.token_usage.total)
                }
                Notification::RateLimits(u) => {
                    a.opts
                        .state
                        .apply_update(&u.rate_limits, crate::clock::now());
                    a.opts.stopper.check_codex(a.opts.state);
                    if a.opts.stopper.reason().is_some() {
                        a.need_interrupt = true;
                    }
                }
                Notification::Rerouted(r) => {
                    a.opts
                        .ledger
                        .record_reroute(&r.thread_id, &r.from_model, &r.to_model)
                }
                Notification::ItemStarted(item) | Notification::ItemCompleted(item) => {
                    upsert(&mut a.turn.items, rec(item))
                }
                Notification::TurnCompleted(c) if c.turn.id == a.turn.turn_id => {
                    a.turn.status = serde_json::to_value(c.turn.status)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_else(|| "unknown".into());
                    a.done = true;
                }
                _ => {}
            }
            let found = check_notification(&n);
            if !found.is_empty() {
                a.turn.violations.extend(found);
                a.need_interrupt = true;
            }
        }
    }
    if let Some(f) = a.opts.interrupt_when {
        if !a.turn.interrupted_by_harness && f(method, params) {
            a.need_interrupt = true;
        }
    }
}

/// Starts the turn and follows it to its terminal state. An interrupt is sent for a violation, a stop,
/// the caller's trigger, or the external timeout, and usage that arrives late is still counted.
pub fn run_turn(
    s: &mut CodexSession,
    thread_id: &str,
    params: Value,
    opts: TurnOpts,
) -> Result<Turn, String> {
    let t0 = Instant::now();
    let (timeout, grace) = (opts.timeout, opts.grace);
    s.supervised().arm_watchdog(timeout, grace);
    let timed_out_flag = s.supervised().timed_out.clone();
    let resp = s
        .call("turn/start", params, Duration::from_secs(30))
        .map_err(|e| format!("turn/start: {e:?}"))?;
    let turn_id = resp["turn"]["id"].as_str().unwrap_or_default().to_string();
    let mut a = Acc {
        turn: Turn {
            turn_id: turn_id.clone(),
            status: "unknown".into(),
            ..Default::default()
        },
        opts,
        done: false,
        need_interrupt: false,
    };
    let hard_deadline = t0 + timeout + grace + Duration::from_secs(20);
    let mut interrupted = false;
    loop {
        let sent = interrupted;
        let end = s.pump_until(Duration::from_millis(100), |m, p| {
            on_note(&mut a, m, p);
            a.done || (a.need_interrupt && !sent)
        });
        if a.done {
            a.turn.terminal_ms = Some(t0.elapsed().as_millis());
            break;
        }
        if !interrupted && a.opts.cancel_when.is_some_and(|f| f()) {
            a.need_interrupt = true;
        }
        let timed_out = timed_out_flag.load(Ordering::SeqCst);
        if (a.need_interrupt || timed_out) && !interrupted {
            interrupted = true;
            a.turn.interrupted_by_harness = true;
            a.turn.timed_out = timed_out;
            let ack = s.call(
                "turn/interrupt",
                json!({"threadId": thread_id, "turnId": turn_id}),
                Duration::from_secs(30),
            );
            a.turn.interrupt_ack_ms = ack.ok().map(|_| t0.elapsed().as_millis());
            continue;
        }
        if end.closed || Instant::now() > hard_deadline {
            break;
        }
    }
    // Usage can arrive after the terminal state, so keep reading briefly.
    s.pump_until(Duration::from_millis(400), |m, p| {
        on_note(&mut a, m, p);
        false
    });
    a.turn.elapsed_ms = t0.elapsed().as_millis();
    Ok(a.turn)
}
