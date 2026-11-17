//! The M1 live check: one lead planning turn and one worker turn on Codex, through the same app core
//! the shell uses. It makes exactly those two calls, with no retry, and stops on any error.
//! Run: cargo run -p bff-core --example m1_live -- <work-dir> <out.json>

use bff_core::app::App;
use bff_core::clock::now;
use bff_core::codex_engine::CodexEngine;
use bff_core::engine::Engine;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (work, out) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let mut engine = CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: std::env::var("HOME").unwrap_or_default(),
        scratch: work.join("scratch"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 180,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let before = engine.status().expect("status before");
    let mut app = App::new(work.join("projects"));
    app.create_project("m1-live", "Write a one-paragraph plan, under 150 words, for a two-week pilot of a small team trying an AI coding assistant.", now()).expect("create");
    let mut report = json!({"engine": engine.label(), "before": before});

    let plan = app.propose(&mut engine, now());
    report["propose"] = json!(plan.as_ref().map(|_| "ok").map_err(|e| e.clone()));
    if plan.is_ok() {
        let st = app.state("live").expect("state");
        report["plan"] = st["plan"].clone();
        report["problems"] = st["problems"].clone();
        // The approval gate is exercised for real: an unapproved run must be refused.
        let stage = st["plan"]["stages"][0]["id"]
            .as_str()
            .unwrap_or("")
            .to_string();
        report["run_before_approval_refused"] =
            json!(app.run_stage(&mut engine, &stage, now()).is_err());
        app.approve("m1-live-check, authorized by Nitish", now())
            .expect("approve");
        let run = app.run_stage(&mut engine, &stage, now());
        report["run"] = json!(run
            .as_ref()
            .map(|o| json!({"task": o.task, "artifact": o.artifact.is_some(), "note": o.note}))
            .map_err(|e| e.clone()));
    }
    let st = app.state("live").expect("state");
    report["meters"] = st["meters"].clone();
    report["artifacts"] = st["artifacts"].clone();
    if let Some(f) = st["artifacts"][0]["file"].as_str() {
        let text = app.read_artifact(f).unwrap_or_default();
        report["artifact_chars"] = json!(text.chars().count());
        report["artifact_head"] = json!(text.chars().take(240).collect::<String>());
    }
    report["after"] = engine
        .status()
        .unwrap_or_else(|e| json!({"stopped_or_unavailable": e}));
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
