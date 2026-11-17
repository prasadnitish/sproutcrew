//! Finding the command-line programs BFF runs, such as `claude` and `codex`, on any Mac. The official
//! installers, Homebrew, and the Node package managers each put them in a different folder, and a program
//! installed through Node runs under the `node` that sits next to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The folders a child gets after the program's own: Homebrew on Apple Silicon and Intel, then the system's.
pub const SYSTEM_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// The programs a person may point BFF at by hand, and the file that keeps their choices.
pub const CHOOSABLE: [&str; 2] = ["claude", "codex"];
const CHOICES: &str = "tools.json";

/// Where `name` may be, in the order it is looked for: the official installers' folders, Homebrew, then the
/// usual Node package managers, with the newest nvm Node first.
pub fn candidates(name: &str, home: &str) -> Vec<PathBuf> {
    let h = Path::new(home);
    let mut dirs = vec![
        h.join(".local/bin"),
        h.join(".claude/local"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        h.join(".npm-global/bin"),
        h.join("Library/pnpm"),
        h.join(".bun/bin"),
        h.join(".volta/bin"),
    ];
    dirs.extend(nvm_bins(&h.join(".nvm/versions/node")));
    dirs.into_iter().map(|d| d.join(name)).collect()
}

/// The `bin` folder of each Node version nvm installed, newest first.
fn nvm_bins(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<((u32, u32, u32), PathBuf)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let mut parts = name.strip_prefix('v')?.split('.').map(|p| p.parse().ok());
            let v = (parts.next()??, parts.next()??, parts.next()??);
            Some((v, e.path().join("bin")))
        })
        .collect();
    found.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
    found.into_iter().map(|(_, p)| p).collect()
}

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

/// The program to run for `name`: the one the person chose, if it answers, else the first candidate that does.
pub fn find(name: &str, home: &str, chosen: Option<&Path>) -> Option<(PathBuf, String)> {
    let mut paths: Vec<PathBuf> = chosen.map(Path::to_path_buf).into_iter().collect();
    paths.extend(candidates(name, home));
    detect_in(&paths)
}

/// The programs the person chose by hand, by name. A missing or unreadable file means none.
pub fn choices(data: &Path) -> BTreeMap<String, PathBuf> {
    std::fs::read_to_string(data.join(CHOICES))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Keeps `path` as the program for `name`, or forgets the choice when `path` is `None`. A chosen path must be
/// absolute and must answer `--version`. Returns the version line.
pub fn choose(data: &Path, name: &str, path: Option<&Path>) -> Result<Option<String>, String> {
    if !CHOOSABLE.contains(&name) {
        return Err(format!("{name} cannot be chosen"));
    }
    let mut all = choices(data);
    let version = match path {
        Some(p) => {
            if !p.is_absolute() {
                return Err("Give the full path, starting with /".into());
            }
            let (_, v) = detect_in(&[p.to_path_buf()])
                .ok_or_else(|| format!("{} did not answer --version", p.display()))?;
            all.insert(name.to_string(), p.to_path_buf());
            Some(v)
        }
        None => {
            all.remove(name);
            None
        }
    };
    let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(data.join(CHOICES), text).map_err(|e| e.to_string())?;
    Ok(version)
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
    fn the_official_installer_comes_first_and_the_newest_nvm_node_last() {
        let home = dir("order");
        for v in ["v9.11.2", "v22.11.0", "v20.5.1", "not-a-version"] {
            std::fs::create_dir_all(home.join(".nvm/versions/node").join(v)).unwrap();
        }
        let h = home.to_str().unwrap();
        let c = candidates("codex", h);
        assert_eq!(c[0], home.join(".local/bin/codex"));
        assert_eq!(c[2], PathBuf::from("/opt/homebrew/bin/codex"));
        assert_eq!(c[3], PathBuf::from("/usr/local/bin/codex"));
        let nvm: Vec<_> = c
            .iter()
            .filter(|p| p.starts_with(home.join(".nvm")))
            .collect();
        assert_eq!(
            nvm,
            ["v22.11.0", "v20.5.1", "v9.11.2"]
                .map(|v| home.join(".nvm/versions/node").join(v).join("bin/codex"))
                .iter()
                .collect::<Vec<_>>()
        );
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

    #[test]
    fn a_program_that_does_not_answer_is_skipped() {
        let home = dir("skip");
        script(&home.join(".local/bin/claude"), "#!/bin/sh\nexit 3\n");
        script(&home.join(".bun/bin/claude"), "#!/bin/sh\necho 2.1.316\n");
        // Only the folders inside this test's home, so programs installed on the machine running it do not count.
        let mine: Vec<_> = candidates("claude", home.to_str().unwrap())
            .into_iter()
            .filter(|p| p.starts_with(&home))
            .collect();
        let (path, _) = detect_in(&mine).unwrap();
        assert_eq!(path, home.join(".bun/bin/claude"));
        assert!(find("nothing-here", home.to_str().unwrap(), None).is_none());
    }

    #[test]
    fn a_chosen_program_wins_and_a_broken_choice_is_refused() {
        let home = dir("choose");
        let data = home.join("data");
        std::fs::create_dir_all(&data).unwrap();
        script(&home.join(".local/bin/claude"), "#!/bin/sh\necho 2.1.316\n");
        let mine = home.join("tools/claude-wrapper");
        script(&mine, "#!/bin/sh\necho 2.1.317\n");
        assert_eq!(
            choose(&data, "claude", Some(&mine)).unwrap().as_deref(),
            Some("2.1.317")
        );
        let chosen = choices(&data);
        let (path, _) = find(
            "claude",
            home.to_str().unwrap(),
            chosen.get("claude").map(PathBuf::as_path),
        )
        .unwrap();
        assert_eq!(path, mine);

        let broken = home.join("tools/broken");
        script(&broken, "#!/bin/sh\nexit 1\n");
        assert!(choose(&data, "claude", Some(&broken)).is_err());
        assert!(choose(&data, "claude", Some(Path::new("relative/claude"))).is_err());
        assert!(choose(&data, "typst", Some(&mine)).is_err());
        assert_eq!(choices(&data).get("claude"), Some(&mine));

        choose(&data, "claude", None).unwrap();
        assert!(choices(&data).is_empty());
    }
}
