//! What a plan would ask of the providers before anyone approves it: how many steps, on which models, how many
//! requests go to OpenRouter's free allowance, and what it could cost. Each step is one request; a revision, a
//! second opinion or a retry adds more, so the counts are a floor and the cost is labelled an estimate. The
//! maximum is each step's request bounds at the model's price. Any step whose price is unknown makes the
//! plan's cost unknown: it is never added up as zero. This is shown to the person and decides nothing.

use crate::apiconfig::RouteConfig;
use crate::catalog::Saved;
use crate::plan::Plan;
use serde::Serialize;
use std::collections::BTreeMap;

/// The share of a step's request bounds a typical step uses, for the rough estimate. Briefs and sources vary
/// widely, so the maximum is the figure to rely on.
const TYPICAL_INPUT: f64 = 0.25;
const TYPICAL_OUTPUT: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Estimate {
    pub steps: usize,
    /// Each distinct model, in plan order, with the number of steps on it.
    pub models: Vec<(String, usize)>,
    /// Steps on OpenRouter's free allowance: one request each, at least.
    pub free_requests: usize,
    /// Requests left today on the free allowance, when OpenRouter could be asked.
    pub free_remaining: Option<u64>,
    /// True when the plan needs more free requests than are left today.
    pub free_short: bool,
    /// Dollars, rough. `None` is unknown.
    pub estimated_usd: Option<f64>,
    /// Dollars at every step's request bounds. `None` is unknown.
    pub max_usd: Option<f64>,
    /// Tokens the plan could use on routes that count tokens against the app's token cap: per step, a typical
    /// share of the input bound and the whole output bound, which each request holds before it is sent.
    pub estimated_tokens: u64,
    /// Tokens left under the token cap, when the app gave it.
    pub tokens_left: Option<u64>,
    /// True when the plan could need more tokens than are left under the cap (Q2-B9).
    pub tokens_short: bool,
}

impl Estimate {
    /// The estimate checked against what is left under the token cap.
    pub fn against_token_cap(mut self, left: u64) -> Estimate {
        self.tokens_left = Some(left);
        self.tokens_short = self.estimated_tokens > left;
        self
    }
}

fn is_free(model: &str) -> bool {
    model == "openrouter/free" || model.ends_with(":free")
}

/// Dollars per million tokens, in and out, for one step's model, or `None` when unknown.
fn price(
    route: &str,
    model: &str,
    cfg: Option<&RouteConfig>,
    catalog: Option<&Saved>,
) -> Option<(f64, f64)> {
    if route == "R12" {
        if is_free(model) {
            return Some((0.0, 0.0));
        }
        let m = catalog?.find(model)?;
        return Some((m.input_per_mtok?, m.output_per_mtok?));
    }
    let cfg = cfg?;
    // A route with no prices counts tokens; its dollar cost is not known in advance.
    if cfg.token_mode {
        return None;
    }
    Some((
        cfg.prices.input_per_mtok as f64 / 1_000_000.0,
        cfg.prices.output_per_mtok as f64 / 1_000_000.0,
    ))
}

