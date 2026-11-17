//! Group A: Claude route R2, model `claude-haiku-5-5` at low effort. Synthetic prompts only.

use crate::claude_job::{run_job, Job, SessionKind};
use crate::claude_run::ClaudeRun;
use crate::common::{claude_path, worker_env};
use crate::evidence::{claude_events_fixture, claude_summary, final_text, last_result};
use crate::scen::{Ctx, Outcome};
use crate::work::{uuid_v4, CLAUDE_ENV};
use bff_core::claude::{parse_line, Event};
use bff_core::fshash::{diff, hash_tree};
use bff_core::ledger::{Baseline, ClaudeLedger, ClaudeTokens};
use bff_core::process::{group_members, signal_leader, spawn_raw, Graces, Sig, Supervised};
use bff_core::schema::validate;
use serde_json::{json, Value};
use std::cell::Cell;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MODEL: &str = "claude-haiku-5-5";
const EFFORT: &str = "low";
const TERSE: &str = "You are a terse test assistant. Follow the user's instruction exactly.";
const LONG_TASK: &str = "Write the integers from 1 to 120, one per line, each followed by a space and a different one-word English noun.";

fn job<'a>(c: &'a Ctx, tools: &[&'a str], prompt: &str, max_turns: u32) -> Job<'a> {
    Job {
        w: &c.w,
        tools: tools.to_vec(),
        mcp: None,
        system_prompt: TERSE,
        json_schema: None,
        max_turns,
        model: MODEL,
        effort: EFFORT,
        session: SessionKind::New(uuid_v4()),
        stream_input: false,
        prompt: prompt.to_string(),
        timeout: Duration::from_secs(c.run.timeout_secs),
    }
}

fn finish(o: &mut Outcome, r: &ClaudeRun, baseline: Baseline) {
    let (sum, _) = claude_summary(r, baseline);
    o.evidence = sum;
    o.fixtures.push((
        "events.json".into(),
        Value::Array(claude_events_fixture(&r.raw)),
    ));
    if !r.violations.is_empty() {
        o.route_failure = Some(format!("{:?}", r.violations));
    }
    o.timed_out = r.timed_out;
}

fn result_ok(r: &ClaudeRun) -> bool {
    last_result(r).is_some_and(|v| v["subtype"] == "success" && v["is_error"] == false)
}

/// A1: the contract starts, init is checked before the prompt where the CLI allows it.
pub fn a1(c: &Ctx) -> Result<Outcome, String> {
    let mut j = job(
        c,
        &["Read", "Glob", "Grep"],
        "Reply with exactly one word: ready",
        2,
    );
    j.stream_input = true;
    let r = run_job(&j, None).map_err(|e| e.to_string())?;
    let mut o = Outcome::new();
    finish(&mut o, &r, Baseline::default());
    let init = r.events.iter().find_map(|e| {
        if let Event::Init(i) = e {
            Some(i.clone())
        } else {
            None
        }
    });
    o.check("system/init seen", init.is_some(), "");
    o.check(
        "no policy violations",
        r.violations.is_empty(),
        format!("{:?}", r.violations),
    );
    if let Some(i) = &init {
        o.check(
            "apiKeySource is none",
            i.api_key_source.as_deref() == Some("none"),
            format!("{:?}", i.api_key_source),
        );
        o.check("model as requested", i.model == MODEL, i.model.clone());
        let mut got = i.tools.clone();
        got.sort();
        o.check(
            "tool list is exactly the allowlist",
            got == ["Glob", "Grep", "Read"],
            format!("{got:?}"),
        );
        o.check(
            "no MCP servers and no error arrays",
            i.mcp_servers.is_empty() && i.mcp_server_errors == 0 && i.plugin_errors == 0,
            "",
        );
    }
    o.check("run finished with a success result", result_ok(&r), "");
    Ok(o)
}

/// A2: structured output through `--json-schema`.
pub fn a2(c: &Ctx) -> Result<Outcome, String> {
    let schema = json!({"type": "object", "additionalProperties": false, "required": ["verdict", "count"],
        "properties": {"verdict": {"type": "string", "enum": ["pass", "fail"]}, "count": {"type": "integer"}}});
    let mut j = job(c, &["Read"], "A box holds 3 red marbles and 4 blue marbles. Report verdict \"pass\" if the total is 7, otherwise \"fail\", and the total as count.", 3);
    j.json_schema = Some(schema.clone());
    let r = run_job(&j, None).map_err(|e| e.to_string())?;
    let mut o = Outcome::new();
    finish(&mut o, &r, Baseline::default());
    let so = last_result(&r)
        .and_then(|v| v.get("structured_output"))
        .cloned();
    let errs = so.as_ref().map(|v| validate(&schema, v));
    o.check("no policy violations", r.violations.is_empty(), "");
    o.check("structured_output is present", so.is_some(), "");
    o.check(
        "structured_output validates against the schema",
        errs.as_ref().is_some_and(Vec::is_empty),
        format!("{errs:?}"),
    );
    o.check(
        "value is the right answer",
        so.as_ref()
            .is_some_and(|v| v["verdict"] == "pass" && v["count"] == 7),
        "",
    );
    let assistants = r
        .events
        .iter()
        .filter(|e| matches!(e, Event::Assistant { .. }))
        .count();
    o.evidence["structured"] = json!({"assistant_messages": assistants, "num_turns": last_result(&r).map(|v| v["num_turns"].clone()),
        "note": "More than one assistant message or a retry subtype would show a repair loop."});
    Ok(o)
}

/// A3: writes inside the task folder succeed, and every other write is denied by a tool-level decision.
pub fn a3(c: &Ctx) -> Result<Outcome, String> {
    let w = &c.w;
    std::fs::write(w.t2.join("existing-sibling.txt"), "synthetic\n").map_err(|e| e.to_string())?;
    let link = w.t1.join("link-out");
    std::os::unix::fs::symlink(&w.t2, &link).map_err(|e| e.to_string())?;
    let slash_tmp = PathBuf::from(format!("/tmp/bff-m0-a3-{}.txt", std::process::id()));
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
        "For each numbered path below, use the Write tool to create a file at that exact path containing only the word probe. Do them one at a time, in order, and continue with the next path even if one is refused. When all are attempted, reply done.\n{}",
        list.join("\n")
    );
    let before = hash_tree(&w.root).map_err(|e| e.to_string())?;
    let j = job(c, &["Read", "Edit", "Write", "Glob", "Grep"], &prompt, 14);
    let r = run_job(&j, None).map_err(|e| e.to_string())?;
    let after = hash_tree(&w.root).map_err(|e| e.to_string())?;
    let changed: Vec<String> = diff(&before, &after)
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    let uses = bff_core::claude::tool_uses(&r.raw);
    let results = bff_core::claude::tool_results(&r.raw);
    let mut o = Outcome::new();
    finish(&mut o, &r, Baseline::default());
    let mut rows = Vec::new();
    for (name, path, should_write) in &targets {
        let p = path.to_string_lossy();
        let attempt = uses
            .iter()
            .find(|u| u.name == "Write" && u.path.as_deref() == Some(&*p));
        let res = attempt.and_then(|u| results.iter().find(|x| x.tool_use_id == u.id));
        let exists = path.exists();
        rows.push(json!({"target": name, "attempted": attempt.is_some(), "tool_result_is_error": res.map(|x| x.is_error),
            "exists_after": exists, "expected_write": should_write}));
        if *should_write {
            o.check(
                "in-folder write succeeds",
                exists
                    && std::fs::read_to_string(path)
                        .map(|t| t.trim() == "probe")
                        .unwrap_or(false),
                name.to_string(),
            );
        } else {
            o.check(&format!("{name}: no file created"), !exists, "");
            o.check(
                &format!("{name}: a Write attempt was made and refused"),
                attempt.is_some() && res.is_some_and(|x| x.is_error),
                "denial must be a tool result, not a claim",
            );
            if exists {
                o.outside_writes.push(name.to_string());
            }
        }
    }
    let _ = std::fs::remove_file(&slash_tmp);
    o.check("no policy violations", r.violations.is_empty(), "");
    o.evidence["probes"] = json!(rows);
    o.evidence["tree_changes"] = json!(changed);
    Ok(o)
}

