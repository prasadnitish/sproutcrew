//! The M2 live check: a Claude author and a Codex reviewer through the same app core the shell uses.
//! At most four model turns (write, review, revise, review again), no retries, stops on any error.
//! Run: cargo run -p bff-core --example m2_live -- <work-dir> <out.json>

use bff_core::app::App;
use bff_core::claude_engine::ClaudeEngine;
use bff_core::clock::now;
use bff_core::codex_engine::CodexEngine;
use bff_core::engine::{Engine, RoutedEngine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn file_hash(path: &str) -> Value {
    match std::fs::read(path) {
        Ok(b) => json!(format!("{:x}", Sha256::digest(&b))),
        Err(_) => Value::Null,
    }
}

/// Hashes of two non-credential config files, to show BFF did not change them. The CLIs write their own
/// session records elsewhere in these folders, which this does not cover.
fn config_hashes(home: &str) -> Value {
    json!({
        "codex_config_toml": file_hash(&format!("{home}/.codex/config.toml")),
        "claude_settings_json": file_hash(&format!("{home}/.claude/settings.json")),
    })
}

fn codex_engine(work: &std::path::Path, home: &str) -> CodexEngine {
    CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: home.to_string(),
        scratch: work.join("scratch-codex"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 180,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn claude_engine(work: &std::path::Path, home: &str) -> ClaudeEngine {
    ClaudeEngine {
        claude: PathBuf::from(home).join(".local/bin/claude"),
        home: home.to_string(),
        scratch: work.join("scratch-claude"),
        state_dir: work.join("state"),
        timeout_secs: 180,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (work, out) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let home = std::env::var("HOME").unwrap_or_default();
    let mut report = json!({"config_hashes_before": config_hashes(&home)});
    report["codex_before"] = codex_engine(&work, &home)
        .status()
        .unwrap_or_else(|e| json!({"unavailable": e}));
    report["claude_before"] = claude_engine(&work, &home)
        .status()
        .unwrap_or_else(|e| json!({"unavailable": e}));
    let mut engine = RoutedEngine::new(
        Box::new(codex_engine(&work, &home)),
        Box::new(claude_engine(&work, &home)),
    );

    let mut app = App::new(work.join("projects"));
    let brief = "Write a note of under 80 words for a small team. A pilot buys 4 licenses at $5 each per month. State the monthly total as $25.";
    app.create_project("m2-live", brief, now()).expect("create");
    let plan = json!({"brief": brief,
        "stages": [{"id": "draft", "title": "Write the note", "owner_role": "author"}],
        "team": [
            {"role": "author", "route": "R2", "model": "claude-haiku-5-5", "effort": "low"},
            {"role": "critic", "route": "R1", "model": "gpt-6-luna", "effort": "low"}]});
    app.save_plan(plan, now()).expect("plan");
    app.approve("m2-live-check, authorized by Nitish", now())
        .expect("approve");
    app.set_budget(300_000, 100_000).expect("budget");

    // Stops at the first step that fails. Each step is one model turn.
    let mut steps: Vec<Value> = Vec::new();
    let mut go = |name: &str, r: Result<Value, String>| -> bool {
        let ok = r.is_ok();
        steps.push(json!({"step": name, "result": r}));
        ok
    };
    let ok = go(
        "write",
        app.run_stage(&mut engine, "draft", now())
            .map(|o| json!({"task": o.task, "artifact": o.artifact.is_some(), "note": o.note})),
    ) && {
        let base = app.state("live").unwrap()["review"]["draft"]["head"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let first = app.review(&mut engine, "draft", "critic", now());
        let open = first
            .as_ref()
            .map(|v| v["outcome"] == "open")
            .unwrap_or(false);
        let first_ok = go("review", first.clone());
        if first_ok && open {
            for id in first.unwrap()["findings"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let id = id.as_str().unwrap_or("").to_string();
                let r = app
                    .decide("draft", &id, "accept", "", now())
                    .map(|_| json!(id));
                go("decide", r);
            }
            go(
                "revise",
                app.revise(&mut engine, "draft", "author", &base, now())
                    .map(|n| json!({"version": n})),
            ) && go(
                "review again",
                app.review(&mut engine, "draft", "critic", now()),
            )
        } else {
            first_ok
        }
    };
    report["completed_without_error"] = json!(ok);
    report["run"] = json!(steps);
    let st = app.state("live").expect("state");
    for k in ["meters", "steps", "budget", "review", "artifacts"] {
        report[k] = st[k].clone();
    }
    if let Some(f) = st["artifacts"]
        .as_array()
        .and_then(|a| a.last())
        .and_then(|a| a["file"].as_str())
    {
        let text = app.read_artifact(f).unwrap_or_default();
        report["latest_artifact_chars"] = json!(text.chars().count());
        report["latest_artifact_head"] = json!(text.chars().take(300).collect::<String>());
    }
    report["engine"] = json!(engine.label());
    report["config_hashes_after"] = config_hashes(&home);
    report["config_files_unchanged"] =
        json!(report["config_hashes_before"] == report["config_hashes_after"]);
    report["codex_after"] = codex_engine(&work, &home)
        .status()
        .unwrap_or_else(|e| json!({"unavailable_or_stopped": e}));
    report["claude_after"] = claude_engine(&work, &home)
        .status()
        .unwrap_or_else(|e| json!({"unavailable_or_stopped": e}));
    let clean = bff_core::redact::scrub_value(&report, &home);
    std::fs::write(&out, serde_json::to_string_pretty(&clean).unwrap()).expect("write report");
    println!("{}", serde_json::to_string_pretty(&clean).unwrap());
}
