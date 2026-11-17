//! The v1 route table and assignment validation. Changing a model or effort is checked here, and an
//! unsupported choice is explained. Data comes from M0: the Codex list is from `model/list` (N1), and
//! the Claude list is what ran in M0.

const ALL_CODEX: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultra"];
const API_EFFORTS: &[&str] = &["low", "medium", "high"];
const NO_ULTRA: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Conservative starting choices; OpenRouter's free router never selects a paid model.
pub fn api_default(route: &str) -> Option<&'static str> {
    match route {
        "R12" => Some("openrouter/free"),
        "R3" => Some("claude-sonnet-5-5"),
        "R5" => Some("gpt-6-luna"),
        "R8" => Some("grok-4.7"),
        "R9" => Some("gemini-3.6-flash"),
        "R10" => Some("deepseek-v4-flash"),
        "R11" => Some("kimi-k3"),
        _ => None,
    }
}

struct Model {
    route: &'static str,
    name: &'static str,
    efforts: &'static [&'static str],
    hidden: bool,
}

const MODELS: &[Model] = &[
    Model {
        route: "R1",
        name: "gpt-6.1-sol",
        efforts: ALL_CODEX,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-6.1-terra",
        efforts: ALL_CODEX,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-6-sol",
        efforts: ALL_CODEX,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-6-luna",
        efforts: NO_ULTRA,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-5.6-sol",
        efforts: ALL_CODEX,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-5.6-terra",
        efforts: ALL_CODEX,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-5.6-luna",
        efforts: NO_ULTRA,
        hidden: false,
    },
    Model {
        route: "R1",
        name: "gpt-reserve",
        efforts: NO_ULTRA,
        hidden: true,
    },
    Model {
        route: "R1",
        name: "codex-auto-review",
        efforts: NO_ULTRA,
        hidden: true,
    },
    // API routes. Prices, bounds, and a key are needed before they run.
    Model {
        route: "R5",
        name: "gpt-6.1-terra",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R5",
        name: "gpt-6-luna",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R8",
        name: "grok-4.7",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R10",
        name: "deepseek-v4-pro",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R10",
        name: "deepseek-v4-flash",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R11",
        name: "kimi-k3",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R9",
        name: "gemini-3.6-flash",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R9",
        name: "gemini-3.8-flash",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R2",
        name: "claude-haiku-5-5",
        efforts: &["low", "medium", "high"],
        hidden: false,
    },
    Model {
        route: "R2",
        name: "claude-sonnet-5-5",
        efforts: &["low", "medium", "high"],
        hidden: false,
    },
    Model {
        route: "R2",
        name: "claude-opus-5-5",
        efforts: &["low", "medium", "high"],
        hidden: false,
    },
    // Claude through the person's own Anthropic API key.
    Model {
        route: "R3",
        name: "claude-haiku-5-5",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R3",
        name: "claude-sonnet-5-5",
        efforts: API_EFFORTS,
        hidden: false,
    },
    Model {
        route: "R3",
        name: "claude-opus-5-5",
        efforts: API_EFFORTS,
        hidden: false,
    },
];

/// Why a choice was refused, in words a person can act on.
#[derive(Debug, PartialEq, Eq)]
pub struct Unsupported(pub String);

