//! Deterministic, model-free run reports. Scan all text before writing Markdown or invoking Typst.
//! The report people read is the summary: steps, models, cost, the second check, and what is still open. The raw
//! records, as JSON, are a separate export for anyone who wants to audit them.
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

/// The raw records: the approved plan and hash, the policy, every attempt, and every cost entry, as JSON.
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

/// Refuses text, or any step's result, that holds something shaped like a key.
fn guard(studio: &Studio, report: &str) -> Result<(), String> {
    if crate::secrets::has_secret(&crate::secrets::scan_export(report)) {
        return Err("Run report blocked: a possible secret was found in its text".into());
    }
    // An artifact excluded from the prose still cannot leave with an unscanned report/package.
    for a in studio.project.artifacts()? {
        let file = a["file"].as_str().ok_or("Invalid artifact record")?;
        crate::policy::confined(&studio.project.root.join("artifacts"), file)?;
        let text = std::fs::read_to_string(studio.project.root.join("artifacts").join(file))
            .map_err(|e| e.to_string())?;
        if crate::secrets::has_secret(&crate::secrets::scan_export(&text)) {
            return Err("Run report blocked: a possible secret was found in an artifact".into());
        }
    }
    Ok(())
}

/// The raw records, checked for secrets.
pub fn checked(studio: &Studio, now: u64) -> Result<String, String> {
    let report = build(studio, now)?;
    guard(studio, &report)?;
    Ok(report)
}

/// The model a step uses: its own choice, or its role's.
fn model_of(plan: &crate::plan::Plan, st: &crate::plan::Stage) -> String {
    match &st.uses {
        Some(u) => format!("{}, effort {}", u.model, u.effort),
        None => plan
            .team
            .iter()
            .find(|a| a.role == st.owner_role)
            .map_or("unknown".into(), |a| {
                format!("{}, effort {}", a.model, a.effort)
            }),
    }
}

/// The report to read: what was made, by which models, what it cost, what the second check said, and what is still
/// open. Built from the project's records with no model.
pub fn summary(studio: &Studio, now: u64) -> Result<String, String> {
    use crate::verdict::Verdict;
    let events = studio.journal.events()?;
    let approval = events.iter().rev().find(|e| e.kind == "plan.approved");
    let artifacts = studio.project.artifacts()?;
    let failed = studio.failed_stages()?;
    let project = studio
        .project
        .root
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let mut out = format!(
        "# Run report\n\n{project}. Built {} UTC from this project's records. No model wrote this report. What is not known is said to be unknown, not zero.\n\n",
        crate::changelog::utc(now)
    );
    let Some(plan) = studio.plan() else {
        out.push_str("## Summary\n\nThere is no plan yet.\n");
        return Ok(out);
    };
    let has_result = |id: &str| {
        artifacts
            .iter()
            .any(|a| a["name"].as_str() == Some(&crate::app::artifact_name(id)))
    };
    let done = plan.stages.iter().filter(|st| has_result(&st.id)).count();
    out.push_str("## Summary\n\n");
    out.push_str(&match approval {
        Some(a) => format!(
            "- Plan approved by {} on {} UTC.\n",
            a.payload["by"].as_str().unwrap_or("unknown"),
            crate::changelog::utc(a.at)
        ),
        None => "- The plan has not been approved.\n".to_string(),
    });
    out.push_str(&format!(
        "- {done} of {} steps have a result.\n",
        plan.stages.len()
    ));
    let failing = plan
        .stages
        .iter()
        .filter(|st| !has_result(&st.id) && failed.iter().any(|(f, _)| *f == st.id))
        .count();
    if failing > 0 {
        out.push_str(&format!(
            "- {failing} step{} failed and has no result.\n",
            if failing == 1 { "" } else { "s" }
        ));
    }
    let ledger = studio.ledger()?;
    out.push_str(&format!(
        "- Cost: {} in API dollars. See Cost below.\n\n",
        ledger.display(Meter::ApiDollars)
    ));

    out.push_str("## Steps\n\n");
    for (n, st) in plan.stages.iter().enumerate() {
        let state = if has_result(&st.id) {
            "done".to_string()
        } else if let Some((_, why)) = failed.iter().find(|(f, _)| *f == st.id) {
            format!("failed: {why}")
        } else {
            "not run".to_string()
        };
        out.push_str(&format!(
            "{}. {}: {state}. Model {}.\n",
            n + 1,
            st.title,
            model_of(plan, st)
        ));
    }

    out.push_str("\n## Second check\n\n");
    let mut checks = 0;
    for st in plan
        .stages
        .iter()
        .filter(|st| crate::verdict::asks_for_verdict(&st.guide))
    {
        let name = crate::app::artifact_name(&st.id);
        let Some(file) = artifacts
            .iter()
            .rev()
            .find(|a| a["name"].as_str() == Some(&name))
            .and_then(|a| a["file"].as_str())
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(studio.project.root.join("artifacts").join(file))
        else {
            continue;
        };
        checks += 1;
        let check = crate::verdict::parse(&text);
        out.push_str(&format!(
            "- {}: {}.\n",
            st.title,
            match check.verdict {
                Some(Verdict::Ready) => "Ready",
                Some(Verdict::FixFirst) => "Fix first",
                Some(Verdict::DoNotSend) => "Do not send",
                None => "No verdict",
            }
        ));
        for f in check.flagged {
            out.push_str(&format!("  - {f}\n"));
        }
    }
    if checks == 0 {
        out.push_str("No second check has a result.\n");
    }

    out.push_str("\n## Still open\n\n");
    let details = crate::app::finish_details(studio)?;
    if details.gaps.is_empty() && details.left_out.is_empty() {
        out.push_str("Nothing is marked as missing from the finished result.\n");
    } else {
        for g in &details.gaps {
            out.push_str(&format!("- {}\n", g.what));
        }
        if !details.left_out.is_empty() {
            out.push_str(&format!(
                "\nThe piece was finished with {} left out. They are not in the text or the files:\n\n",
                match details.left_out.len() {
                    1 => "1 detail".to_string(),
                    n => format!("{n} details"),
                }
            ));
            for g in &details.left_out {
                out.push_str(&format!("- {}\n", g.what));
            }
        }
    }

    out.push_str("\n## Cost\n\n");
    for (name, meter) in [
        ("API dollars", Meter::ApiDollars),
        ("Plan-window %", Meter::PlanWindow),
        ("Tokens", Meter::Tokens),
    ] {
        out.push_str(&format!("- {name}: {}\n", ledger.display(meter)));
    }
    out.push_str("\nThe three meters are never added together. Unknown is not zero. The raw records are in the separate raw data export.\n");
    Ok(out)
}

