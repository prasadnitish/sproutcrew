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
    if studio.gate() != Gate::Allowed {
        rows.push(Item {
            revision: String::new(),
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
            project: project.into(),
            stage: first.clone(),
            id: proposal.id.clone(),
            tier: Tier::Consequential,
            kind: "proposal".into(),
            title: "Review the coordinator's proposed changes".into(),
            anchor: "coordinator-panel".into(),
        });
    }
    for (stage, reason) in studio.blocked_stages() {
        rows.push(Item {
            revision: String::new(),
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
            project: project.into(),
            stage: stage.clone(),
            id: format!("failed-{stage}"),
            tier: Tier::Consequential,
            kind: "failure".into(),
            title: format!("A step stopped: {reason}"),
            anchor: format!("step-{stage}"),
        });
    }
    let revision = studio.approval_hash()?;
    for row in &mut rows {
        row.revision = revision.clone();
    }
    rows.sort_by(|a, b| {
        (a.tier, &a.project, &a.stage, &a.id).cmp(&(b.tier, &b.project, &b.stage, &b.id))
    });
    Ok(rows)
}
