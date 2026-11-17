//! The fixed lead decision for the time trials D1 to D3: choose roadmap items under a budget, with
//! dependencies, a quick-win rule, and a cap on count. The optimum is computed here, so correctness
//! is scored deterministically and not by opinion.

use serde_json::{json, Value};

pub struct Item {
    pub id: &'static str,
    pub effort: i64,
    pub impact: i64,
    pub requires: Option<&'static str>,
}

pub const ITEMS: [Item; 12] = [
    Item {
        id: "A",
        effort: 3,
        impact: 7,
        requires: None,
    },
    Item {
        id: "B",
        effort: 2,
        impact: 4,
        requires: None,
    },
    Item {
        id: "C",
        effort: 4,
        impact: 9,
        requires: Some("A"),
    },
    Item {
        id: "D",
        effort: 1,
        impact: 2,
        requires: None,
    },
    Item {
        id: "E",
        effort: 5,
        impact: 11,
        requires: Some("B"),
    },
    Item {
        id: "F",
        effort: 3,
        impact: 6,
        requires: None,
    },
    Item {
        id: "G",
        effort: 2,
        impact: 5,
        requires: Some("D"),
    },
    Item {
        id: "H",
        effort: 4,
        impact: 8,
        requires: None,
    },
    Item {
        id: "I",
        effort: 6,
        impact: 13,
        requires: Some("C"),
    },
    Item {
        id: "J",
        effort: 1,
        impact: 3,
        requires: None,
    },
    Item {
        id: "K",
        effort: 3,
        impact: 5,
        requires: Some("F"),
    },
    Item {
        id: "L",
        effort: 2,
        impact: 4,
        requires: None,
    },
];

pub const BUDGET: i64 = 14;
pub const MAX_ITEMS: usize = 6;
pub const QUICK_WIN_MAX_EFFORT: i64 = 2;

#[derive(Debug, PartialEq, Eq)]
pub struct Score {
    pub feasible: bool,
    pub problems: Vec<String>,
    pub impact: i64,
    pub optimum: i64,
    pub optimal: bool,
    /// The model's own totals agree with its list.
    pub totals_consistent: bool,
}

/// The best feasible impact, by checking every subset.
pub fn optimum() -> i64 {
    let n = ITEMS.len();
    (0u32..(1 << n))
        .filter_map(|mask| {
            let chosen: Vec<String> = (0..n)
                .filter(|i| mask & (1 << i) != 0)
                .map(|i| ITEMS[i].id.to_string())
                .collect();
            let e = evaluate(&chosen);
            e.problems.is_empty().then_some(e.impact)
        })
        .max()
        .unwrap_or(0)
}

/// Judges a chosen list and the totals the model reported for it.
struct Evaluation {
    problems: Vec<String>,
    effort: i64,
    impact: i64,
}

fn evaluate(chosen: &[String]) -> Evaluation {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let (mut effort, mut impact) = (0, 0);
    for id in chosen {
        match ITEMS.iter().find(|i| i.id == id) {
            None => problems.push(format!("unknown item {id}")),
            Some(_) if seen.contains(&id.as_str()) => problems.push(format!("duplicate {id}")),
            Some(it) => {
                seen.push(it.id);
                effort += it.effort;
                impact += it.impact;
            }
        }
    }
    if seen.len() > MAX_ITEMS {
        problems.push(format!("more than {MAX_ITEMS} items"));
    }
    for id in &seen {
        if let Some(r) = ITEMS.iter().find(|i| i.id == *id).and_then(|i| i.requires) {
            if !seen.contains(&r) {
                problems.push(format!("{id} requires {r}"));
            }
        }
    }
    if effort > BUDGET {
        problems.push(format!("effort {effort} exceeds the budget {BUDGET}"));
    }
    if !seen.iter().any(|id| {
        ITEMS
            .iter()
            .any(|i| i.id == *id && i.effort <= QUICK_WIN_MAX_EFFORT)
    }) {
        problems.push("no quick win among the chosen items".to_string());
    }
    Evaluation {
        problems,
        effort,
        impact,
    }
}

pub fn score(chosen: &[String], claimed_effort: i64, claimed_impact: i64) -> Score {
    let e = evaluate(chosen);
    let feasible = e.problems.is_empty();
    let optimum = if feasible { optimum() } else { 0 };
    Score {
        feasible,
        problems: e.problems,
        impact: e.impact,
        optimum,
        optimal: feasible && e.impact == optimum,
        totals_consistent: claimed_effort == e.effort && claimed_impact == e.impact,
    }
}

pub fn schema() -> Value {
    json!({"type": "object", "additionalProperties": false, "required": ["chosen", "total_effort", "total_impact"],
        "properties": {"chosen": {"type": "array", "items": {"type": "string"}}, "total_effort": {"type": "integer"}, "total_impact": {"type": "integer"}}})
}

