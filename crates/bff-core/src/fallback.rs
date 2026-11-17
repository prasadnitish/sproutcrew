//! Provider fallback. When one route fails in a way that says nothing about safety (a timeout, a crashed
//! process, an outage, a rate limit), the plan may name a stand-in for that role on the other route. The
//! stand-in is part of the approved plan, so using it widens nothing. A safety stop (billing route
//! mismatch, the 50 percent rule, a write outside the folder, a policy violation, a cancel) never falls
//! back, because a failed check is a result and not a reason to try another way. One fallback per task.
//!
//! Only writing roles get a stand-in. A check that moved to the other provider could land on the provider of the
//! work it reads, and then it would look like a second opinion without being one. Web research has none either,
//! because only Claude steps have web tools here.

use crate::plan::{Assignment, Plan};
use crate::route;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Timeout,
    ProcessFailed,
    Unavailable,
    RateLimited,
    BillingRouteMismatch,
    StopRule(String),
    OutsideWrite,
    PolicyViolation,
    Cancelled,
}

impl Failure {
    /// Failures that do not say anything about safety.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Failure::Timeout | Failure::ProcessFailed | Failure::Unavailable | Failure::RateLimited
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Use(Assignment),
    Refuse(String),
}

impl Failure {
    /// The failure as it reads after a provider's name, for the screen. A failure that never moves a step to a
    /// stand-in has no words of its own.
    pub fn words(&self) -> &'static str {
        match self {
            Failure::Timeout => "timed out",
            Failure::ProcessFailed => "stopped unexpectedly",
            Failure::Unavailable => "was unavailable",
            Failure::RateLimited => "hit its rate limit",
            _ => "stopped",
        }
    }

    /// What a worker's note says went wrong. Notes this does not recognize give `None`, and an unrecognized
    /// failure never triggers a fallback.
    pub fn from_note(note: &str) -> Option<Failure> {
        let n = note.to_lowercase();
        let has = |s: &str| n.contains(s);
        if !note.starts_with("paused") {
            match crate::api_limits::kind_of(note) {
                // A stand-in on another route is not a way round an empty account or a key at its limit.
                Some(crate::api_limits::Kind::OutOfCredits | crate::api_limits::Kind::KeyLimit) => {
                    return None
                }
                Some(_) => return Some(Failure::RateLimited),
                None => {}
            }
        }
        if has("billing route") || has("not on the chatgpt route") || has("not openai") {
            Some(Failure::BillingRouteMismatch)
        } else if has("policy violation") || has("route violation") {
            Some(Failure::PolicyViolation)
        } else if has("outside") && has("write") {
            Some(Failure::OutsideWrite)
        } else if has("paused") || has("cancel") {
            Some(Failure::Cancelled)
        } else if has("stopped:")
            || has("usage blocks dispatch")
            || has("plan usage")
            || has("token budget")
        {
            Some(Failure::StopRule(note.chars().take(80).collect()))
        } else if has("timed out") {
            Some(Failure::Timeout)
        } else if has("rate limit") {
            Some(Failure::RateLimited)
        } else if has("unavailable") || has("overloaded") || has("503") || has("529") {
            Some(Failure::Unavailable)
        } else if has("cannot start") || has("did not finish") || has("exit ") {
            Some(Failure::ProcessFailed)
        } else {
            None
        }
    }
}

pub fn decide(plan: &Plan, role: &str, failure: &Failure, fallbacks_used: u32) -> Decision {
    let Some(primary) = plan.team.iter().find(|a| a.role == role) else {
        return Decision::Refuse(format!("the role {role:?} is not on the team"));
    };
    if !failure.is_transient() {
        return Decision::Refuse(format!(
            "{failure:?} is a safety stop. It is a result to report, and no other route is tried"
        ));
    }
    if fallbacks_used >= 1 {
        return Decision::Refuse("a fallback was already used for this task".into());
    }
    match plan.fallbacks.iter().find(|f| f.role == role) {
        None => Decision::Refuse(format!("no fallback was approved for {role}")),
        Some(f) if f.route == primary.route => {
            Decision::Refuse("the approved fallback is not on the other route".into())
        }
        Some(f) => Decision::Use(f.clone()),
    }
}

/// What a note says when a worker reaches the time limit for one step, which SproutCrew sets. It is not a provider's
/// outage, so it says whose limit it was. [`Failure::from_note`] reads it as a timeout.
pub fn timed_out_note(limit_secs: u64) -> String {
    format!(
        "timed out after {}, the time limit for one step",
        crate::heartbeat::elapsed(limit_secs)
    )
}

/// The stand-ins for a team: one for each writing role that has a sensible one.
pub fn stand_ins(team: &[Assignment]) -> Vec<Assignment> {
    team.iter()
        .filter(|a| !crate::workflows::checks(&a.role) && !crate::workflows::uses_web(&a.role))
        .filter_map(suggest)
        .collect()
}

