//! Stopping a run: the stop is recorded and shown until a step starts again, it is one item in the Decision inbox,
//! and another project cannot be opened while a step is still in flight.
use bff_core::{
    app::{App, RunHold},
    flow::Studio,
    workflows::{plan_for, Workflow},
    workspace::Project,
};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

fn base(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("sproutcrew-stopped-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn studio(base: &std::path::Path, name: &str) -> Studio {
    let mut s = Studio::open(Project::create(base, name, "A reviewed memo", 1).unwrap()).unwrap();
    s.set_plan(
        plan_for(
            Workflow::Writing,
            "A reviewed memo",
            ("R12", "openrouter/free", "medium"),
            ("R12", "openrouter/free", "high"),
        )
        .unwrap(),
        2,
    )
    .unwrap();
    s.approve("Fixture owner", 3).unwrap();
    s
}

fn stopped_items(app: &App) -> Vec<Value> {
    app.decision_inbox()["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "stopped")
        .cloned()
        .collect()
}

#[test]
fn another_project_is_not_opened_while_a_step_is_in_flight() {
    let root = base("open");
    let mut one = studio(&root, "one");
    studio(&root, "two");
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    let stage = one.plan().unwrap().stages[0].id.clone();
    one.dispatch(&stage, 4).unwrap();

    let running = app.running();
    running.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        app.open_project("two").unwrap_err(),
        "A step is still running. Try again when it finishes."
    );
    // The open project stays open, and opening it again does not mark its running step as failed.
    app.open_project("one").unwrap();
    assert!(one
        .journal
        .events()
        .unwrap()
        .iter()
        .all(|e| e.kind != "worker.failed"));
    assert_eq!(app.state("live").unwrap()["name"], "one");

    // Once the step returns, the other project opens.
    running.fetch_sub(1, Ordering::SeqCst);
    app.open_project("two").unwrap();
    assert_eq!(app.state("live").unwrap()["name"], "two");
}

#[test]
fn a_stop_shows_after_which_step_until_a_step_starts_again() {
    let root = base("stop");
    let mut s = studio(&root, "one");
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    assert!(app.state("live").unwrap()["stopped"].is_null());
    assert!(stopped_items(&app).is_empty());

    // Stopped before any step has a result.
    app.record_stop(10).unwrap();
    assert_eq!(
        app.state("live").unwrap()["stopped"],
        json!({"after_stage": null})
    );
    assert_eq!(stopped_items(&app).len(), 1);

    // A step starts again, so the project is no longer stopped.
    let first = s.plan().unwrap().stages[0].id.clone();
    let d = s.dispatch(&first, 11).unwrap();
    assert!(app.state("live").unwrap()["stopped"].is_null());
    assert!(stopped_items(&app).is_empty());

    // The step finishes, and a later stop names it.
    std::fs::write(d.dir.join("artifact.md"), "A memo.").unwrap();
    s.complete(&d, "artifact.md", &first, &[], 12).unwrap();
    app.record_stop(13).unwrap();
    assert_eq!(
        app.state("live").unwrap()["stopped"],
        json!({"after_stage": first})
    );
    let items = stopped_items(&app);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["project"], "one");
    assert_eq!(items[0]["stage"], first.as_str());
    assert_eq!(items[0]["anchor"], "run-status");
    assert!(items[0]["title"]
        .as_str()
        .unwrap()
        .starts_with("Stopped by you after step 1"));
}

#[test]
fn a_run_that_was_not_stopped_is_never_called_stopped() {
    // A step held for the person's answers, or one that failed, is not a stop the person asked for.
    let root = base("not-stopped");
    let mut s = studio(&root, "one");
    let stage = s.plan().unwrap().stages[0].id.clone();
    let d = s.dispatch(&stage, 4).unwrap();
    s.fail(&d, "stopped: the API key reached its limit", &[], 5)
        .unwrap();
    s.block_stage(&stage, "waiting for answers", 6).unwrap();
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    assert!(app.state("live").unwrap()["stopped"].is_null());
    assert!(stopped_items(&app).is_empty());
}

#[test]
fn a_new_project_is_not_made_while_a_step_is_in_flight() {
    // Making a project opens it, so a step that returns late would record its result in the new one.
    let root = base("create");
    studio(&root, "one");
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    let running = app.running();
    running.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        app.create_project("fresh", "A brief", 9).unwrap_err(),
        "A step is still running. Try again when it finishes."
    );
    assert!(!root.join("fresh").exists());
    assert_eq!(app.state("live").unwrap()["name"], "one");
    running.fetch_sub(1, Ordering::SeqCst);
    app.create_project("fresh", "A brief", 9).unwrap();
    assert_eq!(app.state("live").unwrap()["name"], "fresh");
}

#[test]
fn a_hold_counts_as_work_in_flight_once_and_is_let_go_once() {
    let app = App::new(base("hold"));
    let hold = RunHold::new(app.running());
    let count = app.running();
    assert_eq!(hold.steps_in_flight(), 0);
    assert!(hold.begin());
    // A second begin takes nothing and says so, so a second run is not started on top of the first.
    assert!(!hold.begin());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // The hold is not a step: only real work counts as a step in flight.
    assert_eq!(hold.steps_in_flight(), 0);
    count.fetch_add(1, Ordering::SeqCst);
    assert_eq!(hold.steps_in_flight(), 1);
    count.fetch_sub(1, Ordering::SeqCst);
    hold.end();
    hold.end();
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn the_project_state_says_a_run_is_active_while_a_hold_or_a_step_is_in_flight() {
    let root = base("run-active");
    studio(&root, "one");
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    assert_eq!(app.state("live").unwrap()["run_active"], false);
    let hold = RunHold::new(app.running());
    assert!(hold.begin());
    assert_eq!(app.state("live").unwrap()["run_active"], true);
    hold.end();
    assert_eq!(app.state("live").unwrap()["run_active"], false);
    app.running().fetch_add(1, Ordering::SeqCst);
    assert_eq!(app.state("live").unwrap()["run_active"], true);
    app.running().fetch_sub(1, Ordering::SeqCst);
}

#[test]
fn a_hold_keeps_other_projects_closed_between_two_steps() {
    let root = base("between");
    studio(&root, "one");
    studio(&root, "two");
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    let hold = RunHold::new(app.running());
    assert!(hold.begin());
    assert!(app.open_project("two").is_err());
    assert!(app.create_project("three", "A brief", 9).is_err());
    hold.end();
    app.open_project("two").unwrap();
}

#[test]
fn a_stop_is_not_reported_once_every_step_has_a_result() {
    // Stop pressed while the last step was in flight: it finished, so there is nothing left to run.
    let root = base("all-done");
    let mut s = studio(&root, "one");
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    app.record_stop(10).unwrap();
    assert_eq!(stopped_items(&app).len(), 1);
    let ids: Vec<String> = s
        .plan()
        .unwrap()
        .stages
        .iter()
        .map(|st| st.id.clone())
        .collect();
    for (i, id) in ids.iter().enumerate() {
        let d = s.dispatch(id, 20 + i as u64).unwrap();
        std::fs::write(d.dir.join("artifact.md"), "Text.").unwrap();
        s.complete(&d, "artifact.md", id, &[], 30 + i as u64)
            .unwrap();
    }
    // The step that was stopped finished after the stop was written.
    app.record_stop(99).unwrap();
    assert!(app.state("live").unwrap()["stopped"].is_null());
    assert!(stopped_items(&app).is_empty());
}
