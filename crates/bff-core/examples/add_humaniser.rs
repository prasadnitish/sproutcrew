//! Adds the humaniser steps to a case-study project that was made before they existed, using the wording the
//! case study now has, and approves the new plan as Nitish. Then runs the two new steps and the final check live.
//! Run: cargo run -p bff-core --example add_humaniser -- <projects-dir> <project>

use bff_core::app::App;
use bff_core::claude_engine::ClaudeEngine;
use bff_core::clock::now;
use bff_core::codex_engine::CodexEngine;
use bff_core::engine::RoutedEngine;
use bff_core::plan::Plan;
use bff_core::workflows::{plan_for, Workflow};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: add_humaniser <projects-dir> <project>");
        std::process::exit(2);
    }
    let mut app = App::new(PathBuf::from(&a[1]));
    app.open_project(&a[2]).expect("open the project");
    let state = app.state("live").expect("state");
    let mut plan: Plan = serde_json::from_value(state["plan"].clone()).expect("a plan");
    if plan.stages.iter().any(|s| s.id == "polish_document") {
        println!("already has the humaniser steps");
    } else {
        let template = plan_for(
            Workflow::CaseStudy,
            &plan.brief,
            ("R1", "gpt-6.1-terra", "medium"),
            ("R2", "claude-sonnet-5-5", "high"),
        )
        .expect("the template");
        let stage = |id: &str| {
            template
                .stages
                .iter()
                .find(|s| s.id == id)
                .cloned()
                .expect("a template step")
        };
        let at = plan
            .stages
            .iter()
            .position(|s| s.id == "deck")
            .expect("a deck step")
            + 1;
        plan.stages.insert(at, stage("polish_document"));
        plan.stages.insert(at + 1, stage("polish_deck"));
        let final_at = plan
            .stages
            .iter()
            .position(|s| s.id == "final")
            .expect("a final step");
        plan.stages[final_at] = stage("final");
        let editor = plan
            .team
            .iter()
            .find(|t| t.role == "editor")
            .expect("an editor")
            .clone();
        plan.team.push(bff_core::plan::Assignment {
            role: "humaniser".into(),
            ..editor
        });
        app.save_plan(serde_json::to_value(&plan).unwrap(), now())
            .expect("the plan is valid");
        app.approve("Nitish", now()).expect("approved");
        println!("added polish_document and polish_deck, and approved the plan");
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let work = std::env::temp_dir().join(format!("bff-human-{}", std::process::id()));
    let codex = CodexEngine {
        codex: PathBuf::from("/opt/homebrew/bin/codex"),
        home: home.clone(),
        scratch: work.join("scratch-codex"),
        state_dir: work.join("state"),
        lead_model: "gpt-6.1-terra".into(),
        lead_effort: bff_core::codex::DEFAULT_CODEX_EFFORT.into(),
        timeout_secs: 900,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let claude = ClaudeEngine {
        claude: PathBuf::from(&home).join(".local/bin/claude"),
        home,
        scratch: work.join("scratch-claude"),
        state_dir: work.join("state"),
        timeout_secs: 900,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut engine = RoutedEngine::new(Box::new(codex), Box::new(claude));
    for step in ["polish_document", "polish_deck", "final"] {
        let started = std::time::Instant::now();
        let r = app.run_stage(&mut engine, step, now());
        let ok = r.as_ref().map(|o| o.artifact.is_some()).unwrap_or(false);
        println!(
            "{step}: {:?} in {}s",
            r.map(|o| (o.artifact.is_some(), o.note)),
            started.elapsed().as_secs()
        );
        if !ok {
            println!("stopped after a failure");
            return;
        }
    }
}