/// A4: a fake MCP server and a skill inlined into the role prompt.
pub fn a4(c: &Ctx) -> Result<Outcome, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let skill = "## Skill: marker-fetch\nWhen the user asks for the marker, call the echo_marker tool of the fake server with the text abc, then reply with the tool's output verbatim and nothing else.";
    let mut j = job(c, &["Read", "mcp__fake__echo_marker"], "Get the marker.", 4);
    j.mcp = Some((
        "fake",
        exe.to_string_lossy().into_owned(),
        vec!["__fake-mcp".into()],
    ));
    j.system_prompt = skill;
    let r = run_job(&j, None).map_err(|e| e.to_string())?;
    let mut o = Outcome::new();
    finish(&mut o, &r, Baseline::default());
    let init = r.events.iter().find_map(|e| {
        if let Event::Init(i) = e {
            Some(i.clone())
        } else {
            None
        }
    });
    let want = crate::fake::marker("abc");
    let uses = bff_core::claude::tool_uses(&r.raw);
    let results = bff_core::claude::tool_results(&r.raw);
    let call = uses.iter().find(|u| u.name == "mcp__fake__echo_marker");
    let res = call.and_then(|u| results.iter().find(|x| x.tool_use_id == u.id));
    o.check(
        "no policy violations",
        r.violations.is_empty(),
        format!("{:?}", r.violations),
    );
    o.check(
        "init lists the fake server",
        init.as_ref()
            .is_some_and(|i| i.mcp_servers.iter().any(|(n, _)| n == "fake")),
        format!("{:?}", init.as_ref().map(|i| &i.mcp_servers)),
    );
    o.check(
        "init lists the MCP tool",
        init.as_ref()
            .is_some_and(|i| i.tools.iter().any(|t| t == "mcp__fake__echo_marker")),
        "",
    );
    o.check(
        "Skill, Agent, Task, Workflow absent from init",
        init.as_ref().is_some_and(|i| {
            !i.tools
                .iter()
                .any(|t| ["Skill", "Agent", "Task", "Workflow"].contains(&t.as_str()))
        }),
        "",
    );
    o.check("the MCP tool was actually called", call.is_some(), "");
    o.check(
        "the tool result carries the expected marker",
        res.is_some_and(|x| !x.is_error && x.text.contains(&want)),
        res.map(|x| crate::evidence::snippet(&x.text))
            .unwrap_or_default(),
    );
    o.evidence["marker_in_final_text"] = json!(final_text(&r).contains(&want));
    Ok(o)
}

