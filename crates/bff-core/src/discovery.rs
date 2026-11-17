//! Read-only discovery of skills and MCP servers already on this Mac, and a per-project selection of the
//! ones to use in future runs, without reinstalling anything. Discovery reads skill files and server names
//! only. It never reads a server's environment, arguments, address, or headers, which is where credentials
//! live, and it never follows a symlink or writes. A selected skill is pinned by the hash of its file, so a
//! later change is reported before the skill is used again.

use crate::codex::mcp_server_names;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_SKILL_BYTES: u64 = 64 * 1024;
const MAX_ENTRIES: usize = 1000;
const MAX_DEPTH: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// `claude-user`, `codex-user`, or `claude-plugin:<plugin>`.
    pub source: String,
    pub sha256: String,
    /// Path relative to the home folder.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpInfo {
    pub name: String,
    /// `claude` or `codex`.
    pub source: String,
    /// `stdio`, `http`, or `unknown`. Nothing else about the server is read.
    pub transport: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Discovery {
    pub skills: Vec<SkillInfo>,
    pub mcp: Vec<McpInfo>,
    /// True if a cap stopped the scan early.
    pub truncated: bool,
}

fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > cap {
        return None;
    }
    std::fs::read(path).ok()
}

fn front_matter(text: &str) -> (Option<String>, String) {
    let mut name = None;
    let mut desc = String::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) == Some("---") {
        for l in lines {
            if l.trim() == "---" {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                let v = v.trim().trim_matches(['"', '\'']).to_string();
                match k.trim() {
                    "name" if !v.is_empty() => name = Some(v),
                    "description" => desc = v.chars().take(200).collect(),
                    _ => {}
                }
            }
        }
    }
    (name, desc)
}

