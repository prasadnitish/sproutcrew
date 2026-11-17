//! Fixture repositories only. No dependency command, provider call, hook, or generated code executes.
use bff_core::{
    code_review::{self, Bundle, File},
    flow::{Dispatch, Studio},
    workflows::{plan_for, Workflow},
    workspace::Project,
};
fn studio(tag: &str) -> Studio {
    let root =
        std::env::temp_dir().join(format!("sproutcrew-worktree-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut s =
        Studio::open(Project::create(&root, "fixture", "A code draft", 1).unwrap()).unwrap();
    s.set_plan(
        plan_for(
            Workflow::Writing,
            "A code draft",
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
fn task(s: &mut Studio) -> Dispatch {
    let stage = s.plan().unwrap().stages[0].clone();
    let d = s.dispatch(&stage.id, 4).unwrap();
    code_review::prepare(s, &d, &stage.owner_role, 4).unwrap();
    d
}
fn finish(s: &Studio, d: &Dispatch, content: &str) {
    let b = Bundle {
        summary: "Code for review; not executed".into(),
        files: vec![File {
            path: "app.js".into(),
            content: content.into(),
        }],
    };
    std::fs::write(d.dir.join("artifact.md"), serde_json::to_vec(&b).unwrap()).unwrap();
    assert!(code_review::finish(s, &d.task, true, 5).unwrap());
}
fn patch(s: &Studio, task: &str) -> String {
    code_review::list(&s.project.root)
        .unwrap()
        .into_iter()
        .find(|d| d.task == task)
        .unwrap()
        .patch_hash
}
#[test]
fn two_parallel_writer_worktrees_are_isolated_and_stale_base_requires_reconciliation() {
    let mut s = studio("parallel");
    let a = task(&mut s);
    let b = task(&mut s);
    assert_ne!(a.dir, b.dir);
    assert!(a.dir.join(".git").is_file() && b.dir.join(".git").is_file());
    std::thread::scope(|scope| {
        let ad = &a.dir;
        let bd = &b.dir;
        scope.spawn(move || std::fs::write(ad.join("writer-marker"), "a").unwrap());
        scope.spawn(move || std::fs::write(bd.join("writer-marker"), "b").unwrap());
    });
    assert_eq!(
        std::fs::read_to_string(a.dir.join("writer-marker")).unwrap(),
        "a"
    );
    assert!(!s.project.root.join("code/writer-marker").exists());
    finish(&s, &a, "const value = 1;\n");
    finish(&s, &b, "const value = 2;\n");
    assert!(
        !s.project.root.join("code/app.js").exists(),
        "candidate code must not land before review"
    );
    code_review::decide(&s, &a.task, &patch(&s, &a.task), true, "Fixture owner", 6).unwrap();
    assert!(
        code_review::decide(&s, &b.task, &patch(&s, &b.task), true, "Fixture owner", 7)
            .unwrap_err()
            .contains("Stale base")
    );
    assert_eq!(
        std::fs::read_to_string(s.project.root.join("code/app.js")).unwrap(),
        "const value = 1;\n"
    );
    let base = code_review::head(&s.project.root).unwrap();
    let next = code_review::reconcile(
        &s,
        &b.task,
        &base,
        Bundle {
            summary: "Explicitly reconcile both edits".into(),
            files: vec![File {
                path: "app.js".into(),
                content: "const value = 1 + 2;\n".into(),
            }],
        },
        8,
    )
    .unwrap();
    code_review::decide(&s, &next, &patch(&s, &next), true, "Fixture owner", 9).unwrap();
    assert_eq!(
        std::fs::read_to_string(s.project.root.join("code/app.js")).unwrap(),
        "const value = 1 + 2;\n"
    );
}
#[test]
fn dependency_install_ask_blocks_before_approval_and_isolation_gate_still_blocks_after() {
    let mut s = studio("dependencies");
    let d = task(&mut s);
    assert!(code_review::dependency_install(&s, &d.task, 5)
        .unwrap_err()
        .contains("asked"));
    let pending = bff_core::policy::pending(&s.journal, &s.approval_hash().unwrap()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["tier"], "consequential");
    let id = pending[0]["id"].as_str().unwrap().to_string();
    s.decide_actions(&[id], true, "Fixture owner", 6).unwrap();
    assert!(code_review::dependency_install(&s, &d.task, 7)
        .unwrap_err()
        .contains("no tested process/filesystem/network isolation"));
    assert!(!d.dir.join("node_modules").exists());
    assert!(!s.project.root.join("code/node_modules").exists());
}
#[test]
fn traversal_metadata_and_seeded_secrets_are_refused_before_code_writes() {
    for (tag, path, content) in [
        ("traversal", "../escape", "x"),
        ("metadata", ".gitattributes", "* filter=evil"),
        (
            "secret",
            "config.py",
            "TOKEN='ghp_abcdefghijklmnopqrstuvwxyz0123456789'",
        ),
    ] {
        let mut s = studio(tag);
        let d = task(&mut s);
        let b = Bundle {
            summary: "Bad candidate".into(),
            files: vec![File {
                path: path.into(),
                content: content.into(),
            }],
        };
        std::fs::write(d.dir.join("artifact.md"), serde_json::to_vec(&b).unwrap()).unwrap();
        assert!(code_review::finish(&s, &d.task, true, 5).is_err());
        assert!(!s.project.root.join("code/escape").exists());
    }
}
#[test]
fn api_code_workflow_stops_for_diff_approval_before_dependent_steps() {
    use bff_core::{
        app::App,
        engine::{Engine, Request, WorkerResult},
        meters::Entry,
        plan::{Assignment, Plan, Stage},
    };
    struct Writer;
    impl Engine for Writer {
        fn label(&self) -> &'static str {
            "replay"
        }
        fn propose_plan(&mut self, _: &str) -> Result<(Plan, Vec<Entry>), String> {
            unreachable!()
        }
        fn run_worker(&mut self, r: &Request) -> WorkerResult {
            assert!(r.task_dir.join(".git").is_file());
            assert!(r.prompt.contains("CODE_DRAFT_JSON_V1"));
            std::fs::write(r.task_dir.join("artifact.md"),r#"{"summary":"A draft; not executed","files":[{"path":"main.txt","content":"hello"}]}"#).unwrap();
            WorkerResult {
                completed: true,
                note: "fixture".into(),
                usage: vec![],
            }
        }
    }
    let s = studio("app");
    let base = s.project.root.parent().unwrap().to_path_buf();
    drop(s);
    let mut app = App::new(base);
    app.open_project("fixture").unwrap();
    let stage = |id: &str, role: &str| Stage {
        id: id.into(),
        title: id.into(),
        owner_role: role.into(),
        guide: String::new(),
        after: vec![],
        uses: None,
    };
    let plan = Plan {
        brief: "A code draft".into(),
        stages: vec![stage("build", "builder"), stage("check", "reviewer")],
        team: vec![
            Assignment {
                role: "builder".into(),
                route: "R12".into(),
                model: "openrouter/free".into(),
                effort: "medium".into(),
            },
            Assignment {
                role: "reviewer".into(),
                route: "R12".into(),
                model: "openrouter/free".into(),
                effort: "high".into(),
            },
        ],
        fallbacks: vec![],
        privacy: Default::default(),
        keynote: false,
    };
    app.save_plan(serde_json::to_value(plan).unwrap(), 2)
        .unwrap();
    app.approve("Fixture owner", 3).unwrap();
    app.run_stage(&mut Writer, "build", 4).unwrap();
    let state = app.state("replay").unwrap();
    let d = &state["code_review"]["items"][0];
    assert_eq!(d["status"], "pending");
    assert!(app
        .begin_stage("check", None, 0, 5)
        .err()
        .unwrap()
        .contains("Review or reconcile"));
    app.decide_code(
        d["task"].as_str().unwrap(),
        d["patch_hash"].as_str().unwrap(),
        true,
        "Fixture owner",
        6,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(app.project_root().unwrap().join("code/main.txt")).unwrap(),
        "hello"
    );
    let exported = app
        .export_package("Read the code.", &Default::default(), 8)
        .unwrap();
    let folder = std::path::PathBuf::from(exported["path"].as_str().unwrap());
    assert!(!folder.join("code/.git").exists());
}

#[test]
fn managed_worktrees_do_not_execute_hooks_and_configuration_drift_fails_closed() {
    use std::os::unix::fs::PermissionsExt;
    let mut s = studio("hooks");
    let _first = task(&mut s);
    let marker = s.project.root.join("unexpected-hook");
    let hook = s.project.root.join("code/.git/hooks/post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!("#!/bin/sh\n/usr/bin/touch '{}'\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _second = task(&mut s);
    assert!(
        !marker.exists(),
        "Git hooks must stay off when a worktree is prepared"
    );
    let config = s.project.root.join("code/.git/config");
    let mut changed = std::fs::read_to_string(&config).unwrap();
    changed.push_str("\n[core]\n\tfsmonitor = unsafe-command\n");
    std::fs::write(config, changed).unwrap();
    assert!(code_review::head(&s.project.root)
        .unwrap_err()
        .contains("configuration changed"));
    assert!(!marker.exists());
}