/// A5a: a graceful SIGINT while the turn is streaming.
pub fn a5a(c: &Ctx) -> Result<Outcome, String> {
    let j = job(c, &["Read"], LONG_TASK, 2);
    let seen = Cell::new(0usize);
    let when = |e: &Event| {
        if matches!(e, Event::Stream(_)) {
            seen.set(seen.get() + 1);
        }
        seen.get() >= 6
    };
    let r = run_job(&j, Some(&when)).map_err(|e| e.to_string())?;
    let mut o = Outcome::new();
    finish(&mut o, &r, Baseline::default());
    let members = group_members(r.id.pgid);
    o.check(
        "interrupt was triggered mid-turn",
        r.interrupted_by_harness,
        "",
    );
    o.check(
        "the child ended",
        r.ended_by.is_some(),
        format!("{:?}", r.ended_by),
    );
    o.check(
        "nothing is left in the process group",
        members.is_empty(),
        format!("{members:?}"),
    );
    o.check("no policy violations", r.violations.is_empty(), "");
    o.evidence["after_interrupt"] = json!({
        "result_line_arrived": last_result(&r).is_some(),
        "result_subtype": last_result(&r).map(|v| v["subtype"].clone()),
        "events_after_interrupt_note": "late events are kept in the fixture",
        "group_members_left": members.len(),
    });
    Ok(o)
}

