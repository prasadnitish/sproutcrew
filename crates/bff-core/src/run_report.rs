//! Deterministic, model-free run report. Scan all text before writing Markdown or invoking Typst.
use crate::{flow::Studio, meters::Meter};
use serde_json::{json, Value};
use std::path::Path;

fn block(value: &Value) -> String {
    // Longer fence than any fence in the serialized data, so evidence cannot escape its quoted block.
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
    let fence = "`".repeat(
        text.split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1,
    );
    format!("{fence}json\n{text}\n{fence}\n\n")
}

pub fn build(studio: &Studio, now: u64) -> Result<String, String> {
    let events = studio.journal.events()?;
    let approval = events.iter().rev().find(|e| e.kind == "plan.approved");
    let mut out = format!("# Run report\n\nBuilt {} UTC from local records. No model generated this report. Missing historical data is unknown.\n\n## Approved plan and hash\n\n", crate::changelog::utc(now));
    match approval {
        Some(a) => {
            out.push_str(&block(&json!({"hash": a.payload["hash"], "approved_by": a.payload["by"], "at": a.at, "plan": a.payload["plan"], "current_gate": format!("{:?}", studio.gate())})));
            if a.payload["plan"].is_null() {
                out.push_str("Historical approval predates plan snapshots: its exact approved plan is unknown.\n\n");
            }
        }
        None => out.push_str("No approved plan.\n\n"),
    }
    out.push_str("## Policy matrix in force\n\n");
    let approved_policy = approval
        .map(|a| a.payload["policy"].clone())
        .unwrap_or(Value::Null);
    out.push_str(&block(&json!({"approved_policy": approved_policy, "current_policy": studio.policy()?, "current_scope": studio.approval_hash()?, "note": "Current scope can differ from the last approved snapshot; approval must be valid before dispatch."})));
    out.push_str("## Approvals and triggers\n\n");
    let approvals: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "plan.approved" || e.kind.starts_with("policy."))
        .map(|e| json!({"event": e.id, "at": e.at, "kind": e.kind, "record": e.payload}))
        .collect();
    out.push_str(&block(&json!(approvals)));
    out.push_str("## Execution attempts and outcomes\n\n");
    out.push_str("Dispatch records show attempts, not successful outputs. Failed or interrupted attempts remain in this history even after a later retry succeeds.\n\n");
    out.push_str(&block(&json!(events
        .iter()
        .filter(|e| matches!(
            e.kind.as_str(),
            "worker.dispatched" | "worker.finished" | "worker.failed"
        ))
        .map(|e| json!({"event": e.id, "at": e.at, "kind": e.kind, "record": e.payload}))
        .collect::<Vec<_>>())));
    out.push_str("## Peer-review findings and dispositions\n\n");
    out.push_str("Accepted findings remain findings until a revision addresses them. Rejections retain reasons. Invalidated reviews are historical. Escalations are human decisions, not clean reviews.\n\n");
    out.push_str(&block(&crate::peer::Peer::open(studio)?.summaries()));
    let dispositions: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind.as_str(),
                "review.decided" | "decision.escalated" | "decision.resolved"
            )
        })
        .map(|e| json!({"kind": e.kind, "at": e.at, "record": e.payload}))
        .collect();
    out.push_str(&block(&json!(dispositions)));
    out.push_str("## Decision log\n\n");
    out.push_str(&crate::changelog::build(studio, now)?);
    out.push_str("\n## Artifacts and base hashes\n\n");
    out.push_str(&block(&json!({"artifacts": studio.project.artifacts()?, "version_history": events.iter().filter(|e| matches!(e.kind.as_str(), "artifact.registered" | "artifact.changed" | "worker.dispatched")).map(|e| json!({"kind": e.kind, "record": e.payload})).collect::<Vec<_>>(), "missing_base": "unknown for initial artifacts or historical records without an exact base"})));
    out.push_str("### Code diff history\n\n");
    out.push_str(&block(&json!(events
        .iter()
        .filter(|e| e.kind.starts_with("code."))
        .map(|e| json!({"kind": e.kind, "record": e.payload}))
        .collect::<Vec<_>>())));
    out.push_str("## Three-meter cost summary\n\n");
    let ledger = studio.ledger()?;
    for (name, meter) in [
        ("API dollars", Meter::ApiDollars),
        ("Plan-window %", Meter::PlanWindow),
        ("Tokens", Meter::Tokens),
    ] {
        out.push_str(&format!("- {name}: {}\n", ledger.display(meter)));
    }
    out.push_str("\nMeters are never summed. Per-entry labels below are reported / estimated / reconciled / unknown. No rows means unknown, not zero. Token-only routes do not promise a dollar cap.\n\n");
    out.push_str(&block(&json!(events
        .iter()
        .filter(|e| e.kind == "usage")
        .map(|e| &e.payload)
        .collect::<Vec<_>>())));
    Ok(out)
}

pub fn checked(studio: &Studio, now: u64) -> Result<String, String> {
    let report = build(studio, now)?;
    if crate::secrets::has_secret(&crate::secrets::scan(&report)) {
        return Err("Run report blocked: a possible secret was found in its text".into());
    }
    // An artifact excluded from the prose still cannot leave with an unscanned report/package.
    for a in studio.project.artifacts()? {
        let file = a["file"].as_str().ok_or("Invalid artifact record")?;
        crate::policy::confined(&studio.project.root.join("artifacts"), file)?;
        let text = std::fs::read_to_string(studio.project.root.join("artifacts").join(file))
            .map_err(|e| e.to_string())?;
        if crate::secrets::has_secret(&crate::secrets::scan(&text)) {
            return Err("Run report blocked: a possible secret was found in an artifact".into());
        }
    }
    Ok(report)
}

/// Stage then rename as one directory. Typst sees only escaped, already-scanned local text.
pub fn export(
    studio: &Studio,
    out: &Path,
    tools: &crate::clientfiles::Tools,
    now: u64,
) -> Result<Value, String> {
    let report = checked(studio, now)?;
    if out.exists() {
        return Err("This run report already exists".into());
    }
    let parent = out.parent().ok_or("Missing export parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = parent.join(format!(".run-report-{}-{now}", std::process::id()));
    std::fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        std::fs::write(stage.join("Run report.md"), &report).map_err(|e| e.to_string())?;
        let pdf = stage.join("Run report.pdf");
        let mut notes = vec![];
        let made_pdf = match &tools.typst {
            Some(typst) => match crate::typst::render(
                typst,
                &crate::typst::markdown_to_typst(&report),
                &stage.join(".typst"),
                &pdf,
            ) {
                Ok(()) => true,
                Err(e) => {
                    notes.push(format!("PDF was not made: {e}"));
                    false
                }
            },
            None => {
                notes.push("PDF was not made: Typst is unavailable".into());
                false
            }
        };
        if !made_pdf && pdf.exists() {
            std::fs::remove_file(&pdf).map_err(|e| e.to_string())?;
        }
        let scratch = stage.join(".typst");
        if scratch.exists() {
            std::fs::remove_dir_all(scratch).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&stage, out).map_err(|e| e.to_string())?;
        Ok(
            json!({"folder": out.display().to_string(), "markdown": out.join("Run report.md").display().to_string(), "pdf": made_pdf.then(|| out.join("Run report.pdf").display().to_string()), "notes": notes}),
        )
    })();
    if stage.exists() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}
