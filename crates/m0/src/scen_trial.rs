//! D1 to D3: one fixed lead decision at three model and effort settings, at Standard speed.
//! Minutes are measured, and the answer is scored against the computed optimum.

use crate::decision::{packet, schema, score};
use crate::scen::{Ctx, Outcome};
use crate::scen_codex::{begin, final_message, finish_checks, turn};
use bff_core::codex::read_only_policy;
use bff_core::schema::validate;
use serde_json::{json, Value};

pub fn trial(c: &Ctx) -> Result<Outcome, String> {
    let schema = schema();
    let mut s = begin(c, &c.w.t1)?;
    let r = turn(
        c,
        &mut s,
        &c.w.t1,
        &packet(),
        Some(&schema),
        &read_only_policy(),
        None,
    )?;
    let parsed: Option<Value> = final_message(&r.turn)
        .and_then(|i| i.text.clone())
        .and_then(|t| serde_json::from_str(&t).ok());
    let errs = parsed.as_ref().map(|v| validate(&schema, v));
    let chosen: Vec<String> = parsed
        .as_ref()
        .and_then(|v| v["chosen"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let sc = parsed.as_ref().map(|v| {
        score(
            &chosen,
            v["total_effort"].as_i64().unwrap_or(-1),
            v["total_impact"].as_i64().unwrap_or(-1),
        )
    });
    let minutes = r.turn.elapsed_ms as f64 / 60_000.0;
    let extra = json!({
        "minutes": minutes, "model": c.run.model, "effort_requested": c.run.effort, "service_tier": "default",
        "chosen": chosen,
        "score": sc.as_ref().map(|s| json!({"feasible": s.feasible, "problems": s.problems, "impact": s.impact,
            "optimum": s.optimum, "optimal": s.optimal, "totals_consistent": s.totals_consistent})),
        "note": "One run per setting is a rough signal. It cannot establish a 60 or 90 minute session.",
    });
    Ok(finish_checks(c, s, r, extra, |o, ev| {
        o.check(
            "turn completed",
            ev["status"] == "completed",
            format!("{}", ev["status"]),
        );
        o.check(
            "the answer is JSON that validates",
            errs.as_ref().is_some_and(Vec::is_empty),
            format!("{errs:?}"),
        );
        o.check(
            "the chosen set follows every rule",
            sc.as_ref().is_some_and(|s| s.feasible),
            format!("{:?}", sc.as_ref().map(|s| &s.problems)),
        );
        o.check("inside the external timeout", ev["timed_out"] == false, "");
    }))
}