fn skill_at(home: &Path, dir: &Path, source: &str) -> Option<SkillInfo> {
    let file = dir.join("SKILL.md");
    let bytes = read_capped(&file, MAX_SKILL_BYTES)?;
    let text = String::from_utf8_lossy(&bytes);
    let (name, description) = front_matter(&text);
    Some(SkillInfo {
        name: name.unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        description,
        source: source.to_string(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        path: file
            .strip_prefix(home)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned(),
    })
}

fn real_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// Finds `skills/<name>/SKILL.md` below a plugins folder, without following links.
fn plugin_skills(
    home: &Path,
    root: &Path,
    depth: usize,
    out: &mut Vec<SkillInfo>,
    full: &mut bool,
) {
    if depth > MAX_DEPTH || *full {
        return;
    }
    for d in real_dirs(root) {
        if d.file_name().is_some_and(|n| n == "skills") {
            let comps: Vec<String> = d
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let plugin = comps
                .len()
                .checked_sub(3)
                .map(|i| comps[i].clone())
                .unwrap_or_default();
            for s in real_dirs(&d) {
                if out.len() >= MAX_ENTRIES {
                    *full = true;
                    return;
                }
                if let Some(info) = skill_at(home, &s, &format!("claude-plugin:{plugin}")) {
                    out.push(info);
                }
            }
        } else {
            plugin_skills(home, &d, depth + 1, out, full);
        }
    }
}

pub fn discover(home: &Path) -> Discovery {
    let mut d = Discovery::default();
    for (rel, source) in [
        (".claude/skills", "claude-user"),
        (".codex/skills", "codex-user"),
    ] {
        for dir in real_dirs(&home.join(rel)) {
            if d.skills.len() >= MAX_ENTRIES {
                d.truncated = true;
                break;
            }
            if let Some(s) = skill_at(home, &dir, source) {
                d.skills.push(s);
            }
        }
    }
    let mut full = false;
    plugin_skills(
        home,
        &home.join(".claude/plugins"),
        0,
        &mut d.skills,
        &mut full,
    );
    d.truncated |= full;
    d.skills
        .sort_by(|a, b| (&a.name, &a.source).cmp(&(&b.name, &b.source)));

    if let Some(bytes) = read_capped(&home.join(".claude.json"), 8 << 20) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(servers) = v.get("mcpServers").and_then(|m| m.as_object()) {
                for (name, def) in servers {
                    let transport = if def.get("command").is_some() {
                        "stdio"
                    } else if def.get("url").is_some()
                        || matches!(
                            def.get("type").and_then(|t| t.as_str()),
                            Some("http" | "sse")
                        )
                    {
                        "http"
                    } else {
                        "unknown"
                    };
                    d.mcp.push(McpInfo {
                        name: name.clone(),
                        source: "claude".into(),
                        transport: transport.into(),
                    });
                }
            }
        }
    }
    if let Some(bytes) = read_capped(&home.join(".codex/config.toml"), 2 << 20) {
        for name in mcp_server_names(&String::from_utf8_lossy(&bytes)) {
            d.mcp.push(McpInfo {
                name,
                source: "codex".into(),
                transport: "unknown".into(),
            });
        }
    }
    d.mcp
        .sort_by(|a, b| (&a.source, &a.name).cmp(&(&b.source, &b.name)));
    d
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedSkill {
    pub name: String,
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Chosen {
    pub skills: Vec<PinnedSkill>,
    pub mcp: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Drift {
    SkillChanged(String),
    SkillMissing(String),
    McpMissing(String),
}

pub struct Selection {
    path: PathBuf,
    pub chosen: Chosen,
}

impl Selection {
    pub fn open(project_root: &Path) -> Result<Selection, String> {
        let path = project_root.join("selection.json");
        let chosen = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("selection.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Chosen::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Selection { path, chosen })
    }

    fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.chosen).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn select_skill(&mut self, d: &Discovery, name: &str) -> Result<(), String> {
        let s = d
            .skills
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("no skill named {name:?} was found"))?;
        self.chosen.skills.retain(|p| p.name != name);
        self.chosen.skills.push(PinnedSkill {
            name: s.name.clone(),
            source: s.source.clone(),
            sha256: s.sha256.clone(),
        });
        self.save()
    }

    pub fn select_mcp(&mut self, d: &Discovery, name: &str) -> Result<(), String> {
        if !d.mcp.iter().any(|m| m.name == name) {
            return Err(format!("no MCP server named {name:?} was found"));
        }
        if !self.chosen.mcp.iter().any(|m| m == name) {
            self.chosen.mcp.push(name.to_string());
        }
        self.save()
    }

    pub fn deselect(&mut self, name: &str) -> Result<(), String> {
        let (a, b) = (self.chosen.skills.len(), self.chosen.mcp.len());
        self.chosen.skills.retain(|p| p.name != name);
        self.chosen.mcp.retain(|m| m != name);
        if a == self.chosen.skills.len() && b == self.chosen.mcp.len() {
            return Err(format!("{name:?} is not selected"));
        }
        self.save()
    }

    /// What changed since the selection was made. Run on launch and before each run.
    pub fn drift(&self, d: &Discovery) -> Vec<Drift> {
        let mut out = Vec::new();
        for p in &self.chosen.skills {
            match d
                .skills
                .iter()
                .find(|s| s.name == p.name && s.source == p.source)
            {
                None => out.push(Drift::SkillMissing(p.name.clone())),
                Some(s) if s.sha256 != p.sha256 => out.push(Drift::SkillChanged(p.name.clone())),
                Some(_) => {}
            }
        }
        for m in &self.chosen.mcp {
            if !d.mcp.iter().any(|x| &x.name == m) {
                out.push(Drift::McpMissing(m.clone()));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-disc-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(home: &Path, rel: &str, text: &str) {
        let p = home.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn skill(name: &str, desc: &str) -> String {
        format!("---\nname: {name}\ndescription: {desc}\n---\n\nBody of {name}.\n")
    }

    fn fixture(tag: &str) -> PathBuf {
        let h = home(tag);
        put(
            &h,
            ".claude/skills/review/SKILL.md",
            &skill("review", "Review a change"),
        );
        put(
            &h,
            ".codex/skills/plan/SKILL.md",
            &skill("plan", "Make a plan"),
        );
        put(
            &h,
            ".claude/plugins/cache/market/acme/1.0.0/skills/deploy/SKILL.md",
            &skill("deploy", "Deploy it"),
        );
        put(
            &h,
            ".claude/skills/nofront/SKILL.md",
            "Just text, no front matter.\n",
        );
        put(
            &h,
            ".claude.json",
            r#"{"mcpServers": {"files": {"command": "npx", "args": ["-y", "server", "--token", "SUPERSECRETARG"], "env": {"API_KEY": "SECRETENVVALUE"}},
                "web": {"type": "http", "url": "https://mcp.example.test/?key=SECRETURLKEY", "headers": {"Authorization": "Bearer SECRETHEADER"}}}}"#,
        );
        put(
            &h,
            ".codex/config.toml",
            "model = \"x\"\n\n[mcp_servers.github]\ncommand = \"gh\"\nbearer_token = \"SECRETTOML\"\n\n[mcp_servers.\"quoted name\"]\nurl = \"https://x.test\"\n",
        );
        h
    }

    #[test]
    fn skills_are_found_across_user_codex_and_plugin_folders_with_names_descriptions_and_hashes() {
        let d = discover(&fixture("skills"));
        let by = |n: &str| {
            d.skills
                .iter()
                .find(|s| s.name == n)
                .unwrap_or_else(|| panic!("no {n}: {:?}", d.skills))
        };
        assert_eq!(by("review").source, "claude-user");
        assert_eq!(by("review").description, "Review a change");
        assert_eq!(by("plan").source, "codex-user");
        assert_eq!(by("deploy").source, "claude-plugin:acme");
        assert_eq!(
            by("nofront").description,
            "",
            "no front matter: the folder name, no description"
        );
        assert_eq!(
            by("review").sha256,
            format!(
                "{:x}",
                Sha256::digest(skill("review", "Review a change").as_bytes())
            )
        );
        assert_eq!(by("review").path, ".claude/skills/review/SKILL.md");
        let names: Vec<&str> = d.skills.iter().map(|s| s.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "sorted by name");
        assert!(!d.truncated);
    }

    #[test]
    fn mcp_servers_are_listed_by_name_and_transport_only_and_no_credential_reaches_the_result() {
        let d = discover(&fixture("mcp"));
        let find = |n: &str| {
            d.mcp
                .iter()
                .find(|m| m.name == n)
                .unwrap_or_else(|| panic!("no {n}: {:?}", d.mcp))
        };
        assert_eq!(
            (
                find("files").source.as_str(),
                find("files").transport.as_str()
            ),
            ("claude", "stdio")
        );
        assert_eq!(find("web").transport, "http");
        assert_eq!(find("github").source, "codex");
        assert!(d.mcp.iter().any(|m| m.name == "quoted name"));
        let all = serde_json::to_string(&d).unwrap();
        for secret in [
            "SUPERSECRETARG",
            "SECRETENVVALUE",
            "SECRETURLKEY",
            "SECRETHEADER",
            "SECRETTOML",
            "example.test",
        ] {
            assert!(!all.contains(secret), "{secret} leaked into {all}");
        }
    }

    #[test]
    fn discovery_writes_nothing_and_does_not_follow_symlinks() {
        let h = fixture("readonly");
        let outside = home("outside");
        put(&outside, "evil/SKILL.md", &skill("evil", "outside"));
        std::fs::create_dir_all(h.join(".claude/skills")).unwrap();
        std::os::unix::fs::symlink(outside.join("evil"), h.join(".claude/skills/linked")).unwrap();
        let before = listing(&h);
        let d = discover(&h);
        assert_eq!(listing(&h), before, "nothing was created or changed");
        assert!(
            !d.skills
                .iter()
                .any(|s| s.name == "evil" || s.name == "linked"),
            "{:?}",
            d.skills
        );
    }

    fn listing(root: &Path) -> Vec<(String, u64, u128)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let m = std::fs::symlink_metadata(e.path()).unwrap();
                let t = m
                    .modified()
                    .unwrap()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                out.push((e.path().to_string_lossy().into_owned(), m.len(), t));
                if m.is_dir() {
                    stack.push(e.path());
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn a_missing_home_or_unreadable_files_give_an_empty_result_not_an_error() {
        assert_eq!(
            discover(Path::new("/nonexistent/home")),
            Discovery::default()
        );
        let h = home("broken");
        put(&h, ".claude.json", "{ not json");
        put(&h, ".codex/config.toml", "\u{0}\u{0}");
        assert_eq!(discover(&h), Discovery::default());
    }

    #[test]
    fn a_very_large_scan_stops_at_the_cap_and_says_so() {
        let h = home("cap");
        for i in 0..(MAX_ENTRIES + 5) {
            put(
                &h,
                &format!(".claude/skills/s{i:04}/SKILL.md"),
                &skill(&format!("s{i:04}"), "x"),
            );
        }
        let d = discover(&h);
        assert!(d.truncated);
        assert_eq!(d.skills.len(), MAX_ENTRIES);
    }

    #[test]
    fn a_selection_pins_skills_by_hash_persists_and_reports_drift() {
        let h = fixture("select");
        let proj = home("select-proj");
        let d = discover(&h);
        let mut sel = Selection::open(&proj).unwrap();
        sel.select_skill(&d, "review").unwrap();
        sel.select_mcp(&d, "github").unwrap();
        assert!(sel.select_skill(&d, "nope").is_err());
        assert!(sel.select_mcp(&d, "nope").is_err());
        assert_eq!(sel.drift(&d), vec![], "nothing changed yet");
        let reopened = Selection::open(&proj).unwrap();
        assert_eq!(reopened.chosen, sel.chosen);
        assert_eq!(reopened.chosen.skills[0].name, "review");
        // The skill's file changes, a server disappears, and a skill is removed.
        let mut sel = reopened;
        sel.select_skill(&d, "plan").unwrap();
        put(
            &h,
            ".claude/skills/review/SKILL.md",
            &skill("review", "Review a change, differently"),
        );
        put(&h, ".codex/config.toml", "model = \"x\"\n");
        std::fs::remove_file(h.join(".codex/skills/plan/SKILL.md")).unwrap();
        let d2 = discover(&h);
        assert_eq!(
            sel.drift(&d2),
            vec![
                Drift::SkillChanged("review".into()),
                Drift::SkillMissing("plan".into()),
                Drift::McpMissing("github".into())
            ]
        );
        sel.deselect("plan").unwrap();
        sel.deselect("github").unwrap();
        assert!(sel.deselect("never-selected").is_err());
        assert_eq!(sel.chosen.mcp, Vec::<String>::new());
        assert_eq!(sel.drift(&d2), vec![Drift::SkillChanged("review".into())]);
    }

    #[test]
    fn selecting_twice_replaces_the_pin() {
        let h = fixture("twice");
        let proj = home("twice-proj");
        let mut sel = Selection::open(&proj).unwrap();
        sel.select_skill(&discover(&h), "review").unwrap();
        put(&h, ".claude/skills/review/SKILL.md", &skill("review", "v2"));
        sel.select_skill(&discover(&h), "review").unwrap();
        assert_eq!(sel.chosen.skills.len(), 1);
        assert_eq!(sel.drift(&discover(&h)), vec![]);
    }
}
