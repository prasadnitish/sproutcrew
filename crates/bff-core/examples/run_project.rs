//! Creates a project in the app's projects folder, adds a folder of material, and runs every step live.
//! Plan quota only. The engines stop on their own conditions (a plan window at 50 percent, a changed credit balance).
//! Run: cargo run -p bff-core --example run_project -- <projects-dir> <project> <use-case> <material-dir> <brief-file> <out.json>

use bff_core::app::App;
use bff_core::claude_engine::ClaudeEngine;
use bff_core::clock::now;
use bff_core::codex_engine::CodexEngine;
use bff_core::engine::RoutedEngine;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 7 {
        eprintln!("usage: run_project <projects-dir> <project> <use-case> <material-dir> <brief-file> <out.json>");
        std::process::exit(2);
    }
    let (projects, name, case, material, brief, out) = (
        PathBuf::from(&a[1]),
        &a[2],
        &a[3],
        &a[4],
        PathBuf::from(&a[5]),
        PathBuf::from(&a[6]),
    );
    let home = std::env::var("HOME").unwrap_or_default();
    let work = std::env::temp_dir().join(format!("bff-run-{}", std::process::id()));
    let codex = || CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: home.clone(),
        scratch: work.join("scratch-codex"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 900,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let claude = || ClaudeEngine {
        claude: PathBuf::from(&home).join(".local/bin/claude"),
        home: home.clone(),
        scratch: work.join("scratch-claude"),
        state_dir: work.join("state"),
        timeout_secs: 900,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut report = json!({"project": name, "use_case": case});
    report["codex_before"] = codex()
        .status()
        .unwrap_or_else(|e| json!({"unavailable": e}));
    report["claude_before"] = claude()
        .status()
        .unwrap_or_else(|e| json!({"unavailable": e}));
    let mut engine = RoutedEngine::new(Box::new(codex()), Box::new(claude()));

    let mut app = App::new(projects);
    let brief_text = std::fs::read_to_string(&brief).expect("brief file");
    app.create_project(name, brief_text.trim(), now())
        .expect("create the project");
    report["material"] = app.add_source_path(material).expect("add the material");
    app.use_workflow(
        case,
        ("R1", "gpt-6.1-terra", "medium"),
        ("R2", "claude-sonnet-5-5", "medium"),
        now(),
    )
    .expect("plan");
    app.approve("run_project, authorized by Nitish", now())
        .expect("approve");
    let ids: Vec<String> = app.state("live").unwrap()["plan"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    let mut steps: Vec<Value> = vec![];
    let mut ok = true;
    for st in &ids {
        let started = std::time::Instant::now();
        let r = app.run_stage(&mut engine, st, now());
        let good = r.as_ref().map(|o| o.artifact.is_some()).unwrap_or(false);
        println!("{st}: ok={good} in {}s", started.elapsed().as_secs());
        steps.push(json!({"step": st, "seconds": started.elapsed().as_secs(), "ok": good, "note": r.map(|o| o.note).unwrap_or_else(|e| e)}));
        if !good {
            ok = false;
            break;
        }
    }
    // A use case that makes a document and a deck also gets its client files.
    if ok && ids.iter().any(|i| i == "document") && ids.iter().any(|i| i == "deck") {
        let tools = bff_core::clientfiles::Tools::detect(&home);
        report["client_files"] = match app.export_client_files(
            Some("document"),
            Some("deck"),
            "Prepared for the board",
            &tools,
            now(),
        ) {
            Ok(v) => v,
            Err(e) => json!({"error": e}),
        };
    }
    let state = app.state("live").unwrap();
    report["completed_without_error"] = json!(ok);
    report["steps"] = json!(steps);
    report["meters"] = state["meters"].clone();
    report["tokens_by_model"] = state["tokens_by_model"].clone();
    report["suspicious"] = json!(state["sources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| !s["suspicious"].as_array().unwrap().is_empty())
        .map(|s| json!({"name": s["name"], "findings": s["suspicious"]}))
        .collect::<Vec<_>>());
    report["codex_after"] = codex()
        .status()
        .unwrap_or_else(|e| json!({"unavailable_or_stopped": e}));
    report["claude_after"] = claude()
        .status()
        .unwrap_or_else(|e| json!({"unavailable_or_stopped": e}));
    let clean = bff_core::redact::scrub_value(&report, &home);
    std::fs::write(&out, serde_json::to_string_pretty(&clean).unwrap()).expect("write");
    println!("{}", serde_json::to_string_pretty(&clean).unwrap());
}
