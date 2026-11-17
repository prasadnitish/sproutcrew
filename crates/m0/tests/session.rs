//! The Codex session client against a scripted fake `app-server` (`m0 __fake-codex`).

use bff_core::process::spawn;
use m0::session::{CallError, CodexSession};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn session(mode: &str) -> CodexSession {
    let exe = env!("CARGO_BIN_EXE_m0");
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let sup = spawn(
        Path::new(exe),
        &["__fake-codex".into(), mode.into()],
        &env,
        Path::new("/private/tmp"),
    )
    .unwrap();
    CodexSession::new(sup)
}

const T: Duration = Duration::from_secs(5);

#[test]
fn call_returns_the_matching_result() {
    let mut s = session("echo");
    let r = s.call("account/read", json!({"a": 1}), T).unwrap();
    assert_eq!(r["echoedMethod"], "account/read");
    assert_eq!(r["echoedParams"]["a"], 1);
    s.shutdown();
}

#[test]
fn notifications_that_arrive_first_are_kept_in_order() {
    let mut s = session("notify-first");
    s.call("account/read", json!({}), T).unwrap();
    let methods: Vec<&str> = s.notes.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(methods, vec!["account/updated", "warning"]);
    s.shutdown();
}

#[test]
fn server_requests_are_refused_and_recorded_while_waiting() {
    let mut s = session("server-request");
    let r = s.call("turn/start", json!({}), T).unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(
        s.server_requests,
        vec!["item/commandExecution/requestApproval".to_string()]
    );
    // The fake echoes back whether our refusal was a JSON-RPC error.
    let seen = s.call("probe/refusal", json!({}), T).unwrap();
    assert_eq!(seen["refusalWasError"], true);
    s.shutdown();
}

#[test]
fn rpc_errors_surface_with_code_and_message() {
    let mut s = session("error");
    match s.call("x", json!({}), T) {
        Err(CallError::Rpc { code, message }) => {
            assert_eq!((code, message.as_str()), (-32600, "nope"))
        }
        other => panic!("{other:?}"),
    }
    s.shutdown();
}

#[test]
fn silence_times_out_and_shutdown_still_ends_the_child() {
    let mut s = session("silent");
    assert!(matches!(
        s.call("x", json!({}), Duration::from_millis(300)),
        Err(CallError::Timeout)
    ));
    let id = s.pid_identity();
    s.shutdown();
    assert!(!bff_core::process::is_running(&id));
}

#[test]
fn child_exit_is_reported_as_closed() {
    let mut s = session("exit");
    assert!(matches!(s.call("x", json!({}), T), Err(CallError::Closed)));
    s.shutdown();
}

#[test]
fn pump_until_stops_on_the_predicate() {
    let mut s = session("stream");
    s.call("turn/start", json!({}), T).unwrap();
    let end = s.pump_until(T, |m, _| m == "turn/completed");
    assert!(end.matched);
    assert!(s.notes.iter().any(|(m, _)| m == "item/started"));
    s.shutdown();
}

#[test]
fn stderr_lines_are_collected_separately() {
    let mut s = session("stderr");
    s.call("x", json!({}), T).unwrap();
    assert!(s.stderr.iter().any(|l| l.contains("diagnostic")));
    s.shutdown();
}
