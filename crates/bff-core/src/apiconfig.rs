//! Which API routes are switched on. A route runs only if `api-routes.json` describes it (prices, bounds, and
//! an allowance, in dollars) and its Keychain item exists. The file holds no secret. A route that is missing
//! either one is reported as such and stays off, so setting a key alone never starts spending.

use crate::api_engine::ApiEngine;
use crate::apiroute::{Bounds, Prices, Provider, OUTPUT_HARD_CAP};
use crate::engine::Engine;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConfig {
    pub prices: Prices,
    pub bounds: Bounds,
    /// The most this route may use in a session: micro-dollars, or tokens in token mode.
    pub allowance_micro: u64,
    /// The route counts tokens and has no prices. Its prices are one micro-unit per token, so the same
    /// arithmetic that reserves dollars reserves tokens. Dollar prices are not inferred; separate provider-
    /// reported dollar costs can still appear in the usage ledger without becoming a dollar spending cap.
    pub token_mode: bool,
    /// The checked address of the person's own service (R13 only).
    pub base_url: Option<String>,
    /// The model name the person typed for their own service (R13 only), offered first when choosing a model.
    pub model: Option<String>,
}

/// The settings of a route that counts tokens and has no prices.
const TOKEN_KEYS: &[&str] = &["max_input_tokens", "max_output_tokens", "allowance_tokens"];
/// Any of these means the route is priced in dollars and needs every dollar setting.
const DOLLAR_KEYS: &[&str] = &[
    "input_per_mtok_usd",
    "output_per_mtok_usd",
    "tool_call_usd",
    "max_request_usd",
    "allowance_usd",
];

const KEYS: &[&str] = &[
    "input_per_mtok_usd",
    "output_per_mtok_usd",
    "tool_call_usd",
    "max_input_tokens",
    "max_output_tokens",
    "max_tool_calls",
    "max_request_usd",
    "allowance_usd",
];

fn micro(usd: f64) -> u64 {
    (usd * 1_000_000.0).round() as u64
}

/// A route that counts tokens. `max_input_tokens` and `max_output_tokens` are required, and the allowance is the
/// shared default unless `allowance_tokens` says otherwise.
fn token_route(
    route: &str,
    obj: &serde_json::Map<String, Value>,
    problems: &mut Vec<String>,
) -> Option<RouteConfig> {
    let before = problems.len();
    let own = route == "R13";
    for k in obj.keys().filter(|k| {
        !TOKEN_KEYS.contains(&k.as_str()) && !(own && ["base_url", "model"].contains(&k.as_str()))
    }) {
        problems.push(format!("{route}: unknown setting {k:?}"));
    }
    let (mut base_url, mut model) = (None, None);
    if own {
        match obj
            .get("base_url")
            .and_then(Value::as_str)
            .map(crate::custom_ai::check_address)
        {
            Some(Ok(a)) => base_url = Some(a.base),
            Some(Err(e)) => problems.push(format!("{route}: {e}")),
            None => problems.push(format!("{route}: base_url is missing")),
        }
        match obj.get("model").map(Value::as_str) {
            None => {}
            Some(Some(m)) if crate::custom_ai::model_ok(m) => model = Some(m.to_string()),
            Some(_) => problems.push(format!("{route}: model is not a usable model name")),
        }
    }
    let mut num = |k: &str, required: bool| -> Option<f64> {
        match obj.get(k).and_then(Value::as_f64) {
            Some(n) if n.is_finite() && n >= 0.0 => Some(n),
            Some(_) => {
                problems.push(format!("{route}: {k} must be a number of zero or more"));
                None
            }
            None => {
                if required || obj.contains_key(k) {
                    problems.push(format!("{route}: {k} is missing"));
                }
                None
            }
        }
    };
    let (max_in, max_out) = (
        num("max_input_tokens", true),
        num("max_output_tokens", true),
    );
    let allow = num("allowance_tokens", false);
    let (Some(max_in), Some(max_out)) = (max_in, max_out) else {
        return None;
    };
    if max_in < 1.0 {
        problems.push(format!("{route}: max_input_tokens must be at least 1"));
    }
    if max_out < 1.0 || max_out > OUTPUT_HARD_CAP as f64 {
        problems.push(format!(
            "{route}: max_output_tokens must be between 1 and {OUTPUT_HARD_CAP}"
        ));
    }
    let allowance = allow.map_or(crate::spend::DEFAULT_TOKEN_CAP as f64, |a| a);
    if allowance < max_in + max_out {
        problems.push(format!("{route}: allowance_tokens must be at least max_input_tokens plus max_output_tokens, so one request can fit"));
    }
    if problems.len() > before {
        return None;
    }
    // One micro-dollar per token: the dollar arithmetic then reserves and charges tokens.
    Some(RouteConfig {
        prices: Prices {
            input_per_mtok: 1_000_000,
            output_per_mtok: 1_000_000,
            tool_call: 0,
        },
        bounds: Bounds {
            max_input_tokens: max_in as u64,
            max_output_tokens: max_out as u64,
            max_tool_calls: 0,
            max_cost_micro: (max_in + max_out) as u64,
        },
        allowance_micro: allowance as u64,
        token_mode: true,
        base_url,
        model,
    })
}

