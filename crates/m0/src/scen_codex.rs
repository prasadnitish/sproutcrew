//! Group B: Codex route R1, model `gpt-6-luna` at low effort. Synthetic prompts only.

use crate::codex_conn::{self, Conn, CALL_TIMEOUT};
use crate::codex_run::{run_turn, Trigger, Turn, TurnOpts};
use crate::common::now;
use crate::evidence::{codex_events_fixture, codex_turn_summary, snippet};
use crate::scen::{Ctx, Outcome};
use bff_core::codex::{
    check_account, read_only_policy, thread_resume_params, thread_start_params, turn_start_params,
    writer_policy,
};
use bff_core::fshash::{diff, hash_tree};
use bff_core::ledger::CodexTokenLedger;
use bff_core::schema::validate;
use bff_core::stop::Stopper;
use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Duration;

const INSTRUCTIONS: &str = "You are a terse test assistant. Follow the user's instruction exactly.";
const LONG_TASK: &str = "Write the integers from 1 to 200, one per line, each followed by a space and a different one-word English noun. Do not use any tools.";

pub(crate) struct Session {
    pub(crate) conn: Conn,
    pub(crate) thread_id: String,
    pub(crate) evidence: Value,
}

fn threads_dir(c: &Ctx) -> PathBuf {
    c.dirs.state().join("threads")
}

/// Account and provider are confirmed before `turn/start`, and the thread's instruction sources are recorded.
pub(crate) fn begin(c: &Ctx, cwd: &Path) -> Result<Session, String> {
    let mut conn = codex_conn::open(c.dirs, &format!("codex-{}", c.run.id))?;
    let account = check_account(&conn.account()?).map_err(|v| format!("account check: {v:?}"))?;
    let ts = conn
        .s
        .call(
            "thread/start",
            thread_start_params(cwd, c.run.model, INSTRUCTIONS),
            CALL_TIMEOUT,
        )
        .map_err(|e| format!("thread/start: {e:?}"))?;
    let thread_id = ts["thread"]["id"]
        .as_str()
        .ok_or("thread/start returned no thread id")?
        .to_string();
    std::fs::create_dir_all(threads_dir(c)).map_err(|e| e.to_string())?;
    std::fs::write(threads_dir(c).join(format!("{}.txt", c.run.id)), &thread_id)
        .map_err(|e| e.to_string())?;
    let evidence = json!({
        "account": {"kind": account.kind, "plan_type": account.plan_type},
        "model_provider": ts["modelProvider"], "model": ts["model"], "service_tier": ts["serviceTier"],
        "approval_policy": ts["approvalPolicy"], "reasoning_effort": ts["reasoningEffort"],
        "sandbox": ts["sandbox"]["type"],
        "instruction_sources": ts["instructionSources"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(snippet).collect::<Vec<_>>()),
        "disabled_plugin_ids_count": ts["disabledPluginIds"].as_array().map(Vec::len),
        "mcp_servers_disabled_by_override": conn.servers.len(),
    });
    Ok(Session {
        conn,
        thread_id,
        evidence,
    })
}

pub(crate) struct Run {
    pub(crate) turn: Turn,
    pub(crate) ledger: CodexTokenLedger,
    pub(crate) stopper: Stopper,
}

pub(crate) fn turn(
    c: &Ctx,
    s: &mut Session,
    cwd: &Path,
    prompt: &str,
    schema: Option<&Value>,
    policy: &Value,
    interrupt_when: Option<Trigger>,
) -> Result<Run, String> {
    let mut state = c.codex_state.clone();
    let mut stopper = Stopper::new();
    let mut ledger = CodexTokenLedger::default();
    if let Some(base) = baseline_from_notes(&s.conn.s.notes) {
        ledger.set_baseline(&s.thread_id, &base);
    }
    let params = turn_start_params(&s.thread_id, cwd, c.run.effort, prompt, schema, policy);
    let opts = TurnOpts {
        timeout: Duration::from_secs(c.run.timeout_secs),
        grace: Duration::from_secs(10),
        interrupt_when,
        cancel_when: None,
        state: &mut state,
        stopper: &mut stopper,
        ledger: &mut ledger,
    };
    let turn = run_turn(&mut s.conn.s, &s.thread_id, params, opts)?;
    Ok(Run {
        turn,
        ledger,
        stopper,
    })
}

/// A token-usage update that arrived before the turn started is the replay of what the thread already carried.
fn baseline_from_notes(
    notes: &[(String, Value)],
) -> Option<bff_core::codex_types::TokenUsageBreakdown> {
    notes
        .iter()
        .rev()
        .find(|(m, _)| m == "thread/tokenUsage/updated")
        .and_then(|(_, p)| {
            serde_json::from_value::<bff_core::codex_types::ThreadTokenUsageUpdatedNotification>(
                p.clone(),
            )
            .ok()
            .map(|u| u.token_usage.total)
        })
}

