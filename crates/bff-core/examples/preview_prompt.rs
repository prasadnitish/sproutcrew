//! Prints what a worker would be told for one step of an existing project, with no model call. It only reads the
//! project. Usage: `cargo run -p bff-core --example preview_prompt -- <project folder> <step id>`.
use bff_core::plan::Plan;
use bff_core::workspace::Project;

fn main() {
    let mut a = std::env::args().skip(1);
    let (Some(folder), Some(step)) = (a.next(), a.next()) else {
        eprintln!("usage: preview_prompt <project folder> <step id>");
        std::process::exit(2);
    };
    let project = Project::open(std::path::Path::new(&folder)).expect("open the project");
    // The newest plan is in the journal.
    let journal = bff_core::journal::Journal::open(&project.journal_path()).expect("journal");
    let plan: Plan = journal
        .events()
        .expect("events")
        .into_iter()
        .rev()
        .find(|e| e.kind == "plan.set")
        .and_then(|e| serde_json::from_value(e.payload["plan"].clone()).ok())
        .expect("a plan");
    let stage = plan
        .stages
        .iter()
        .find(|s| s.id == step)
        .expect("that step");
    let material = bff_core::context::build(&project.root, &plan, &step).expect("material");
    let prompt = bff_core::lead::worker_prompt_for(
        &plan.brief,
        &stage.title,
        &stage.guide,
        &material,
        "artifact.md",
        false,
    );
    // Print the instructions up to the material, then the header of each block, not the whole files.
    let (rules, _) = prompt.split_once("\n## ").unwrap_or((&prompt, ""));
    println!("{rules}");
    for line in prompt.lines() {
        if line.starts_with("<<DATA ") || line.starts_with("<<REQ ") {
            println!("{line}");
        }
    }
    println!("--- {} characters in all", prompt.chars().count());
}
