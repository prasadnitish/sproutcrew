//! `m0 run <ID>`: the only path to a live generation. Refuses anything outside the nineteen, checks the
//! stop rules before dispatch, marks the run used before it starts, and records usage before and after.

use crate::common::{claude_path, now, set_proc_log, version_output, write_fixture, Dirs, CODEX};
use crate::ledger::{Ledger, Status};
use crate::pins::{version_of, PinCheck};
use crate::runs::Run;
use crate::scen::{Ctx, Outcome};
use crate::telemetry::{preflight, Preflight};
use crate::{scen_claude, scen_codex, scen_peer, scen_trial, work};
use bff_core::stop::Stopper;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

type Scenario = fn(&Ctx) -> Result<Outcome, String>;

fn scenario_for(id: &str) -> Option<Scenario> {
    Some(match id {
        "A1" => scen_claude::a1,
        "A2" | "A2r" => scen_claude::a2,
        "A3" => scen_claude::a3,
        "A4" => scen_claude::a4,
        "A5a" => scen_claude::a5a,
        "A5b" => scen_claude::a5b,
        "A6a" => scen_claude::a6a,
        "A6b" => scen_claude::a6b,
        "B1" => scen_codex::b1,
        "B2" => scen_codex::b2,
        "B3" => scen_codex::b3,
        "B4" => scen_codex::b4,
        "B5" => scen_codex::b5,
        "C1a" => scen_peer::c1a,
        "C1b" => scen_peer::c1b,
        "C1c" => scen_peer::c1c,
        "D1" | "D2" | "D3" => scen_trial::trial,
        _ => return None,
    })
}

pub fn pins() -> PinCheck {
    PinCheck {
        claude: version_output(&claude_path())
            .as_deref()
            .and_then(version_of),
        codex: version_output(Path::new(CODEX))
            .as_deref()
            .and_then(version_of),
    }
}

/// Counts of other clients that could be drawing on either account, by name only.
pub fn clients_snapshot() -> Value {
    let out = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "comm="])
        .env_clear()
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let count = |f: &dyn Fn(&str) -> bool| out.lines().filter(|l| f(l)).count();
    let base = |l: &str| l.rsplit('/').next().unwrap_or(l).to_string();
    json!({
        "chatgpt_codex_app_processes": count(&|l| l.contains("/ChatGPT.app/")),
        "codexbar": count(&|l| l.contains("/CodexBar.app/")),
        "claude_desktop_processes": count(&|l| l.contains("/Claude.app/")),
        "claude_cli_processes": count(&|l| base(l) == "claude"),
        "codex_cli_processes_outside_chatgpt_app": count(&|l| base(l) == "codex" && !l.contains("/ChatGPT.app/")),
    })
}

fn delta_claude(before: &Preflight, after: &Preflight) -> Value {
    json!(before.claude.windows.iter().filter_map(|b| {
        let a = after.claude.windows.iter().find(|w| w.label == b.label)?;
        Some(json!({"window": b.label, "before": b.percent, "after": a.percent, "delta": a.percent - b.percent}))
    }).collect::<Vec<_>>())
}

fn status_of(o: &Outcome, stopper: &Stopper) -> Status {
    if stopper.reason().is_some() {
        Status::Stopped
    } else if o.all_ok() {
        Status::Done
    } else {
        Status::Failed
    }
}

