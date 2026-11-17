//! The replay adapter over the real redacted streams from the M0 live runs.

use bff_core::replay::{AdapterEvent, Replay, Source};
use serde_json::Value;

fn run(id: &str, source: Source) -> Vec<AdapterEvent> {
    let path = format!(
        "{}/../../fixtures/redacted/runs/{id}/events.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
    )
    .unwrap();
    Replay::new(source, &v).unwrap().collect()
}

#[test]
fn a_completed_codex_turn_replays_start_items_usage_and_finish() {
    let ev = run("B1", Source::Codex);
    // Startup notifications come first. The turn starts later and finishes last.
    let started = ev
        .iter()
        .position(|e| *e == AdapterEvent::Started)
        .expect("a turn start");
    let finished = ev
        .iter()
        .position(|e| matches!(e, AdapterEvent::Finished { .. }))
        .expect("a finish");
    assert!(started < finished);
    assert!(ev.contains(&AdapterEvent::ItemCompleted {
        kind: "agentMessage".into()
    }));
    assert!(ev
        .iter()
        .any(|e| matches!(e, AdapterEvent::Usage { total_tokens: Some(n) } if *n > 0)));
    assert!(ev.contains(&AdapterEvent::Finished {
        status: "completed".into()
    }));
}

#[test]
fn an_interrupted_codex_turn_replays_without_inventing_usage() {
    let ev = run("B4", Source::Codex);
    assert!(ev.contains(&AdapterEvent::Finished {
        status: "interrupted".into()
    }));
    assert!(
        !ev.iter().any(|e| matches!(e, AdapterEvent::Usage { .. })),
        "B4 had no usage update"
    );
}

#[test]
fn a_claude_run_replays_start_and_a_success_result() {
    let ev = run("A1", Source::Claude);
    assert_eq!(ev.first(), Some(&AdapterEvent::Started));
    assert_eq!(
        ev.last(),
        Some(&AdapterEvent::Finished {
            status: "success".into()
        })
    );
    assert!(ev
        .iter()
        .any(|e| matches!(e, AdapterEvent::Usage { total_tokens: Some(n) } if *n > 0)));
}

#[test]
fn an_interrupted_claude_run_ends_in_its_error_subtype() {
    let ev = run("A5a", Source::Claude);
    assert_eq!(
        ev.last(),
        Some(&AdapterEvent::Finished {
            status: "error_during_execution".into()
        })
    );
}
