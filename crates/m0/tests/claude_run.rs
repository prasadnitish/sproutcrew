//! The Claude runner against scripted `sh` stand-ins that print stream-json lines.

use bff_core::claude::{Event, Policy, Violation};
use bff_core::process::is_running;
use m0::claude_run::{run, RunSpec};
use std::path::Path;
use std::time::{Duration, Instant};

fn policy() -> Policy {
    Policy {
        tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
        mcp_servers: vec![],
        ..Default::default()
    }
}

const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"s","model":"m","tools":["Read"],"mcp_servers":[],"apiKeySource":"none"}"#;
const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.001,"usage":{"input_tokens":3,"output_tokens":2}}"#;

fn go(script: &str, stdin: Option<&str>, timeout_ms: u64) -> (m0::claude_run::ClaudeRun, Duration) {
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let args = vec!["-c".to_string(), script.to_string()];
    let p = policy();
    let mut spec = RunSpec::new(
        Path::new("/bin/sh"),
        &args,
        &env,
        Path::new("/private/tmp"),
        &p,
        Duration::from_millis(timeout_ms),
        Duration::from_millis(300),
    );
    if let Some(s) = stdin {
        spec = spec.stdin(s);
    }
    let t = Instant::now();
    (run(&spec).unwrap(), t.elapsed())
}

fn go_with(
    script: &str,
    stdin: Option<&str>,
    defer: Option<Duration>,
    interrupt: Option<&dyn Fn(&Event) -> bool>,
) -> (m0::claude_run::ClaudeRun, Duration) {
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let args = vec!["-c".to_string(), script.to_string()];
    let p = policy();
    let mut spec = RunSpec::new(
        Path::new("/bin/sh"),
        &args,
        &env,
        Path::new("/private/tmp"),
        &p,
        Duration::from_secs(10),
        Duration::from_millis(300),
    );
    if let Some(s) = stdin {
        spec = spec.stdin(s);
    }
    if let Some(d) = defer {
        spec = spec.defer_until_init(d);
    }
    if let Some(f) = interrupt {
        spec = spec.interrupt_when(f);
    }
    let t = Instant::now();
    (run(&spec).unwrap(), t.elapsed())
}

#[test]
fn collects_events_exit_code_and_identity() {
    let (r, _) = go(&format!("echo '{INIT}'; echo '{RESULT}'"), None, 5000);
    assert!(matches!(r.events[0], Event::Init(_)));
    assert!(matches!(r.events[1], Event::Result(_)));
    assert_eq!(r.exit_code, Some(0));
    assert!(r.violations.is_empty());
    assert!(!r.timed_out);
    assert_eq!(r.id.pid, r.id.pgid);
    assert_eq!(r.raw.len(), 2);
}

#[test]
fn the_prompt_goes_in_through_stdin_after_spawn() {
    let (r, _) = go(
        r#"read line; echo "{\"type\":\"result\",\"subtype\":\"success\",\"echo\":\"$line\"}""#,
        Some("/usage"),
        5000,
    );
    match &r.events[0] {
        Event::Result(v) => assert_eq!(v["echo"], "/usage"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_policy_violation_at_init_interrupts_the_run_at_once() {
    let bad = r#"{"type":"system","subtype":"init","session_id":"s","model":"m","tools":["Read","Bash"],"mcp_servers":[],"apiKeySource":"none"}"#;
    let (r, took) = go(&format!("echo '{bad}'; sleep 30"), None, 20000);
    assert!(r
        .violations
        .contains(&Violation::ToolOutsideAllowlist("Bash".into())));
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert!(!is_running(&r.id));
    assert!(r.ended_by.is_some());
}

#[test]
fn a_tool_use_outside_the_allowlist_interrupts_the_run() {
    let msg = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t","name":"Write","input":{}}],"usage":{}}}"#;
    let (r, took) = go(
        &format!("echo '{INIT}'; echo '{msg}'; sleep 30"),
        None,
        20000,
    );
    assert!(r
        .violations
        .contains(&Violation::ToolOutsideAllowlist("Write".into())));
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn the_external_timeout_ends_a_silent_run() {
    let (r, took) = go("sleep 30", None, 400);
    assert!(r.timed_out);
    assert!(took < Duration::from_secs(6), "{took:?}");
    assert!(!is_running(&r.id));
}

#[test]
fn stderr_is_kept_apart_and_garbage_lines_are_counted() {
    let (r, _) = go(
        &format!("echo 'not json'; echo oops 1>&2; echo '{RESULT}'"),
        None,
        5000,
    );
    assert_eq!(r.bad_lines, 1);
    assert_eq!(r.stderr, vec!["oops".to_string()]);
    assert_eq!(r.events.len(), 1);
}

#[test]
fn a_prompt_can_be_held_until_init_has_been_seen_and_checked() {
    let script = format!(
        "echo '{INIT}'; read line; echo \"{{\\\"type\\\":\\\"result\\\",\\\"subtype\\\":\\\"success\\\",\\\"echo\\\":\\\"$line\\\"}}\""
    );
    let (r, _) = go_with(&script, Some("hello"), Some(Duration::from_secs(3)), None);
    assert_eq!(r.init_before_prompt, Some(true));
    match r.events.last().unwrap() {
        Event::Result(v) => assert_eq!(v["echo"], "hello"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_cli_that_waits_for_input_before_init_is_recorded_as_such() {
    let script = format!("read line; echo '{INIT}'; echo '{RESULT}'");
    let (r, took) = go_with(
        &script,
        Some("hello"),
        Some(Duration::from_millis(500)),
        None,
    );
    assert_eq!(r.init_before_prompt, Some(false));
    assert!(matches!(r.events[0], Event::Init(_)));
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn a_bad_init_stops_the_run_before_the_prompt_is_ever_sent() {
    let bad = r#"{"type":"system","subtype":"init","session_id":"s","model":"m","tools":["Bash"],"mcp_servers":[],"apiKeySource":"none"}"#;
    let script =
        format!("echo '{bad}'; if read -t 3 line; then echo PROMPT_ARRIVED 1>&2; fi; sleep 20");
    let (r, took) = go_with(
        &script,
        Some("secret task"),
        Some(Duration::from_secs(3)),
        None,
    );
    assert!(!r.violations.is_empty());
    assert!(
        !r.stderr.iter().any(|l| l.contains("PROMPT_ARRIVED")),
        "{:?}",
        r.stderr
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn the_harness_can_interrupt_on_an_event_and_still_collects_what_follows() {
    let asst = r#"{"type":"assistant","message":{"id":"m","content":[{"type":"text","text":"x"}],"usage":{}}}"#;
    let script = format!(
        "trap 'echo \"{RESULT_ESC}\"; exit 0' INT; echo '{INIT}'; echo '{asst}'; echo '{asst}'; while true; do sleep 0.1; done"
    );
    let stop_at_first_assistant = |e: &Event| matches!(e, Event::Assistant { .. });
    let (r, took) = go_with(&script, None, None, Some(&stop_at_first_assistant));
    assert!(r.interrupted_by_harness);
    assert!(r.violations.is_empty());
    assert!(!is_running(&r.id));
    assert!(
        matches!(r.events.last(), Some(Event::Result(_))),
        "late result must be kept: {} events",
        r.events.len()
    );
    assert!(took < Duration::from_secs(6), "{took:?}");
}

const RESULT_ESC: &str = r#"{\"type\":\"result\",\"subtype\":\"success\"}"#;
