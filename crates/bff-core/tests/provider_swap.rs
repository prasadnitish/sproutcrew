//! Changing a role from one provider to another must not break the project: earlier artifacts and reviews
//! stay, the new plan needs a fresh approval, and cost stays attributed to the route that ran each task,
//! with tokens and dollars kept apart. The API route here runs against a recorded answer and a fake
//! transport, with no key and no network.

use bff_core::api_engine::{ApiEngine, ApiTransport, PostFailure};
use bff_core::apiroute::{Bounds, Prices, Provider};
use bff_core::app::App;
use bff_core::connector::{Credential, Credentials};
use bff_core::engine::{ReplayEngine, RoutedEngine};
use serde_json::{json, Value};

struct Recorded;
impl ApiTransport for Recorded {
    fn post(
        &mut self,
        _: &str,
        _: &[(String, String)],
        _: &Value,
    ) -> Result<(u16, Value), PostFailure> {
        Ok((
            200,
            json!({"status": "completed", "output": [{"type": "message", "content": [{"type": "output_text", "text": "# Polished\nbody"}]}],
                   "usage": {"input_tokens": 500, "output_tokens": 100}}),
        ))
    }
}

struct Key;
impl Credentials for Key {
    fn get(&self, _: &str) -> Option<Credential> {
        Some(Credential::new("test-key", &[]))
    }
}

fn plan(writer_route: &str, writer_model: &str) -> Value {
    json!({"brief": "Write a note.",
        "stages": [
            {"id": "draft", "title": "Draft", "owner_role": "writer"},
            {"id": "polish", "title": "Polish", "owner_role": "writer"}],
        "team": [
            {"role": "writer", "route": writer_route, "model": writer_model, "effort": "low"},
            {"role": "critic", "route": "R2", "model": "claude-sonnet-5-5", "effort": "low"}]})
}

fn engine() -> RoutedEngine {
    let api = ApiEngine::new(
        Provider::OpenAi,
        Box::new(Recorded),
        Box::new(Key),
        Bounds {
            max_input_tokens: 10_000,
            max_output_tokens: 1000,
            max_tool_calls: 0,
            max_cost_micro: 100_000,
        },
        Prices {
            input_per_mtok: 2_000_000,
            output_per_mtok: 8_000_000,
            tool_call: 10_000,
        },
        1_000_000,
    );
    RoutedEngine::new(Box::new(ReplayEngine), Box::new(ReplayEngine))
        .with_route("R5", Box::new(api))
}

#[test]
fn moving_the_writer_to_another_provider_keeps_project_state_reviews_and_cost_attribution() {
    let base = std::env::temp_dir().join(format!("bff-swap-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut app = App::new(base);
    let mut e = engine();
    app.create_project("case", "Write a note.", 1).unwrap();
    app.save_plan(plan("R1", "gpt-6-luna"), 2).unwrap();
    app.approve("Nitish", 3).unwrap();
    app.run_stage(&mut e, "draft", 4).unwrap();
    assert_eq!(
        app.review(&mut e, "draft", "critic", 5).unwrap()["outcome"],
        "reviewed_no_change"
    );
    let before = app.state("replay").unwrap();
    let draft_head = before["review"]["draft"]["head"].clone();

    // The writer moves to the API route. The plan changed, so the approval no longer holds.
    app.save_plan(plan("R5", "gpt-6.1-terra"), 6).unwrap();
    assert_eq!(app.state("replay").unwrap()["gate"], "plan_changed");
    assert!(
        app.run_stage(&mut e, "polish", 7).is_err(),
        "no run on a changed plan"
    );
    app.approve("Nitish", 8).unwrap();
    let out = app.run_stage(&mut e, "polish", 9).unwrap();
    assert!(out.artifact.is_some(), "{}", out.note);
    assert_eq!(
        app.review(&mut e, "polish", "critic", 10).unwrap()["outcome"],
        "reviewed_no_change"
    );

    let after = app.state("replay").unwrap();
    // Earlier work and its review are untouched.
    assert_eq!(after["review"]["draft"]["head"], draft_head);
    assert_eq!(after["review"]["draft"]["reviewers"]["critic"], "valid");
    assert_eq!(after["review"]["polish"]["owner"], "writer");
    assert_eq!(after["artifacts"].as_array().unwrap().len(), 2);

    // Cost is attributed to the route that ran each task, and the meters stay apart.
    let c = &after["cost_by_route"];
    assert_eq!(c["R5"]["tokens"], 600.0);
    assert!(c["R5"]["api_dollars"].as_f64().unwrap() > 0.0);
    assert!(
        c["R1"]["api_dollars"].is_null(),
        "the replay route has no dollars"
    );
    assert!(
        c["R1"]["tokens"].is_null() && c["R1"]["unknown_token_steps"].as_u64().unwrap() >= 1,
        "replay usage is unknown, not zero"
    );
    assert!(c["R2"]["unknown_token_steps"].as_u64().unwrap() >= 1);
    let tokens = after["meters"]["tokens"].as_str().unwrap();
    assert!(
        tokens.starts_with("600") && tokens.contains("unknown"),
        "{tokens}"
    );
    assert_eq!(
        after["meters"]["api_dollars"]
            .as_str()
            .unwrap()
            .trim_start_matches('$'),
        "0.0018"
    );

    // After a restart the same state comes back from the journal.
    app.open_project("case").unwrap();
    let again = app.state("replay").unwrap();
    assert_eq!(again["review"], after["review"]);
    assert_eq!(again["cost_by_route"], after["cost_by_route"]);
}
