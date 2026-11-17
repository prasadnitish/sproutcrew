//! Structure-golden and cross-project acceptance fixtures; no provider requests.
use bff_core::{
    app::App,
    flow::Studio,
    policy::{Action, Tool},
    workflows::{plan_for, Workflow},
    workspace::Project,
};
use serde_json::json;
fn base(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("sproutcrew-reports-{}-{tag}", std::process::id()));
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
fn artifact(s: &mut Studio, text: &str) -> (String, String) {
    let st = s.plan().unwrap().stages[0].clone();
    let d = s.dispatch(&st.id, 4).unwrap();
    std::fs::write(d.dir.join("artifact.md"), text).unwrap();
    s.complete(&d, "artifact.md", &st.id, &[], 5).unwrap();
    let mut peer = bff_core::peer::Peer::open(s).unwrap();
    peer.register(s, &st.id, &st.owner_role, 6).unwrap();
    let sha = peer.summaries()[&st.id]["head"]
        .as_str()
        .unwrap()
        .to_string();
    (st.id, sha)
}
#[test]
fn run_report_structure_matches_golden_and_retains_approval_and_unknown_costs() {
    let root = base("structure");
    let mut s = studio(&root, "one");
    let (name, sha) = artifact(&mut s, "A source-backed memo.");
    s.journal.append("review.received", &json!({"name": name, "reviewer": "reviewer", "report": {"reviewed_sha256":sha,"findings":[{"id":"F1","claim":"A judgment call", "evidence":"A source-backed memo."}]}}),7).unwrap();
    let report = bff_core::run_report::checked(&s, 9).unwrap();
    let headings: Vec<_> = report
        .lines()
        .filter(|l| {
            include_str!("golden/run-report-headings.txt")
                .lines()
                .any(|h| h == *l)
        })
        .collect();
    assert_eq!(
        headings.join("\n") + "\n",
        include_str!("golden/run-report-headings.txt")
    );
    assert!(report.contains(&s.approval_hash().unwrap()));
    assert!(report.contains("Fixture owner"));
    assert!(report.contains("A judgment call"));
    assert!(report.contains("API dollars: unknown"));
    assert!(report.contains("reported / estimated / reconciled / unknown"));
    let out = root.join("out");
    let made = bff_core::run_report::export(&s, &out, &Default::default(), 9).unwrap();
    assert!(out.join("Run report.md").is_file());
    assert!(made["pdf"].is_null());
    let tools = bff_core::clientfiles::Tools::detect("");
    if tools.typst.is_some() {
        let made = bff_core::run_report::export(&s, &root.join("pdf"), &tools, 10).unwrap();
        assert!(made["pdf"].is_string(), "{made}");
        assert!(std::fs::read(root.join("pdf/Run report.pdf"))
            .unwrap()
            .starts_with(b"%PDF-"));
    }
}
#[test]
fn failed_attempt_is_preserved_in_report_and_project_status_without_a_result() {
    let root = base("failed-attempt");
    let mut s = studio(&root, "one");
    let stage = s.plan().unwrap().stages[0].id.clone();
    let d = s.dispatch(&stage, 4).unwrap();
    s.fail(&d, "rate limit reached (status 429)", &[], 5)
        .unwrap();
    let report = bff_core::run_report::checked(&s, 6).unwrap();
    assert!(report.contains("## Execution attempts and outcomes"));
    assert!(report.contains("worker.failed"));
    assert!(report.contains("rate limit reached (status 429)"));
    assert!(!report.contains("not run yet"));
    assert!(!report.contains("No run has been given instructions of mine yet"));
    let rows = App::new(root.clone()).projects_overview();
    assert_eq!(rows[0]["steps_failed"], 1);
    assert_eq!(rows[0]["steps_done"], 0);
    let inbox = App::new(root.clone()).decision_inbox();
    assert!(inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "failure"
            && i["stage"] == stage
            && i["anchor"] == format!("step-{stage}")));
    let retry = s.dispatch(&stage, 7).unwrap();
    s.finish_task(&retry, "fixture", &[], 8).unwrap();
    assert!(s.failed_stages().unwrap().is_empty());
    assert!(bff_core::run_report::checked(&s, 9)
        .unwrap()
        .contains("rate limit reached (status 429)"));
    drop(s);
    let rows = App::new(root.clone()).projects_overview();
    assert_eq!(rows[0]["steps_failed"], 0);
    assert!(App::new(root.clone()).decision_inbox()["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["kind"] != "failure"));
    assert_eq!(rows[0]["steps_done"], 0);
}

#[test]
fn decision_inbox_aggregates_two_runs_orders_risk_and_links_exact_steps() {
    let root = base("inbox");
    let a = studio(&root, "alpha");
    let mut b = studio(&root, "beta");
    let stage_a = a.plan().unwrap().stages[0].id.clone();
    let action = Action {
        stage: stage_a.clone(),
        tool: Tool::Delete,
        target: "../outside".into(),
        payload_hash: "exact-base".into(),
    };
    assert!(a.authorize_action(&action, 4).is_err());
    let (name, sha) = artifact(&mut b, "A memo.");
    b.journal.append("review.received", &json!({"name": name, "reviewer": "reviewer", "report": {"reviewed_sha256":sha,"findings":[{"id":"F1","claim":"Check this", "evidence":"A memo."}]}}),7).unwrap();
    b.journal.append("decision.escalated", &json!({"name":name,"finding":"F1","base":sha,"reason":"Needs owner judgment","by":"Fixture owner"}),8).unwrap();
    drop(a);
    drop(b);
    let mut app = App::new(root.clone());
    app.open_project("beta").unwrap();
    assert!(app
        .escalate_finding(&name, "F1", "stale", "Needs judgment", "Owner", 9)
        .is_err());
    app.escalate_finding(&name, "F1", &sha, "Needs judgment", "Owner", 9)
        .unwrap();
    let inbox = App::new(root).decision_inbox();
    assert_eq!(inbox["errors"], json!([]));
    assert_eq!(inbox["items"][0]["project"], "alpha");
    assert_eq!(inbox["items"][0]["tier"], "destructive");
    assert_eq!(inbox["items"][0]["stage"], stage_a);
    let row = inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["project"] == "beta")
        .unwrap();
    assert_eq!(row["kind"], "escalation");
    assert_eq!(row["tier"], "consequential");
    assert_eq!(row["anchor"], format!("finding-{name}-F1"));
}
#[test]
fn seeded_secret_in_fixture_artifact_blocks_report_before_any_output() {
    let root = base("secret");
    let mut s = studio(&root, "one");
    artifact(&mut s, "TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789");
    let out = root.join("must-not-exist");
    assert!(
        bff_core::run_report::export(&s, &out, &Default::default(), 9)
            .unwrap_err()
            .contains("secret")
    );
    assert!(!out.exists());
}

#[test]
fn a_step_stopped_by_a_limit_carries_the_kind_of_limit_in_the_project_state() {
    let root = base("failure-kind");
    let mut s = studio(&root, "one");
    let stage = s.plan().unwrap().stages[0].id.clone();
    let d = s.dispatch(&stage, 4).unwrap();
    let note = format!(
        "{} Choose another model, or add credit to raise the allowance.",
        bff_core::api_limits::sentence(bff_core::api_limits::Kind::DailyCap, true, true)
    );
    s.fail(&d, &note, &[], 5).unwrap();
    drop(s);
    let mut app = App::new(root.clone());
    app.open_project("one").unwrap();
    let state = app.state("live").unwrap();
    assert_eq!(state["failure_kinds"][&stage], "daily_cap");
    assert_eq!(state["failures"][&stage], note);
}