/// Reads the file's JSON: `{"R10": {"input_per_mtok_usd": 0.14, ...}}`. Every problem is reported at once.
pub fn parse(v: &Value) -> Result<BTreeMap<String, RouteConfig>, Vec<String>> {
    let Some(map) = v.as_object() else {
        return Err(vec![
            "the file must be an object keyed by route, such as R10".into(),
        ]);
    };
    let (mut out, mut problems) = (BTreeMap::new(), Vec::new());
    for (route, body) in map {
        if Provider::for_route(route).is_none() {
            problems.push(format!(
                "{route}: not an API route. Use R3, R5, R8, R9, R10, R11, R12, or R13"
            ));
            continue;
        }
        let Some(obj) = body.as_object() else {
            problems.push(format!("{route}: must be an object"));
            continue;
        };
        if route == "R13" && DOLLAR_KEYS.iter().any(|k| obj.contains_key(*k)) {
            problems.push(format!(
                "{route}: your own AI service counts tokens only; dollar prices are not used"
            ));
            continue;
        }
        if !DOLLAR_KEYS.iter().any(|k| obj.contains_key(*k)) {
            if let Some(c) = token_route(route, obj, &mut problems) {
                out.insert(route.clone(), c);
            }
            continue;
        }
        for k in obj.keys().filter(|k| !KEYS.contains(&k.as_str())) {
            problems.push(format!("{route}: unknown setting {k:?}"));
        }
        let num = |k: &str, problems: &mut Vec<String>| -> Option<f64> {
            match obj.get(k).and_then(Value::as_f64) {
                Some(n) if n.is_finite() && n >= 0.0 => Some(n),
                Some(_) => {
                    problems.push(format!("{route}: {k} must be a number of zero or more"));
                    None
                }
                None => {
                    problems.push(format!("{route}: {k} is missing"));
                    None
                }
            }
        };
        let vals: Vec<Option<f64>> = KEYS.iter().map(|k| num(k, &mut problems)).collect();
        let [Some(inp), Some(outp), Some(tool), Some(max_in), Some(max_out), Some(max_calls), Some(max_req), Some(allow)] =
            vals[..]
        else {
            continue;
        };
        let before = problems.len();
        if inp <= 0.0 || outp <= 0.0 {
            problems.push(format!("{route}: both token prices must be above zero"));
        }
        if max_in < 1.0 {
            problems.push(format!("{route}: max_input_tokens must be at least 1"));
        }
        if max_out < 1.0 || max_out > OUTPUT_HARD_CAP as f64 {
            problems.push(format!(
                "{route}: max_output_tokens must be between 1 and {OUTPUT_HARD_CAP}"
            ));
        }
        if max_req <= 0.0 || allow < max_req {
            problems.push(format!("{route}: max_request_usd must be above zero and the allowance must cover at least one request"));
        }
        if problems.len() > before {
            continue;
        }
        out.insert(
            route.clone(),
            RouteConfig {
                prices: Prices {
                    input_per_mtok: micro(inp),
                    output_per_mtok: micro(outp),
                    tool_call: micro(tool),
                },
                bounds: Bounds {
                    max_input_tokens: max_in as u64,
                    max_output_tokens: max_out as u64,
                    max_tool_calls: max_calls as u32,
                    max_cost_micro: micro(max_req),
                },
                allowance_micro: micro(allow),
                token_mode: false,
                base_url: None,
                model: None,
            },
        );
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// What a route gets when a person saves its key in Settings and the route has no settings yet: it counts tokens,
/// with room for a whole project's context and a long document, and no dollar price is assumed. The person sets a
/// dollar limit at the provider, and the shared token cap still applies.
pub const KEY_DEFAULTS: (u64, u64) = (100_000, 32_000);

/// The settings file's text with `route` added at `KEY_DEFAULTS`, or unchanged if the route is already there. A file
/// that cannot be read as settings is never overwritten.
pub fn with_key_defaults(text: Option<&str>, route: &str) -> Result<String, String> {
    let mut v: Value = match text {
        Some(t) if !t.trim().is_empty() => serde_json::from_str(t)
            .map_err(|e| format!("api-routes.json cannot be read, so it was left alone: {e}"))?,
        _ => Value::Object(Default::default()),
    };
    let obj = v
        .as_object_mut()
        .ok_or("api-routes.json is not an object, so it was left alone")?;
    obj.entry(route.to_string()).or_insert_with(|| {
        // Input is counted as one token a byte, about four times the real count, so 200,000 is roughly 50,000 real
        // tokens: room for a few long sources on any model OpenRouter lists.
        let (input, output) = if route == "R12" {
            (200_000, 8_000)
        } else {
            KEY_DEFAULTS
        };
        serde_json::json!({"max_input_tokens": input, "max_output_tokens": output})
    });
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// The settings file's text with the person's own service set to `base_url` and `model`, keeping its token limits
/// if it had some. Both are checked first; nothing is written on a problem.
pub fn with_custom_ai(text: Option<&str>, base_url: &str, model: &str) -> Result<String, String> {
    let a = crate::custom_ai::check_address(base_url)?;
    if !crate::custom_ai::model_ok(model.trim()) {
        return Err(
            "Type the model's name exactly as the service lists it, such as llama3.3 or qwen3:8b."
                .into(),
        );
    }
    let mut v: Value = match text {
        Some(t) if !t.trim().is_empty() => serde_json::from_str(t)
            .map_err(|e| format!("api-routes.json cannot be read, so it was left alone: {e}"))?,
        _ => Value::Object(Default::default()),
    };
    let obj = v
        .as_object_mut()
        .ok_or("api-routes.json is not an object, so it was left alone")?;
    let entry = obj.entry("R13").or_insert_with(
        || serde_json::json!({"max_input_tokens": KEY_DEFAULTS.0, "max_output_tokens": 8_000}),
    );
    entry["base_url"] = Value::String(a.base);
    entry["model"] = Value::String(model.trim().to_string());
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// The settings file's text without the person's own service.
pub fn without_custom_ai(text: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(text)
        .map_err(|e| format!("api-routes.json cannot be read, so it was left alone: {e}"))?;
    if let Some(o) = v.as_object_mut() {
        o.remove("R13");
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteState {
    Ready,
    NoKey,
    NotConfigured,
}

/// The state of every API route, for the interface.
pub fn states(
    cfg: &BTreeMap<String, RouteConfig>,
    key_present: &dyn Fn(&str) -> bool,
) -> Vec<(String, RouteState)> {
    ["R3", "R5", "R8", "R9", "R10", "R11", "R12", "R13"]
        .iter()
        .map(|r| {
            let p = Provider::for_route(r).expect("a listed route");
            // The person's own service may need no key.
            let key = *r == "R13" || key_present(p.credential());
            let st = match (cfg.contains_key(*r), key) {
                (false, _) => RouteState::NotConfigured,
                (true, false) => RouteState::NoKey,
                (true, true) => RouteState::Ready,
            };
            (r.to_string(), st)
        })
        .collect()
}

/// An engine for each route that is configured and has its key. `make` builds the engine for one provider.
pub fn ready_engines(
    cfg: &BTreeMap<String, RouteConfig>,
    key_present: &dyn Fn(&str) -> bool,
    make: &mut dyn FnMut(Provider, &RouteConfig) -> ApiEngine,
) -> Vec<(&'static str, Box<dyn Engine>)> {
    states(cfg, key_present)
        .into_iter()
        .filter(|(_, s)| *s == RouteState::Ready)
        .filter_map(|(r, _)| {
            let p = Provider::for_route(&r)?;
            Some((p.route(), Box::new(make(p, &cfg[&r])) as Box<dyn Engine>))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn your_own_ai_service_is_saved_checked_counts_tokens_and_needs_no_key() {
        let t = with_custom_ai(None, "http://localhost:11434/v1/", "qwen3:8b").unwrap();
        let cfg = parse(&serde_json::from_str(&t).unwrap()).unwrap();
        assert_eq!(
            cfg["R13"].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(cfg["R13"].model.as_deref(), Some("qwen3:8b"));
        assert!(cfg["R13"].token_mode);
        let st = states(&cfg, &|_| false);
        assert!(st.contains(&("R13".into(), RouteState::Ready)), "{st:?}");
        assert!(with_custom_ai(None, "http://example.com/v1", "m").is_err());
        assert!(with_custom_ai(None, "https://example.com/v1", "a b").is_err());
        // A hand-edited file with a bad address, a missing one, or dollar prices is refused.
        for bad in [
            r#"{"R13": {"max_input_tokens": 5, "max_output_tokens": 6, "base_url": "http://example.com"}}"#,
            r#"{"R13": {"max_input_tokens": 5, "max_output_tokens": 6}}"#,
            r#"{"R13": {"input_per_mtok_usd": 1, "base_url": "https://x.example"}}"#,
            r#"{"R12": {"max_input_tokens": 5, "max_output_tokens": 6, "base_url": "https://x.example"}}"#,
        ] {
            assert!(parse(&serde_json::from_str(bad).unwrap()).is_err(), "{bad}");
        }
        let gone = without_custom_ai(&t).unwrap();
        assert!(parse(&serde_json::from_str(&gone).unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_saved_key_adds_a_token_route_and_never_changes_settings_the_person_made() {
        let fresh = with_key_defaults(None, "R3").unwrap();
        let cfg = parse(&serde_json::from_str(&fresh).unwrap()).unwrap();
        assert!(cfg["R3"].token_mode, "no dollar price is assumed");
        assert_eq!(cfg["R3"].bounds.max_output_tokens, 32_000);
        let free: Value = serde_json::from_str(&with_key_defaults(None, "R12").unwrap()).unwrap();
        // Input is counted as one token a byte, about four times the real count, so a step with a few long sources
        // still fits.
        assert_eq!(free["R12"]["max_input_tokens"], 200_000);
        assert_eq!(free["R12"]["max_output_tokens"], 8_000);
        let mine = r#"{"R3": {"max_input_tokens": 5, "max_output_tokens": 6}, "R10": {"max_input_tokens": 1, "max_output_tokens": 1}}"#;
        let kept = with_key_defaults(Some(mine), "R3").unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&kept).unwrap(),
            serde_json::from_str::<Value>(mine).unwrap()
        );
        let added: Value =
            serde_json::from_str(&with_key_defaults(Some(mine), "R5").unwrap()).unwrap();
        assert_eq!(added["R5"]["max_input_tokens"], 100_000);
        assert_eq!(added["R10"]["max_input_tokens"], 1);
        assert!(with_key_defaults(Some("{not json"), "R3").is_err());
        assert!(with_key_defaults(Some("[1]"), "R3").is_err());
    }
    use crate::api_engine::NoNetwork;
    use crate::connector::{Credential, Credentials};
    use serde_json::json;

    fn good() -> Value {
        json!({"R10": {"input_per_mtok_usd": 0.14, "output_per_mtok_usd": 0.28, "tool_call_usd": 0.0,
            "max_input_tokens": 100000, "max_output_tokens": 4000, "max_tool_calls": 0, "max_request_usd": 0.05, "allowance_usd": 5.0}})
    }

    #[test]
    fn dollars_become_micro_dollars_and_the_bounds_carry_over() {
        let c = parse(&good()).unwrap();
        let r = c["R10"].clone();
        assert_eq!(
            (
                r.prices.input_per_mtok,
                r.prices.output_per_mtok,
                r.prices.tool_call
            ),
            (140_000, 280_000, 0)
        );
        assert_eq!(
            (
                r.bounds.max_input_tokens,
                r.bounds.max_output_tokens,
                r.bounds.max_tool_calls,
                r.bounds.max_cost_micro
            ),
            (100_000, 4000, 0, 50_000)
        );
        assert_eq!(r.allowance_micro, 5_000_000);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let v = json!({
            "R4": {},
            "R5": {"input_per_mtok_usd": 0.0, "output_per_mtok_usd": 1.0, "tool_call_usd": 0.0, "max_input_tokens": 10, "max_output_tokens": 99999, "max_tool_calls": 0, "max_request_usd": 1.0, "allowance_usd": 0.5, "extra": 1},
            "R8": {"input_per_mtok_usd": 1.0},
            "R9": "nope",
        });
        let e = parse(&v).unwrap_err().join("\n");
        for want in [
            "R4: not an API route",
            "R5: unknown setting \"extra\"",
            "R5: both token prices",
            "R5: max_output_tokens",
            "R5: max_request_usd",
            "R8: output_per_mtok_usd is missing",
            "R9: must be an object",
        ] {
            assert!(e.contains(want), "missing {want:?} in:\n{e}");
        }
        assert!(parse(&json!([])).is_err());
        assert_eq!(parse(&json!({})).unwrap().len(), 0);
        let negative = json!({"R10": {"input_per_mtok_usd": -1.0, "output_per_mtok_usd": 1.0, "tool_call_usd": 0.0, "max_input_tokens": 1, "max_output_tokens": 1, "max_tool_calls": 0, "max_request_usd": 1.0, "allowance_usd": 1.0}});
        assert!(parse(&negative).unwrap_err()[0].contains("zero or more"));
    }

    struct Keys(Vec<&'static str>);
    impl Credentials for Keys {
        fn get(&self, name: &str) -> Option<Credential> {
            self.0.contains(&name).then(|| Credential::new("k", &[]))
        }
    }

    #[test]
    fn a_route_runs_only_with_both_its_settings_and_its_key() {
        let mut cfg = parse(&good()).unwrap();
        cfg.insert("R11".into(), cfg["R10"].clone());
        let keys = Keys(vec!["deepseek-api", "openai-api"]);
        let present = |n: &str| keys.get(n).is_some();
        let st: BTreeMap<String, RouteState> = states(&cfg, &present).into_iter().collect();
        assert_eq!(st["R10"], RouteState::Ready);
        assert_eq!(st["R11"], RouteState::NoKey, "configured, no key");
        assert_eq!(
            st["R5"],
            RouteState::NotConfigured,
            "a key alone does not switch a route on"
        );
        assert_eq!(st["R12"], RouteState::NotConfigured);
        let mut built = Vec::new();
        let engines = ready_engines(&cfg, &present, &mut |p, c| {
            built.push(p);
            ApiEngine::new(
                p,
                Box::new(NoNetwork),
                Box::new(Keys(vec![])),
                c.bounds,
                c.prices,
                c.allowance_micro,
            )
        });
        assert_eq!(
            engines.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            vec!["R10"]
        );
        assert_eq!(built, vec![Provider::DeepSeek]);
    }

    #[test]
    fn a_route_with_only_token_limits_counts_tokens_and_needs_no_prices() {
        let v = json!({
            "R5": {"max_input_tokens": 50000, "max_output_tokens": 4000},
            "R10": {"max_input_tokens": 100000, "max_output_tokens": 8000, "allowance_tokens": 250000},
        });
        let c = parse(&v).unwrap();
        let r = c["R5"].clone();
        assert!(r.token_mode);
        // Tokens stand in for micro-dollars, one for one, so the same reservation arithmetic counts tokens.
        assert_eq!(
            (
                r.prices.input_per_mtok,
                r.prices.output_per_mtok,
                r.prices.tool_call
            ),
            (1_000_000, 1_000_000, 0)
        );
        assert_eq!(
            (
                r.bounds.max_input_tokens,
                r.bounds.max_output_tokens,
                r.bounds.max_tool_calls
            ),
            (50_000, 4_000, 0)
        );
        assert_eq!(
            r.bounds.max_cost_micro, 54_000,
            "the most one request may use is its input and output limits together"
        );
        assert_eq!(
            r.allowance_micro,
            crate::spend::DEFAULT_TOKEN_CAP,
            "an unset allowance is the shared default, not unlimited"
        );
        assert_eq!(c["R10"].allowance_micro, 250_000);
        assert!(!parse(&good()).unwrap()["R10"].token_mode);
    }

    #[test]
    fn token_limits_are_checked_and_mixing_in_one_dollar_setting_means_all_of_them_are_needed() {
        let e = parse(&json!({"R5": {"max_input_tokens": 0, "max_output_tokens": 99999}}))
            .unwrap_err()
            .join("\n");
        assert!(
            e.contains("R5: max_input_tokens must be at least 1")
                && e.contains("R5: max_output_tokens"),
            "{e}"
        );
        let e = parse(&json!({"R5": {"max_output_tokens": 100}}))
            .unwrap_err()
            .join("\n");
        assert!(e.contains("R5: max_input_tokens is missing"), "{e}");
        let e = parse(&json!({"R5": {"max_input_tokens": 10, "max_output_tokens": 10, "max_request_usd": 0.5}})).unwrap_err().join("\n");
        assert!(
            e.contains("input_per_mtok_usd is missing"),
            "a dollar setting switches the route to dollars: {e}"
        );
        let e = parse(&json!({"R5": {"max_input_tokens": 10, "max_output_tokens": 10, "allowance_tokens": 0}})).unwrap_err().join("\n");
        assert!(e.contains("allowance_tokens must be at least"), "{e}");
        let e =
            parse(&json!({"R5": {"max_input_tokens": 10, "max_output_tokens": 10, "bogus": 1}}))
                .unwrap_err()
                .join("\n");
        assert!(e.contains("unknown setting \"bogus\""), "{e}");
        let mut dollars = good();
        dollars["R10"]["allowance_tokens"] = json!(5);
        assert!(parse(&dollars)
            .unwrap_err()
            .join("\n")
            .contains("unknown setting \"allowance_tokens\""));
    }
}