/// A reasonable stand-in on the other route for the plan's author to review. Not used without approval.
pub fn suggest(a: &Assignment) -> Option<Assignment> {
    let (route, model) = match (a.route.as_str(), a.model.as_str()) {
        ("R1", "gpt-6-luna") | ("R1", "gpt-5.6-luna") => ("R2", "claude-haiku-5-5"),
        ("R1", "gpt-6.1-terra" | "gpt-6.1-sol" | "gpt-6-sol" | "gpt-5.6-sol" | "gpt-5.6-terra") => {
            ("R2", "claude-sonnet-5-5")
        }
        ("R2", "claude-opus-5-5") => ("R1", "gpt-6.1-sol"),
        ("R2", "claude-haiku-5-5") => ("R1", "gpt-6-luna"),
        ("R2", "claude-sonnet-5-5") => ("R1", "gpt-6.1-terra"),
        // API keys stand in for each other: Anthropic for OpenAI and back.
        ("R3", "claude-haiku-5-5") => ("R5", "gpt-6-luna"),
        ("R3", "claude-sonnet-5-5" | "claude-opus-5-5") => ("R5", "gpt-6.1-terra"),
        ("R5", "gpt-6-luna") => ("R3", "claude-haiku-5-5"),
        ("R5", "gpt-6.1-terra") => ("R3", "claude-sonnet-5-5"),
        _ => return None,
    };
    // Keep the effort if the stand-in supports it, otherwise take the highest one it does.
    let effort = ["ultra", "high", "medium", "low"]
        .into_iter()
        .skip_while(|e| *e != a.effort)
        .find(|e| route::validate(route, model, e).is_ok())?;
    Some(Assignment {
        role: a.role.clone(),
        route: route.into(),
        model: model.into(),
        effort: effort.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Stage;

    fn asg(role: &str, route: &str, model: &str, effort: &str) -> Assignment {
        Assignment {
            role: role.into(),
            route: route.into(),
            model: model.into(),
            effort: effort.into(),
        }
    }

    fn plan(with_fallback: bool) -> Plan {
        Plan {
            brief: "b".into(),
            stages: vec![Stage {
                uses: None,
                after: vec![],
                id: "s".into(),
                title: "t".into(),
                owner_role: "writer".into(),
                guide: String::new(),
            }],
            team: vec![
                asg("writer", "R1", "gpt-6-luna", "low"),
                asg("critic", "R2", "claude-sonnet-5-5", "low"),
            ],
            fallbacks: if with_fallback {
                vec![asg("writer", "R2", "claude-haiku-5-5", "low")]
            } else {
                vec![]
            },
            privacy: Default::default(),
            keynote: false,
        }
    }

    #[test]
    fn a_transient_failure_uses_the_approved_stand_in() {
        for f in [
            Failure::Timeout,
            Failure::ProcessFailed,
            Failure::Unavailable,
            Failure::RateLimited,
        ] {
            assert_eq!(
                decide(&plan(true), "writer", &f, 0),
                Decision::Use(asg("writer", "R2", "claude-haiku-5-5", "low")),
                "{f:?}"
            );
        }
    }

    #[test]
    fn a_safety_stop_never_falls_back_whatever_is_approved() {
        for f in [
            Failure::BillingRouteMismatch,
            Failure::StopRule("plan window at 50%".into()),
            Failure::OutsideWrite,
            Failure::PolicyViolation,
            Failure::Cancelled,
        ] {
            let d = decide(&plan(true), "writer", &f, 0);
            assert!(
                matches!(&d, Decision::Refuse(why) if why.contains("safety")),
                "{f:?} -> {d:?}"
            );
            assert!(!f.is_transient());
        }
    }

    #[test]
    fn no_approved_stand_in_one_use_per_task_and_unknown_roles_are_refused() {
        assert!(
            matches!(decide(&plan(false), "writer", &Failure::Timeout, 0), Decision::Refuse(w) if w.contains("no fallback was approved"))
        );
        assert!(
            matches!(decide(&plan(true), "writer", &Failure::Timeout, 1), Decision::Refuse(w) if w.contains("already"))
        );
        assert!(
            matches!(decide(&plan(true), "critic", &Failure::Timeout, 0), Decision::Refuse(w) if w.contains("no fallback was approved"))
        );
        assert!(
            matches!(decide(&plan(true), "ghost", &Failure::Timeout, 0), Decision::Refuse(w) if w.contains("not on the team"))
        );
    }

    #[test]
    fn a_stand_in_on_the_same_route_as_the_role_is_refused() {
        let mut p = plan(false);
        p.fallbacks = vec![asg("writer", "R1", "gpt-6.1-terra", "low")];
        assert!(
            matches!(decide(&p, "writer", &Failure::Timeout, 0), Decision::Refuse(w) if w.contains("other route"))
        );
    }

    #[test]
    fn adding_a_fallback_changes_the_plan_hash_and_no_fallback_keeps_the_old_json() {
        let (a, b) = (plan(false), plan(true));
        assert_ne!(
            crate::plan::plan_hash(&a),
            crate::plan::plan_hash(&b),
            "a stand-in needs a fresh approval"
        );
        assert!(!serde_json::to_string(&a).unwrap().contains("fallbacks"));
        let back: Plan = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(back, b);
        let old: Plan = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert!(old.fallbacks.is_empty());
    }

    #[test]
    fn every_writing_role_gets_a_stand_in_and_a_checking_role_stays_on_its_provider() {
        let team = vec![
            asg("analyst", "R2", "claude-opus-5-5", "high"),
            asg("editor", "R1", "gpt-6.1-terra", "medium"),
            asg("numbers", "R1", "gpt-6.1-sol", "high"),
            asg("reviewer", "R1", "gpt-6.1-sol", "high"),
        ];
        let s = stand_ins(&team);
        assert_eq!(
            s,
            vec![
                asg("analyst", "R1", "gpt-6.1-sol", "high"),
                asg("editor", "R2", "claude-sonnet-5-5", "medium"),
            ]
        );
    }

    #[test]
    fn a_failure_can_be_said_in_words() {
        assert_eq!(Failure::Timeout.words(), "timed out");
        assert_eq!(Failure::RateLimited.words(), "hit its rate limit");
        assert_eq!(Failure::Unavailable.words(), "was unavailable");
        assert_eq!(Failure::ProcessFailed.words(), "stopped unexpectedly");
    }

    #[test]
    fn suggestions_cross_routes_clamp_effort_and_always_validate() {
        let s = suggest(&asg("w", "R1", "gpt-6-luna", "medium")).unwrap();
        assert_eq!(
            (s.route.as_str(), s.model.as_str(), s.effort.as_str()),
            ("R2", "claude-haiku-5-5", "medium")
        );
        let s = suggest(&asg("w", "R1", "gpt-6.1-sol", "ultra")).unwrap();
        assert_eq!(
            (s.route.as_str(), s.model.as_str(), s.effort.as_str()),
            ("R2", "claude-sonnet-5-5", "high"),
            "ultra is not offered on R2"
        );
        let s = suggest(&asg("w", "R2", "claude-sonnet-5-5", "high")).unwrap();
        assert_eq!(s.route, "R1");
        assert!(route::validate(&s.route, &s.model, &s.effort).is_ok());
        let s = suggest(&asg("w", "R2", "claude-haiku-5-5", "low")).unwrap();
        assert_eq!((s.route.as_str(), s.model.as_str()), ("R1", "gpt-6-luna"));
        assert_eq!(suggest(&asg("w", "R1", "unknown-model", "low")), None);
        assert_eq!(suggest(&asg("w", "R3", "gpt-6-luna", "low")), None);
    }

    #[test]
    fn worker_notes_are_classified_and_anything_unrecognized_is_not_transient() {
        for (note, want) in [
            ("timed out", Some(Failure::Timeout)),
            (
                "the run did not finish (exit Some(1))",
                Some(Failure::ProcessFailed),
            ),
            (
                "cannot start claude: No such file",
                Some(Failure::ProcessFailed),
            ),
            (
                "the API is overloaded, try later",
                Some(Failure::Unavailable),
            ),
            ("rate limit reached", Some(Failure::RateLimited)),
            (
                "billing route stop: BillingRoute(\"x\")",
                Some(Failure::BillingRouteMismatch),
            ),
            (
                "the Codex account is not on the ChatGPT route: [..]",
                Some(Failure::BillingRouteMismatch),
            ),
            (
                "policy violation: [ToolOutsideAllowlist(\"Bash\")]",
                Some(Failure::PolicyViolation),
            ),
            (
                "interrupted; route violation: []",
                Some(Failure::PolicyViolation),
            ),
            ("paused: stopped on request", Some(Failure::Cancelled)),
        ] {
            assert_eq!(Failure::from_note(note), want, "{note}");
        }
        assert!(matches!(
            Failure::from_note("plan usage blocks dispatch: AtOrOverThreshold"),
            Some(Failure::StopRule(_))
        ));
        assert!(matches!(
            Failure::from_note("interrupted; stopped: PlanWindow"),
            Some(Failure::StopRule(_))
        ));
        assert_eq!(Failure::from_note("something odd happened"), None);
        assert_eq!(Failure::from_note(""), None);
        // A timeout that was also a policy violation is the safety reading.
        assert_eq!(
            Failure::from_note("timed out after a policy violation"),
            Some(Failure::PolicyViolation)
        );
    }
}
