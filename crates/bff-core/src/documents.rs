//! Finished files copied to the person's Documents folder, under `SproutCrew/<project>`. Nothing there is ever
//! replaced. The copy is a job the caller runs after letting go of the app lock, because the first copy can wait on
//! a macOS permission prompt for as long as the person takes to answer it.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Said on the result when the files are copied, because macOS may ask once.
pub const FIRST_COPY_NOTE: &str =
    "Saving to your Documents folder. macOS may ask once for permission.";

/// One copy at a time, so two screens asking at once cannot make a second set.
static COPYING: Mutex<()> = Mutex::new(());

/// The folder inside `SproutCrew` for a project's finished files: its name with anything but letters, digits, spaces,
/// hyphens, and underscores turned into a space, so it is one plain folder name.
pub fn folder_name(project: &str) -> String {
    let name = project
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name: String = name.chars().take(100).collect();
    if name.is_empty() {
        "Project".into()
    } else {
        name
    }
}

/// Whether `path` is `root` or inside it. Both are resolved first, so `..` and a link that points out do not get
/// through, and the comparison is by whole path components, so `SproutCrewEvil` is not inside `SproutCrew`. A
/// path or root that does not exist is not within.
pub fn within(root: &Path, path: &Path) -> bool {
    match (std::fs::canonicalize(root), std::fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => path.starts_with(root),
        _ => false,
    }
}

