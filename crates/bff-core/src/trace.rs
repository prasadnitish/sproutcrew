//! What one task was given, saved beside the project so a step's result can be traced afterwards: the step's
//! instructions, the notes a person added, and each file and earlier result that was put in front of it (by id
//! and hash, so the exact text can be found again). It is written when the step starts, before any model runs.
//! A trace holds no result text and no credentials.

use crate::context::Input;
use crate::notes::Note;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trace {
    pub task: String,
    pub stage: String,
    pub route: String,
    pub model: String,
    pub effort: String,
    pub at: u64,
    /// The step's own instructions, as written in the plan.
    pub guide: String,
    /// The hash of the result this run produced. Empty until the run produces one, and for a run that failed.
    #[serde(default)]
    pub result_sha256: String,
    pub notes: Vec<Note>,
    /// The notes kept for every project that this run was given. Runs recorded before they existed have none.
    #[serde(default)]
    pub everywhere_notes: Vec<Note>,
    /// The rules for the exercise that applied to this run, as the person wrote them. Empty when there were none.
    #[serde(default)]
    pub rules: String,
    pub inputs: Vec<Input>,
    /// The skills this run was given as guidance.
    #[serde(default)]
    pub skills: Vec<String>,
}

fn file(root: &Path, task: &str) -> Result<PathBuf, String> {
    let ok = !task.is_empty()
        && task
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(format!("{task:?} is not a task name"));
    }
    Ok(root.join("trace").join(format!("{task}.json")))
}

pub fn record(root: &Path, t: &Trace) -> Result<(), String> {
    let path = file(root, &t.task)?;
    std::fs::create_dir_all(path.parent().expect("a file has a folder"))
        .map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    crate::workspace::write_atomic(&path, text.as_bytes())
}

/// Keeps exactly what the step was shown, so a later question about the result sees the same text, cut the same way.
/// It is beside the trace, in the project, like the files it quotes.
pub fn record_material(root: &Path, task: &str, material: &str) -> Result<(), String> {
    let path = file(root, task)?.with_extension("material.txt");
    crate::workspace::write_atomic(&path, material.as_bytes())
}

