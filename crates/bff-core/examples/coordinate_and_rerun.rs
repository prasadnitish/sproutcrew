//! Says one message to the coordinator, applies its proposal if it is clean, and runs the steps it suggests again,
//! live, on plan quota. Stops at the first failure.
//! Run: cargo run -p bff-core --example coordinate_and_rerun -- <projects-dir> <project> <message>

use bff_core::app::App;
use bff_core::claude_engine::ClaudeEngine;
use bff_core::clock::now;
use bff_core::codex_engine::CodexEngine;
use bff_core::engine::RoutedEngine;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: coordinate_and_rerun <projects-dir> <project> <message>");
        std::process::exit(2);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let work = std::env::temp_dir().join(format!("bff-rerun-{}", std::process::id()));
    let codex = CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: home.clone(),
        scratch: work.join("scratch-codex"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 900,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let claude = ClaudeEngine {
        claude: PathBuf::from(&home).join(".local/bin/claude"),
        home,
        scratch: work.join("scratch-claude"),
        state_dir: work.join("state"),
        timeout_secs: 900,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut engine = RoutedEngine::new(Box::new(codex), Box::new(claude));
    let mut app = App::new(PathBuf::from(&a[1]));
    app.open_project(&a[2]).expect("open the project");
    let turn = app
        .coordinator_say(&mut engine, "", &a[3], now())
        .expect("the coordinator answered");
    println!("coordinator: {}", turn["text"].as_str().unwrap_or_default());
    let p = &turn["proposal"];
    if p.is_null() {
        println!("no proposal, nothing applied");
        return;
    }
    println!("{}", serde_json::to_string_pretty(&p["summary"]).unwrap());
    if !p["problems"].as_array().is_none_or(|v| v.is_empty()) {
        println!(
            "the proposal has problems, nothing applied: {}",
            p["problems"]
        );
        return;
    }
    let id = p["id"].as_str().expect("an id");
    app.coordinator_apply(id, "Nitish", now()).expect("applied");
    println!("applied {id}");
    let rerun: Vec<String> = p["rerun"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for step in rerun {
        let started = std::time::Instant::now();
        let r = app.run_stage(&mut engine, &step, now());
        let ok = r.as_ref().map(|o| o.artifact.is_some()).unwrap_or(false);
        println!(
            "{step}: {:?} in {}s",
            r.map(|o| (o.artifact.is_some(), o.note)),
            started.elapsed().as_secs()
        );
        if !ok {
            println!("stopped after a failure");
            return;
        }
    }
}