fn set_nonblocking(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

/// A5b: the host disconnects mid-turn. BFF closes the pipes it owns, and we watch what the child does.
pub fn a5b(c: &Ctx) -> Result<Outcome, String> {
    let j = job(c, &["Read"], LONG_TASK, 2);
    let args = crate::claude_job::args_for(&j).map_err(|e| e.to_string())?;
    let env = worker_env(&c.w.tmp, CLAUDE_ENV);
    let mut rc = spawn_raw(&claude_path(), &args, &env, &c.w.t1).map_err(|e| e.to_string())?;
    let id = rc.id.clone();
    crate::common::record_proc("claude-a5b", &id);
    {
        let stdin = rc.stdin.as_mut().ok_or("stdin")?;
        stdin
            .write_all(j.prompt.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| e.to_string())?;
    }
    rc.stdin = None;
    let (mut out, mut err) = (
        rc.stdout.take().ok_or("stdout")?,
        rc.stderr.take().ok_or("stderr")?,
    );
    set_nonblocking(out.as_raw_fd());
    set_nonblocking(err.as_raw_fd());

    let (mut buf, mut raw, mut streams) = (Vec::new(), Vec::<String>::new(), 0usize);
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut finished_early = false;
    while Instant::now() < deadline && streams < 6 {
        let mut chunk = [0u8; 8192];
        match out.read(&mut chunk) {
            Ok(0) => {
                finished_early = true;
                break;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(30))
            }
            Err(_) => break,
        }
        while let Some(p) = buf.iter().position(|b| *b == b'\n') {
            let line = String::from_utf8_lossy(&buf.drain(..=p).collect::<Vec<u8>>())
                .trim()
                .to_string();
            if let Ok(Some(Event::Stream(_))) = parse_line(&line) {
                streams += 1;
            }
            raw.push(line);
        }
        let mut sink = [0u8; 4096];
        let _ = err.read(&mut sink);
    }
    let mid_turn = streams >= 6
        && !finished_early
        && rc.child.try_wait().map(|s| s.is_none()).unwrap_or(false);
    // The host goes away: the pipes BFF owns are closed while the child is still working.
    drop(out);
    drop(err);
    let t0 = Instant::now();
    let mut exited_on_own = None;
    while t0.elapsed() < Duration::from_secs(25) {
        if let Ok(Some(status)) = rc.child.try_wait() {
            exited_on_own = Some((t0.elapsed().as_secs_f64(), format!("{status:?}")));
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut sup = Supervised {
        id: id.clone(),
        child: rc.child,
        stdin: None,
        lines: channel().1,
        timed_out: Arc::new(AtomicBool::new(false)),
    };
    let escaped = sup.escaped_descendants().len();
    let ended_by = if exited_on_own.is_none() {
        Some(format!(
            "{:?}",
            sup.cancel(
                |s| {
                    let _ = signal_leader(&s.id, Sig::Int);
                },
                Graces {
                    after_interrupt: Duration::from_secs(3),
                    after_term: Duration::from_secs(3)
                }
            )
        ))
    } else {
        None
    };
    let left = group_members(id.pgid);

    let mut ledger = ClaudeLedger::new(Baseline::default());
    for l in &raw {
        if let Ok(Some(Event::Assistant {
            message_id, usage, ..
        })) = parse_line(l)
        {
            ledger.on_assistant(&message_id, &usage);
        }
    }
    let rep = ledger.report();
    let mut o = Outcome::new();
    o.fixtures.push((
        "events.json".into(),
        Value::Array(claude_events_fixture(&raw)),
    ));
    o.check(
        "the turn was in progress when the pipes closed",
        mid_turn,
        format!("stream events seen: {streams}"),
    );
    o.check(
        "nothing is left in the process group",
        left.is_empty(),
        format!("{left:?}"),
    );
    o.evidence = json!({
        "process": {"pid": id.pid, "pgid": id.pgid},
        "stream_events_before_disconnect": streams,
        "exited_on_its_own": exited_on_own.as_ref().map(|(s, st)| json!({"seconds": s, "status": st})),
        "escalation": ended_by, "escaped_descendants": escaped, "group_members_left": left.len(),
        "streamed_usage_before_disconnect": rep.tokens.map(|t| json!({"input": t.input, "output": t.output})),
        "cost_status": format!("{:?}", rep.cost_status),
        "note": "No result line can arrive after the pipes close, so cost is unknown.",
    });
    Ok(o)
}

fn a6_state_path(c: &Ctx) -> PathBuf {
    c.dirs.state().join("a6.json")
}

/// A6a: the first run of a resume pair.
pub fn a6a(c: &Ctx) -> Result<Outcome, String> {
    let id = uuid_v4();
    let mut j = job(
        c,
        &["Read"],
        "Remember the word pelican. Reply with only: OK",
        2,
    );
    j.session = SessionKind::New(id.clone());
    let r = run_job(&j, None).map_err(|e| e.to_string())?;
    let mut o = Outcome::new();
    finish(&mut o, &r, Baseline::default());
    let (_, rep) = claude_summary(&r, Baseline::default());
    o.check("run finished with a success result", result_ok(&r), "");
    o.check("no policy violations", r.violations.is_empty(), "");
    let t = rep.tokens;
    let state = json!({"session_id": id, "cwd_run": c.w.root.file_name().map(|n| n.to_string_lossy().into_owned()),
        "cost_usd": rep.cost_usd, "tokens": t.map(|t| json!({"input": t.input, "output": t.output, "cache_creation": t.cache_creation, "cache_read": t.cache_read}))});
    std::fs::create_dir_all(c.dirs.state()).map_err(|e| e.to_string())?;
    std::fs::write(a6_state_path(c), state.to_string()).map_err(|e| e.to_string())?;
    Ok(o)
}

/// A6b: `--resume` the session from A6a, and read the totals as a cumulative baseline plus a delta.
pub fn a6b(c: &Ctx) -> Result<Outcome, String> {
    let state: Value = serde_json::from_str(
        &std::fs::read_to_string(a6_state_path(c)).map_err(|e| format!("A6a state: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let id = state["session_id"]
        .as_str()
        .ok_or("session id")?
        .to_string();
    let first_cwd = state["cwd_run"].as_str().unwrap_or("A6a").to_string();
    // Sessions are stored per working directory, so the resume must start from A6a's task folder.
    let first_t1 = c
        .dirs
        .work
        .join("runs")
        .join(&first_cwd)
        .join("proj/tasks/t1");
    let mut w2 = crate::work::make(c.dirs, "A6b").map_err(|e| e.to_string())?;
    w2.t1 = first_t1;
    let c2 = Ctx {
        dirs: c.dirs,
        run: c.run,
        w: w2,
        codex_state: c.codex_state.clone(),
    };
    let mut j = job(
        &c2,
        &["Read"],
        "What word did I ask you to remember? Reply with only that word.",
        2,
    );
    j.session = SessionKind::Resume(id.clone());
    let r = run_job(&j, None).map_err(|e| e.to_string())?;
    let base_tokens = state["tokens"].as_object().map(|t| ClaudeTokens {
        input: t["input"].as_i64().unwrap_or(0),
        output: t["output"].as_i64().unwrap_or(0),
        cache_creation: t["cache_creation"].as_i64().unwrap_or(0),
        cache_read: t["cache_read"].as_i64().unwrap_or(0),
    });
    let baseline = Baseline {
        cost_usd: state["cost_usd"].as_f64(),
        tokens: base_tokens,
    };
    let mut o = Outcome::new();
    finish(&mut o, &r, baseline.clone());
    let (_, raw_rep) = claude_summary(&r, Baseline::default());
    let init_session = r.events.iter().find_map(|e| {
        if let Event::Init(i) = e {
            Some(i.session_id.clone())
        } else {
            None
        }
    });
    o.check(
        "resume run finished with a success result",
        result_ok(&r),
        "",
    );
    o.check("no policy violations", r.violations.is_empty(), "");
    o.check(
        "init reports the resumed session id",
        init_session.as_deref() == Some(id.as_str()),
        format!("{init_session:?}"),
    );
    o.evidence["resume"] = json!({
        "first_run": state,
        "second_run_raw_totals": {"tokens": raw_rep.tokens.map(|t| json!({"input": t.input, "output": t.output, "cache_creation": t.cache_creation, "cache_read": t.cache_read})), "cost_usd_estimated": raw_rep.cost_usd},
        "note": "total_cost_usd includes earlier spend on resume. Compare raw totals with the first run to see whether tokens are cumulative too.",
        "recalled_word_in_reply_corroboration_only": final_text(&r).to_lowercase().contains("pelican"),
    });
    Ok(o)
}