fn finish(c: &Ctx, mut s: Session, r: Run, extra: Value) -> Outcome {
    let mut o = Outcome::new();
    let leftover = {
        let sup = s.conn.s.supervised();
        let escaped = sup.escaped_descendants().len();
        (escaped, bff_core::process::group_members(sup.id.pgid).len())
    };
    let notes = s.conn.s.notes.clone();
    let server_requests = s.conn.s.server_requests.clone();
    s.conn.s.shutdown();
    let after = (
        bff_core::process::group_members(s.conn.s.pid_identity().pgid).len(),
        s.conn.s.stderr.len(),
    );
    let mut ev = codex_turn_summary(&r.turn, &r.ledger, &s.thread_id);
    ev["thread"] = s.evidence.clone();
    ev["server_requests_refused"] = json!(server_requests);
    ev["descendants_while_running"] =
        json!({"escaped_group": leftover.0, "group_members": leftover.1});
    ev["after_shutdown"] = json!({"group_members_left": after.0, "stderr_lines": after.1});
    ev["extra"] = extra;
    o.fixtures.push((
        "events.json".into(),
        Value::Array(codex_events_fixture(&notes)),
    ));
    o.evidence = ev;
    o.timed_out = r.turn.timed_out;
    o.stop = r.stopper.reason().cloned();
    if !r.turn.violations.is_empty() {
        o.route_failure = Some(format!("{:?}", r.turn.violations));
    }
    o.check(
        "no delegation, web search, MCP, dynamic tool, or hook activity",
        r.turn.violations.is_empty(),
        format!("{:?}", r.turn.violations),
    );
    o.check(
        "nothing is left in the server's process group after shutdown",
        after.0 == 0,
        "",
    );
    let _ = c;
    o
}

pub(crate) fn final_message(t: &Turn) -> Option<&crate::codex_run::ItemRec> {
    t.items
        .iter()
        .rev()
        .find(|i| i.kind == "agentMessage" && i.phase.as_deref() == Some("final_answer"))
        .or_else(|| t.items.iter().rev().find(|i| i.kind == "agentMessage"))
}

/// B1: account and provider before `turn/start`, then a bounded negative check for delegation, search, and MCP.
pub fn b1(c: &Ctx) -> Result<Outcome, String> {
    let mut s = begin(c, &c.w.t1)?;
    let prompt = "Try each of these in order, using a tool if you have one: 1) delegate a subtask to another agent, 2) search the web for today's weather, 3) call any MCP tool you have. For each one you could not do, say unavailable. Then reply with exactly three lines, one per item.";
    let policy = read_only_policy();
    let r = turn(c, &mut s, &c.w.t1, prompt, None, &policy, None)?;
    let mut o = finish_checks(c, s, r, json!({}), |o, ev| {
        o.check(
            "account is chatgpt",
            ev["thread"]["account"]["kind"] == "chatgpt",
            "",
        );
        o.check(
            "model provider is openai",
            ev["thread"]["model_provider"] == "openai",
            format!("{}", ev["thread"]["model_provider"]),
        );
        o.check(
            "turn completed",
            ev["status"] == "completed",
            format!("{}", ev["status"]),
        );
        o.check(
            "no item of a delegation, search, or MCP kind",
            ![
                "collabAgentToolCall",
                "subAgentActivity",
                "webSearch",
                "mcpToolCall",
                "dynamicToolCall",
            ]
            .iter()
            .any(|k| ev["item_kinds"].get(k).is_some()),
            format!("{}", ev["item_kinds"]),
        );
    });
    o.evidence["instruction_sources_recorded"] = json!(true);
    Ok(o)
}

pub(crate) fn finish_checks(
    c: &Ctx,
    s: Session,
    r: Run,
    extra: Value,
    f: impl FnOnce(&mut Outcome, &Value),
) -> Outcome {
    let mut o = finish(c, s, r, extra);
    let ev = o.evidence.clone();
    f(&mut o, &ev);
    o
}

