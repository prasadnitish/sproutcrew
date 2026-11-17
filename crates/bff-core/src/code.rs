//! The project's code, shared between steps. Each step works in a folder of its own, so without this the
//! tester, the reviewers, and the technical writer would never see what the coder wrote. Before a step the
//! project's `code/` folder is copied into its folder, and after a step that writes code the new and changed
//! files are copied back. Only regular files with plain names are moved, nothing hidden, no symlinks, and
//! no dependency or build folders, and the size and count are capped.

use std::path::{Path, PathBuf};

pub const DIR: &str = "code";
const MAX_FILES: usize = 500;
const MAX_FILE: u64 = 1024 * 1024;
const MAX_TOTAL: u64 = 10 * 1024 * 1024;
const SKIP_DIRS: &[&str] = &["node_modules", "__pycache__", "target", "venv", "dist"];
/// The one file a step is expected to write besides code.
const RESULT: &str = "artifact.md";

/// Regular files under `dir` as `(relative path, size)`, in path order, skipping what is not code.
fn walk(dir: &Path, skip_result: bool) -> Result<Vec<(String, u64)>, String> {
    fn go(
        base: &Path,
        at: &Path,
        out: &mut Vec<(String, u64)>,
        skip_result: bool,
    ) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(at)
            .map_err(|e| format!("{}: {e}", at.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
            let rel = e
                .path()
                .strip_prefix(base)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if meta.file_type().is_symlink() || (name.starts_with('.') && name != ".gitignore") {
                continue;
            }
            if meta.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    go(base, &e.path(), out, skip_result)?;
                }
            } else if meta.is_file()
                && meta.len() <= MAX_FILE
                && !(skip_result && rel == RESULT)
                && crate::writes::check_repo_path(&rel).is_ok()
            {
                out.push((rel, meta.len()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    if dir.is_dir() {
        go(dir, dir, &mut out, skip_result)?;
    }
    Ok(out)
}

fn copy(from: &Path, to: &Path, files: &[(String, u64)]) -> Result<Vec<String>, String> {
    let (mut done, mut total) = (Vec::new(), 0u64);
    for (rel, size) in files.iter().take(MAX_FILES) {
        total += size;
        if total > MAX_TOTAL {
            break;
        }
        let src = from.join(rel);
        let dest = to.join(rel);
        let bytes = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        if std::fs::read(&dest).ok().as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
        done.push(rel.clone());
    }
    Ok(done)
}

fn code_dir(project_root: &Path) -> PathBuf {
    project_root.join(DIR)
}

pub fn candidate_files(task: &Path) -> Result<Vec<(String, u64)>, String> {
    walk(task, true)
}

/// Puts the project's code in a step's folder. Returns the paths copied.
pub fn seed(project_root: &Path, task_dir: &Path) -> Result<Vec<String>, String> {
    let from = code_dir(project_root);
    let files = walk(&from, false)?;
    copy(&from, task_dir, &files)
}

/// Takes new and changed code files from a step's folder into the project. Returns the paths that changed.
pub fn harvest(project_root: &Path, task_dir: &Path) -> Result<Vec<String>, String> {
    let to = code_dir(project_root);
    let files = walk(task_dir, true)?;
    if files.is_empty() {
        return Ok(vec![]);
    }
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    copy(task_dir, &to, &files)
}

/// The project's code files, for showing and for proposing to GitHub.
pub fn files(project_root: &Path) -> Result<Vec<(String, u64)>, String> {
    walk(&code_dir(project_root), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(tag: &str) -> (PathBuf, PathBuf) {
        let b = std::env::temp_dir().join(format!("bff-code-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&b);
        let (p, t) = (b.join("project"), b.join("task"));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::create_dir_all(&t).unwrap();
        (p, t)
    }
    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn a_step_gets_the_project_code_in_its_own_folder() {
        let (p, t) = dirs("seed");
        put(&p, "code/app.py", "print(1)");
        put(&p, "code/src/util.py", "x = 1");
        put(&p, "code/.env", "SECRET=1");
        put(&p, "code/node_modules/x/index.js", "x");
        put(&p, "code/__pycache__/a.pyc", "x");
        std::os::unix::fs::symlink("/etc/hosts", p.join("code/link.txt")).unwrap();
        let got = seed(&p, &t).unwrap();
        assert_eq!(got, vec!["app.py", "src/util.py"]);
        assert_eq!(
            std::fs::read_to_string(t.join("src/util.py")).unwrap(),
            "x = 1"
        );
        assert!(
            !t.join(".env").exists()
                && !t.join("link.txt").exists()
                && !t.join("node_modules").exists()
        );
        assert!(
            seed(&p, &p.join("nothing-here-yet")).is_ok(),
            "a step folder that is missing is created by the copy"
        );
        // A project with no code seeds nothing.
        let (p2, t2) = dirs("seed-empty");
        assert_eq!(seed(&p2, &t2).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_step_that_writes_code_leaves_its_new_and_changed_files_in_the_project() {
        let (p, t) = dirs("harvest");
        put(&p, "code/app.py", "v1");
        put(&t, "app.py", "v2");
        put(&t, "tests/test_app.py", "assert True");
        put(&t, "artifact.md", "# the result, not code");
        put(&t, ".hidden", "x");
        put(&t, "bad name.py", "x");
        put(&t, "big.bin", &"x".repeat(2 * 1024 * 1024));
        let got = harvest(&p, &t).unwrap();
        assert_eq!(got, vec!["app.py", "tests/test_app.py"]);
        assert_eq!(
            std::fs::read_to_string(p.join("code/app.py")).unwrap(),
            "v2"
        );
        assert!(
            !p.join("code/artifact.md").exists()
                && !p.join("code/.hidden").exists()
                && !p.join("code/big.bin").exists()
        );
        // Nothing changed the second time, and the code is listed.
        assert_eq!(harvest(&p, &t).unwrap(), Vec::<String>::new());
        assert_eq!(
            files(&p)
                .unwrap()
                .iter()
                .map(|f| f.0.as_str())
                .collect::<Vec<_>>(),
            vec!["app.py", "tests/test_app.py"]
        );
    }

    #[test]
    fn a_step_with_no_code_leaves_the_project_alone() {
        let (p, t) = dirs("none");
        put(&t, "artifact.md", "# result");
        assert_eq!(harvest(&p, &t).unwrap(), Vec::<String>::new());
        assert!(!p.join("code").exists());
    }
}
