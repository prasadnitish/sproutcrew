//! C1: Codex writes, Claude reviews, Codex applies one finding. `gpt-6.1-sol` at high and
//! `claude-sonnet-5-5` at high, on a short synthetic brief with one seeded error.

use crate::claude_job::{run_job, Job, SessionKind};
use crate::common::sha256_hex;
use crate::evidence::{claude_events_fixture, claude_summary, last_result};
use crate::peer::{
    applier_prompt, first_finding, line_diff, review_schema, reviewer_prompt, seeded_error_state,
    BRIEF, WRITER_PROMPT,
};
use crate::scen::{Ctx, Outcome};
use crate::scen_codex::{begin, finish_checks, turn};
use crate::work::uuid_v4;
use bff_core::codex::writer_policy;
use bff_core::ledger::Baseline;
use bff_core::schema::validate;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The exchange folder is C1a's task folder, so the artifact is one file throughout.
fn exchange_dir(c: &Ctx) -> PathBuf {
    c.dirs.work.join("runs").join("C1a").join("proj/tasks/t1")
}

fn state_path(c: &Ctx) -> PathBuf {
    c.dirs.state().join("c1.json")
}

fn load_state(c: &Ctx) -> Result<Value, String> {
    serde_json::from_str(
        &std::fs::read_to_string(state_path(c)).map_err(|e| format!("C1 state: {e}"))?,
    )
    .map_err(|e| e.to_string())
}

fn save_state(c: &Ctx, v: &Value) -> Result<(), String> {
    std::fs::create_dir_all(c.dirs.state()).map_err(|e| e.to_string())?;
    std::fs::write(state_path(c), v.to_string()).map_err(|e| e.to_string())
}

fn read_artifact(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("artifact.md")).ok()
}

fn has_headings(text: &str) -> bool {
    ["Scope", "Capacity", "Risks", "Decision"]
        .iter()
        .all(|h| text.lines().any(|l| l.starts_with('#') && l.contains(h)))
}

/// C1a: Codex writes the artifact from the brief.
pub fn c1a(c: &Ctx) -> Result<Outcome, String> {
    let dir = exchange_dir(c);
    std::fs::write(dir.join("brief.md"), BRIEF).map_err(|e| e.to_string())?;
    let mut s = begin(c, &dir)?;
    let r = turn(
        c,
        &mut s,
        &dir,
        WRITER_PROMPT,
        None,
        &writer_policy(&dir),
        None,
    )?;
    let artifact = read_artifact(&dir);
    let sha = artifact.as_deref().map(|t| sha256_hex(t.as_bytes()));
    let (has25, has20) = artifact
        .as_deref()
        .map(seeded_error_state)
        .unwrap_or((false, false));
    save_state(
        c,
        &json!({"brief_sha256": sha256_hex(BRIEF.as_bytes()), "artifact_a_sha256": sha, "artifact_a": artifact}),
    )?;
    Ok(finish_checks(
        c,
        s,
        r,
        json!({"brief_sha256": sha256_hex(BRIEF.as_bytes()), "artifact_sha256": sha, "artifact_chars": artifact.as_ref().map(|t| t.chars().count()),
               "headings_present": artifact.as_deref().map(has_headings), "seeded_error_present": has25, "corrected_figure_present": has20}),
        |o, ev| {
            o.check(
                "turn completed",
                ev["status"] == "completed",
                format!("{}", ev["status"]),
            );
            o.check("artifact.md was written", artifact.is_some(), "");
            o.check(
                "the four headings are present",
                artifact.as_deref().is_some_and(has_headings),
                "",
            );
            o.check(
                "the seeded error carried into the artifact (precondition of the seeded case)",
                has25 && !has20,
                format!("25: {has25}, 20: {has20}"),
            );
        },
    ))
}