/// Copies the folder `src` (an Export everything set) into `<base>/SproutCrew/<project>/` under its own name, or
/// `name 2`, `name 3` when that is taken, so nothing there is ever replaced. Links are not followed. Returns the copy.
pub fn copy_folder_into(base: &Path, project: &str, src: &Path) -> Result<PathBuf, String> {
    let folder = base.join("SproutCrew").join(folder_name(project));
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let stem = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the export folder has no name")?;
    let mut target = None;
    for n in 1..1000 {
        let path = folder.join(if n == 1 {
            stem.to_string()
        } else {
            format!("{stem} {n}")
        });
        match std::fs::create_dir(&path) {
            Ok(()) => {
                target = Some(path);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    let target = target.ok_or("too many copies of this export already")?;
    fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let dest = to.join(entry.file_name());
            if kind.is_dir() {
                std::fs::create_dir(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                copy_tree(&entry.path(), &dest)?;
            } else if kind.is_file() {
                std::fs::copy(entry.path(), &dest)
                    .map_err(|e| format!("{}: {e}", dest.display()))?;
            }
        }
        Ok(())
    }
    copy_tree(src, &target)?;
    Ok(target)
}

/// Copies `files` into `<base>/SproutCrew/<project>/` and returns where each went, in order. A file is never
/// replaced. A name already taken by a file with the same bytes is that file, so copying the same thing again adds
/// nothing; one with other bytes makes `name 2.docx`, then `name 3.docx`.
pub fn copy_into(base: &Path, project: &str, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let folder = base.join("SproutCrew").join(folder_name(project));
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut copies = Vec::new();
    for file in files {
        let stem = file
            .file_stem()
            .and_then(|n| n.to_str())
            .ok_or("a file has no name")?;
        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .map_or(String::new(), |e| format!(".{e}"));
        let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut copied = None;
        for n in 1..1000 {
            let name = if n == 1 {
                format!("{stem}{ext}")
            } else {
                format!("{stem} {n}{ext}")
            };
            let path = folder.join(name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut to) => {
                    use std::io::Write;
                    to.write_all(&bytes)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    copied = Some(path);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if std::fs::read(&path).is_ok_and(|have| have == bytes) {
                        copied = Some(path);
                        break;
                    }
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        copies.push(copied.ok_or("too many files of that name in the folder")?);
    }
    Ok(copies)
}

/// The copies an earlier run recorded, when there is a record that reads.
fn recorded(marker: &Path) -> Option<Vec<Value>> {
    serde_json::from_slice::<Vec<Value>>(&std::fs::read(marker).ok()?).ok()
}

fn folder_of(files: &[Value]) -> Option<String> {
    Path::new(files.first()?["path"].as_str()?)
        .parent()
        .map(|p| p.display().to_string())
}

/// The files of a finished set, to copy to Documents.
#[derive(Debug, Clone)]
pub struct CopyJob {
    pub(crate) base: PathBuf,
    pub(crate) project: String,
    /// Each file's `kind` and `path` where it was made.
    pub(crate) made: Vec<Value>,
    /// Written once the copy is done. A record that reads is final: the set is not copied again, even if the person
    /// has since moved or deleted the copies.
    pub(crate) marker: PathBuf,
}

impl CopyJob {
    /// What to show for the set when it need not be copied: the recorded copies still there, or, when the person
    /// has moved or deleted them all, the set where it was made. `None` when there is no record, so it is copied.
    pub(crate) fn already(&self) -> Option<(Vec<Value>, Option<String>)> {
        let kept: Vec<Value> = recorded(&self.marker)?
            .into_iter()
            .filter(|f| f["path"].as_str().is_some_and(|p| Path::new(p).is_file()))
            .collect();
        Some(if kept.is_empty() {
            (self.made.clone(), None)
        } else {
            let folder = folder_of(&kept);
            (kept, folder)
        })
    }

    /// Copies the set and puts the result in `shown`: its `files`, its `folder`, and a note. A copy that fails
    /// leaves the files where they were made, says so, and writes no record, so opening the result again tries again.
    /// Call it with no lock held.
    pub fn run(&self, shown: &mut Value) {
        let _one_at_a_time = COPYING.lock().unwrap_or_else(|e| e.into_inner());
        // Another screen may have made the copy while this one waited.
        if let Some((files, folder)) = self.already() {
            shown["files"] = json!(files);
            if let Some(folder) = folder {
                shown["folder"] = json!(folder);
            }
            return;
        }
        let from: Vec<PathBuf> = self
            .made
            .iter()
            .filter_map(|f| f["path"].as_str().map(PathBuf::from))
            .collect();
        let note = match copy_into(&self.base, &self.project, &from) {
            Ok(copies) => {
                let files: Vec<Value> = self
                    .made
                    .iter()
                    .zip(copies)
                    .map(|(f, c)| json!({"kind": f["kind"], "path": c.display().to_string()}))
                    .collect();
                if let Ok(bytes) = serde_json::to_vec(&files) {
                    let _ = crate::workspace::write_atomic(&self.marker, &bytes);
                }
                shown["folder"] = json!(folder_of(&files));
                shown["files"] = json!(files);
                FIRST_COPY_NOTE.to_string()
            }
            Err(e) => format!("The files could not be saved to Documents: {e}. Allow Documents in System Settings, Privacy & Security, Files and Folders, then open the result again. They are in this project's exports folder."),
        };
        if let Some(notes) = shown["notes"].as_array_mut() {
            notes.push(json!(note));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sprout-within-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn an_export_folder_is_copied_whole_into_documents_and_never_replaces_one() {
        // QA case 6: Export everything was saved in a hidden folder.
        let d = dir("folder");
        let src = d.join("package-7");
        std::fs::create_dir_all(src.join("steps")).unwrap();
        std::fs::write(src.join("README.md"), "read me").unwrap();
        std::fs::write(src.join("steps").join("01.md"), "step").unwrap();
        let docs = d.join("Documents");
        let first = copy_folder_into(&docs, "My project", &src).unwrap();
        assert_eq!(
            first,
            docs.join("SproutCrew")
                .join(folder_name("My project"))
                .join("package-7")
        );
        assert_eq!(
            std::fs::read_to_string(first.join("steps").join("01.md")).unwrap(),
            "step"
        );
        let second = copy_folder_into(&docs, "My project", &src).unwrap();
        assert_ne!(first, second);
        assert!(second.join("README.md").is_file());
    }

    #[test]
    fn a_path_is_within_a_folder_only_if_it_really_is_inside_it() {
        let d = dir("a");
        let root = d.join("SproutCrew");
        std::fs::create_dir_all(root.join("case")).unwrap();
        std::fs::write(root.join("case/a.docx"), b"x").unwrap();
        assert!(within(&root, &root.join("case/a.docx")));
        assert!(within(&root, &root), "the folder itself");
        assert!(within(&root, &root.join("case")));
        // `..` out of the folder.
        std::fs::write(d.join("secret.docx"), b"x").unwrap();
        assert!(!within(&root, &root.join("case/../../secret.docx")));
        // A sibling whose name starts with the folder's name.
        std::fs::create_dir_all(d.join("SproutCrewEvil")).unwrap();
        std::fs::write(d.join("SproutCrewEvil/a.docx"), b"x").unwrap();
        assert!(!within(&root, &d.join("SproutCrewEvil/a.docx")));
        // Not there at all.
        assert!(!within(&root, &root.join("case/none.docx")));
        assert!(!within(&d.join("nope"), &root));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_folder_that_points_out_is_not_within() {
        let d = dir("link");
        let root = d.join("SproutCrew");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(d.join("outside.docx"), b"x").unwrap();
        std::fs::create_dir_all(d.join("outside")).unwrap();
        std::fs::write(d.join("outside/b.docx"), b"x").unwrap();
        std::os::unix::fs::symlink(d.join("outside.docx"), root.join("link.docx")).unwrap();
        std::os::unix::fs::symlink(d.join("outside"), root.join("dirlink")).unwrap();
        assert!(!within(&root, &root.join("link.docx")));
        assert!(!within(&root, &root.join("dirlink/b.docx")));
        // A link that stays inside is fine.
        std::fs::write(root.join("real.docx"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("real.docx"), root.join("ok.docx")).unwrap();
        assert!(within(&root, &root.join("ok.docx")));
    }
}