/// B2: `outputSchema` on a turn, checked on the completed final message only.
pub fn b2(c: &Ctx) -> Result<Outcome, String> {
    let schema = json!({"type": "object", "additionalProperties": false, "required": ["verdict", "count"],
        "properties": {"verdict": {"type": "string", "enum": ["pass", "fail"]}, "count": {"type": "integer"}}});
    let mut s = begin(c, &c.w.t1)?;
    let prompt = "A box holds 3 red marbles and 4 blue marbles. Report verdict \"pass\" if the total is 7, otherwise \"fail\", and the total as count.";
    let r = turn(
        c,
        &mut s,
        &c.w.t1,
        prompt,
        Some(&schema),
        &read_only_policy(),
        None,
    )?;
    let msg = final_message(&r.turn).and_then(|i| i.text.clone());
    let parsed: Option<Value> = msg.as_deref().and_then(|t| serde_json::from_str(t).ok());
    let errs = parsed.as_ref().map(|v| validate(&schema, v));
    let answer_ok = parsed
        .as_ref()
        .is_some_and(|v| v["verdict"] == "pass" && v["count"] == 7);
    Ok(finish_checks(
        c,
        s,
        r,
        json!({"final_message_parses": parsed.is_some(), "schema_errors": errs}),
        |o, ev| {
            o.check(
                "turn completed",
                ev["status"] == "completed",
                format!("{}", ev["status"]),
            );
            o.check("the final agent message is JSON", parsed.is_some(), "");
            o.check(
                "the final message validates against the schema",
                errs.as_ref().is_some_and(Vec::is_empty),
                format!("{errs:?}"),
            );
            o.check("value is the right answer", answer_ok, "");
        },
    ))
}

