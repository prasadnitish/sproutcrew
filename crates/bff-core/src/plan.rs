//! A plan, and the approval bound to it. M1 exit criteria: no worker starts before approval, and a
//! changed plan no longer counts as approved. Approval is a hash of the whole plan, so any edit breaks it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub title: String,
    pub owner_role: String,
    /// What this step must produce, in the words the worker is given. Empty means the title is all it has.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guide: String,
    /// The steps this one waits for. Empty means the step before it, so a plan with no lists is one chain. Steps
    /// that wait for the same earlier steps, and not for each other, can run side by side. Left out of the JSON
    /// when empty, so plans without lists keep their approval hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    /// The provider, model, and effort this step uses instead of its role's. Left out of the JSON when there is
    /// none, so plans without any keep their approval hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<StepModel>,
}

/// A step's own choice of provider, model, and effort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepModel {
    pub route: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub role: String,
    pub route: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub brief: String,
    pub stages: Vec<Stage>,
    pub team: Vec<Assignment>,
    /// Approved stand-ins, one per role at most, on the other route. Used only after a transient failure.
    /// Left out of the JSON when empty, so plans without fallbacks keep their hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<Assignment>,
    /// Whether AI providers may keep what they are sent. Left out of the JSON when it is the default, so plans made
    /// before it existed keep their hash.
    #[serde(default, skip_serializing_if = "Privacy::is_standard")]
    pub privacy: Privacy,
    /// The person allows Keynote to build a new deck from the deck step's slides, saved in this project's exports.
    /// Left out of the JSON when off, so plans made before it existed keep their hash.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keynote: bool,
}

/// What the providers may keep. `NoRetention` sends only to OpenRouter endpoints that keep nothing, and refuses
/// every provider that offers no such promise for a single request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privacy {
    #[default]
    Standard,
    NoRetention,
}

impl Privacy {
    pub fn is_standard(&self) -> bool {
        *self == Privacy::Standard
    }
}

impl Plan {
    fn index(&self, id: &str) -> Option<usize> {
        self.stages.iter().position(|s| s.id == id)
    }

    /// The steps `id` waits for: its `after` list, or the step before it when that is empty. Nothing for the first
    /// step or for an id that is not in the plan.
    pub fn needs(&self, id: &str) -> Vec<String> {
        let Some(i) = self.index(id) else {
            return vec![];
        };
        let s = &self.stages[i];
        if !s.after.is_empty() {
            // A step taken out of the plan is no longer waited for; see [`Plan::has_no_input`].
            return s
                .after
                .iter()
                .filter(|a| self.index(a).is_some())
                .cloned()
                .collect();
        }
        match i.checked_sub(1) {
            Some(p) => vec![self.stages[p].id.clone()],
            None => vec![],
        }
    }

    /// The steps `id` names in its `after` list that are not in the plan, such as a writing step someone took out.
    pub fn missing_waits(&self, id: &str) -> Vec<String> {
        self.index(id).map_or(vec![], |i| {
            self.stages[i]
                .after
                .iter()
                .filter(|a| self.index(a).is_none())
                .cloned()
                .collect()
        })
    }

    /// True when every step `id` waits for has been taken out of the plan, so it has nothing to work on. Such a
    /// step is skipped with a note instead of stopping the run: a checker of a deck nobody makes checks nothing.
    pub fn has_no_input(&self, id: &str) -> bool {
        self.index(id).is_some_and(|i| {
            let after = &self.stages[i].after;
            !after.is_empty() && after.iter().all(|a| self.index(a).is_none())
        })
    }

    /// Every step `id` waits for, directly or through others, in plan order.
    pub fn ancestors(&self, id: &str) -> Vec<String> {
        let mut seen: Vec<String> = vec![];
        let mut todo = self.needs(id);
        while let Some(n) = todo.pop() {
            if !seen.contains(&n) {
                todo.extend(self.needs(&n));
                seen.push(n);
            }
        }
        self.stages
            .iter()
            .filter(|s| seen.contains(&s.id))
            .map(|s| s.id.clone())
            .collect()
    }

    /// Every step that waits for `id`, directly or through others, in plan order.
    pub fn descendants(&self, id: &str) -> Vec<String> {
        self.stages
            .iter()
            .filter(|s| s.id != id && self.ancestors(&s.id).iter().any(|a| a == id))
            .map(|s| s.id.clone())
            .collect()
    }

