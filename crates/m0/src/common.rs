//! Paths, the worker environment, and the single durable write path for evidence.

use bff_core::env::{build_child_env, EnvSpec};
use bff_core::redact::scrub_value;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const WORKER_PATH: &str = "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";
pub const CODEX: &str = "/opt/homebrew/bin/codex";

pub fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

pub fn claude_path() -> PathBuf {
    PathBuf::from(home()).join(".local/bin/claude")
}

pub fn now() -> u64 {
    bff_core::clock::now()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone)]
pub struct Dirs {
    pub repo: PathBuf,
    pub work: PathBuf,
}

impl Dirs {
    pub fn fixtures(&self) -> PathBuf {
        self.repo.join("fixtures/redacted")
    }
    pub fn state(&self) -> PathBuf {
        self.repo.join("m0-runs")
    }
    pub fn ledger_path(&self) -> PathBuf {
        self.state().join("ledger.json")
    }
    /// A fresh scratch directory under the work base.
    pub fn scratch(&self, name: &str) -> io::Result<PathBuf> {
        let d = self.work.join(name);
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }
}

/// Empty environment plus the section 12 allowlist.
pub fn worker_env(tmpdir: &Path, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut spec = EnvSpec::from_ambient(WORKER_PATH, tmpdir.to_path_buf());
    spec.extra = extra
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    build_child_env(&spec).expect("worker env is built from constants")
}

/// The only way evidence reaches disk. Every string is scrubbed again on the way out.
pub fn write_fixture(dirs: &Dirs, rel: &str, v: &Value) -> io::Result<PathBuf> {
    let path = dirs.fixtures().join(rel);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let clean = scrub_value(v, &home());
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&clean).map_err(io::Error::other)? + "\n",
    )?;
    Ok(path)
}

/// Runs a short-lived tool to completion with the worker environment. Never goes through a shell.
pub fn run_tool(
    program: &Path,
    args: &[&str],
    tmpdir: &Path,
    cwd: &Path,
) -> io::Result<std::process::Output> {
    Command::new(program)
        .args(args)
        .env_clear()
        .envs(worker_env(tmpdir, &[]))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
}

pub fn version_output(program: &Path) -> Option<String> {
    let out = Command::new(program)
        .arg("--version")
        .env_clear()
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

static PROC_LOG: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Where child identities are appended before dispatch. Set per run by the orchestrator.
pub fn set_proc_log(path: Option<PathBuf>) {
    *PROC_LOG.lock().expect("proc log lock") = path;
}

/// Appends `pid`, `pgid`, and start time to the run's `proc.jsonl`, so a sweep can find an orphan.
pub fn record_proc(label: &str, id: &bff_core::process::ProcId) {
    let Some(path) = PROC_LOG.lock().expect("proc log lock").clone() else {
        return;
    };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let line = serde_json::json!({"label": label, "pid": id.pid, "pgid": id.pgid, "start_time": id.start_time, "at": now()});
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}