/// Routes open: R1 (Codex over stdio), R2 (`claude -p`), and the API routes R3 (Anthropic), R5 (OpenAI), R8 (xAI),
/// R9 (Gemini), R10 (DeepSeek), R11 (Kimi), and R12 (OpenRouter), which need a key, prices, and bounds first.
pub fn validate(route: &str, model: &str, effort: &str) -> Result<(), Unsupported> {
    let no = |m: String| Err(Unsupported(m));
    if is_excluded(model) {
        return no(format!(
            "{model} is excluded and cannot be assigned. Choose another model."
        ));
    }
    if ![
        "R1", "R2", "R3", "R5", "R8", "R9", "R10", "R11", "R12", "R13",
    ]
    .contains(&route)
    {
        return no(format!(
            "Route {route:?} is not available. Use R1 (Codex), R2 (Claude), or an API route: R3 (Anthropic), R5 (OpenAI), R8 (xAI), R9 (Gemini), R10 (DeepSeek), R11 (Kimi), R12 (OpenRouter), R13 (your own AI service)."
        ));
    }
    if route == "R13" {
        if !crate::custom_ai::model_ok(model) {
            return no(format!("{model:?} is not a usable model name. Type it exactly as your service lists it, such as llama3.3."));
        }
        if !API_EFFORTS.contains(&effort) {
            return no(format!(
                "Your own AI service supports efforts: {}.",
                API_EFFORTS.join(", ")
            ));
        }
        return Ok(());
    }
    if route == "R12" {
        if !openrouter_name(model) {
            return no(format!("{model:?} is not an OpenRouter model name. Use vendor/model in lowercase, such as deepseek/deepseek-v4-flash."));
        }
        if !API_EFFORTS.contains(&effort) {
            return no(format!(
                "OpenRouter models support efforts: {}.",
                API_EFFORTS.join(", ")
            ));
        }
        return Ok(());
    }
    let Some(spec) = MODELS.iter().find(|m| m.name == model && m.route == route) else {
        if let Some(other) = MODELS.iter().find(|m| m.name == model && !m.hidden) {
            return no(format!(
                "{model} is a {} model and is not available on {route}. Assign it on {}.",
                other.route, other.route
            ));
        }
        return no(format!(
            "{model:?} is not a known model on {route}. Available: {}.",
            models_for(route).join(", ")
        ));
    };
    if spec.hidden {
        return no(format!(
            "{model} is hidden and is not offered for assignment."
        ));
    }
    if !spec.efforts.contains(&effort) {
        return no(format!(
            "{model} does not support effort {effort:?}. Supported efforts: {}.",
            spec.efforts.join(", ")
        ));
    }
    Ok(())
}

/// Models Nitish excluded from use on October 31, 2026: Astra and Fable. They are refused wherever a model is
/// chosen, so no plan, fallback, or API request can name them.
pub fn is_excluded(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("astra") || m.contains("fable")
}

/// The reviewer used when none is chosen: a stronger setting on the route other than the author's, because the
/// cheap setting missed a plain arithmetic error in the M2 live check.
pub fn default_reviewer_for(author_route: &str) -> (&'static str, &'static str, &'static str) {
    if author_route == "R2" {
        ("R1", "gpt-6.1-terra", "high")
    } else if author_route == "R3" {
        ("R5", "gpt-6.1-terra", "high")
    } else {
        ("R2", "claude-sonnet-5-5", "high")
    }
}

/// The same kind of model on the other way to reach its provider: Claude Code and an Anthropic key stand in for
/// each other, and so do Codex and an OpenAI key. Other routes have none.
pub fn counterpart(route: &str, model: &str) -> Option<(&'static str, &'static str)> {
    let same = |to: &'static str| models_for(to).into_iter().find(|m| *m == model);
    match route {
        "R2" => same("R3").map(|m| ("R3", m)),
        "R3" => same("R2").map(|m| ("R2", m)),
        "R1" if model.contains("luna") => Some(("R5", "gpt-6-luna")),
        "R1" => Some(("R5", "gpt-6.1-terra")),
        "R5" if model.contains("luna") => Some(("R1", "gpt-6-luna")),
        "R5" => Some(("R1", "gpt-6.1-terra")),
        _ => None,
    }
}

/// Moves each assignment whose route this Mac cannot run to its counterpart, when the counterpart can run. The
/// effort is kept if the new model offers it, otherwise the nearest lower one. Used only on plans SproutCrew made,
/// before they are approved, so a choice the person made is never changed.
pub fn fit(team: &mut [crate::plan::Assignment], runnable: &dyn Fn(&str) -> bool) {
    for a in team.iter_mut().filter(|a| !runnable(&a.route)) {
        let Some((route, model)) = counterpart(&a.route, &a.model).filter(|(r, _)| runnable(r))
        else {
            continue;
        };
        let Some(effort) = ["ultra", "max", "xhigh", "high", "medium", "low"]
            .into_iter()
            .skip_while(|e| *e != a.effort)
            .find(|e| validate(route, model, e).is_ok())
        else {
            continue;
        };
        a.route = route.into();
        a.model = model.into();
        a.effort = effort.into();
    }
}

/// `vendor/model` in lowercase letters, digits, and `.`, `_`, `-`, `:`, `+`, with exactly one slash.
pub fn openrouter_name(name: &str) -> bool {
    let ok = |part: &str| {
        part.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-:+".contains(c))
    };
    match name.split_once('/') {
        Some((a, b)) => ok(a) && ok(b),
        None => false,
    }
}

