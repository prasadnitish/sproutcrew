//! The M3 pieces together on replay: a source is imported, an artifact makes claims, a seeded unit error and
//! a missing citation are caught, the lesson list and omissions are written, and the package exports with
//! its evidence. No model runs.

use bff_core::calc::{reproduce, Claim, Input};
use bff_core::engine::ARTIFACT_FILE;
use bff_core::evidence::{classify, parse_claims, summarize, CalcResult, Context};
use bff_core::flow::Studio;
use bff_core::lead::parse_plan;
use bff_core::lessons::{LessonClass, Lessons};
use bff_core::package::{export, ExportOpts};
use bff_core::playbook::{missing_sections, Playbook};
use bff_core::sources::SourceStore;
use bff_core::timebox::Timebox;
use bff_core::workspace::Project;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

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
    let artifact_sha = format!("{:x}", Sha256::digest(DRAFT.as_bytes()));

    // The draft has every required heading for case framing.
    assert!(missing_sections(Playbook::CaseFraming, &[], DRAFT)
        .unwrap()
        .is_empty());

    // A source is imported and kept.
    let mut sources = SourceStore::open(&studio.project.root).unwrap();
    sources
        .import(
            "pricing.txt",
            b"Each license costs $5 per month. The pilot lasts two weeks.",
        )
        .unwrap();

    // The artifact's claims: one cited fact, one with a span that is not there, one calculation with a
    // seeded unit error, and one calculation that is right.
    let claims = parse_claims(&json!({"claims": [
        {"id": "f1", "text": "A license costs $5 a month", "kind": "fact", "source": "src-1#p1", "quote": "costs $5 per month"},
        {"id": "f2", "text": "The pilot lasts a month", "kind": "fact", "source": "src-1#p1", "quote": "lasts a month"},
        {"id": "k1", "text": "Four licenses cost $240 a month", "kind": "calculation"},
        {"id": "k2", "text": "Four licenses cost $20 a month", "kind": "calculation"},
        {"id": "a1", "text": "The team has four people", "kind": "assumption"}
    ]})).unwrap();
    let inputs = vec![
        Input {
            name: "licenses".into(),
            value: 4.0,
            unit: "".into(),
        },
        Input {
            name: "price".into(),
            value: 5.0,
            unit: "usd/month".into(),
        },
    ];
    let calc = |id: &str, formula: &str, v: f64, unit: &str| CalcResult {
        record: reproduce(
            &inputs,
            &Claim {
                id: id.into(),
                formula: formula.into(),
                stated_value: v,
                stated_unit: unit.into(),
                tolerance: 0.001,
            },
        ),
        artifact_sha256: artifact_sha.clone(),
    };
    let calcs = vec![
        calc(
            "k1",
            "licenses * price * 12 [month/year]",
            240.0,
            "usd/month",
        ),
        calc("k2", "licenses * price", 20.0, "usd/month"),
    ];
    let confirmed: BTreeSet<String> = BTreeSet::new();
    let ev = classify(
        &claims,
        &Context {
            sources: &sources,
            calcs: &calcs,
            artifact_sha256: &artifact_sha,
            confirmed: &confirmed,
        },
    )
    .unwrap();
    let by = |id: &str| ev.iter().find(|e| e.id == id).unwrap();
    assert!(by("f1").passed, "the cited span exists");
    assert!(
        !by("f2").passed,
        "a span that is not in the source fails E2"
    );
    assert!(
        !by("k1").passed,
        "the seeded unit error is caught by reproduction"
    );
    assert!(by("k1").notes.iter().any(|n| n.contains("UnitMismatch")));
    assert!(by("k2").passed);
    assert!(
        !by("a1").passed,
        "an unconfirmed assumption is not evidence"
    );
    let s = summarize(&ev);
    assert_eq!((s.e1, s.e2, s.e3, s.m, s.failed), (1, 1, 0, 3, 3));

    // Lessons and omissions.
    let mut lessons = Lessons::open(&studio.project.root).unwrap();
    lessons
        .add(
            "Always state the unit next to a monthly or yearly figure",
            LessonClass::Process,
            None,
        )
        .unwrap();
    let mut clock = Timebox::new(0, 3600);
    assert!(!clock.may_start("competitor table", true, 3500));

    // Export. The evidence travels with the package, and the omissions and lessons are in the run notes.
    let evidence_json = json!(ev.iter().map(|e| json!({"id": e.id, "class": format!("{:?}", e.class), "passed": e.passed, "notes": e.notes})).collect::<Vec<_>>());
    let run = format!(
        "Open the artifact in artifacts/. Failed checks are listed in evidence.json.\n\n{}\n{}",
        clock.omissions_markdown(),
        lessons.session_end_markdown()
    );
    let out = base.join("package");
    let m = export(
        &studio,
        &out,
        &ExportOpts {
            run_instructions: &run,
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
        .contains("competitor table"));
    assert!(m.missing.is_empty(), "{:?}", m.missing);
    assert!(m.files.iter().any(|f| f.path.starts_with("sources/src-1/")));
}