/// What the step was shown, if it was kept.
pub fn load_material(root: &Path, task: &str) -> Result<Option<String>, String> {
    let path = file(root, task)?.with_extension("material.txt");
    match std::fs::read_to_string(&path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn load(root: &Path, task: &str) -> Result<Option<Trace>, String> {
    let path = file(root, task)?;
    match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Marks the run of `task` as the one that produced the result with this hash.
pub fn set_result(root: &Path, task: &str, sha256: &str) -> Result<(), String> {
    let mut t = load(root, task)?.ok_or_else(|| format!("no trace for task {task}"))?;
    t.result_sha256 = sha256.to_string();
    record(root, &t)
}

fn number(task: &str) -> u64 {
    task.trim_start_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .unwrap_or(0)
}

/// The trace of the newest run that produced the step's current result: the one whose result has the hash of
/// the step's newest stored result. A run that failed has none, and a re-run that gave identical text counts
/// as the newest. Results made before traces existed have no trace.
pub fn latest_for(root: &Path, stage: &str) -> Result<Option<Trace>, String> {
    let artifacts = crate::workspace::Project::open(root)?.artifacts()?;
    let name = crate::app::artifact_name(stage);
    let Some(sha) = artifacts
        .iter()
        .rev()
        .find(|a| a["name"].as_str() == Some(&name))
        .and_then(|a| a["sha256"].as_str().map(str::to_string))
    else {
        return Ok(None);
    };
    let mut best: Option<Trace> = None;
    let Ok(dir) = std::fs::read_dir(root.join("trace")) else {
        return Ok(None);
    };
    for entry in dir.flatten() {
        let Some(task) = entry
            .file_name()
            .to_string_lossy()
            .strip_suffix(".json")
            .map(str::to_string)
        else {
            continue;
        };
        let Some(t) = load(root, &task)? else {
            continue;
        };
        if t.stage == stage
            && t.result_sha256 == sha
            && best
                .as_ref()
                .is_none_or(|b| number(&t.task) > number(&b.task))
        {
            best = Some(t);
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Input;
    use crate::notes::Note;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bff-trace-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(task: &str, stage: &str) -> Trace {
        Trace {
            task: task.into(),
            stage: stage.into(),
            route: "R1".into(),
            model: "gpt-6.1-terra".into(),
            effort: "medium".into(),
            at: 7,
            guide: "Write the document.".into(),
            result_sha256: String::new(),
            rules: String::new(),
            skills: vec![],
            everywhere_notes: vec![],
            notes: vec![Note {
                id: "n1".into(),
                stage: stage.into(),
                text: "Cut option 3.".into(),
                at: 3,
            }],
            inputs: vec![Input {
                kind: "source".into(),
                id: "src-1".into(),
                label: "src-1 (a.pdf)".into(),
                file: String::new(),
                sha256: "ab".repeat(32),
            }],
        }
    }

    #[test]
    fn a_trace_is_kept_for_its_task() {
        let d = dir("keep");
        record(&d, &sample("t4", "document")).unwrap();
        let t = load(&d, "t4").unwrap().unwrap();
        assert_eq!(t, sample("t4", "document"));
    }

    #[test]
    fn a_missing_trace_is_none_not_an_error() {
        let d = dir("none");
        assert!(load(&d, "t9").unwrap().is_none());
    }

    #[test]
    fn a_task_name_cannot_leave_the_trace_folder() {
        let d = dir("path");
        assert!(record(&d, &sample("../evil", "x")).is_err());
        assert!(load(&d, "../evil").is_err());
    }

    #[test]
    fn the_trace_of_a_step_is_the_newest_run_that_made_its_current_result() {
        let b = dir("latest");
        let d = crate::workspace::Project::create(&b, "p", "brief", 1)
            .unwrap()
            .root;
        let with = |task: &str, stage: &str, sha: &str| {
            let mut t = sample(task, stage);
            t.result_sha256 = sha.into();
            record(&d, &t).unwrap();
        };
        with("t4", "document", "x");
        with("t9", "document", "y");
        with("t12", "document", "y"); // a re-run that produced the very same text
        with("t13", "document", ""); // a run that failed: no result
        std::fs::write(
            d.join("artifacts.json"),
            r#"[{"name":"document","from_task":"t4","file":"a.md","sha256":"x"},{"name":"document","from_task":"t9","file":"b.md","sha256":"y"},{"name":"deck","from_task":"t10","file":"c.md","sha256":"z"}]"#,
        )
        .unwrap();
        assert_eq!(latest_for(&d, "document").unwrap().unwrap().task, "t12");
        assert!(
            latest_for(&d, "deck").unwrap().is_none(),
            "no trace was recorded for the deck"
        );
        assert!(latest_for(&d, "nothing").unwrap().is_none());
    }

    #[test]
    fn tasks_are_ordered_by_number_not_by_text() {
        let b = dir("order");
        let d = crate::workspace::Project::create(&b, "p", "brief", 1)
            .unwrap()
            .root;
        for task in ["t9", "t10"] {
            let mut t = sample(task, "document");
            t.result_sha256 = "y".into();
            record(&d, &t).unwrap();
        }
        std::fs::write(
            d.join("artifacts.json"),
            r#"[{"name":"document","from_task":"t9","file":"b.md","sha256":"y"}]"#,
        )
        .unwrap();
        assert_eq!(latest_for(&d, "document").unwrap().unwrap().task, "t10");
    }

    #[test]
    fn a_trace_can_be_marked_with_its_result() {
        let d = dir("mark");
        record(&d, &sample("t4", "document")).unwrap();
        set_result(&d, "t4", "abc").unwrap();
        assert_eq!(load(&d, "t4").unwrap().unwrap().result_sha256, "abc");
        assert!(set_result(&d, "t99", "abc").is_err());
    }
}
