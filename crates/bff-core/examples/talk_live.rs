//! Live check of asking about a step and talking to the coordinator, on plan quota. Applies nothing.
//! Run: cargo run -p bff-core --example talk_live -- <projects-dir> <project> <step> <question> <message>

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
    if a.len() < 6 {
        eprintln!("usage: talk_live <projects-dir> <project> <step> <question> <message>");
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
    for role in ["", "reviewer"] {
        let who = if role.is_empty() {
            "the step's owner"
        } else {
            role
        };
        match app.ask_step(&mut engine, &a[3], role, &a[4], now()) {
            Ok(m) => println!(
                "--- ask {who}\n{}\n",
                serde_json::to_string_pretty(&m).unwrap()
            ),
            Err(e) => println!("--- ask {who} failed: {e}\n"),
        }
    }
    match app.coordinator_say(&mut engine, "", &a[5], now()) {
        Ok(t) => println!(
            "--- coordinator\n{}",
            serde_json::to_string_pretty(&t).unwrap()
        ),
        Err(e) => println!("--- coordinator failed: {e}"),
    }
}
