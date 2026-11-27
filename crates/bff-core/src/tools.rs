//! Finding and starting the command-line programs SproutCrew runs, such as the bundled `typst`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The folders a child gets after the program's own: Homebrew on Apple Silicon and Intel, then the system's.
pub const SYSTEM_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// A helper program that ships inside the app, next to its executable (Typst makes PDFs).
/// Absent when running from a development build without the helper.
pub fn bundled(name: &str) -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.is_file().then_some(p)
}

/// The PATH for a child that runs `program`: the program's own folder first, so a Node install finds its
/// `node`, then `SYSTEM_PATH`. A folder whose name holds a colon cannot be a PATH entry and is left out.
pub fn child_path(program: &Path) -> String {
    match program.parent().and_then(Path::to_str) {
        Some(d)
            if program.is_absolute()
                && !d.contains(':')
                && !SYSTEM_PATH.split(':').any(|s| s == d) =>
        {
            format!("{d}:{SYSTEM_PATH}")
        }
        _ => SYSTEM_PATH.to_string(),
    }
}

/// The first candidate that is a file and answers `--version`, run with only `child_path` set. Returns the
/// path and the version line.
pub fn detect_in(paths: &[PathBuf]) -> Option<(PathBuf, String)> {
    paths.iter().filter(|p| p.is_file()).find_map(|p| {
        let out = Command::new(p)
            .arg("--version")
            .env_clear()
            .env("PATH", child_path(p))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (p.clone(), v)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-tools-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn script(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_child_finds_programs_beside_its_own_first() {
        assert_eq!(
            child_path(Path::new("/Users/a/.nvm/versions/node/v22.1.0/bin/codex")),
            format!("/Users/a/.nvm/versions/node/v22.1.0/bin:{SYSTEM_PATH}")
        );
        assert_eq!(
            child_path(Path::new("/opt/homebrew/bin/codex")),
            SYSTEM_PATH
        );
        assert_eq!(child_path(Path::new("codex")), SYSTEM_PATH);
        assert_eq!(child_path(Path::new("/odd:dir/codex")), SYSTEM_PATH);
    }

    #[test]
    fn a_node_install_answers_because_its_node_is_on_the_path() {
        // npm puts `codex` and `node` in the same folder, and `codex` starts with `#!/usr/bin/env node`.
        let bin = dir("node").join(".nvm/versions/node/v22.1.0/bin");
        script(&bin.join("node"), "#!/bin/sh\necho codex-cli 0.166.0\n");
        script(&bin.join("codex"), "#!/usr/bin/env node\n");
        let (path, version) = detect_in(&[bin.join("codex")]).unwrap();
        assert_eq!(path, bin.join("codex"));
        assert_eq!(version, "codex-cli 0.166.0");
    }
}
