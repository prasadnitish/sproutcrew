//! A project and its workspace on disk. Task folders are created fresh and never reset or deleted,
//! because Nitish may have edited one during a run. A checkpoint is a hash manifest of one task folder,
//! taken before dispatch, and it never captures another task's output. Promoting a file to an artifact
//! records its hash and never overwrites an earlier artifact.

use crate::fshash::{diff, hash_tree, Change, Entry, Tree};
use serde_json::{json, Value};
use sha2::Digest;
use std::path::{Path, PathBuf};

pub struct Project {
    pub root: PathBuf,
}

/// Letters, digits, `-` and `_` only, so a name can never reach outside the workspace.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn tree_to_json(t: &Tree) -> Value {
    Value::Object(
        t.iter()
            .map(|(k, v)| {
                let e = match v {
                    Entry::File(h) => json!({"file": h}),
                    Entry::Link(l) => json!({"link": l}),
                    Entry::Dir => json!({"dir": true}),
                };
                (k.clone(), e)
            })
            .collect(),
    )
}

fn tree_from_json(v: &Value) -> Result<Tree, String> {
    let m = v.as_object().ok_or("a manifest is a JSON object")?;
    m.iter()
        .map(|(k, e)| {
            let entry = if let Some(h) = e.get("file").and_then(Value::as_str) {
                Entry::File(h.to_string())
            } else if let Some(l) = e.get("link").and_then(Value::as_str) {
                Entry::Link(l.to_string())
            } else if e.get("dir").is_some() {
                Entry::Dir
            } else {
                return Err(format!("unreadable manifest entry {k}"));
            };
            Ok((k.clone(), entry))
        })
        .collect()
}

/// Writes a file so that it is either the old content or the new, never half of one: the bytes go to a file
/// beside it, are flushed to disk, and then replace it.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    write_atomic(
        path,
        serde_json::to_string_pretty(v)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_str(
        &std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| e.to_string())
}