/// C1b: Claude reviews the exact artifact version, with structured findings and the hash echoed back.
pub fn c1b(c: &Ctx) -> Result<Outcome, String> {
    let dir = exchange_dir(c);
    let mut state = load_state(c)?;
    let before = read_artifact(&dir).ok_or("artifact.md is missing")?;
    let sha = sha256_hex(before.as_bytes());
    let mut w = c.w.clone();
    w.t1 = dir.clone();
    let schema = review_schema();
    let job = Job {
        w: &w,
        tools: vec!["Read", "Glob", "Grep"],
        mcp: None,
        system_prompt: "You are a careful reviewer of short plans. Report only real factual or arithmetic errors.",
        json_schema: Some(schema.clone()),
        max_turns: 8,
        model: c.run.model,
        effort: c.run.effort,
        session: SessionKind::New(uuid_v4()),
        stream_input: false,
        prompt: reviewer_prompt(&sha),
        timeout: Duration::from_secs(c.run.timeout_secs),
    };
    let r = run_job(&job, None).map_err(|e| e.to_string())?;
    let mut o = Outcome::new();
    let (sum, _) = claude_summary(&r, Baseline::default());
    o.evidence = sum;
    o.fixtures.push((
        "events.json".into(),
        Value::Array(claude_events_fixture(&r.raw)),
    ));
    if !r.violations.is_empty() {
        o.route_failure = Some(format!("{:?}", r.violations));
    }
    o.timed_out = r.timed_out;
    let review = last_result(&r)
        .and_then(|v| v.get("structured_output"))
        .cloned();
    let errs = review.as_ref().map(|v| validate(&schema, v));
    let after = read_artifact(&dir).unwrap_or_default();
    let finding = review.as_ref().and_then(first_finding);
    let finds_seeded = review
        .as_ref()
        .and_then(|v| v["findings"].as_array())
        .is_some_and(|a| a.iter().any(|f| f.to_string().contains("25")));
    o.check(
        "no policy violations",
        r.violations.is_empty(),
        format!("{:?}", r.violations),
    );
    o.check(
        "run finished with a success result",
        last_result(&r).is_some_and(|v| v["subtype"] == "success"),
        "",
    );
    o.check(
        "the review validates against the schema",
        errs.as_ref().is_some_and(Vec::is_empty),
        format!("{errs:?}"),
    );
    o.check(
        "the review names the version it reviewed",
        review
            .as_ref()
            .is_some_and(|v| v["reviewed_sha256"] == json!(sha)),
        "reviewed_sha256 must equal the artifact hash",
    );
    o.check(
        "the artifact is unchanged by the review",
        sha256_hex(after.as_bytes()) == sha,
        "",
    );
    o.check(
        "the seeded error was found (mechanism case, not a quality claim)",
        finds_seeded,
        format!(
            "findings: {}",
            review
                .as_ref()
                .map_or(0, |v| v["findings"].as_array().map_or(0, Vec::len))
        ),
    );
    o.evidence["review"] = json!({"artifact_sha256": sha, "reviewed_sha256": review.as_ref().map(|v| v["reviewed_sha256"].clone()),
        "finding_count": review.as_ref().and_then(|v| v["findings"].as_array()).map(Vec::len), "first_finding_id": finding.as_ref().map(|f| f.0.clone())});
    state["review"] = review.unwrap_or(Value::Null);
    state["artifact_b_sha256"] = json!(sha);
    save_state(c, &state)?;
    Ok(o)
}

/// C1c: Codex applies exactly one finding, and the result is judged by a deterministic check.
pub fn c1c(c: &Ctx) -> Result<Outcome, String> {
    let dir = exchange_dir(c);
    let state = load_state(c)?;
    let base = read_artifact(&dir).ok_or("artifact.md is missing")?;
    let base_sha = sha256_hex(base.as_bytes());
    let finding = first_finding(&state["review"]);
    let prompt = match &finding {
        Some((id, quote, problem, fix)) => applier_prompt(id, quote, problem, fix),
        None => "There are no findings to apply. Reply exactly: no change needed. Do not modify any file.".to_string(),
    };
    let mut s = begin(c, &dir)?;
    let r = turn(c, &mut s, &dir, &prompt, None, &writer_policy(&dir), None)?;
    let after = read_artifact(&dir).unwrap_or_default();
    let after_sha = sha256_hex(after.as_bytes());
    let d = line_diff(&base, &after);
    let (has25, has20) = seeded_error_state(&after);
    let provenance = json!({
        "brief_sha256": state["brief_sha256"], "artifact_a_sha256": state["artifact_a_sha256"],
        "reviewed_sha256": state["review"]["reviewed_sha256"], "base_sha256_at_apply": base_sha,
        "finding_id": finding.as_ref().map(|f| f.0.clone()), "artifact_c_sha256": after_sha,
        "diff": {"changed_lines": d.changed, "removed": d.removed.iter().map(|l| crate::evidence::snippet(l)).collect::<Vec<_>>(),
                 "added": d.added.iter().map(|l| crate::evidence::snippet(l)).collect::<Vec<_>>()},
        "seeded_error_remaining": has25, "corrected_figure_present": has20,
    });
    Ok(finish_checks(c, s, r, provenance, |o, ev| {
        o.check(
            "turn completed",
            ev["status"] == "completed",
            format!("{}", ev["status"]),
        );
        o.check(
            "the reviewed version is the version that was edited",
            state["review"]["reviewed_sha256"] == json!(base_sha),
            "provenance chain: reviewed hash equals the base hash",
        );
        if finding.is_some() {
            o.check("the artifact changed", after_sha != base_sha, "");
            o.check(
                "only the finding's line changed",
                (1..=2).contains(&d.changed),
                format!("{} lines changed", d.changed),
            );
            o.check(
                "the seeded error is gone and the corrected figure is present",
                !has25 && has20,
                format!("25: {has25}, 20: {has20}"),
            );
        } else {
            o.check(
                "a clean review leaves the artifact unchanged",
                after_sha == base_sha,
                "",
            );
        }
    }))
}
