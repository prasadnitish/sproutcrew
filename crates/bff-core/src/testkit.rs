//! Shared helpers for tests that need a project with a plan, an approval, and one finished step.

use crate::engine::{Engine, Request, ReviewRun, WorkerResult, ARTIFACT_FILE};
use crate::flow::Studio;
use crate::lead::usage_entries;
use crate::meters::Entry;
use crate::plan::Plan;
use crate::workspace::Project;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Writes `artifact` for a worker and answers a review-style call with `review`, keeping every prompt it sees.
pub struct Script {
    pub artifact: String,
    pub review: Option<Value>,
    pub prompts: Arc<Mutex<Vec<String>>>,
    /// The model each review-style call asked for.
    pub models: Arc<Mutex<Vec<String>>>,
}

impl Script {
    pub fn new(artifact: &str, review: Option<Value>) -> (Script, Arc<Mutex<Vec<String>>>) {
        let prompts = Arc::new(Mutex::new(vec![]));
        (
            Script {
                artifact: artifact.into(),
                review,
                prompts: prompts.clone(),
                models: Arc::new(Mutex::new(vec![])),
            },
            prompts,
        )
    }
}

impl Engine for Script {
    fn label(&self) -> &'static str {
        "replay"
    }
    fn propose_plan(&mut self, _: &str) -> Result<(Plan, Vec<Entry>), String> {
        Err("n/a".into())
    }
    fn run_worker(&mut self, req: &Request) -> WorkerResult {
        self.prompts.lock().unwrap().push(req.prompt.to_string());
        std::fs::write(req.task_dir.join(ARTIFACT_FILE), &self.artifact).unwrap();
        WorkerResult {
            completed: true,
            note: "ok".into(),
            usage: usage_entries(req.step, req.model, None, None),
        }
    }
    fn run_review(&mut self, req: &Request, _schema: &Value) -> ReviewRun {
        self.prompts.lock().unwrap().push(req.prompt.to_string());
        self.models.lock().unwrap().push(req.model.to_string());
        ReviewRun {
            completed: self.review.is_some(),
            note: if self.review.is_some() {
                "ok".into()
            } else {
                "the model did not answer".into()
            },
            json: self.review.clone(),
            usage: usage_entries(req.step, req.model, None, None),
        }
    }
}

/// An approved one-step plan (`draft`, owned by `writer`) whose step has run and produced `artifact`.
pub fn ran(tag: &str, artifact: &str) -> (Studio, String) {
    ran_with(tag, artifact, &[])
}

/// Like [`ran`], with `sources` (name, bytes) added to the project before the step runs.
pub fn ran_with(tag: &str, artifact: &str, sources: &[(&str, &[u8])]) -> (Studio, String) {
    let d = std::env::temp_dir().join(format!("bff-kit-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut s = Studio::open(Project::create(&d, "case", "Write a short market note.", 1).unwrap())
        .unwrap();
    let (plan, _) = crate::engine::ReplayEngine
        .propose_plan("Write a short market note.")
        .unwrap();
    let stage = plan.stages[0].id.clone();
    s.set_plan(plan, 2).unwrap();
    let mut store = crate::sources::SourceStore::open(&s.project.root).unwrap();
    for (name, bytes) in sources {
        store.import(name, bytes).unwrap();
    }
    s.approve("Nitish", 3).unwrap();
    let (mut e, _) = Script::new(artifact, None);
    crate::engine::run_stage(&mut s, &mut e, &stage, 4).unwrap();
    (s, stage)
}

/// When each run began and ended, by step.
pub type RunLog = Arc<Mutex<Vec<(String, std::time::Instant, std::time::Instant)>>>;

/// A worker that takes `ms` milliseconds, writes a small result, and logs when it started and stopped, so a test
/// can tell whether two of them ran at the same time. `fail` makes it report a failure instead.
pub struct Slow {
    pub ms: u64,
    pub fail: bool,
    pub log: RunLog,
}

impl Engine for Slow {
    fn label(&self) -> &'static str {
        "replay"
    }
    fn propose_plan(&mut self, _: &str) -> Result<(Plan, Vec<Entry>), String> {
        Err("n/a".into())
    }
    fn run_worker(&mut self, req: &Request) -> WorkerResult {
        let t0 = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(self.ms));
        let t1 = std::time::Instant::now();
        self.log
            .lock()
            .unwrap()
            .push((req.step.to_string(), t0, t1));
        if self.fail {
            return WorkerResult {
                completed: false,
                note: "boom".into(),
                usage: usage_entries(req.step, req.model, None, None),
            };
        }
        std::fs::write(
            req.task_dir.join(ARTIFACT_FILE),
            format!("# Result from {}\n", req.step),
        )
        .unwrap();
        WorkerResult {
            completed: true,
            note: "ok".into(),
            usage: usage_entries(req.step, req.model, None, None),
        }
    }
}
