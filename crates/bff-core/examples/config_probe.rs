//! Does starting the Codex app-server (no model turn) change `~/.codex/config.toml`? Hashes the file, starts
//! the server through `CodexEngine::status`, and hashes again. Uses no tokens.
//! Run: cargo run -p bff-core --example config_probe -- <work-dir>

use bff_core::codex_engine::CodexEngine;
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
    let engine = CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: home.clone(),
        scratch: work.join("scratch"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 60,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let before = hash(&path);
    let status = engine
        .status()
        .map(|_| "ok".to_string())
        .unwrap_or_else(|e| e);
    let after = hash(&path);
    println!(
        "before {before}\nstatus {status}\nafter  {after}\nchanged {}",
        before != after
    );
}
