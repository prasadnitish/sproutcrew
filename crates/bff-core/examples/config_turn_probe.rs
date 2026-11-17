//! One Codex turn (a trivial write on `gpt-6-luna`, low effort), with `~/.codex/config.toml` hashed before and
//! after, to see whether a model turn changes that file. Run: cargo run -p bff-core --example config_turn_probe -- <work-dir>

use bff_core::codex_engine::CodexEngine;
use bff_core::engine::{Engine, Request};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn hash(path: &str) -> String {
    std::fs::read(path)
        .map(|b| format!("{:x}", Sha256::digest(&b))[..16].to_string())
        .unwrap_or_else(|_| "absent".into())
}

fn main() {
    let work = PathBuf::from(std::env::args().nth(1).expect("work dir"));
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{home}/.codex/config.toml");
    let mut e = CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: home.clone(),
        scratch: work.join("scratch"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 120,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let task = work.join("task");
    std::fs::create_dir_all(&task).unwrap();
    let before = hash(&path);
    let r = e.run_worker(&Request {
        web: false,
        route: "R1",
        step: "t1",
        task_dir: &task,
        prompt: "Write the single word ok to a file named artifact.md. Reply done.",
        model: "gpt-6-luna",
        effort: "low",
    });
    let after = hash(&path);
    println!("completed {} note {:?}", r.completed, r.note);
    println!(
        "usage {:?}",
        r.usage
            .iter()
            .map(|u| (u.meter, u.value))
            .collect::<Vec<_>>()
    );
    println!(
        "config.toml before {before}\nconfig.toml after  {after}\nchanged {}",
        before != after
    );
}