pub fn execute(dirs: &Dirs, id: &str, clients_checked: bool) -> Result<Value, String> {
    let mut ledger = Ledger::load(&dirs.ledger_path()).map_err(|e| e.to_string())?;
    let pin_check = pins();
    let run: &'static Run = ledger
        .gate(id, pin_check.ok())
        .map_err(|r| format!("refused: {r:?}"))?;
    let scenario = scenario_for(id).ok_or_else(|| {
        format!("{id}: no scenario is built yet. Nothing was dispatched and the run is not used.")
    })?;
    if ledger.entries().is_empty() && !clients_checked {
        return Err("the first dispatch needs --clients-checked, after the other clients were closed or accepted".into());
    }

    let mut stopper = Stopper::new();
    let before = preflight(dirs, &mut stopper)
        .map_err(|e| format!("pre-dispatch telemetry unavailable, nothing dispatched: {e}"))?;
    if let Some(r) = stopper.reason() {
        let msg = format!("{r:?}");
        ledger.latch_stop(&msg);
        ledger
            .save(&dirs.ledger_path())
            .map_err(|e| e.to_string())?;
        return Err(format!(
            "stop condition before dispatch, nothing dispatched: {msg}"
        ));
    }
    let clients = clients_snapshot();

    let w = work::make(dirs, id).map_err(|e| e.to_string())?;
    set_proc_log(Some(dirs.state().join(id).join("proc.jsonl")));
    ledger.begin(id, now());
    ledger
        .save(&dirs.ledger_path())
        .map_err(|e| e.to_string())?;

    let started = Instant::now();
    let ctx = Ctx {
        dirs,
        run,
        w,
        codex_state: before.codex.state.clone(),
    };
    let result = scenario(&ctx);
    let elapsed = started.elapsed().as_secs_f64();
    set_proc_log(None);

    let after = preflight(dirs, &mut stopper);
    let (mut outcome, error) = match result {
        Ok(o) => (o, None),
        Err(e) => (Outcome::new(), Some(e)),
    };
    outcome.check(
        "scenario ran to completion",
        error.is_none(),
        error.clone().unwrap_or_default(),
    );
    if let Some(r) = outcome.stop.take() {
        stopper.trip(r);
    }
    stopper.check_outside_write(&outcome.outside_writes);
    if let Some(f) = &outcome.route_failure {
        stopper.route_failed(f);
    }
    if outcome.timed_out {
        stopper.check_timeout(id, run.timeout_secs);
    }
    let status = status_of(&outcome, &stopper);
    let note = match stopper.reason() {
        Some(r) => format!("STOPPED: {r:?}"),
        None => outcome.failures().join("; "),
    };
    if let Some(r) = stopper.reason() {
        ledger.latch_stop(&format!("{r:?}"));
    }
    ledger.finish(id, status, &note, now());
    ledger
        .save(&dirs.ledger_path())
        .map_err(|e| e.to_string())?;

    let (post_claude, post_codex, deltas) = match &after {
        Ok(a) => (
            a.claude_evidence.clone(),
            a.codex.evidence.clone(),
            delta_claude(&before, a),
        ),
        Err(e) => (
            json!({"unavailable": e}),
            json!({"unavailable": e}),
            Value::Null,
        ),
    };
    let summary = json!({
        "id": id, "route": format!("{:?}", run.route), "model": run.model, "effort": run.effort, "what": run.what,
        "status": format!("{status:?}"), "stop": stopper.reason().map(|r| format!("{r:?}")),
        "elapsed_seconds": elapsed, "started_at": ledger.entries().iter().find(|e| e.id == id).and_then(|e| e.started_at),
        "pins": {"claude": pin_check.claude, "codex": pin_check.codex},
        "other_clients": clients,
        "checks": outcome.checks.iter().map(|(n, ok, d)| json!({"check": n, "passed": ok, "detail": d})).collect::<Vec<_>>(),
        "usage_before": {"claude": before.claude_evidence, "codex": before.codex.evidence},
        "usage_after": {"claude": post_claude, "codex": post_codex},
        "claude_window_deltas": deltas,
        "evidence": outcome.evidence,
    });
    write_fixture(dirs, &format!("runs/{id}/summary.json"), &summary).map_err(|e| e.to_string())?;
    for (name, v) in &outcome.fixtures {
        write_fixture(dirs, &format!("runs/{id}/{name}"), v).map_err(|e| e.to_string())?;
    }
    if let Err(e) = after {
        return Err(format!(
            "{id} ran ({status:?}) but the post-run reading failed: {e}"
        ));
    }
    Ok(summary)
}

/// Marks a run skipped with a reason. A skipped run is never replaced.
pub fn skip(dirs: &Dirs, id: &str, reason: &str) -> Result<(), String> {
    let mut l = Ledger::load(&dirs.ledger_path()).map_err(|e| e.to_string())?;
    l.skip(id, reason, now())
        .map_err(|r| format!("refused: {r:?}"))?;
    l.save(&dirs.ledger_path()).map_err(|e| e.to_string())
}

/// Clears a latched stop. Only for use when Nitish has said so, and his words are recorded.
pub fn clear_stop(dirs: &Dirs, authorization: &str) -> Result<String, String> {
    let mut l = Ledger::load(&dirs.ledger_path()).map_err(|e| e.to_string())?;
    let reason = l
        .clear_stop(authorization, now())
        .map_err(|r| format!("refused: {r:?}"))?;
    l.save(&dirs.ledger_path()).map_err(|e| e.to_string())?;
    Ok(reason)
}
