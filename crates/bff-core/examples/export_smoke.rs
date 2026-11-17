//! Generate synthetic export evidence without providers, credentials or the user's application data.
use bff_core::{
    app::App,
    clientfiles::Tools,
    engine::{Engine, ReplayEngine, Request, ReviewRun, WorkerResult},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

struct Fixture;
impl Engine for Fixture {
    fn label(&self) -> &'static str {
        "replay"
    }
    fn propose_plan(
        &mut self,
        brief: &str,
    ) -> Result<(bff_core::plan::Plan, Vec<bff_core::meters::Entry>), String> {
        ReplayEngine.propose_plan(brief)
    }
    fn run_worker(&mut self, request: &Request) -> WorkerResult {
        let result = ReplayEngine.run_worker(request);
        std::fs::write(
            request.task_dir.join(bff_core::engine::ARTIFACT_FILE),
            include_str!("../tests/fixtures/office-export.md"),
        )
        .unwrap();
        result
    }
    fn run_review(&mut self, request: &Request, schema: &Value) -> ReviewRun {
        ReplayEngine.run_review(request, schema)
    }
}

pub fn run(base: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    std::fs::create_dir(base)?; // A caller-owned, new directory only. Never reset an existing project.
    let absolute = std::fs::canonicalize(base)?;
    let base = absolute.as_path();
    let mut app = App::new(base.to_path_buf());
    app.create_project(
        "export-fixture",
        "Synthetic document export verification",
        1,
    )?;
    let source = base.join("synthetic-research.md");
    std::fs::write(&source, "Fictional export evidence only. Reviewed pilot: 12 participants; immediate rollout: 8. No real users or market claims.")?;
    app.add_source_path(source.to_str().ok_or("Invalid fixture path")?)?;
    app.use_workflow(
        "case_study",
        ("R12", "openrouter/free", "medium"),
        ("R12", "openrouter/free", "high"),
        2,
    )
    .map_err(|e| format!("{e:?}"))?;
    app.approve("Fixture owner", 3)?;
    let mut engine = Fixture;
    for _ in 0..50 {
        let ready = app.ready_stages()?;
        if ready.is_empty() {
            break;
        }
        for id in ready {
            app.run_stage(&mut engine, &id, 4)?;
        }
    }
    let tools = Tools::detect(&std::env::var("HOME").unwrap_or_default());
    if tools.typst.is_none() {
        return Err("Typst is required for this full export smoke check".into());
    }
    let client = app.export_client_files(
        Some("document"),
        Some("deck"),
        "Synthetic review packet",
        &tools,
        10,
    )?;
    // Word, the tables as Excel, PDF and PowerPoint.
    assert_eq!(client["files"].as_array().unwrap().len(), 4, "{client}");
    let final_files = app.final_files(&tools)?;
    assert_eq!(
        final_files["files"].as_array().unwrap().len(),
        4,
        "{final_files}"
    );
    let report = app.export_run_report(&tools, 11)?;
    let decisions = app.export_decision_log(12)?;
    let package = app.export_package(
        "Synthetic export fixture; no model requests were made.",
        &tools,
        13,
    )?;
    assert!(
        package["missing_steps"]
            .as_array()
            .is_none_or(|a| a.is_empty()),
        "{package}"
    );
    let folder = Path::new(package["path"].as_str().unwrap());
    let manifest: bff_core::package::Manifest =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json"))?)?;
    assert!(
        manifest.missing.is_empty() && manifest.incomplete.is_empty(),
        "{manifest:?}"
    );
    for file in &manifest.files {
        let bytes = std::fs::read(folder.join(&file.path))?;
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            file.sha256,
            "{}",
            file.path
        );
        assert_eq!(bytes.len() as u64, file.bytes);
    }
    for name in [
        "Final document.docx",
        "Final document.pdf",
        "Final deck.pptx",
        "Run report.md",
        "Run report.pdf",
    ] {
        assert!(folder.join(name).is_file(), "missing {name}: {package}");
    }
    for (kind, name) in [("docx", "Final document.docx"), ("pptx", "Final deck.pptx")] {
        let text = bff_core::office::read(kind, &std::fs::read(folder.join(name))?)?
            .pages
            .join("\n");
        assert!(
            text.contains("reviewed pilot")
                && text.contains("日本語")
                && text.contains("Gather more evidence")
        );
        assert!(!text.contains("Internal fixture warning"));
    }
    Ok(
        json!({"client":client,"final":final_files,"report":report,"decisions":decisions["path"],"package":package,"verified_package_files":manifest.files.len(),"provider_requests":0}),
    )
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .ok_or("Usage: export_smoke NEW_DIRECTORY")?;
    println!("{}", serde_json::to_string_pretty(&run(Path::new(&out))?)?);
    Ok(())
}