pub fn estimate(
    plan: &Plan,
    routes: &BTreeMap<String, RouteConfig>,
    catalog: Option<&Saved>,
    free_remaining: Option<u64>,
) -> Estimate {
    let mut models: Vec<(String, usize)> = Vec::new();
    let mut free_requests = 0;
    let mut est = Some(0.0);
    let mut max = Some(0.0);
    let mut tokens = 0u64;
    for stage in &plan.stages {
        let (route, model) = match &stage.uses {
            Some(u) => (u.route.as_str(), u.model.as_str()),
            None => match plan.team.iter().find(|a| a.role == stage.owner_role) {
                Some(a) => (a.route.as_str(), a.model.as_str()),
                None => {
                    est = None;
                    max = None;
                    continue;
                }
            },
        };
        match models.iter_mut().find(|(m, _)| m == model) {
            Some((_, n)) => *n += 1,
            None => models.push((model.to_string(), 1)),
        }
        if route == "R12" && is_free(model) {
            free_requests += 1;
        }
        let cfg = routes.get(route);
        if let Some(c) = cfg.filter(|c| c.token_mode) {
            tokens += (c.bounds.max_input_tokens as f64 * TYPICAL_INPUT) as u64
                + c.bounds.max_output_tokens;
        }
        let cost = price(route, model, cfg, catalog).zip(cfg.map(|c| c.bounds));
        match cost {
            Some(((input, output), b)) => {
                let (i, o) = (b.max_input_tokens as f64, b.max_output_tokens as f64);
                // A web step on OpenRouter also pays for its one search.
                let search = if route == "R12" && crate::workflows::uses_web(&stage.owner_role) {
                    crate::apiroute::OPENROUTER_SEARCH_USD
                } else {
                    0.0
                };
                max = max.map(|m| m + (input * i + output * o) / 1_000_000.0 + search);
                est = est.map(|e| {
                    e + (input * i * TYPICAL_INPUT + output * o * TYPICAL_OUTPUT) / 1_000_000.0
                        + search
                });
            }
            None => {
                est = None;
                max = None;
            }
        }
    }
    Estimate {
        steps: plan.stages.len(),
        models,
        free_requests,
        free_remaining,
        free_short: free_remaining.is_some_and(|left| (free_requests as u64) > left),
        estimated_usd: est,
        max_usd: max,
        estimated_tokens: tokens,
        tokens_left: None,
        tokens_short: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apiroute::{Bounds, Prices};
    use crate::catalog::Model;
    use crate::plan::{Assignment, Stage, StepModel};

    fn stage(id: &str, role: &str, uses: Option<(&str, &str)>) -> Stage {
        Stage {
            id: id.into(),
            title: id.into(),
            owner_role: role.into(),
            guide: String::new(),
            after: vec![],
            uses: uses.map(|(route, model)| StepModel {
                route: route.into(),
                model: model.into(),
                effort: "medium".into(),
            }),
        }
    }

    fn plan(stages: Vec<Stage>, writer: &str, checker: &str) -> Plan {
        Plan {
            brief: "b".into(),
            stages,
            team: vec![
                Assignment {
                    role: "writer".into(),
                    route: "R12".into(),
                    model: writer.into(),
                    effort: "medium".into(),
                },
                Assignment {
                    role: "reviewer".into(),
                    route: "R12".into(),
                    model: checker.into(),
                    effort: "high".into(),
                },
            ],
            fallbacks: vec![],
            privacy: Default::default(),
            keynote: false,
        }
    }

    fn routes() -> BTreeMap<String, RouteConfig> {
        let token = RouteConfig {
            prices: Prices {
                input_per_mtok: 1,
                output_per_mtok: 1,
                tool_call: 0,
            },
            bounds: Bounds {
                max_input_tokens: 60_000,
                max_output_tokens: 8_000,
                max_tool_calls: 0,
                max_cost_micro: u64::MAX,
            },
            allowance_micro: 500_000,
            token_mode: true,
            base_url: None,
            model: None,
        };
        BTreeMap::from([("R12".to_string(), token)])
    }

    fn catalog() -> Saved {
        let m = |id: &str, i: Option<f64>, o: Option<f64>| Model {
            id: id.into(),
            name: id.into(),
            maker: "x".into(),
            context_length: None,
            input_per_mtok: i,
            output_per_mtok: o,
            free: false,
            sees_images: false,
        };
        Saved {
            fetched_at: 0,
            models: vec![
                m("anthropic/claude-haiku-5.5", Some(0.1), Some(0.5)),
                m("mistralai/mistral-small-2603", Some(0.15), Some(0.6)),
                m("openrouter/auto", None, None),
            ],
        }
    }

    #[test]
    fn a_paid_plan_is_priced_at_its_request_bounds_from_the_saved_list() {
        let p = plan(
            vec![
                stage("a", "writer", None),
                stage("b", "writer", None),
                stage("c", "reviewer", None),
            ],
            "anthropic/claude-haiku-5.5",
            "mistralai/mistral-small-2603",
        );
        let e = estimate(&p, &routes(), Some(&catalog()), Some(50));
        assert_eq!(e.steps, 3);
        assert_eq!(
            e.models,
            vec![
                ("anthropic/claude-haiku-5.5".to_string(), 2),
                ("mistralai/mistral-small-2603".to_string(), 1)
            ]
        );
        // Haiku: 60k in at $0.10 + 8k out at $0.50 = $0.010 a step; Mistral: $0.009 + $0.0048 = $0.0138.
        let max = e.max_usd.unwrap();
        assert!((max - (2.0 * 0.010 + 0.0138)).abs() < 1e-9, "{max}");
        assert!(e.estimated_usd.unwrap() < max);
        assert_eq!((e.free_requests, e.free_short), (0, false));
    }

    #[test]
    fn a_plan_warns_when_it_could_use_more_tokens_than_the_token_cap_has_left() {
        let p = plan(
            vec![stage("a", "writer", None), stage("b", "reviewer", None)],
            "anthropic/claude-haiku-5.5",
            "mistralai/mistral-small-2603",
        );
        let e = estimate(&p, &routes(), Some(&catalog()), None);
        // Each step: a quarter of the 60,000 input bound, typical, plus its full 8,000 output bound.
        assert_eq!(e.estimated_tokens, 2 * (15_000 + 8_000));
        assert_eq!((e.tokens_left, e.tokens_short), (None, false));
        let short = e.clone().against_token_cap(45_000);
        assert_eq!(
            (short.tokens_left, short.tokens_short),
            (Some(45_000), true)
        );
        assert!(!e.against_token_cap(46_000).tokens_short);
    }

    #[test]
    fn one_step_with_an_unknown_price_makes_the_whole_cost_unknown_not_smaller() {
        let p = plan(
            vec![
                stage("a", "writer", None),
                stage("b", "writer", Some(("R12", "openrouter/auto"))),
            ],
            "anthropic/claude-haiku-5.5",
            "x/y",
        );
        let e = estimate(&p, &routes(), Some(&catalog()), None);
        assert_eq!((e.estimated_usd, e.max_usd), (None, None));
        let none = estimate(&p, &routes(), None, None);
        assert_eq!(none.max_usd, None, "no saved list means unknown prices");
    }

    #[test]
    fn free_steps_cost_nothing_and_warn_when_today_has_too_few_requests_left() {
        let stages: Vec<Stage> = (0..12)
            .map(|i| stage(&format!("s{i}"), "writer", None))
            .collect();
        let p = plan(stages, "openrouter/free", "openrouter/free");
        let e = estimate(&p, &routes(), None, Some(8));
        assert_eq!(e.free_requests, 12);
        assert!(e.free_short);
        assert_eq!(e.max_usd, Some(0.0));
        let unknown = estimate(&p, &routes(), None, None);
        assert!(
            !unknown.free_short,
            "no warning is invented when the allowance could not be read"
        );
    }

    #[test]
    fn a_web_step_on_openrouter_adds_the_price_of_its_search() {
        let mut p = plan(
            vec![stage("a", "writer", None)],
            "anthropic/claude-haiku-5.5",
            "x/y",
        );
        let without = estimate(&p, &routes(), Some(&catalog()), None)
            .max_usd
            .unwrap();
        p = crate::workflows::with_web_research(p, ("R12", "anthropic/claude-haiku-5.5", "medium"));
        let with = estimate(&p, &routes(), Some(&catalog()), None)
            .max_usd
            .unwrap();
        assert!(
            (with - 2.0 * without - crate::apiroute::OPENROUTER_SEARCH_USD).abs() < 1e-9,
            "{with} {without}"
        );
    }

    #[test]
    fn a_token_counted_route_other_than_openrouter_has_unknown_dollars() {
        let mut p = plan(
            vec![stage("a", "writer", Some(("R3", "claude-haiku-5-5")))],
            "x/y",
            "x/y",
        );
        let mut r = routes();
        let token = r["R12"].clone();
        r.insert("R3".into(), token);
        assert_eq!(estimate(&p, &r, Some(&catalog()), None).max_usd, None);
        p.stages[0].uses = None;
        p.team.clear();
        assert_eq!(
            estimate(&p, &r, None, None).max_usd,
            None,
            "a step with no one assigned is not counted as free"
        );
    }
}