impl Project {
    /// Creates `<base>/<name>` with `tasks/`, `artifacts/`, `checkpoints/`, and `project.json`. Refuses an existing folder.
    pub fn create(base: &Path, name: &str, brief: &str, now: u64) -> Result<Project, String> {
        if !valid_name(name) {
            return Err(format!(
                "{name:?} is not a valid project name. Use letters, digits, - and _ (up to 64)."
            ));
        }
        let root = base.join(name);
        std::fs::create_dir(&root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
        for d in ["tasks", "artifacts", "checkpoints"] {
            std::fs::create_dir(root.join(d)).map_err(|e| e.to_string())?;
        }
        write_json(
            &root.join("project.json"),
            &json!({"name": name, "brief": brief, "created_at": now}),
        )?;
        Ok(Project { root })
    }

    pub fn open(root: &Path) -> Result<Project, String> {
        read_json(&root.join("project.json"))?;
        Ok(Project {
            root: root.to_path_buf(),
        })
    }

    pub fn brief(&self) -> Result<String, String> {
        read_json(&self.root.join("project.json"))?["brief"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "project.json has no brief".into())
    }

    pub fn journal_path(&self) -> PathBuf {
        self.root.join("journal.db")
    }

    fn task_path(&self, task: &str) -> Result<PathBuf, String> {
        if !valid_name(task) {
            return Err(format!("{task:?} is not a valid task name"));
        }
        Ok(self.root.join("tasks").join(task))
    }

    /// A fresh task folder. Refuses a name that exists, and never touches an existing one.
    pub fn new_task_dir(&self, task: &str) -> Result<PathBuf, String> {
        let p = self.task_path(task)?;
        std::fs::create_dir(&p).map_err(|e| format!("task folder {task}: {e}"))?;
        Ok(p)
    }

    /// Writes a manifest of the task folder and returns its path. The first checkpoint of a task is `1`.
    pub fn checkpoint(&self, task: &str) -> Result<(u32, PathBuf), String> {
        let dir = self.task_path(task)?;
        let tree = hash_tree(&dir).map_err(|e| format!("task {task}: {e}"))?;
        let mut n = 1;
        while self.manifest_path(task, n).exists() {
            n += 1;
        }
        let path = self.manifest_path(task, n);
        write_json(&path, &tree_to_json(&tree))?;
        Ok((n, path))
    }

    fn manifest_path(&self, task: &str, n: u32) -> PathBuf {
        self.root
            .join("checkpoints")
            .join(format!("{task}-{n}.json"))
    }

    /// What changed in the task folder since a checkpoint.
    pub fn changes_since(&self, task: &str, checkpoint: u32) -> Result<Vec<Change>, String> {
        let before = tree_from_json(&read_json(&self.manifest_path(task, checkpoint))?)?;
        let after = hash_tree(&self.task_path(task)?).map_err(|e| e.to_string())?;
        Ok(diff(&before, &after))
    }

    /// Copies a file from a task folder into `artifacts/` under `<name>-<hash8>`, and records it.
    pub fn promote(&self, task: &str, rel: &str, name: &str) -> Result<PathBuf, String> {
        if !valid_name(name) {
            return Err(format!("{name:?} is not a valid artifact name"));
        }
        let dir = self
            .task_path(task)?
            .canonicalize()
            .map_err(|e| format!("task {task}: {e}"))?;
        if Path::new(rel).is_absolute()
            || Path::new(rel)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("{rel:?} leaves the task folder"));
        }
        let src = dir
            .join(rel)
            .canonicalize()
            .map_err(|e| format!("{rel}: {e}"))?;
        if !src.starts_with(&dir) || !src.is_file() {
            return Err(format!("{rel:?} is not a file inside the task folder"));
        }
        let bytes = std::fs::read(&src).map_err(|e| e.to_string())?;
        let sha: String = format!("{:x}", sha2::Sha256::digest(&bytes));
        let ext = src
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let dest = self
            .root
            .join("artifacts")
            .join(format!("{name}-{}{ext}", &sha[..8]));
        let file = dest.file_name().map(|f| f.to_string_lossy().into_owned());
        let mut list = self.artifacts()?;
        if dest.exists() {
            if std::fs::read(&dest).map_err(|e| e.to_string())? != bytes {
                return Err(format!("{} exists with different content", dest.display()));
            }
            // The same text again is already listed. A file a crash left before it was listed is listed now.
            if list
                .iter()
                .any(|a| a["file"].as_str() == file.as_deref() && a["name"].as_str() == Some(name))
            {
                return Ok(dest);
            }
        } else {
            write_atomic(&dest, &bytes)?;
        }
        list.push(json!({"name": name, "sha256": sha, "file": dest.file_name().map(|f| f.to_string_lossy().into_owned()), "from_task": task}));
        write_json(&self.root.join("artifacts.json"), &Value::Array(list))?;
        Ok(dest)
    }

    pub fn artifacts(&self) -> Result<Vec<Value>, String> {
        match read_json(&self.root.join("artifacts.json")) {
            Ok(Value::Array(a)) => Ok(a),
            Ok(_) => Err("artifacts.json is not a list".into()),
            Err(_) if !self.root.join("artifacts.json").exists() => Ok(vec![]),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-ws-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn creating_a_project_lays_out_the_folders_and_keeps_the_brief() {
        let b = base("create");
        let p = Project::create(&b, "case-one", "Write a plan.", 5).unwrap();
        for d in ["tasks", "artifacts", "checkpoints"] {
            assert!(p.root.join(d).is_dir(), "{d}");
        }
        assert_eq!(p.brief().unwrap(), "Write a plan.");
        assert_eq!(
            Project::open(&p.root).unwrap().brief().unwrap(),
            "Write a plan."
        );
    }

    #[test]
    fn an_existing_project_is_never_overwritten_and_bad_names_are_refused() {
        let b = base("names");
        Project::create(&b, "p", "x", 1).unwrap();
        assert!(Project::create(&b, "p", "y", 2).is_err());
        for bad in ["", "..", "a/b", "../x", "a b", ".hidden", &"x".repeat(65)] {
            assert!(Project::create(&b, bad, "x", 1).is_err(), "{bad:?}");
        }
        assert_eq!(Project::open(&b.join("p")).unwrap().brief().unwrap(), "x");
        assert!(Project::open(&b.join("missing")).is_err());
    }

    #[test]
    fn a_task_folder_is_fresh_and_a_second_request_for_it_is_refused() {
        let p = Project::create(&base("task"), "p", "x", 1).unwrap();
        let t = p.new_task_dir("t1").unwrap();
        assert!(t.is_dir() && t.starts_with(&p.root));
        std::fs::write(t.join("edited-by-nitish.txt"), "keep me").unwrap();
        assert!(p.new_task_dir("t1").is_err());
        assert!(p.new_task_dir("../escape").is_err());
        assert_eq!(
            std::fs::read_to_string(t.join("edited-by-nitish.txt")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn a_checkpoint_sees_later_changes_in_its_own_task_only() {
        let p = Project::create(&base("cp"), "p", "x", 1).unwrap();
        let t1 = p.new_task_dir("t1").unwrap();
        let t2 = p.new_task_dir("t2").unwrap();
        std::fs::write(t1.join("a.txt"), "1").unwrap();
        let (n, path) = p.checkpoint("t1").unwrap();
        assert_eq!(n, 1);
        assert!(path.starts_with(p.root.join("checkpoints")));
        std::fs::write(t1.join("a.txt"), "2").unwrap();
        std::fs::write(t1.join("b.txt"), "new").unwrap();
        std::fs::write(t2.join("other.txt"), "unfinished output of another writer").unwrap();
        let ch = p.changes_since("t1", 1).unwrap();
        assert_eq!(
            ch,
            vec![
                Change::Modified("a.txt".into()),
                Change::Added("b.txt".into())
            ]
        );
        assert_eq!(p.checkpoint("t1").unwrap().0, 2);
        assert!(p.changes_since("t1", 9).is_err());
    }

    #[test]
    fn promoting_records_the_hash_and_never_overwrites_a_different_artifact() {
        let p = Project::create(&base("promote"), "p", "x", 1).unwrap();
        let t = p.new_task_dir("t1").unwrap();
        std::fs::write(t.join("artifact.md"), "# v1").unwrap();
        let a = p.promote("t1", "artifact.md", "plan").unwrap();
        assert!(a.starts_with(p.root.join("artifacts")));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "# v1");
        let list = p.artifacts().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "plan");
        assert_eq!(list[0]["sha256"].as_str().unwrap().len(), 64);
        // The same content again is the same artifact. A new version gets a new file.
        assert_eq!(p.promote("t1", "artifact.md", "plan").unwrap(), a);
        std::fs::write(t.join("artifact.md"), "# v2").unwrap();
        let b = p.promote("t1", "artifact.md", "plan").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "# v1");
        assert_eq!(p.artifacts().unwrap().len(), 2);
    }

    #[test]
    fn promote_refuses_paths_that_leave_the_task_folder() {
        let p = Project::create(&base("escape"), "p", "x", 1).unwrap();
        p.new_task_dir("t1").unwrap();
        std::fs::write(p.root.join("secret.txt"), "s").unwrap();
        assert!(p.promote("t1", "../../secret.txt", "x").is_err());
        assert!(p.promote("t1", "/etc/hosts", "x").is_err());
        assert!(p.promote("t1", "missing.md", "x").is_err());
        assert!(p.promote("t1", "a.md", "bad name").is_err());
    }

    #[test]
    fn a_result_file_left_by_a_crash_before_it_was_listed_is_listed_by_the_next_run() {
        let p = Project::create(&base("orphan"), "p", "x", 1).unwrap();
        let t = p.new_task_dir("t1").unwrap();
        std::fs::write(t.join("artifact.md"), "# Result").unwrap();
        let dest = p.promote("t1", "artifact.md", "draft").unwrap();
        // The crash: the file is there, the list never got it.
        std::fs::write(p.root.join("artifacts.json"), "[]").unwrap();
        assert!(p.artifacts().unwrap().is_empty());
        let again = p.promote("t1", "artifact.md", "draft").unwrap();
        assert_eq!(again, dest);
        assert_eq!(p.artifacts().unwrap().len(), 1, "listed now");
        // The same text again adds nothing.
        p.promote("t1", "artifact.md", "draft").unwrap();
        assert_eq!(p.artifacts().unwrap().len(), 1);
        assert!(
            !p.root.join("artifacts.json.tmp").exists(),
            "no temporary file is left"
        );
    }
}