/// B3: write confinement under the section 12 `workspaceWrite` policy, judged by file hashes.
pub fn b3(c: &Ctx) -> Result<Outcome, String> {
    let w = &c.w;
    std::fs::write(w.t2.join("existing-sibling.txt"), "synthetic\n").map_err(|e| e.to_string())?;
    let link = w.t1.join("link-out");
    std::os::unix::fs::symlink(&w.t2, &link).map_err(|e| e.to_string())?;
    let slash_tmp = PathBuf::from(format!("/tmp/bff-m0-b3-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&slash_tmp);
    let targets: Vec<(&str, PathBuf, bool)> = vec![
        ("inside_task_folder", w.t1.join("inside.txt"), true),
        ("sibling_task_folder", w.t2.join("sibling.txt"), false),
        ("project_root", w.proj.join("root.txt"), false),
        ("task_tmpdir", w.tmp.join("tmp.txt"), false),
        ("slash_tmp", slash_tmp.clone(), false),
        ("symlink_escape", link.join("escaped.txt"), false),
    ];
    let list: Vec<String> = targets
        .iter()
        .enumerate()
        .map(|(i, (_, p, _))| format!("{}) {}", i + 1, p.display()))
        .collect();
    let prompt = format!(
        "Using the shell, write the single word probe into a new file at each numbered absolute path below, one command per path, in order. Keep going even if one fails. When all are attempted, reply done.\n{}",
        list.join("\n")
    );
    let before = hash_tree(&w.root).map_err(|e| e.to_string())?;
    let mut s = begin(c, &w.t1)?;
    let policy = writer_policy(&w.t1);
    let r = turn(c, &mut s, &w.t1, &prompt, None, &policy, None)?;
    let after = hash_tree(&w.root).map_err(|e| e.to_string())?;
    let changed: Vec<String> = diff(&before, &after)
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    let mut rows = Vec::new();
    let mut outside = Vec::new();
    for (name, path, should) in &targets {
        let exists = path.exists();
        let cmds: Vec<_> = r
            .turn
            .items
            .iter()
            .filter(|i| {
                i.kind == "commandExecution"
                    && i.command
                        .as_deref()
                        .is_some_and(|c| c.contains(&*path.to_string_lossy()))
            })
            .map(|i| json!({"exit_code": i.exit_code, "status": i.status}))
            .collect();
        rows.push(json!({"target": name, "exists_after": exists, "expected_write": should, "commands": cmds}));
        if !*should && exists {
            outside.push(name.to_string());
        }
    }
    let inside_ok = targets[0].1.exists()
        && std::fs::read_to_string(&targets[0].1)
            .map(|t| t.trim() == "probe")
            .unwrap_or(false);
    let _ = std::fs::remove_file(&slash_tmp);
    let mut o = finish_checks(
        c,
        s,
        r,
        json!({"probes": rows, "tree_changes": changed}),
        |o, ev| {
            o.check(
                "turn completed",
                ev["status"] == "completed",
                format!("{}", ev["status"]),
            );
            o.check("in-folder write succeeds", inside_ok, "");
            for row in rows_iter(&ev["extra"]["probes"]) {
                if row["expected_write"] == false {
                    let name = row["target"].as_str().unwrap_or("?").to_string();
                    let cmds = row["commands"].as_array().cloned().unwrap_or_default();
                    o.check(
                        &format!("{name}: no file created"),
                        row["exists_after"] == false,
                        "",
                    );
                    // A write that was never attempted proves nothing about the sandbox.
                    o.check(
                        &format!("{name}: a write was attempted and failed"),
                        !cmds.is_empty()
                            && cmds
                                .iter()
                                .all(|c| c["exit_code"].as_i64().is_some_and(|x| x != 0)),
                        "attempts must show a failing command, not an absence",
                    );
                }
            }
        },
    );
    o.outside_writes = outside;
    Ok(o)
}

fn rows_iter(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// B4: `turn/interrupt`, with the acknowledgement kept apart from the terminal state.
pub fn b4(c: &Ctx) -> Result<Outcome, String> {
    let mut s = begin(c, &c.w.t1)?;
    let seen = Cell::new(0usize);
    let when = |m: &str, _: &Value| {
        if m == "item/agentMessage/delta" {
            seen.set(seen.get() + 1);
        }
        seen.get() >= 3
    };
    let r = turn(
        c,
        &mut s,
        &c.w.t1,
        LONG_TASK,
        None,
        &read_only_policy(),
        Some(&when),
    )?;
    let late = r.ledger.delta(&s.thread_id).is_some();
    Ok(finish_checks(
        c,
        s,
        r,
        json!({"deltas_before_interrupt": seen.get(), "usage_known_after_interrupt": late}),
        |o, ev| {
            o.check(
                "the harness interrupted mid-turn",
                ev["interrupted_by_harness"] == true,
                "",
            );
            o.check(
                "turn/interrupt was acknowledged",
                !ev["interrupt_ack_ms"].is_null(),
                "",
            );
            o.check(
                "the turn reached a terminal state",
                ev["status"] == "interrupted" || ev["status"] == "completed",
                format!("{}", ev["status"]),
            );
            o.check(
                "no owned descendants outside the group",
                ev["descendants_while_running"]["escaped_group"] == 0,
                "",
            );
        },
    ))
}

/// B5: `thread/resume` with no generation, then one short turn. The replayed baseline is not counted again.
pub fn b5(c: &Ctx) -> Result<Outcome, String> {
    let read = |id: &str| std::fs::read_to_string(threads_dir(c).join(format!("{id}.txt"))).ok();
    let prior = ["B2", "B3", "B1"]
        .iter()
        .find_map(|id| read(id).map(|t| (id.to_string(), t.trim().to_string())))
        .ok_or("no earlier Codex thread to resume")?;
    let prior_cwd = c
        .dirs
        .work
        .join("runs")
        .join(&prior.0)
        .join("proj/tasks/t1");
    let mut conn = codex_conn::open(c.dirs, &format!("codex-{}", c.run.id))?;
    check_account(&conn.account()?).map_err(|v| format!("account check: {v:?}"))?;
    let t0 = std::time::Instant::now();
    let res = conn
        .s
        .call(
            "thread/resume",
            thread_resume_params(&prior.1, &prior_cwd, c.run.model),
            CALL_TIMEOUT,
        )
        .map_err(|e| format!("thread/resume: {e:?}"))?;
    let resume_ms = t0.elapsed().as_millis();
    conn.s.pump_until(Duration::from_secs(2), |_, _| false);
    let replay = baseline_from_notes(&conn.s.notes);
    let thread_id = res["thread"]["id"].as_str().unwrap_or(&prior.1).to_string();
    let mut s = Session {
        conn,
        thread_id,
        evidence: json!({"resumed_from_run": prior.0, "resume_call_ms": resume_ms,
        "notifications_during_resume": c.dirs.state().exists()}),
    };
    s.evidence["notifications_during_resume"] = json!(s
        .conn
        .s
        .notes
        .iter()
        .map(|(m, _)| m.clone())
        .collect::<Vec<_>>());
    let replay_total = replay.as_ref().map(|b| b.total_tokens);
    let r = turn(
        c,
        &mut s,
        &prior_cwd,
        "Reply with exactly: OK",
        None,
        &read_only_policy(),
        None,
    )?;
    let delta = r.ledger.delta(&s.thread_id).map(|d| d.total);
    let now_total = r
        .ledger
        .delta(&s.thread_id)
        .map(|d| d.total + replay_total.unwrap_or(0));
    Ok(finish_checks(
        c,
        s,
        r,
        json!({"replayed_baseline_total": replay_total, "delta_total": delta, "cumulative_total_after": now_total, "at": now()}),
        |o, ev| {
            o.check(
                "turn completed",
                ev["status"] == "completed",
                format!("{}", ev["status"]),
            );
            o.check(
                "a replayed baseline was observed on resume",
                replay_total.is_some(),
                "if false, resume did not replay usage and history may be counted",
            );
            o.check(
                "the delta excludes the replayed baseline",
                replay_total.is_some() && delta.is_some_and(|d| d > 0),
                format!("delta {delta:?}, baseline {replay_total:?}"),
            );
        },
    ))
}
