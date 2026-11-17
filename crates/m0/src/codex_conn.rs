//! Opens a Codex `app-server` over stdio under the section 12 contract and completes the handshake.

use crate::common::{worker_env, Dirs, CODEX};
use crate::session::CodexSession;
use bff_core::codex::{initialize_params, launch_args, mcp_server_names, shell_set_names};
use bff_core::process::spawn;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Conn {
    pub s: CodexSession,
    pub cwd: PathBuf,
    pub servers: Vec<String>,
    pub shell_keys: Vec<String>,
}

/// Spawns with a minimal environment and a scratch `cwd`, then `initialize` and `initialized`.
pub fn open(dirs: &Dirs, name: &str) -> Result<Conn, String> {
    let cwd = dirs
        .scratch(&format!("{name}/cwd"))
        .map_err(|e| e.to_string())?;
    let tmp = dirs
        .scratch(&format!("{name}/tmp"))
        .map_err(|e| e.to_string())?;
    let toml =
        std::fs::read_to_string(Path::new(&crate::common::home()).join(".codex/config.toml"))
            .unwrap_or_default();
    let (servers, shell_keys) = (mcp_server_names(&toml), shell_set_names(&toml));
    drop(toml);
    let args = launch_args(&servers, &shell_keys, false);
    let sup = spawn(Path::new(CODEX), &args, &worker_env(&tmp, &[]), &cwd)
        .map_err(|e| format!("spawn: {e}"))?;
    crate::common::record_proc("codex", &sup.id);
    let mut s = CodexSession::new(sup);
    s.call("initialize", initialize_params(), CALL_TIMEOUT)
        .map_err(|e| format!("initialize: {e:?}"))?;
    s.notify("initialized", None);
    Ok(Conn {
        s,
        cwd,
        servers,
        shell_keys,
    })
}

impl Conn {
    pub fn account(&mut self) -> Result<Value, String> {
        self.s
            .call("account/read", json!({"refreshToken": false}), CALL_TIMEOUT)
            .map_err(|e| format!("account/read: {e:?}"))
    }

    pub fn rate_limits(&mut self) -> Result<Value, String> {
        self.s
            .call("account/rateLimits/read", Value::Null, CALL_TIMEOUT)
            .map_err(|e| format!("rateLimits: {e:?}"))
    }
}
