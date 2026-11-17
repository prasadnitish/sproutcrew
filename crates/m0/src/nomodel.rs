//! No-model checks N1, N2, N3, N4, and the process evidence for N6.
//! They cost no generation, but they do contact provider services and the CLIs may update their own state.

use crate::claude_run::{run, RunSpec};
use crate::common::{
    claude_path, now, run_tool, sha256_hex, version_output, worker_env, write_fixture, Dirs, CODEX,
};
use crate::runs::RUNS;
use crate::session::CodexSession;
use bff_core::claude::{Event, Policy};
use bff_core::codex::{
    check_account, check_effective_config, initialize_params, launch_args, mcp_server_names,
    shell_set_names, ConfigStatus, CURATED,
};
use bff_core::codex_types::GetAccountRateLimitsResponse;
use bff_core::ledger::{Baseline, ClaudeLedger};
use bff_core::process::{descendants_of, parse_ps_table, spawn, PsRow};
use bff_core::ratelimit::RateLimitState;
use bff_core::redact::{redact_json, redact_stderr};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const T: Duration = Duration::from_secs(30);

const RATE_LIMIT_ALLOW: &[&str] = &[
    "ordinaryUsageAllowed",
    "rateLimits.limitId",
    "rateLimits.limitName",
    "rateLimits.planType",
    "rateLimits.primary.**",
    "rateLimits.secondary.**",
    "rateLimits.credits.**",
    "rateLimits.rateLimitReachedType",
    "rateLimits.spendControlReached",
    "rateLimits.individualLimit.**",
    "rateLimitsByLimitId.*.limitId",
    "rateLimitsByLimitId.*.planType",
    "rateLimitsByLimitId.*.primary.**",
    "rateLimitsByLimitId.*.secondary.**",
    "rateLimitsByLimitId.*.credits.**",
    "rateLimitResetCredits.availableCount",
];

fn keys_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    k.sort();
    k
}

/// Processes whose executable is named `codex`, from a `ps` snapshot.
fn codex_processes() -> Vec<PsRow> {
    let out = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,pgid=,comm="])
        .env_clear()
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let named: Vec<i32> = out
        .lines()
        .filter(|l| {
            l.split_whitespace()
                .nth(3)
                .is_some_and(|c| c.rsplit('/').next() == Some("codex"))
        })
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect();
    parse_ps_table(&out)
        .into_iter()
        .filter(|r| named.contains(&r.pid))
        .collect()
}

