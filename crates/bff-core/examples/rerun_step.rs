//! Runs one step of an existing project again, live, on plan quota. The engines stop on their own conditions.
//! Run: cargo run -p bff-core --example rerun_step -- <projects-dir> <project> <step>

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
        eprintln!("usage: rerun_step <projects-dir> <project> <step>");
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
    let started = std::time::Instant::now();
    let r = app.run_stage(&mut engine, &a[3], now());
    println!(
        "{}: {:?} in {}s",
        a[3],
        r.map(|o| (o.artifact.is_some(), o.note)),
        started.elapsed().as_secs()
    );
}
