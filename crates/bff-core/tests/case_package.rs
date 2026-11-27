//! A source is imported, a stage is completed, and the package exports with its evidence and sources.
//! No model runs.

use bff_core::engine::ARTIFACT_FILE;
use bff_core::flow::Studio;
use bff_core::lead::parse_plan;
use bff_core::package::{export, ExportOpts};
use bff_core::sources::SourceStore;
use bff_core::workspace::Project;
use serde_json::json;

const DRAFT: &str = "## Problem\nA small team wants to try a coding assistant.\n## Audience\nThe team lead.\n## Constraints\nTwo weeks.\n## Rubric\nClear.\n## Assumptions\nThe team has four people.\n## Questions\nWho pays?\n## Success criteria\nAdoption by week two.\n";

#[test]
fn a_case_goes_from_source_to_checked_claims_to_an_exported_package() {
    let base = std::env::temp_dir().join(format!("bff-case-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut studio =
        Studio::open(Project::create(&base, "case", "Frame the pilot case.", 1).unwrap()).unwrap();
    let plan = json!({"stages": [{"id": "frame", "title": "Frame the case", "owner_role": "analyst"}],
        "team": [{"role": "analyst", "route": "R1", "model": "gpt-6-luna", "effort": "low"}]});
    studio
        .set_plan(parse_plan("Frame the pilot case.", &plan).unwrap(), 2)
        .unwrap();
    studio.approve("Nitish", 3).unwrap();
    let d = studio.dispatch("frame", 4).unwrap();
    std::fs::write(d.dir.join(ARTIFACT_FILE), DRAFT).unwrap();
    studio.complete(&d, ARTIFACT_FILE, "frame", &[], 5).unwrap();
    // A source is imported and kept.
    let mut sources = SourceStore::open(&studio.project.root).unwrap();
    sources
        .import(
            "pricing.txt",
            b"Each license costs $5 per month. The pilot lasts two weeks.",
        )
        .unwrap();

    // Export. The evidence travels with the package, and the omissions and lessons are in the run notes.
    let evidence_json = json!([{"id": "f1", "class": "E2", "passed": true, "notes": []}]);
    let run = "Open the artifact in artifacts/. Failed checks are listed in evidence.json.";
    let out = base.join("package");
    let m = export(
        &studio,
        &out,
        &ExportOpts {
            run_instructions: run,
            evidence: Some(&evidence_json),
            prototype_dir: None,
            code_dir: None,
            extras: &[],
        },
    )
    .unwrap();
    assert!(out.join("evidence.json").is_file() && out.join("manifest.json").is_file());
    assert!(std::fs::read_to_string(out.join("RUN.md"))
        .unwrap()
        .contains("evidence.json"));
    assert!(m.missing.is_empty(), "{:?}", m.missing);
    assert!(m.files.iter().any(|f| f.path.starts_with("sources/src-1/")));
}