    /// The steps that can start now: not done, and everything they wait for is done. Plan order.
    pub fn ready(&self, done: &[String]) -> Vec<String> {
        self.stages
            .iter()
            .filter(|s| !done.contains(&s.id) && self.needs(&s.id).iter().all(|n| done.contains(n)))
            .map(|s| s.id.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub plan_hash: String,
    pub approved_by: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    Allowed,
    NotApproved,
    /// An approval exists, but for a different plan.
    PlanChanged,
}

/// SHA-256 of the plan's canonical JSON. Order matters: reordering stages is a change.
pub fn plan_hash(plan: &Plan) -> String {
    // JSON quotes and escapes every string, so field boundaries cannot be forged.
    let canonical = serde_json::to_string(plan).expect("a plan is plain data");
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
}

pub fn approve(plan: &Plan, approved_by: &str) -> Approval {
    Approval {
        plan_hash: plan_hash(plan),
        approved_by: approved_by.to_string(),
    }
}

/// Every worker dispatch passes through here.
pub fn worker_gate(plan: &Plan, approval: Option<&Approval>) -> Gate {
    match approval {
        None => Gate::NotApproved,
        Some(a) if a.plan_hash == plan_hash(plan) => Gate::Allowed,
        Some(_) => Gate::PlanChanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_stage(json: &str) -> Stage {
        serde_json::from_str(json).unwrap()
    }

    fn plan() -> Plan {
        Plan {
            brief: "Write a short plan.".into(),
            stages: vec![
                Stage {
                    uses: None,
                    after: vec![],
                    id: "s1".into(),
                    title: "Draft".into(),
                    owner_role: "writer".into(),
                    guide: String::new(),
                },
                Stage {
                    uses: None,
                    after: vec![],
                    id: "s2".into(),
                    title: "Review".into(),
                    owner_role: "reviewer".into(),
                    guide: String::new(),
                },
            ],
            team: vec![
                Assignment {
                    role: "writer".into(),
                    route: "R1".into(),
                    model: "gpt-6-luna".into(),
                    effort: "medium".into(),
                },
                Assignment {
                    role: "reviewer".into(),
                    route: "R2".into(),
                    model: "claude-sonnet-5-5".into(),
                    effort: "high".into(),
                },
            ],
            fallbacks: vec![],
            privacy: Default::default(),
            keynote: false,
        }
    }

    #[test]
    fn no_worker_starts_before_approval() {
        assert_eq!(worker_gate(&plan(), None), Gate::NotApproved);
    }

    #[test]
    fn an_approved_plan_may_start_workers() {
        let p = plan();
        let a = approve(&p, "Nitish");
        assert_eq!(worker_gate(&p, Some(&a)), Gate::Allowed);
        assert_eq!(a.approved_by, "Nitish");
    }

    #[test]
    fn any_edit_makes_the_approval_stale() {
        let a = approve(&plan(), "Nitish");
        type Edit = Box<dyn Fn(&mut Plan)>;
        let edits: Vec<Edit> = vec![
            Box::new(|p| p.brief.push('!')),
            Box::new(|p| p.stages[0].title = "Draft v2".into()),
            Box::new(|p| p.stages.swap(0, 1)),
            Box::new(|p| {
                p.stages.push(Stage {
                    uses: None,
                    after: vec![],
                    id: "s3".into(),
                    title: "Ship".into(),
                    owner_role: "writer".into(),
                    guide: String::new(),
                })
            }),
            Box::new(|p| p.team[0].model = "gpt-6.1-sol".into()),
            Box::new(|p| p.team[1].effort = "low".into()),
            Box::new(|p| p.team[0].route = "R2".into()),
            Box::new(|p| {
                p.team.pop();
            }),
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut p = plan();
            edit(&mut p);
            assert_eq!(worker_gate(&p, Some(&a)), Gate::PlanChanged, "edit {i}");
        }
    }

    #[test]
    fn undoing_an_edit_restores_the_approval() {
        let a = approve(&plan(), "Nitish");
        let mut p = plan();
        p.brief.push('x');
        assert_eq!(worker_gate(&p, Some(&a)), Gate::PlanChanged);
        p.brief.pop();
        assert_eq!(worker_gate(&p, Some(&a)), Gate::Allowed);
    }

    #[test]
    fn the_hash_is_stable_and_field_boundaries_cannot_collide() {
        assert_eq!(plan_hash(&plan()), plan_hash(&plan()));
        assert_eq!(plan_hash(&plan()).len(), 64);
        let mut a = plan();
        let mut b = plan();
        a.stages[0].title = "ab".into();
        a.stages[0].owner_role = "c".into();
        b.stages[0].title = "a".into();
        b.stages[0].owner_role = "bc".into();
        assert_ne!(plan_hash(&a), plan_hash(&b));
    }

    fn chain(ids: &[(&str, &[&str])]) -> Plan {
        Plan {
            brief: "b".into(),
            stages: ids
                .iter()
                .map(|(id, after)| Stage {
                    uses: None,
                    id: (*id).into(),
                    title: (*id).into(),
                    owner_role: "writer".into(),
                    guide: String::new(),
                    after: after.iter().map(|x| x.to_string()).collect(),
                })
                .collect(),
            team: vec![],
            fallbacks: vec![],
            privacy: Default::default(),
            keynote: false,
        }
    }

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    // spec -> build -> (security, ux) -> fix -> test
    fn diamond() -> Plan {
        chain(&[
            ("spec", &[]),
            ("build", &[]),
            ("security", &["build"]),
            ("ux", &["build"]),
            ("fix", &["security", "ux"]),
            ("test", &[]),
        ])
    }

    #[test]
    fn a_step_with_no_after_follows_the_step_before_it_and_the_first_follows_nothing() {
        let p = diamond();
        assert!(p.needs("spec").is_empty());
        assert_eq!(p.needs("build"), v(&["spec"]));
        assert_eq!(p.needs("test"), v(&["fix"]));
        assert_eq!(
            p.needs("ux"),
            v(&["build"]),
            "an explicit list wins over the step before it"
        );
        assert_eq!(p.needs("fix"), v(&["security", "ux"]));
    }

    #[test]
    fn ancestors_and_descendants_follow_the_chains_in_plan_order() {
        let p = diamond();
        assert_eq!(p.ancestors("fix"), v(&["spec", "build", "security", "ux"]));
        assert_eq!(
            p.ancestors("ux"),
            v(&["spec", "build"]),
            "a sibling is not an ancestor"
        );
        assert_eq!(
            p.descendants("build"),
            v(&["security", "ux", "fix", "test"])
        );
        assert_eq!(p.descendants("security"), v(&["fix", "test"]));
        assert!(p.descendants("test").is_empty());
        assert!(p.ancestors("nope").is_empty());
    }

    #[test]
    fn a_plain_chain_has_every_earlier_step_as_an_ancestor_and_every_later_one_as_a_descendant() {
        let p = chain(&[("a", &[]), ("b", &[]), ("c", &[])]);
        assert_eq!(p.ancestors("c"), v(&["a", "b"]));
        assert_eq!(p.descendants("a"), v(&["b", "c"]));
    }

    #[test]
    fn the_ready_steps_are_the_ones_not_done_whose_prerequisites_are_all_done() {
        let p = diamond();
        assert_eq!(p.ready(&[]), v(&["spec"]));
        assert_eq!(p.ready(&v(&["spec"])), v(&["build"]));
        assert_eq!(
            p.ready(&v(&["spec", "build"])),
            v(&["security", "ux"]),
            "two at once"
        );
        assert_eq!(p.ready(&v(&["spec", "build", "security"])), v(&["ux"]));
        assert_eq!(
            p.ready(&v(&["spec", "build", "security", "ux"])),
            v(&["fix"])
        );
        assert!(p
            .ready(&v(&["spec", "build", "security", "ux", "fix", "test"]))
            .is_empty());
    }

    #[test]
    fn a_plan_with_no_dependencies_keeps_its_approval_hash_and_one_with_them_does_not() {
        let plain = chain(&[("a", &[]), ("b", &[])]);
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("after"), "{json}");
        let mut with = plain.clone();
        with.stages[1].after = v(&["a"]);
        assert_ne!(plan_hash(&plain), plan_hash(&with));
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn a_step_without_its_own_model_leaves_the_plan_json_and_hash_alone() {
        let plain = parse_stage(r#"{"id": "a", "title": "A", "owner_role": "writer"}"#);
        assert!(plain.uses.is_none());
        assert!(serde_json::to_value(&plain).unwrap().get("uses").is_none());
    }
}
