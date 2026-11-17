//! Launch contract with a deterministic HTTP double. This tests the real API request and project pipeline,
//! not inference quality or account availability; those still need a live key.
use bff_core::{
    api_engine::{ApiEngine, ApiTransport, PostFailure},
    apiroute::Provider,
    app::App,
    connector::{Credential, Credentials},
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct Keys;
impl Credentials for Keys {
    fn get(&self, name: &str) -> Option<Credential> {
        (name == "openrouter-api").then(|| Credential::new("test-key-only", &[]))
    }
}
struct Transport(Arc<Mutex<Vec<Value>>>);
impl ApiTransport for Transport {
    fn post(
        &mut self,
        url: &str,
        _: &[(String, String)],
        body: &Value,
    ) -> Result<(u16, Value), PostFailure> {
        assert_eq!(url, "https://openrouter.ai/api/v1/chat/completions");
        self.0.lock().unwrap().push(body.clone());
        Ok((
            200,
            json!({"choices":[{"finish_reason":"stop","message":{"content":"```markdown\n# Clear writing\n\nReaders prefer clear prose. The source supports this statement.\n```"}}],"usage":{"prompt_tokens":1000,"completion_tokens":100}}),
        ))
    }
}

#[test]
fn one_free_api_key_runs_writing_with_approval_memory_reopen_and_a_real_export() {
    let base = std::env::temp_dir().join(format!("sproutcrew-launch-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let source = base.join("research.md");
    std::fs::write(&source, "Readers prefer clear prose.").unwrap();
    let mut app = App::new(base.clone());
    app.create_project("writing", "Write a short article for founders.", 1)
        .unwrap();
    app.add_source_path(source.to_str().unwrap()).unwrap();
    app.set_runnable(vec!["R12".into()]);
    app.use_workflow(
        "writing",
        ("R12", "openrouter/free", "medium"),
        ("R12", "openrouter/free", "high"),
        2,
    )
    .unwrap();
    let memory = app.add_note("", "Our audience is founders.", 3).unwrap();
    app.replace_note(
        memory["id"].as_str().unwrap(),
        "Our audience is product managers.",
        4,
    )
    .unwrap();
    let config = bff_core::apiconfig::parse(
        &json!({"R12":{"max_input_tokens":100000,"max_output_tokens":8000}}),
    )
    .unwrap()["R12"]
        .clone();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut engine = ApiEngine::new(
        Provider::OpenRouter,
        Box::new(Transport(sent.clone())),
        Box::new(Keys),
        config.bounds,
        config.prices,
        config.allowance_micro,
    )
    .token_mode();
    assert!(app.run_stage(&mut engine, "outline", 5).is_err());
    assert!(sent.lock().unwrap().is_empty());
    app.approve("Tester", 6).unwrap();
    app.run_stage(&mut engine, "outline", 7).unwrap();
    drop(app);
    let mut app = App::new(base.clone());
    app.open_project("writing").unwrap();
    for (i, stage) in ["draft", "edit", "review", "finish"]
        .into_iter()
        .enumerate()
    {
        app.run_stage(&mut engine, stage, 8 + i as u64).unwrap();
    }
    let requests = sent.lock().unwrap();
    assert_eq!(requests.len(), 5);
    for request in requests.iter() {
        let prompt = request["messages"].to_string();
        assert!(prompt.contains("Our audience is product managers."));
        assert!(!prompt.contains("Our audience is founders."));
        assert!(!prompt.contains("When it is written, reply done."));
        assert_eq!(request["model"], "openrouter/free");
        assert_eq!(request["provider"]["max_price"]["completion"], 0);
    }
    let exported = app
        .export_package(
            "Read Final document.md.",
            &bff_core::clientfiles::Tools::default(),
            20,
        )
        .unwrap();
    let folder = std::path::Path::new(exported["path"].as_str().unwrap());
    let final_text = std::fs::read_to_string(folder.join("Final document.md")).unwrap();
    assert!(final_text.contains("Readers prefer clear prose."));
    assert!(
        !final_text.contains("```markdown"),
        "the API's whole-document envelope is not the artifact format"
    );
    assert_eq!(
        app.state("live").unwrap()["artifacts"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    std::fs::remove_dir_all(base).unwrap();
}