/// The summary, checked for secrets.
pub fn checked_summary(studio: &Studio, now: u64) -> Result<String, String> {
    let report = summary(studio, now)?;
    guard(studio, &report)?;
    Ok(report)
}

/// Stage then rename as one directory. Typst sees only escaped, already-scanned local text. The summary is the
/// main report: `Run report.md` and, with Typst, `Run report.pdf`.
pub fn export(
    studio: &Studio,
    out: &Path,
    tools: &crate::clientfiles::Tools,
    now: u64,
) -> Result<Value, String> {
    let report = checked_summary(studio, now)?;
    stage_and_rename(out, now, |stage| {
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
        Ok(
            json!({"folder": out.display().to_string(), "markdown": out.join("Run report.md").display().to_string(), "pdf": made_pdf.then(|| out.join("Run report.pdf").display().to_string()), "notes": notes}),
        )
    })
}

/// The advanced export: the raw records as `Run report (raw data).md`, for anyone who wants to audit them.
pub fn export_raw(studio: &Studio, out: &Path, now: u64) -> Result<Value, String> {
    let report = checked(studio, now)?;
    stage_and_rename(out, now, |stage| {
        std::fs::write(stage.join("Run report (raw data).md"), &report)
            .map_err(|e| e.to_string())?;
        Ok(
            json!({"folder": out.display().to_string(), "markdown": out.join("Run report (raw data).md").display().to_string(), "pdf": null, "notes": []}),
        )
    })
}

/// Builds a folder in a hidden stage beside `out` and renames it into place, so a failure leaves nothing behind and
/// an existing folder is never touched.
fn stage_and_rename(
    out: &Path,
    now: u64,
    fill: impl FnOnce(&Path) -> Result<Value, String>,
) -> Result<Value, String> {
    if out.exists() {
        return Err("This run report already exists".into());
    }
    let parent = out.parent().ok_or("Missing export parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = parent.join(format!(".run-report-{}-{now}", std::process::id()));
    std::fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = fill(&stage).and_then(|made| {
        std::fs::rename(&stage, out).map_err(|e| e.to_string())?;
        Ok(made)
    });
    if stage.exists() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}