/// Whether a model is on the route's list. OpenRouter has no fixed list, so its names are checked by shape,
/// and an excluded name never passes.
pub fn model_known(route: &str, model: &str) -> bool {
    if is_excluded(model) {
        return false;
    }
    if route == "R12" {
        return openrouter_name(model);
    }
    if route == "R13" {
        return crate::custom_ai::model_ok(model);
    }
    models_for(route).contains(&model)
}

pub fn models_for(route: &str) -> Vec<&'static str> {
    MODELS
        .iter()
        .filter(|m| m.route == route && !m.hidden)
        .map(|m| m.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn why(r: Result<(), Unsupported>) -> String {
        r.unwrap_err().0
    }

    #[test]
    fn supported_choices_pass() {
        assert!(validate("R1", "gpt-6-luna", "medium").is_ok());
        assert!(validate("R1", "gpt-6.1-sol", "ultra").is_ok());
        assert!(validate("R2", "claude-haiku-5-5", "low").is_ok());
        assert!(validate("R2", "claude-sonnet-5-5", "high").is_ok());
        assert!(validate("R2", "claude-opus-5-5", "high").is_ok());
    }

    #[test]
    fn an_unsupported_effort_names_the_efforts_that_work() {
        let m = why(validate("R1", "gpt-6-luna", "ultra"));
        assert!(
            m.contains("gpt-6-luna") && m.contains("ultra") && m.contains("max"),
            "{m}"
        );
        let m = why(validate("R2", "claude-haiku-5-5", "max"));
        assert!(m.contains("low") && m.contains("high"), "{m}");
    }

    #[test]
    fn effort_names_are_exact() {
        let m = why(validate("R1", "gpt-6-luna", "Medium"));
        assert!(m.contains("medium"), "{m}");
        assert!(validate("R1", "gpt-6-luna", "").is_err());
    }

    #[test]
    fn a_model_on_the_wrong_route_says_where_it_belongs() {
        assert!(why(validate("R2", "gpt-6-luna", "low")).contains("R1"));
        assert!(why(validate("R1", "claude-haiku-5-5", "low")).contains("R2"));
    }

    #[test]
    fn unknown_and_hidden_models_are_refused_with_the_alternatives() {
        let m = why(validate("R1", "gpt-9", "low"));
        assert!(m.contains("gpt-9") && m.contains("gpt-6-luna"), "{m}");
        assert!(
            !m.contains("gpt-reserve"),
            "hidden models are not offered: {m}"
        );
        let m = why(validate("R1", "gpt-reserve", "low"));
        assert!(m.contains("hidden"), "{m}");
    }

    #[test]
    fn routes_outside_v1_are_refused_with_the_reason() {
        for r in ["R4", "R6", "R7", "", "r1"] {
            let m = why(validate(r, "gpt-6-luna", "low"));
            assert!(m.contains("R1") && m.contains("R2"), "{r}: {m}");
        }
    }

    #[test]
    fn models_for_lists_only_visible_models_of_the_route() {
        let r1 = models_for("R1");
        assert!(
            r1.contains(&"gpt-6.1-terra")
                && !r1.contains(&"gpt-6-astra")
                && !r1.contains(&"gpt-reserve")
                && !r1.contains(&"claude-haiku-5-5")
        );
        assert_eq!(
            models_for("R2"),
            vec!["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"]
        );
        assert_eq!(
            models_for("R9"),
            vec!["gemini-3.6-flash", "gemini-3.8-flash"]
        );
        assert_eq!(models_for("R8"), vec!["grok-4.7"]);
        assert!(models_for("R4").is_empty());
    }

    #[test]
    fn astra_and_fable_are_excluded_everywhere_a_model_is_chosen() {
        for (route, model) in [
            ("R1", "gpt-6-astra"),
            ("R5", "gpt-6-astra"),
            ("R2", "claude-fable-5-1"),
            ("R2", "Claude-Fable-5"),
            ("R9", "gemini-astra"),
        ] {
            let e = validate(route, model, "low").unwrap_err().0;
            assert!(e.contains("excluded"), "{route} {model}: {e}");
            assert!(is_excluded(model));
        }
        for m in [
            "gpt-6.1-sol",
            "gpt-6.1-terra",
            "gpt-6-luna",
            "claude-sonnet-5-5",
            "claude-haiku-5-5",
            "grok-4.7",
        ] {
            assert!(!is_excluded(m), "{m}");
        }
        for r in ["R1", "R2", "R3", "R5", "R8", "R9"] {
            assert!(models_for(r).iter().all(|m| !is_excluded(m)), "{r}");
        }
    }

    #[test]
    fn the_default_reviewer_is_strong_valid_and_on_the_other_route() {
        for author in ["R1", "R5", "R8", "R9"] {
            let (r, m, e) = default_reviewer_for(author);
            assert_eq!((r, m, e), ("R2", "claude-sonnet-5-5", "high"));
            assert!(validate(r, m, e).is_ok());
        }
        let (r, m, e) = default_reviewer_for("R2");
        assert_eq!((r, m, e), ("R1", "gpt-6.1-terra", "high"));
        assert!(validate(r, m, e).is_ok());
        // Claude through an API key is checked by OpenAI through an API key, not by Claude again.
        let (r, m, e) = default_reviewer_for("R3");
        assert_eq!((r, m, e), ("R5", "gpt-6.1-terra", "high"));
        assert!(validate(r, m, e).is_ok());
    }

    #[test]
    fn a_team_moves_to_the_routes_this_mac_can_run_and_nothing_else_changes() {
        use crate::plan::Assignment;
        let a = |role: &str, route: &str, model: &str, effort: &str| Assignment {
            role: role.into(),
            route: route.into(),
            model: model.into(),
            effort: effort.into(),
        };
        let mut team = vec![
            a("writer", "R1", "gpt-6.1-sol", "ultra"),
            a("reviewer", "R2", "claude-sonnet-5-5", "high"),
            a("reader", "R1", "gpt-6-luna", "low"),
            a("other", "R8", "grok-4.7", "low"),
        ];
        // Only API keys on this Mac.
        fit(&mut team, &|r| r == "R3" || r == "R5");
        assert_eq!(
            team.iter()
                .map(|t| (t.route.as_str(), t.model.as_str(), t.effort.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("R5", "gpt-6.1-terra", "high"),
                ("R3", "claude-sonnet-5-5", "high"),
                ("R5", "gpt-6-luna", "low"),
                ("R8", "grok-4.7", "low"),
            ]
        );
        for t in &team[..3] {
            assert!(validate(&t.route, &t.model, &t.effort).is_ok());
        }
        // Nothing to move to: the plan stays as it was.
        let mut alone = vec![a("writer", "R2", "claude-opus-5-5", "high")];
        fit(&mut alone, &|r| r == "R5");
        assert_eq!(alone[0].route, "R2");
        // Everything runs: nothing moves.
        let mut fine = vec![a("writer", "R1", "gpt-6.1-sol", "ultra")];
        fit(&mut fine, &|_| true);
        assert_eq!(
            (fine[0].route.as_str(), fine[0].effort.as_str()),
            ("R1", "ultra")
        );
    }

    #[test]
    fn deepseek_kimi_and_openrouter_routes_validate_their_models() {
        assert!(validate("R10", "deepseek-v4-flash", "low").is_ok());
        assert!(validate("R10", "deepseek-v4-pro", "high").is_ok());
        assert!(validate("R11", "kimi-k3", "medium").is_ok());
        assert!(
            validate("R10", "kimi-k3", "low").is_err()
                && validate("R11", "deepseek-v4-flash", "low").is_err()
        );
        assert_eq!(
            models_for("R10"),
            vec!["deepseek-v4-pro", "deepseek-v4-flash"]
        );
        assert!(validate("R12", "deepseek/deepseek-v4-flash", "low").is_ok());
        assert!(validate("R12", "meta-llama/llama-4-maverick", "high").is_ok());
        for bad in [
            "deepseek-v4-flash",
            "a/b/c",
            "openai/gpt-6-astra",
            "anthropic/claude-fable-5",
            "A/b",
            "",
        ] {
            assert!(validate("R12", bad, "low").is_err(), "{bad:?}");
        }
        assert!(
            validate("R12", "deepseek/deepseek-v4-flash", "ultra").is_err(),
            "efforts are low, medium, high"
        );
        assert!(
            model_known("R12", "x/y")
                && !model_known("R12", "xy")
                && model_known("R10", "deepseek-v4-flash")
                && !model_known("R10", "x/y")
        );
    }
}
