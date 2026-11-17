//! How reliable each crew has been on this Mac, read from the projects' own journals and nothing else. For each
//! outcome (such as Writing) and pair of models (the writer's and the checker's): how many projects, how many
//! finished every step without a step failing, how many hit a safety stop, and how many review findings were
//! raised and accepted. Nothing leaves the Mac.
//!
//! "Finished" means every step in the current plan has a result and no step's latest attempt failed. A project
//! still in progress counts as not finished.

use crate::journal::Event;
use crate::plan::Plan;
use serde::Serialize;
use std::collections::BTreeMap;

/// What one project's journal says.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    pub outcome: String,
    pub writer: String,
    pub checker: String,
    pub finished: bool,
    pub stopped: bool,
    pub failed_steps: usize,
    pub findings: usize,
    pub accepted: usize,
    pub stand_ins: usize,
}

/// One project as a sample. `done` is how many of the plan's steps have a result, `failed` how many steps' latest
/// attempt failed.
pub fn sample(plan: &Plan, events: &[Event], done: usize, failed: usize) -> Sample {
    let model_of = |checking: bool| {
        plan.team
            .iter()
            .find(|a| crate::workflows::checks(&a.role) == checking)
            .map(|a| a.model.clone())
            .unwrap_or_else(|| "none".into())
    };
    let mut s = Sample {
        outcome: crate::workflows::Workflow::of_plan(plan)
            .map(|w| w.name().to_string())
            .unwrap_or_else(|| "Your own plan".into()),
        writer: model_of(false),
        checker: model_of(true),
        finished: !plan.stages.is_empty() && done == plan.stages.len() && failed == 0,
        stopped: false,
        failed_steps: failed,
        findings: 0,
        accepted: 0,
        stand_ins: 0,
    };
    for e in events {
        match e.kind.as_str() {
            "worker.failed" => {
                if e.payload["reason"]
                    .as_str()
                    .is_some_and(|r| r.starts_with("stopped:"))
                {
                    s.stopped = true;
                }
            }
            "review.received" => {
                s.findings += e.payload["report"]["findings"]
                    .as_array()
                    .map_or(0, Vec::len)
            }
            "review.decided" if e.payload["decision"] == "accept" => s.accepted += 1,
            "fallback.used" => s.stand_ins += 1,
            _ => {}
        }
    }
    s
}

/// A row of the report: one outcome and model pair over every project that used it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub outcome: String,
    pub writer: String,
    pub checker: String,
    pub projects: usize,
    pub finished: usize,
    pub stopped: usize,
    pub findings: usize,
    pub accepted: usize,
    pub stand_ins: usize,
}

/// The samples grouped by outcome and model pair, most used first.
pub fn report(samples: &[Sample]) -> Vec<Row> {
    let mut rows: BTreeMap<(String, String, String), Row> = BTreeMap::new();
    for s in samples {
        let r = rows
            .entry((s.outcome.clone(), s.writer.clone(), s.checker.clone()))
            .or_insert_with(|| Row {
                outcome: s.outcome.clone(),
                writer: s.writer.clone(),
                checker: s.checker.clone(),
                projects: 0,
                finished: 0,
                stopped: 0,
                findings: 0,
                accepted: 0,
                stand_ins: 0,
            });
        r.projects += 1;
        r.finished += usize::from(s.finished);
        r.stopped += usize::from(s.stopped);
        r.findings += s.findings;
        r.accepted += s.accepted;
        r.stand_ins += s.stand_ins;
    }
    let mut out: Vec<Row> = rows.into_values().collect();
    out.sort_by(|a, b| b.projects.cmp(&a.projects).then(a.outcome.cmp(&b.outcome)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Assignment, Stage};
    use serde_json::json;

    fn plan(stages: &[&str]) -> Plan {
        Plan {
            brief: "b".into(),
            stages: stages
                .iter()
                .map(|id| Stage {
                    id: id.to_string(),
                    title: id.to_string(),
                    owner_role: if *id == "review" {
                        "reviewer".into()
                    } else {
                        "writer".into()
                    },
                    guide: String::new(),
                    after: vec![],
                    uses: None,
                })
                .collect(),
            team: vec![
                Assignment {
                    role: "writer".into(),
                    route: "R12".into(),
                    model: "anthropic/claude-haiku-5.5".into(),
                    effort: "low".into(),
                },
                Assignment {
                    role: "reviewer".into(),
                    route: "R12".into(),
                    model: "mistralai/mistral-small-2603".into(),
                    effort: "low".into(),
                },
            ],
            fallbacks: vec![],
            privacy: Default::default(),
            keynote: false,
        }
    }

    fn ev(kind: &str, payload: serde_json::Value) -> Event {
        Event {
            id: 1,
            kind: kind.into(),
            payload,
            at: 1,
        }
    }

    #[test]
    fn a_project_is_finished_only_when_every_step_has_a_result_and_none_failed() {
        let p = plan(&["outline", "draft", "edit", "review", "finish"]);
        let events = vec![
            ev(
                "review.received",
                json!({"report": {"findings": [{"id": "f1"}, {"id": "f2"}, {"id": "f3"}]}}),
            ),
            ev("review.decided", json!({"decision": "accept"})),
            ev("review.decided", json!({"decision": "reject"})),
        ];
        let s = sample(&p, &events, 5, 0);
        assert_eq!(s.outcome, "Writing & editing");
        assert_eq!(
            (s.writer.as_str(), s.checker.as_str()),
            ("anthropic/claude-haiku-5.5", "mistralai/mistral-small-2603")
        );
        assert!(s.finished && !s.stopped);
        assert_eq!((s.findings, s.accepted), (3, 1));
        assert!(
            !sample(&p, &events, 4, 0).finished,
            "a step without a result"
        );
        let stopped = vec![ev(
            "worker.failed",
            json!({"reason": "stopped: OpenRouter did not answer within 5 minutes"}),
        )];
        let s = sample(&p, &stopped, 3, 1);
        assert!(!s.finished && s.stopped && s.failed_steps == 1);
    }

    #[test]
    fn samples_are_grouped_by_outcome_and_model_pair_most_used_first() {
        let p = plan(&["outline", "draft", "edit", "review", "finish"]);
        let a = sample(&p, &[], 5, 0);
        let b = sample(&p, &[], 2, 1);
        let mut other = p.clone();
        other.team[1].model = "deepseek/deepseek-v4.1-flash".into();
        let c = sample(&other, &[], 5, 0);
        let r = report(&[a, b, c]);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].projects, r[0].finished), (2, 1));
        assert_eq!(r[1].checker, "deepseek/deepseek-v4.1-flash");
    }
}