pub fn n1(dirs: &Dirs) -> Result<Value, String> {
    let cwd = dirs.scratch("n1/cwd").map_err(|e| e.to_string())?;
    let tmp = dirs.scratch("n1/tmp").map_err(|e| e.to_string())?;
    let config_toml =
        std::fs::read_to_string(Path::new(&crate::common::home()).join(".codex/config.toml"))
            .unwrap_or_default();
    // Only table-header names leave the file. Values are never read into the evidence.
    let servers = mcp_server_names(&config_toml);
    let shell_keys = shell_set_names(&config_toml);
    drop(config_toml);
    let args = launch_args(&servers, &shell_keys, false);

    let before = codex_processes();
    let sup = spawn(Path::new(CODEX), &args, &worker_env(&tmp, &[]), &cwd)
        .map_err(|e| format!("spawn: {e}"))?;
    let id = sup.id.clone();
    let mut s = CodexSession::new(sup);
    let mut out = json!({"check": "N1", "at": now(), "mcp_servers_disabled": servers.len()});

    let init = s
        .call("initialize", initialize_params(), T)
        .map_err(|e| format!("initialize: {e:?}"))?;
    s.notify("initialized", None);
    out["initialize_keys"] = json!(keys_of(&init));

    let account = s
        .call("account/read", json!({"refreshToken": false}), T)
        .map_err(|e| format!("account/read: {e:?}"))?;
    out["account"] = match check_account(&account) {
        Ok(a) => json!({"kind": a.kind, "plan_type": a.plan_type,
                        "requires_openai_auth": account["requiresOpenaiAuth"]}),
        Err(v) => json!({"violation": format!("{v:?}")}),
    };

    let rl = s
        .call("account/rateLimits/read", Value::Null, T)
        .map_err(|e| format!("rateLimits: {e:?}"))?;
    let red = redact_json(&rl, RATE_LIMIT_ALLOW, &crate::common::home());
    let mut state = RateLimitState::new();
    let parsed = serde_json::from_value::<GetAccountRateLimitsResponse>(rl.clone());
    out["rate_limits"] = json!({
        "redacted": red.value, "dropped_fields": red.dropped,
        "account_id_hash": rl.get("accountId").and_then(Value::as_str).map(|a| sha256_hex(a.as_bytes())[..12].to_string()),
        "parsed": parsed.is_ok(),
    });
    if let Ok(resp) = parsed {
        state.apply_read(&resp, now());
        out["rate_limits"]["baseline_freeze"] = json!(format!("{:?}", state.freeze_baseline()));
        out["rate_limits"]["dispatch_gate"] = json!(format!("{:?}", state.dispatch_gate(now())));
        out["rate_limits"]["buckets"] = json!(state.buckets().keys().collect::<Vec<_>>());
    }

    let models = s
        .call("model/list", json!({"includeHidden": true}), T)
        .map_err(|e| format!("model/list: {e:?}"))?;
    let listed: Vec<Value> = models["data"].as_array().cloned().unwrap_or_default();
    let ids: Vec<&str> = listed.iter().filter_map(|m| m["model"].as_str()).collect();
    let wanted: std::collections::BTreeSet<&str> = RUNS
        .iter()
        .filter(|r| r.route == crate::runs::Route::Codex || r.id == "C1a")
        .map(|r| r.model)
        .collect();
    out["models"] = json!({
        "count": listed.len(),
        "required": wanted.iter().map(|m| (m.to_string(), json!(ids.contains(m)))).collect::<serde_json::Map<_, _>>(),
        "listed": listed.iter().map(|m| json!({
            "model": m["model"], "hidden": m["hidden"], "isDefault": m["isDefault"],
            "defaultReasoningEffort": m["defaultReasoningEffort"],
            "efforts": m["supportedReasoningEfforts"].as_array().map(|a| a.iter().filter_map(|e| e["reasoningEffort"].as_str()).collect::<Vec<_>>()),
        })).collect::<Vec<_>>(),
    });

    let cfg = s
        .call(
            "config/read",
            json!({"cwd": cwd.to_string_lossy(), "includeLayers": false}),
            T,
        )
        .map_err(|e| format!("config/read: {e:?}"))?;
    let statuses = check_effective_config(&cfg["config"], &servers, &shell_keys);
    let count = |f: fn(&ConfigStatus) -> bool| statuses.iter().filter(|(_, s)| f(s)).count();
    out["config"] = json!({
        "top_level_keys": keys_of(&cfg["config"]),
        "overrides_total": statuses.len(),
        "match": count(|s| matches!(s, ConfigStatus::Match)),
        "mismatch": count(|s| matches!(s, ConfigStatus::Mismatch { .. })),
        "not_reported": count(|s| matches!(s, ConfigStatus::NotReported)),
        "statuses": statuses.iter().map(|(k, s)| (k.clone(), json!(match s {
            ConfigStatus::Match => "match".to_string(),
            ConfigStatus::Mismatch { actual } => format!("MISMATCH actual={actual}"),
            ConfigStatus::NotReported => "not_reported".to_string(),
        }))).collect::<serde_json::Map<_, _>>(),
    });

    let req = s
        .call("configRequirements/read", Value::Null, T)
        .map_err(|e| format!("configRequirements: {e:?}"))?;
    out["config_requirements"] =
        json!({"keys": keys_of(&req), "requirements_keys": keys_of(&req["requirements"])});

    let hooks = s
        .call("hooks/list", json!({"cwds": [cwd.to_string_lossy()]}), T)
        .map_err(|e| format!("hooks/list: {e:?}"))?;
    let profiles = s
        .call(
            "permissionProfile/list",
            json!({"cwd": cwd.to_string_lossy()}),
            T,
        )
        .map_err(|e| format!("permissionProfile/list: {e:?}"))?;
    out["permission_profiles"] = json!(profiles["data"].as_array().map(|a| a
        .iter()
        .map(|p| json!({"id": p["id"], "allowed": p["allowed"]}))
        .collect::<Vec<_>>()));
    // An ephemeral thread makes no model call and writes no rollout. It shows the thread's default effort.
    let mut tp = bff_core::codex::thread_start_params(&cwd, "gpt-6-luna", "effort probe");
    tp["ephemeral"] = json!(true);
    let ts = s
        .call("thread/start", tp, T)
        .map_err(|e| format!("thread/start: {e:?}"))?;
    out["thread_default"] =
        json!({"reasoning_effort": ts["reasoningEffort"], "model_provider": ts["modelProvider"]});
    let len = |v: &Value| v.as_array().map(Vec::len);
    out["hooks"] = json!({"entries": len(&hooks["data"]), "hooks": len(&hooks["data"][0]["hooks"]),
        "errors": len(&hooks["data"][0]["errors"]), "warnings": len(&hooks["data"][0]["warnings"])});
    let skills = s
        .call(
            "skills/list",
            json!({"cwds": [cwd.to_string_lossy()], "forceReload": false}),
            T,
        )
        .map_err(|e| format!("skills/list: {e:?}"))?;
    out["skills"] = json!({"entries": skills["data"].as_array().map(Vec::len),
        "skills_in_first": skills["data"][0]["skills"].as_array().map(Vec::len)});

    // N6: process separation. Descendants are read while the child is still alive.
    let during = codex_processes();
    let rows: Vec<PsRow> = parse_ps_table(&String::from_utf8_lossy(
        &Command::new("/bin/ps")
            .args(["-A", "-o", "pid=,ppid=,pgid=,stat="])
            .env_clear()
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default(),
    ));
    let ours = descendants_of(&rows, id.pid);
    let new: Vec<i32> = during
        .iter()
        .map(|r| r.pid)
        .filter(|p| !before.iter().any(|b| b.pid == *p) && *p != id.pid && !ours.contains(p))
        .collect();
    s.shutdown();
    let after = codex_processes();
    let daemon_survivors: Vec<i32> = after
        .iter()
        .map(|r| r.pid)
        .filter(|p| !before.iter().any(|b| b.pid == *p))
        .collect();
    out["notifications_during_n1"] =
        json!(s.notes.iter().map(|(m, _)| m.clone()).collect::<Vec<_>>());
    out["config_warnings"] = json!(s
        .notes
        .iter()
        .filter(|(m, _)| m == "configWarning")
        .map(|(_, p)| json!({"summary": p["summary"], "path": p["path"]}))
        .collect::<Vec<_>>());
    out["remote_control_notifications"] = json!(s
        .notes
        .iter()
        .filter(|(m, _)| m == "remoteControl/status/changed")
        .map(|(_, p)| json!({"keys": keys_of(p), "status": p["status"]}))
        .collect::<Vec<_>>());
    out["server_requests"] = json!(s.server_requests);
    let stderr = redact_stderr(
        &s.stderr.join("\n"),
        &["WARN", "warning:", "ERROR", "error:"],
        &crate::common::home(),
    );
    let prefixes: Vec<String> = s
        .stderr
        .iter()
        .map(|l| {
            l.split_whitespace()
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(12)
                .collect()
        })
        .collect();
    out["stderr"] = json!({"lines_kept": stderr.kept, "lines_dropped": stderr.dropped_lines, "bytes": stderr.bytes, "first_words": prefixes});
    out["n6"] = json!({
        "pid": id.pid, "pgid": id.pgid, "start_time": id.start_time,
        "own_group_leader": id.pid == id.pgid,
        "own_descendants_during": ours.len(),
        "codex_processes_before": before.len(),
        "new_codex_processes_not_ours": new,
        "new_codex_processes_after_shutdown": daemon_survivors,
    });
    write_fixture(dirs, "N1.json", &out).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn n2(dirs: &Dirs) -> Result<Value, String> {
    let schema_dir = dirs.scratch("n2/schema").map_err(|e| e.to_string())?;
    let tmp = dirs.scratch("n2/tmp").map_err(|e| e.to_string())?;
    let mut out = json!({"check": "N2", "at": now(), "codex_version": version_output(Path::new(CODEX)).map(|v| v.trim().to_string())});

    // Regenerate from the pinned binary, without --experimental, and compare with the committed bundle.
    let gen = Command::new(CODEX)
        .args(["app-server", "generate-json-schema", "--out"])
        .arg(&schema_dir)
        .env_clear()
        .envs(worker_env(&tmp, &[]))
        .output()
        .map_err(|e| e.to_string())?;
    let fresh = std::fs::read(schema_dir.join("codex_app_server_protocol.v2.schemas.json"))
        .map_err(|e| e.to_string())?;
    let committed = std::fs::read(
        dirs.repo
            .join("crates/bff-core/schema/codex-0.166.0.v2.schemas.json"),
    )
    .map_err(|e| e.to_string())?;
    out["schema"] = json!({"generate_exit_ok": gen.status.success(), "experimental_flag_used": false,
        "fresh_sha256": sha256_hex(&fresh), "committed_sha256": sha256_hex(&committed), "identical": fresh == committed});

    let schema: Value = serde_json::from_slice(&committed).map_err(|e| e.to_string())?;
    let defs = &schema["definitions"];
    let methods = |name: &str| -> Vec<String> {
        defs[name]["oneOf"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        o["properties"]["method"]["enum"][0]
                            .as_str()
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (requests, notes) = (methods("ClientRequest"), methods("ServerNotification"));
    let need_req = [
        "initialize",
        "account/read",
        "account/rateLimits/read",
        "model/list",
        "config/read",
        "configRequirements/read",
        "hooks/list",
        "skills/list",
        "thread/start",
        "thread/resume",
        "turn/start",
        "turn/interrupt",
    ];
    let need_note = [
        "thread/tokenUsage/updated",
        "account/rateLimits/updated",
        "account/updated",
        "turn/started",
        "turn/completed",
        "item/started",
        "item/completed",
        "model/rerouted",
        "model/safetyBuffering/updated",
        "model/verification",
        "configWarning",
        "warning",
        "error",
        "deprecationNotice",
        "hook/started",
    ];
    let missing = |need: &[&str], have: &[String]| {
        need.iter()
            .filter(|n| !have.iter().any(|h| h == *n))
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    };
    out["names"] = json!({"requests": requests.len(), "notifications": notes.len(),
        "missing_requests": missing(&need_req, &requests), "missing_notifications": missing(&need_note, &notes)});
    let policies: Vec<Value> = defs["SandboxPolicy"]["oneOf"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let ww = policies
        .iter()
        .find(|p| p["properties"]["type"]["enum"][0] == "workspaceWrite");
    out["policy_shapes"] = json!({
        "variants": policies.iter().filter_map(|p| p["properties"]["type"]["enum"][0].as_str()).collect::<Vec<_>>(),
        "workspaceWrite_fields": ww.map(|p| keys_of(&p["properties"])),
        "readOnly_fields": policies.iter().find(|p| p["properties"]["type"]["enum"][0] == "readOnly").map(|p| keys_of(&p["properties"])),
    });

    // --print-startup-config with and without --strict-config, with the real user config in place.
    let toml =
        std::fs::read_to_string(Path::new(&crate::common::home()).join(".codex/config.toml"))
            .unwrap_or_default();
    let (servers, shell_keys) = (mcp_server_names(&toml), shell_set_names(&toml));
    let mut startup = serde_json::Map::new();
    for strict in [false, true] {
        // clap refuses --print-startup-config together with --listen, so the probe drops it.
        let mut args = launch_args(&servers, &shell_keys, strict);
        args.drain(1..3);
        args.insert(1, "--print-startup-config".into());
        let sup = spawn(Path::new(CODEX), &args, &worker_env(&tmp, &[]), &tmp)
            .map_err(|e| e.to_string())?;
        let mut sup = sup;
        sup.stdin = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        while std::time::Instant::now() < deadline {
            match sup.lines.recv_timeout(Duration::from_millis(200)) {
                Ok(bff_core::process::Line::Out(l)) => stdout.push(l),
                Ok(bff_core::process::Line::Err(l)) => stderr.push(l),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(_) => {}
            }
        }
        let code = sup.child.wait().ok().and_then(|s| s.code());
        let parsed: Option<Value> = serde_json::from_str(&stdout.join("\n")).ok();
        let err = redact_stderr(
            &stderr.join("\n"),
            &["error:", "Error:", "Error loading", "unknown", "invalid"],
            &crate::common::home(),
        );
        startup.insert(
            if strict { "with_strict_config" } else { "without_strict_config" }.into(),
            json!({"exit_code": code, "stdout_json": parsed.is_some(), "stdout_top_keys": parsed.as_ref().map(keys_of),
                   "stderr_kept": err.kept, "stderr_dropped_lines": err.dropped_lines}),
        );
    }
    out["print_startup_config"] = Value::Object(startup);
    out["curated_overrides"] = json!(CURATED.len() + servers.len() + shell_keys.len());
    write_fixture(dirs, "N2.json", &out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Pulls every human-readable string out of a Claude stream, in order. Kept in memory only.
fn texts_of(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in raw {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("assistant") | Some("user") => match &v["message"]["content"] {
                Value::String(s) => out.push(s.clone()),
                Value::Array(a) => out.extend(
                    a.iter()
                        .filter_map(|b| b["text"].as_str().map(str::to_string)),
                ),
                _ => {}
            },
            Some("result") => {
                if let Some(s) = v["result"].as_str() {
                    out.push(s.to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// N3: billing route, then one metadata-only `/usage` call in the exact worker environment.
pub fn n3(dirs: &Dirs) -> Result<Value, String> {
    let cwd = dirs.scratch("n3/cwd").map_err(|e| e.to_string())?;
    let tmp = dirs.scratch("n3/tmp").map_err(|e| e.to_string())?;
    let claude = claude_path();
    let mut out = json!({"check": "N3", "at": now()});

    let auth = run_tool(&claude, &["auth", "status"], &tmp, &cwd)
        .map_err(|e| format!("auth status: {e}"))?;
    let parsed: Value = serde_json::from_slice(&auth.stdout).unwrap_or(Value::Null);
    let method = parsed["authMethod"].as_str().unwrap_or("").to_string();
    out["auth_status"] = json!({"exit_code": auth.status.code(), "authMethod": method, "keys": keys_of(&parsed),
        "subscriptionType": parsed["subscriptionType"]});
    let mut stopper = bff_core::stop::Stopper::new();
    stopper.check_claude_auth(&method);
    if let Some(reason) = stopper.reason() {
        out["stopped"] = json!(format!("{reason:?}"));
        write_fixture(dirs, "N3.json", &out).map_err(|e| e.to_string())?;
        return Ok(out);
    }

    let args: Vec<String> = [
        "-p",
        "--restricted",
        "--tools",
        "",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-hook-events",
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

    let mut ledger = ClaudeLedger::new(Baseline::default());
    for e in &r.events {
        match e {
            Event::Assistant {
                message_id, usage, ..
            } => ledger.on_assistant(message_id, usage),
            Event::Result(v) => ledger.on_result(v),
            _ => {}
        }
    }
    let rep = ledger.report();
    let kinds: std::collections::BTreeMap<String, usize> = r
        .raw
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .fold(Default::default(), |mut m, v| {
            *m.entry(format!(
                "{}/{}",
                v["type"].as_str().unwrap_or("?"),
                v["subtype"].as_str().unwrap_or("")
            ))
            .or_default() += 1;
            m
        });
    let init = r.events.iter().find_map(|e| {
        if let Event::Init(i) = e {
            Some(i)
        } else {
            None
        }
    });
    out["usage_call"] = json!({
        "flags_accepted": r.exit_code == Some(0),
        "exit_code": r.exit_code, "timed_out": r.timed_out, "bad_lines": r.bad_lines,
        "violations": r.violations.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        "event_kinds": kinds,
        "init": init.map(|i| json!({"model": i.model, "tools": i.tools, "api_key_source": i.api_key_source,
            "version": i.version, "permission_mode": i.permission_mode, "mcp_servers": i.mcp_servers.len(),
            "mcp_server_errors": i.mcp_server_errors, "plugin_errors": i.plugin_errors})),
        "tokens": rep.tokens.map(|t| json!({"input": t.input, "output": t.output, "cache_creation": t.cache_creation, "cache_read": t.cache_read})),
        "tokens_status": format!("{:?}", rep.tokens_status),
        "cost_usd": rep.cost_usd, "cost_status": format!("{:?}", rep.cost_status),
        "stderr_first_words": r.stderr.iter().map(|l| l.split_whitespace().next().unwrap_or("").chars().take(24).collect::<String>()).collect::<Vec<_>>(),
        "stderr_lines": r.stderr.iter().take(3).map(|l| bff_core::redact::scrub_str(&l.chars().take(200).collect::<String>(), &crate::common::home())).collect::<Vec<_>>(),
    });
    let texts: Vec<String> = texts_of(&r.raw);
    let reading = texts
        .iter()
        .find_map(|t| bff_core::claude_usage::parse_usage(t, now()));
    out["usage_reading"] = match &reading {
        Some(rd) => json!({
            "windows": rd.windows.iter().map(|w| json!({"label": w.label, "percent": w.percent, "resets": w.resets})).collect::<Vec<_>>(),
            "gate": format!("{:?}", rd.gate(now())),
        }),
        None => json!({"parsed": false}),
    };
    write_fixture(dirs, "N3.json", &out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// One `codex sandbox` run that tries to write `target` from a shell. Returns (exit code, file exists after).
fn sandbox_write(
    profile: &str,
    cwd: &Path,
    tmp: &Path,
    target: &Path,
) -> Result<(Option<i32>, bool), String> {
    let (cwd_s, target_s) = (cwd.to_string_lossy(), target.to_string_lossy());
    let args = [
        "sandbox",
        "-P",
        profile,
        "-C",
        &cwd_s,
        "-c",
        "project_root_markers=[]",
        "/bin/sh",
        "-c",
        "echo probe > \"$0\"",
        &target_s,
    ];
    let out = run_tool(Path::new(CODEX), &args, tmp, cwd).map_err(|e| e.to_string())?;
    Ok((out.status.code(), target.exists()))
}

/// N4: deterministic command-sandbox evidence from `codex sandbox` on synthetic paths.
/// It uses the built-in `:workspace` profile, which approximates but is not the section 12 policy.
pub fn n4(dirs: &Dirs) -> Result<Value, String> {
    use bff_core::fshash::{diff, hash_tree};
    // Outside every temp root: `:workspace` makes /tmp and $TMPDIR writable by default.
    let root = dirs.state().join("n4/root");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let (t1, t2, task_tmp) = (
        root.join("tasks/t1"),
        root.join("tasks/t2"),
        root.join("task-tmp"),
    );
    for d in [&t1, &t2, &task_tmp] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(t2.join("sibling-secret.txt"), "synthetic sibling content\n")
        .map_err(|e| e.to_string())?;
    let link = t1.join("link-out");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&t2, &link).map_err(|e| e.to_string())?;
    let slash_tmp = std::path::PathBuf::from(format!("/tmp/bff-m0-n4-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&slash_tmp);

    // (name, profile, target, should the write succeed)
    let probes: Vec<(&str, &str, std::path::PathBuf, Option<bool>)> = vec![
        (
            "inside_task_folder",
            ":workspace",
            t1.join("inside.txt"),
            Some(true),
        ),
        (
            "sibling_task_folder",
            ":workspace",
            t2.join("sibling.txt"),
            Some(false),
        ),
        (
            "project_root",
            ":workspace",
            root.join("root.txt"),
            Some(false),
        ),
        // `:workspace` is not the section 12 policy. Whether it allows temp roots is observed, not asserted.
        ("task_tmpdir", ":workspace", task_tmp.join("tmp.txt"), None),
        ("slash_tmp", ":workspace", slash_tmp.clone(), None),
        (
            "symlink_escape",
            ":workspace",
            link.join("escaped.txt"),
            Some(false),
        ),
        (
            "read_only_profile_inside_write",
            ":read-only",
            t1.join("ro.txt"),
            Some(false),
        ),
    ];
    let before = hash_tree(&root).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for (name, profile, target, expect_ok) in &probes {
        let (code, exists) = sandbox_write(profile, &t1, &task_tmp, target)?;
        rows.push(json!({"probe": name, "profile": profile, "expected_write": expect_ok, "exit_code": code,
            "file_exists_after": exists, "as_expected": expect_ok.map(|e| e == exists)}));
        // Keep each probe independent, and record the tree change only for what was left behind.
        if *name != "inside_task_folder" {
            let _ = std::fs::remove_file(target);
        }
    }
    let secret = t2.join("sibling-secret.txt");
    let (t1_s, secret_s) = (t1.to_string_lossy(), secret.to_string_lossy());
    let read_args = [
        "sandbox",
        "-P",
        ":workspace",
        "-C",
        &t1_s,
        "/bin/cat",
        &secret_s,
    ];
    let read = run_tool(Path::new(CODEX), &read_args, &task_tmp, &t1).map_err(|e| e.to_string())?;
    let after = hash_tree(&root).map_err(|e| e.to_string())?;
    let changes: Vec<String> = diff(&before, &after)
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    let _ = std::fs::remove_file(&slash_tmp);
    let out = json!({
        "check": "N4", "at": now(), "profile_used": ":workspace",
        "probes": rows,
        "all_asserted_as_expected": rows.iter().all(|r| r["as_expected"] != false),
        "sibling_read_allowed": read.status.success() && String::from_utf8_lossy(&read.stdout).contains("synthetic sibling content"),
        "tree_changes": changes,
        "note": "Command sandbox only. File tools and MCP are not covered. :workspace is not the section 12 policy, so tmp exclusion is tested in B3.",
    });
    write_fixture(dirs, "N4.json", &out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// N7 (added): the full section 12 flag set with `/usage` as the prompt, to see whether every flag is
/// accepted and what `system/init` reports, before a generation run is spent on it. The one flag left
/// out is `--disable-slash-commands`, since it would stop `/usage` from being a command.
pub fn n7(dirs: &Dirs) -> Result<Value, String> {
    use crate::claude_job::{args_for, user_message_line, Job, SessionKind};
    use bff_core::claude::Policy;
    let w = crate::work::make(dirs, "N7").map_err(|e| e.to_string())?;
    let claude = claude_path();
    let mut out = json!({"check": "N7", "at": now()});
    let mut variants = serde_json::Map::new();
    for (name, stream) in [("stdin_text", false), ("stream_json_input", true)] {
        let job = Job {
            w: &w,
            tools: vec!["Read", "Glob", "Grep"],
            mcp: None,
            system_prompt: "You are a terse test assistant.",
            json_schema: None,
            max_turns: 2,
            model: "claude-haiku-5-5",
            effort: "low",
            session: SessionKind::New(crate::work::uuid_v4()),
            stream_input: stream,
            prompt: "/usage".into(),
            timeout: Duration::from_secs(60),
        };
        let mut args = args_for(&job).map_err(|e| e.to_string())?;
        args.retain(|a| a != "--disable-slash-commands");
        let env = worker_env(&w.tmp, crate::work::CLAUDE_ENV);
        let policy = Policy {
            tools: job.tools.iter().map(|s| s.to_string()).collect(),
            mcp_servers: vec![],
            ..Default::default()
        };
        let text = if stream {
            user_message_line("/usage")
        } else {
            "/usage".to_string()
        };
        let mut spec = RunSpec::new(
            &claude,
            &args,
            &env,
            &w.t1,
            &policy,
            Duration::from_secs(60),
            Duration::from_secs(3),
        )
        .stdin(&text);
        if stream {
            spec = spec.defer_until_init(Duration::from_secs(10));
        }
        let r = run(&spec).map_err(|e| format!("spawn: {e}"))?;
        let (sum, rep) = crate::evidence::claude_summary(&r, Baseline::default());
        variants.insert(
            name.into(),
            json!({
                "exit_code": r.exit_code, "init_before_prompt": r.init_before_prompt,
                "violations": r.violations.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
                "init": sum["init"], "event_kinds": sum["event_kinds"], "result": sum["result"],
                "tokens": sum["usage"]["tokens"], "tokens_status": format!("{:?}", rep.tokens_status),
                "stderr": sum["stderr"], "usage_text_parsed": texts_of(&r.raw).iter().any(|t| bff_core::claude_usage::parse_usage(t, 0).is_some()),
            }),
        );
    }
    out["variants"] = Value::Object(variants);
    write_fixture(dirs, "N7.json", &out).map_err(|e| e.to_string())?;
    Ok(out)
}
