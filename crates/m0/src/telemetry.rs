//! Usage readings before and after every dispatch, for both providers, and the persisted credit baseline.

use crate::claude_run::{run, RunSpec};
use crate::codex_conn;
use crate::common::{claude_path, now, run_tool, sha256_hex, worker_env, Dirs};
use bff_core::claude::Policy;
use bff_core::claude_usage::{parse_usage, UsageReading};
use bff_core::codex::{check_account, AccountSummary};
use bff_core::codex_types::GetAccountRateLimitsResponse;
use bff_core::decimal::Decimal;
use bff_core::ratelimit::{Balance, RateLimitState};
use bff_core::stop::{StopReason, Stopper};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io;
use std::path::Path;
use std::time::Duration;

/// Saved at the first live reading so a change between invocations is still seen.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Baselines {
    pub codex_balance: Option<String>,
    pub codex_account_hash: Option<String>,
}

impl Baselines {
    pub fn load(path: &Path) -> io::Result<Baselines> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                serde_json::from_str(&t).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Baselines::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )
    }
}

pub fn hash_account_id(id: &str) -> String {
    sha256_hex(id.as_bytes())[..12].to_string()
}

pub struct CodexReading {
    pub state: RateLimitState,
    pub account: AccountSummary,
    pub account_hash: Option<String>,
    pub evidence: Value,
}

/// Claude plan windows from `claude -p "/usage"`, which costs no tokens. Also checks the billing route.
pub fn read_claude(dirs: &Dirs, stopper: &mut Stopper) -> Result<(UsageReading, Value), String> {
    let cwd = dirs
        .scratch("telemetry/claude/cwd")
        .map_err(|e| e.to_string())?;
    let tmp = dirs
        .scratch("telemetry/claude/tmp")
        .map_err(|e| e.to_string())?;
    let claude = claude_path();
    let auth = run_tool(&claude, &["auth", "status"], &tmp, &cwd)
        .map_err(|e| format!("auth status: {e}"))?;
    let parsed: Value = serde_json::from_slice(&auth.stdout).unwrap_or(Value::Null);
    let method = parsed["authMethod"].as_str().unwrap_or("").to_string();
    stopper.check_claude_auth(&method);

    let args: Vec<String> = [
        "-p",
        "--restricted",
        "--tools",
        "",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        "claude-haiku-5-5",
        "--effort",
        "low",
        "--max-turns",
        "1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let policy = Policy {
        tools: vec![],
        mcp_servers: vec![],
        ..Default::default()
    };
    let env = worker_env(&tmp, &[]);
    let spec = RunSpec::new(
        &claude,
        &args,
        &env,
        &cwd,
        &policy,
        Duration::from_secs(60),
        Duration::from_secs(3),
    )
    .stdin("/usage");
    let r = run(&spec).map_err(|e| format!("spawn: {e}"))?;
    let tokens_used: i64 = r
        .raw
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "result")
        .map(|v| {
            v["usage"]["input_tokens"].as_i64().unwrap_or(-1)
                + v["usage"]["output_tokens"].as_i64().unwrap_or(-1)
        })
        .sum();
    let text = r
        .raw
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "result")
        .filter_map(|v| v["result"].as_str().map(str::to_string))
        .next()
        .unwrap_or_default();
    let reading = parse_usage(&text, now())
        .ok_or_else(|| "claude /usage output could not be parsed".to_string())?;
    let evidence = json!({
        "at": reading.observed_at, "auth_method": method, "usage_call_tokens": tokens_used,
        "windows": reading.windows.iter().map(|w| json!({"label": w.label, "percent": w.percent, "resets": w.resets})).collect::<Vec<_>>(),
    });
    for w in &reading.windows {
        stopper.check_claude_percent(w.percent);
    }
    Ok((reading, evidence))
}