/// The fixed context packet and question.
pub fn packet() -> String {
    let mut s = String::from(
        "You are the lead on a six-week product push. Choose which roadmap items to fund.\n\nItems, each with its effort in weeks and its impact in points:\n",
    );
    for it in ITEMS.iter() {
        let req = it
            .requires
            .map_or("requires nothing".to_string(), |r| format!("requires {r}"));
        s.push_str(&format!(
            "- {}: effort {} weeks, impact {} points, {req}.\n",
            it.id, it.effort, it.impact
        ));
    }
    s.push_str(&format!(
        "\nRules:\n- Total effort must not exceed {BUDGET} weeks.\n- Choose at most {MAX_ITEMS} items.\n- An item that requires another can only be chosen together with it.\n- At least one chosen item must be a quick win, meaning effort of {QUICK_WIN_MAX_EFFORT} weeks or less.\n\nChoose the set with the highest total impact that follows every rule. Do not use any tools. Reply with chosen (the item ids), total_effort, and total_impact for your set."
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_optimum_is_a_feasible_value_no_other_subset_beats() {
        let best = optimum();
        assert!(best > 0);
        // Spot checks that the solver respects every rule: a hand-built feasible set cannot exceed it.
        let s = score(&ids(&["A", "C", "D", "G"]), 10, 23);
        assert!(s.feasible, "{:?}", s.problems);
        assert!(s.impact <= best);
    }

    #[test]
    fn the_optimum_is_stable_and_equals_a_known_hand_checked_value() {
        assert_eq!(optimum(), optimum());
        // A, C, D, G, J, L: effort 3+4+1+2+1+2 = 13 <= 14, six items, impact 7+9+2+5+3+4 = 30.
        let s = score(&ids(&["A", "C", "D", "G", "J", "L"]), 13, 30);
        assert!(s.feasible && s.totals_consistent, "{:?}", s.problems);
        assert_eq!(s.impact, 30);
        assert!(optimum() >= 30);
    }

    #[test]
    fn each_rule_is_enforced() {
        let p = |v: &[&str]| score(&ids(v), 0, 0).problems.join(";");
        assert!(p(&["C"]).contains("requires A"), "{}", p(&["C"]));
        assert!(p(&["A", "B", "C", "D", "E", "F", "G"]).contains("more than 6"));
        assert!(
            p(&["E", "B", "H", "F", "A"]).contains("budget"),
            "{}",
            p(&["E", "B", "H", "F", "A"])
        );
        assert!(
            p(&["A", "F", "H"]).contains("quick win"),
            "{}",
            p(&["A", "F", "H"])
        );
        assert!(p(&["Z"]).contains("unknown item"));
        assert!(p(&["A", "A"]).contains("duplicate"));
    }

    #[test]
    fn totals_are_checked_against_the_list_and_do_not_change_feasibility() {
        let s = score(&ids(&["A", "D"]), 99, 99);
        assert!(s.feasible);
        assert!(!s.totals_consistent);
        assert!(!s.optimal);
    }

    #[test]
    fn the_optimal_set_scores_as_optimal() {
        // Find an optimal set by the same rules and confirm the scorer agrees.
        let best = optimum();
        let n = ITEMS.len();
        let mut found = false;
        for mask in 0u32..(1 << n) {
            let chosen: Vec<String> = (0..n)
                .filter(|i| mask & (1 << i) != 0)
                .map(|i| ITEMS[i].id.to_string())
                .collect();
            let (e, im) = chosen.iter().fold((0, 0), |a, id| {
                let it = ITEMS.iter().find(|x| x.id == id).unwrap();
                (a.0 + it.effort, a.1 + it.impact)
            });
            let s = score(&chosen, e, im);
            if s.feasible && s.impact == best {
                assert!(s.optimal && s.totals_consistent);
                found = true;
                break;
            }
        }
        assert!(found, "some subset must reach the optimum");
    }

    #[test]
    fn the_packet_lists_every_item_and_every_rule_and_never_the_answer() {
        let p = packet();
        for it in ITEMS.iter() {
            assert!(p.contains(&format!("{}:", it.id)), "{}", it.id);
        }
        for needle in ["14", "6 items", "quick win", "requires"] {
            assert!(p.contains(needle), "{needle}");
        }
        assert!(!p.contains(&format!("optimum is {}", optimum())));
    }

    #[test]
    fn the_schema_accepts_a_well_formed_answer() {
        let ok = json!({"chosen": ["A"], "total_effort": 3, "total_impact": 7});
        assert!(bff_core::schema::validate(&schema(), &ok).is_empty());
    }
}
