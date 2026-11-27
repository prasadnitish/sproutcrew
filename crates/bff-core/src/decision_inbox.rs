//! Cross-project projection. It cannot approve anything; rows lead back to the owning workflow.
use crate::{flow::Studio, plan::Gate, policy::Tier};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub project: String,
    pub stage: String,
    pub id: String,
    pub tier: Tier,
    pub kind: String,
    pub title: String,
    pub anchor: String,
    #[serde(default)]
    pub revision: String,
    /// The step's title, so a card names the step in words and not by its id.
    #[serde(default)]
    pub step_title: String,
}

pub fn collect(project: &str, studio: &Studio) -> Result<Vec<Item>, String> {
    let Some(plan) = studio.plan() else {
        return Ok(vec![]);
    };
    let first = plan
        .stages
        .first()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let mut rows = vec![];
    // A plan is listed for approval only while some step has not run. A finished project has nothing left to approve,
    // so a change in its scope (a settings file, a late source) does not bring it back.
    let artifacts = studio.project.artifacts().unwrap_or_default();
    let unfinished = plan.stages.iter().any(|st| {
        !artifacts
            .iter()
            .any(|a| a["name"].as_str() == Some(st.id.as_str()))
    });
    if unfinished && studio.gate() != Gate::Allowed {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: first.clone(),
            id: "plan".into(),
            tier: Tier::Consequential,
            kind: "plan".into(),
            title: "Review and approve the current plan and permissions".into(),
            anchor: "plan-approval".into(),
        });
    }
    for p in crate::policy::pending(&studio.journal, &studio.approval_hash()?)? {
        let stage = p["action"]["stage"].as_str().unwrap_or_default();
        let id = p["id"].as_str().unwrap_or_default();
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: stage.into(),
            id: id.into(),
            tier: serde_json::from_value(p["tier"].clone()).map_err(|e| e.to_string())?,
            kind: "action".into(),
            title: format!(
                "{} → {}",
                p["action"]["tool"].as_str().unwrap_or("action"),
                p["action"]["target"].as_str().unwrap_or("target")
            ),
            anchor: format!("decision-{id}"),
        });
    }
    let summaries = crate::peer::Peer::open(studio)?.summaries();
    if let Some(artifacts) = summaries.as_object() {
        for (name, review) in artifacts {
            // Findings from invalidated versions are history, not decisions on the current work.
            let valid = review["reviewers"]
                .as_object()
                .is_some_and(|m| m.values().any(|v| v == "valid"));
            if !valid {
                continue;
            }
            let stage = plan
                .stages
                .iter()
                .find(|s| crate::app::artifact_name(&s.id) == *name)
                .map(|s| s.id.clone())
                .unwrap_or(first.clone());
            if let Some(findings) = review["findings"].as_array() {
                for f in findings.iter().filter(|f| f["decision"].is_null()) {
                    let id = f["id"].as_str().ok_or("Finding is missing its id")?;
                    rows.push(Item {
                        revision: String::new(),
                        step_title: String::new(),
                        project: project.into(),
                        stage: stage.clone(),
                        id: format!("{name}:{id}"),
                        tier: if f["escalated"] == true {
                            Tier::Consequential
                        } else {
                            Tier::Reversible
                        },
                        kind: if f["escalated"] == true {
                            "escalation".into()
                        } else {
                            "finding".into()
                        },
                        title: f["claim"].as_str().unwrap_or("Review finding").into(),
                        anchor: format!("finding-{name}-{id}"),
                    });
                }
            }
        }
    }
    for draft in crate::code_review::list(&studio.project.root)?
        .into_iter()
        .filter(|d| {
            matches!(
                d.status.as_str(),
                "pending" | "stale" | "needs_permission" | "applying"
            )
        })
    {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: draft.stage,
            id: draft.task.clone(),
            tier: Tier::Destructive,
            kind: "code".into(),
            title: format!("Review code diff ({})", draft.status),
            anchor: format!("code-{}", draft.task),
        });
    }
    for w in crate::writes::Writes::open(&studio.project.root)?
        .items()
        .iter()
        .filter(|w| w.status == crate::writes::Status::Proposed)
    {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: first.clone(),
            id: w.id.clone(),
            tier: Tier::Consequential,
            kind: "write".into(),
            title: "Review an external write proposal".into(),
            anchor: format!("write-{}", w.id),
        });
    }
    for proposal in crate::coordinator::history(&studio.project.root)?
        .iter()
        .filter_map(|t| t.proposal.as_ref())
        .filter(|p| p.status == "pending" || p.status == "blocked")
    {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: first.clone(),
            id: proposal.id.clone(),
            tier: Tier::Consequential,
            kind: "proposal".into(),
            title: "Review the proposed changes".into(),
            anchor: "coordinator-panel".into(),
        });
    }
    // The steps held for the person's answers are one item, which leads to the form that asks the questions.
    if let Some((stage, _)) = studio
        .blocked_stages()
        .into_iter()
        .find(|(_, why)| why == crate::facts::WAITING_REASON)
    {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage,
            id: "questions".into(),
            tier: Tier::Consequential,
            kind: "questions".into(),
            title: "Answer the crew's questions".into(),
            anchor: "questions".into(),
        });
    }
    for (stage, reason) in studio
        .blocked_stages()
        .into_iter()
        .filter(|(_, why)| why != crate::facts::WAITING_REASON)
    {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: stage.clone(),
            id: format!("blocked-{stage}"),
            tier: Tier::Consequential,
            kind: "escalation".into(),
            title: reason,
            anchor: format!("step-{stage}"),
        });
    }
    for (stage, reason) in studio.failed_stages()? {
        if !plan.stages.iter().any(|st| st.id == stage) {
            continue;
        }
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: stage.clone(),
            id: format!("failed-{stage}"),
            tier: Tier::Consequential,
            kind: "failure".into(),
            title: format!("A step stopped: {}", crate::stop::without_label(&reason)),
            anchor: format!("step-{stage}"),
        });
    }
    // A run the person stopped is one item, until a step starts again. Only Stop makes it, so a run held for answers or
    // blocked on "Do not send" is never listed as stopped.
    if let Some(after) = studio.stopped_run()? {
        let artifacts = studio.project.artifacts()?;
        let done = plan
            .stages
            .iter()
            .filter(|st| {
                artifacts
                    .iter()
                    .any(|a| a["name"].as_str() == Some(crate::app::artifact_name(&st.id).as_str()))
            })
            .count();
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: after.unwrap_or(first.clone()),
            id: "stopped".into(),
            tier: Tier::Reversible,
            kind: "stopped".into(),
            title: if done == 0 {
                "Stopped by you before any step finished. Run the rest when you are ready.".into()
            } else {
                format!("Stopped by you after step {done}. Run the rest when you are ready.")
            },
            anchor: "run-status".into(),
        });
    }
    // A finished draft that still lacks details only the person can supply is one item for the project.
    if let Some(d) = crate::app::finish_details(studio)
        .ok()
        .filter(crate::app::Details::open)
    {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: d.note_steps.first().cloned().unwrap_or(first.clone()),
            id: "details".into(),
            tier: Tier::Reversible,
            kind: "details".into(),
            title: format!("{} from the draft", crate::gaps::missing(d.gaps.len())),
            anchor: "final-files".into(),
        });
    }
    // Finished work waits until its results are reviewed as they are now (DESIGN_SPEC.md §14). Missing details come
    // first: that row already asks for the person, so finished work is not listed twice.
    if !unfinished && !rows.iter().any(|r| r.kind == "details") && !is_reviewed(studio)? {
        rows.push(Item {
            revision: String::new(),
            step_title: String::new(),
            project: project.into(),
            stage: plan
                .stages
                .last()
                .map(|s| s.id.clone())
                .unwrap_or(first.clone()),
            id: "finished".into(),
            tier: Tier::Reversible,
            kind: "finished".into(),
            title: "Finished: review the result".into(),
            anchor: "final-files".into(),
        });
    }
    let revision = studio.approval_hash()?;
    for row in &mut rows {
        row.revision = revision.clone();
        row.step_title = plan
            .stages
            .iter()
            .find(|st| st.id == row.stage)
            .map(|st| st.title.clone())
            .unwrap_or_default();
    }
    rows.sort_by(|a, b| {
        (a.tier, &a.project, &a.stage, &a.id).cmp(&(b.tier, &b.project, &b.stage, &b.id))
    });
    Ok(rows)
}

/// The version of a project's results: a digest of every result it has recorded. Any new result changes it.
pub fn results_version(studio: &Studio) -> Result<String, String> {
    let list = studio.project.artifacts()?;
    Ok(crate::engine::sha256_hex(
        serde_json::to_string(&list)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    ))
}

/// Every step of the plan has a result.
pub fn is_finished(studio: &Studio) -> bool {
    let Some(plan) = studio.plan() else {
        return false;
    };
    let artifacts = studio.project.artifacts().unwrap_or_default();
    !plan.stages.is_empty()
        && plan.stages.iter().all(|st| {
            artifacts
                .iter()
                .any(|a| a["name"].as_str() == Some(st.id.as_str()))
        })
}

/// The person marked these results reviewed, as they are now. A result recorded since needs reviewing again.
pub fn is_reviewed(studio: &Studio) -> Result<bool, String> {
    let now = results_version(studio)?;
    Ok(studio
        .journal
        .events()?
        .iter()
        .rev()
        .find(|e| e.kind == "result.reviewed")
        .is_some_and(|e| e.payload["version"].as_str() == Some(now.as_str())))
}
