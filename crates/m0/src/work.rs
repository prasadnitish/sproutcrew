//! Per-run working directories, outside every temp root and every repository, so that
//! "sibling", "project root", and "temp root" mean something in the isolation probes.

use crate::common::Dirs;
use std::io::Read;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Work {
    pub root: PathBuf,
    /// The synthetic project root.
    pub proj: PathBuf,
    /// The task folder, and the `cwd` of the run.
    pub t1: PathBuf,
    /// A sibling task folder.
    pub t2: PathBuf,
    /// The per-task `TMPDIR`.
    pub tmp: PathBuf,
    /// Settings, MCP config, and prompt files. Outside the task folder.
    pub run: PathBuf,
}

pub fn make(dirs: &Dirs, id: &str) -> std::io::Result<Work> {
    let root = dirs.work.join("runs").join(id);
    let w = Work {
        proj: root.join("proj"),
        t1: root.join("proj/tasks/t1"),
        t2: root.join("proj/tasks/t2"),
        tmp: root.join("tmp"),
        run: root.join("run"),
        root,
    };
    for d in [&w.t1, &w.t2, &w.tmp, &w.run] {
        std::fs::create_dir_all(d)?;
    }
    Ok(w)
}

/// A random version 4 UUID, for `--session-id`.
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("urandom");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Section 12 extra environment for Claude children.
pub const CLAUDE_ENV: &[(&str, &str)] = &[
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
    ("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS", "1"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_version_4_and_differ() {
        let (a, b) = (uuid_v4(), uuid_v4());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(["8", "9", "a", "b"].contains(&&a[19..20]));
    }

    #[test]
    fn work_dirs_are_siblings_inside_one_project_and_the_run_files_sit_outside_the_task() {
        let d = Dirs {
            repo: std::env::temp_dir(),
            work: std::env::temp_dir().join(format!("bff-work-{}", std::process::id())),
        };
        let w = make(&d, "T").unwrap();
        assert_eq!(w.t1.parent(), w.t2.parent());
        assert!(w.t1.starts_with(&w.proj) && w.t2.starts_with(&w.proj));
        assert!(!w.run.starts_with(&w.t1) && !w.tmp.starts_with(&w.t1));
        std::fs::remove_dir_all(&d.work).unwrap();
    }
}