/// Codex account, plan windows and credit balance through a short-lived stdio server.
pub fn read_codex(dirs: &Dirs, baselines: &Baselines) -> Result<CodexReading, String> {
    let mut c = codex_conn::open(dirs, "telemetry/codex")?;
    let account_raw = c.account()?;
    let rl_raw = c.rate_limits()?;
    c.s.shutdown();
    let account =
        check_account(&account_raw).map_err(|v| format!("account check failed: {v:?}"))?;
    let resp: GetAccountRateLimitsResponse = serde_json::from_value(rl_raw.clone())
        .map_err(|_| "rate limits response is unparseable".to_string())?;
    let account_id = rl_raw
        .get("accountId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let hash = account_id.as_deref().map(hash_account_id);
    let mut state = RateLimitState::new();
    if let Some(b) = baselines.codex_balance.as_deref().and_then(Decimal::parse) {
        // The saved hash stands in for the account id, since only a hash is persisted.
        state.restore_baseline(b, baselines.codex_account_hash.clone());
    }
    let shown = match &resp.rate_limits.credits {
        Some(c) => c.balance.clone(),
        None => None,
    };
    // Compare accounts by hash: feed the state the hashed id, so the raw id never leaves this function.
    let mut hashed = resp;
    hashed.account_id = hash.clone();
    state.apply_read(&hashed, now());
    let evidence = json!({
        "at": now(), "account": {"kind": account.kind, "plan_type": account.plan_type},
        "account_hash": hash, "balance": shown,
        "ordinary_usage_allowed": rl_raw["ordinaryUsageAllowed"],
        "buckets": state.buckets().iter().map(|(k, b)| json!({
            "bucket": k,
            "primary": b.primary.as_ref().map(|w| json!({"used_percent": w.used_percent, "window_mins": w.duration_mins, "resets_at": w.resets_at})),
            "secondary": b.secondary.as_ref().map(|w| json!({"used_percent": w.used_percent, "window_mins": w.duration_mins, "resets_at": w.resets_at})),
        })).collect::<Vec<_>>(),
        "balance_state": match state.balance() { Balance::Known(d) => format!("known {d}"), Balance::Unlimited => "unlimited".into(), Balance::Unknown => "unknown".into() },
    });
    Ok(CodexReading {
        state,
        account,
        account_hash: hash,
        evidence,
    })
}

/// Everything the gate needs before a dispatch.
pub struct Preflight {
    pub claude: UsageReading,
    pub claude_evidence: Value,
    pub codex: CodexReading,
}

/// Reads both providers and applies the stop rules. The first call also saves the credit baseline.
pub fn preflight(dirs: &Dirs, stopper: &mut Stopper) -> Result<Preflight, String> {
    let (claude, claude_evidence) = read_claude(dirs, stopper)?;
    if let Err(b) = claude.gate(now()) {
        stopper.trip(StopReason::Telemetry(format!("claude usage: {b:?}")));
    }
    let path = dirs.state().join("baselines.json");
    let mut baselines = Baselines::load(&path).map_err(|e| e.to_string())?;
    let mut codex = read_codex(dirs, &baselines)?;
    if baselines.codex_balance.is_none() {
        // First live reading: record the baseline, then it holds for every later invocation.
        codex
            .state
            .freeze_baseline()
            .map_err(|b| format!("credit balance unavailable: {b:?}"))?;
        baselines.codex_balance = codex.state.baseline().map(|d| d.to_string());
        baselines.codex_account_hash = codex.account_hash.clone();
        baselines.save(&path).map_err(|e| e.to_string())?;
    }
    stopper.check_codex(&codex.state);
    if let Err(b) = codex.state.dispatch_gate(now()) {
        stopper.trip(StopReason::Telemetry(format!("codex: {b:?}")));
    }
    Ok(Preflight {
        claude,
        claude_evidence,
        codex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baselines_round_trip_and_a_missing_file_is_empty() {
        let p = std::env::temp_dir().join(format!("bff-baselines-{}.json", std::process::id()));
        assert_eq!(Baselines::load(&p).unwrap(), Baselines::default());
        let b = Baselines {
            codex_balance: Some("1250.0000000000".into()),
            codex_account_hash: Some("abc123".into()),
        };
        b.save(&p).unwrap();
        assert_eq!(Baselines::load(&p).unwrap(), b);
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn the_account_hash_is_short_stable_and_not_the_id() {
        let h = hash_account_id("acct-secret-id");
        assert_eq!(h.len(), 12);
        assert_eq!(h, hash_account_id("acct-secret-id"));
        assert!(!h.contains("acct"));
        assert_ne!(h, hash_account_id("another"));
    }

    #[test]
    fn corrupt_baselines_are_an_error_not_a_silent_reset() {
        let p = std::env::temp_dir().join(format!("bff-baselines-bad-{}.json", std::process::id()));
        std::fs::write(&p, "{ not json").unwrap();
        assert!(Baselines::load(&p).is_err());
        std::fs::remove_file(&p).unwrap();
    }
}
