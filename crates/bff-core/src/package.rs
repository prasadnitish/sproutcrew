//! The portable package: brief, plan, sources, artifacts, decisions, evidence, optional prototype, and run
//! instructions, with a manifest of hashes. The text going out is scanned first, and one secret stops the
//! export before anything is written. Private details are listed as warnings. Links between the package's
//! Markdown files are checked, and a required part that is empty is named in the manifest instead of
//! being skipped silently.

use crate::flow::Studio;
use crate::secrets::{scan, Class};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::path::Path;

pub struct ExportOpts<'a> {
    pub run_instructions: &'a str,
    pub evidence: Option<&'a Value>,
    pub prototype_dir: Option<&'a Path>,
    /// The project's code, under `code/`.
    pub code_dir: Option<&'a Path>,
    /// Files made for the package, as `(path, bytes)`.
    pub extras: &'a [(String, Vec<u8>)],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub files: Vec<FileEntry>,
    /// Private details found, as `path: kind`. Never the value.
    pub warnings: Vec<String>,
    /// `path -> target` for Markdown links that do not resolve inside the package.
    pub broken_links: Vec<String>,
    /// Required parts that have no content.
    pub missing: Vec<String>,
    /// `stage: reason` for each stage or task that failed and never finished, so a partial package says so.
    #[serde(default)]
    pub incomplete: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExportError {
    /// The destination already exists. Nothing is overwritten.
    Exists,
    /// `path: kind (line N)` for each secret. Nothing was written.
    Secrets(Vec<String>),
    NoPlan,
    Io(String),
}

const TEXT_EXT: &[&str] = &[
    "md", "txt", "json", "csv", "html", "htm", "js", "css", "svg",
];
const MAX_FILE: u64 = 25 * 1024 * 1024;

type Parts = std::collections::BTreeMap<String, Vec<u8>>;

fn io<E: std::fmt::Display>(e: E) -> ExportError {
    ExportError::Io(e.to_string())
}

/// Reads a folder's files into `parts` under `prefix`. A symlink is named in the warnings and not followed.
fn walk(
    dir: &Path,
    prefix: &str,
    parts: &mut Parts,
    warnings: &mut Vec<String>,
) -> Result<(), ExportError> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if [".git", ".hg", ".svn"].contains(&name.as_str()) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let meta = std::fs::symlink_metadata(e.path()).map_err(io)?;
        if meta.file_type().is_symlink() {
            warnings.push(format!("{rel}: symlink not followed"));
        } else if meta.is_dir() {
            walk(&e.path(), &rel, parts, warnings)?;
        } else if meta.is_file() {
            if meta.len() > MAX_FILE {
                warnings.push(format!("{rel}: larger than the package limit, left out"));
            } else {
                parts.insert(rel, std::fs::read(e.path()).map_err(io)?);
            }
        }
    }
    Ok(())
}

/// Stages and side tasks that failed and have no finished attempt, with the latest reason.
fn incomplete(studio: &Studio) -> Result<Vec<String>, ExportError> {
    let mut stage_of = std::collections::BTreeMap::new();
    let mut reason: std::collections::BTreeMap<String, String> = Default::default();
    let mut finished = std::collections::BTreeSet::new();
    for e in studio.journal.events().map_err(io)? {
        let task = e.payload["task"].as_str().unwrap_or("").to_string();
        match e.kind.as_str() {
            "worker.dispatched" => {
                stage_of.insert(task, e.payload["stage"].as_str().unwrap_or("").to_string());
            }
            "worker.failed" => {
                if let Some(stage) = stage_of.get(&task) {
                    reason.insert(
                        stage.clone(),
                        e.payload["reason"].as_str().unwrap_or("").to_string(),
                    );
                }
            }
            "worker.finished" => {
                if let Some(stage) = stage_of.get(&task) {
                    finished.insert(stage.clone());
                }
            }
            _ => {}
        }
    }
    Ok(reason
        .into_iter()
        .filter(|(stage, _)| !finished.contains(stage))
        .map(|(stage, why)| format!("{stage}: {why}"))
        .collect())
}

fn decisions(studio: &Studio) -> Result<String, ExportError> {
    let mut out = String::from("# Decisions\n\n");
    for e in studio.journal.events().map_err(io)? {
        let p = &e.payload;
        let t = |k: &str| p[k].as_str().unwrap_or("").to_string();
        let line = match e.kind.as_str() {
            "plan.approved" => format!("Plan approved by {}.", t("by")),
            "review.received" => format!(
                "{} reviewed {}: {} finding(s).",
                t("reviewer"),
                t("name"),
                p["report"]["findings"].as_array().map_or(0, Vec::len)
            ),
            "review.decided" if p["decision"] == "accept" => {
                format!("Finding {} on {} accepted.", t("finding"), t("name"))
            }
            "review.decided" => format!(
                "Finding {} on {} rejected: {}",
                t("finding"),
                t("name"),
                t("reason")
            ),
            "artifact.changed" => format!(
                "{} revised by {} ({} change).",
                t("name"),
                t("by"),
                if p["material"] == false {
                    "cosmetic"
                } else {
                    "material"
                }
            ),
            "worker.failed" => format!("Task {} failed: {}", t("task"), t("reason")),
            _ => continue,
        };
        out.push_str(&format!("- {line}\n"));
    }
    Ok(out)
}

/// Relative Markdown link targets in `text`.
fn md_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        if let Some(j) = rest.find(')') {
            let t = rest[..j].trim();
            let t = t.split(['#', '?']).next().unwrap_or("");
            if !t.is_empty() && !t.contains("://") && !t.starts_with("mailto:") {
                out.push(t.to_string());
            }
            rest = &rest[j..];
        }
    }
    out
}

/// `dir/../x` resolved against the package root, or `None` if it escapes.
fn join_rel(dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// The steps that hold the deck, the finished one first.
const DECKS: [&str; 3] = ["finish_deck", "polish_deck", "deck"];

/// The step whose newest result is the project's final document. A check, a README, code, and a deck are not it, so
/// it is the finished document if there is one, and otherwise the last writing step that has a result. A project
/// that builds code and has no such document has no final document: what it built is its result.
pub fn final_step(plan: &crate::plan::Plan, artifacts: &[Value]) -> Option<String> {
    let done = |id: &str| artifacts.iter().any(|a| a["name"].as_str() == Some(id));
    let prose = |s: &&crate::plan::Stage| {
        !crate::workflows::checks(&s.owner_role)
            && !crate::workflows::writes_code(&s.owner_role)
            && done(&s.id)
    };
    let named = ["finish_document", "polish_document", "document"]
        .iter()
        .find_map(|id| plan.stages.iter().filter(prose).find(|s| s.id == *id));
    let builds = plan
        .stages
        .iter()
        .any(|s| crate::workflows::writes_code(&s.owner_role));
    named
        .or_else(|| {
            let last = plan
                .stages
                .iter()
                .rev()
                .filter(prose)
                .find(|s| !DECKS.contains(&s.id.as_str()));
            last.filter(|_| !builds)
        })
        .map(|s| s.id.clone())
}

/// The step whose newest result is the project's final deck. A plan with no deck has none, even if a closing step
/// for one was left in it.
pub fn final_deck(plan: &crate::plan::Plan, artifacts: &[Value]) -> Option<String> {
    let planned = |id: &str| plan.stages.iter().any(|s| s.id == id);
    if !planned("deck") && !planned("polish_deck") {
        return None;
    }
    DECKS
        .iter()
        .find(|id| planned(id) && artifacts.iter().any(|a| a["name"].as_str() == Some(id)))
        .map(|id| id.to_string())
}

/// The most slides, and the most points on one, of a deck made from a document.
const DECK_SLIDES: usize = 12;
const DECK_POINTS: usize = 5;
const POINT_CHARS: usize = 160;

/// A paragraph or list item as one slide point: its first sentence, without the closing full stop, and cut at a
/// word when it is long and has no formatting a cut could break.
fn slide_point(text: &str) -> Option<String> {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    // A sentence ends at . ! or ? followed by a space and a capital, a digit, a quote, or the end. "e.g. in" and
    // "Dr. Smith" do not end one.
    const ABBREVIATIONS: [&str; 10] = [
        "e.g", "i.e", "vs", "etc", "dr", "mr", "mrs", "ms", "no", "inc",
    ];
    let mut end = chars.len();
    for (i, c) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let next = chars.get(i + 1);
        if next.is_some_and(|n| *n != ' ') {
            continue;
        }
        let after = chars.get(i + 2);
        if after.is_some_and(|a| a.is_lowercase()) {
            continue;
        }
        let word: String = chars[..i]
            .iter()
            .rev()
            .take_while(|c| !c.is_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if *c == '.' && ABBREVIATIONS.contains(&word.to_lowercase().as_str()) {
            continue;
        }
        end = i + 1;
        break;
    }
    let mut point: String = chars[..end].iter().collect();
    if let Some(p) = point.strip_suffix('.') {
        point = p.to_string();
    }
    if point.chars().count() > POINT_CHARS && !point.contains(['*', '_', '`', '[', ']', '<']) {
        let cut: String = point.chars().take(POINT_CHARS - 3).collect();
        let at = cut.rfind(' ').unwrap_or(cut.len());
        point = format!("{}…", cut[..at].trim_end_matches([',', ';', ':', ' ']));
    }
    let point = point.trim().to_string();
    (!point.is_empty()).then_some(point)
}

/// A short deck from a document: its title, then a slide for each `##` section (twelve at most) with up to five
/// points, each the first sentence of a paragraph or list item. Tables, code, and the notes for the team are not
/// points. A document with no sections gets one "Summary" slide from its opening.
pub fn deck_from_document(md: &str) -> String {
    let (md, _) = crate::clientfiles::strip_internal(md);
    let mut title: Option<String> = None;
    // The opening before the first section, then each section's title and points.
    let mut slides: Vec<(String, Vec<String>)> = vec![(String::new(), vec![])];
    let mut buffer: Vec<String> = vec![];
    let mut fence: Option<char> = None;
    fn flush(buffer: &mut Vec<String>, slides: &mut [(String, Vec<String>)]) {
        let text = buffer.join(" ");
        buffer.clear();
        let points = &mut slides.last_mut().expect("a slide").1;
        if points.len() < DECK_POINTS {
            points.extend(slide_point(&text));
        }
    }
    for raw in md.lines() {
        let line = raw.trim();
        let opens = ['`', '~']
            .into_iter()
            .find(|c| line.starts_with(&c.to_string().repeat(3)));
        match (fence, opens) {
            (None, Some(c)) => {
                flush(&mut buffer, &mut slides);
                fence = Some(c);
                continue;
            }
            (Some(f), Some(c)) if f == c => {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            _ => {}
        }
        if let Some((level, text)) = crate::clientfiles::heading_level(raw) {
            flush(&mut buffer, &mut slides);
            match level {
                1 if title.is_none() && slides.len() == 1 => title = Some(text.to_string()),
                1 | 2 => slides.push((text.to_string(), vec![])),
                _ => {}
            }
            continue;
        }
        let rest = line.trim_start_matches('>').trim_start();
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let item = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| rest.strip_prefix(m))
            .or_else(|| {
                rest[digits..]
                    .strip_prefix(". ")
                    .or_else(|| rest[digits..].strip_prefix(") "))
                    .filter(|_| digits > 0)
            });
        if line.is_empty()
            || line.starts_with('|')
            || line.starts_with("![")
            || (line.len() >= 3 && line.chars().all(|c| matches!(c, '-' | '*' | '_')))
        {
            flush(&mut buffer, &mut slides);
        } else if let Some(text) = item {
            flush(&mut buffer, &mut slides);
            buffer.push(text.to_string());
        } else {
            buffer.push(rest.to_string());
        }
    }
    flush(&mut buffer, &mut slides);
    let opening = slides.remove(0);
    if slides.is_empty() && !opening.1.is_empty() {
        slides.push(("Summary".to_string(), opening.1));
    }
    let mut out = title.map_or(String::new(), |t| format!("# {t}\n\n"));
    for (heading, points) in slides
        .into_iter()
        .filter(|s| !s.1.is_empty())
        .take(DECK_SLIDES)
    {
        out.push_str(&format!("## {heading}\n\n"));
        for p in points {
            out.push_str(&format!("- {p}\n"));
        }
        out.push('\n');
    }
    out.trim_end().to_string() + "\n"
}

/// A step's title as a file name.
fn slug(title: &str) -> String {
    let words: Vec<String> = title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    words.join("-").chars().take(60).collect()
}

/// The numbered results, the final document, and a guide to the folder.
fn readable_parts(
    studio: &Studio,
    plan: &crate::plan::Plan,
    parts: &mut Parts,
    code: bool,
) -> Result<(), ExportError> {
    let root = &studio.project.root;
    let artifacts = studio.project.artifacts().map_err(io)?;
    let newest = |id: &str| -> Option<Vec<u8>> {
        let file = artifacts
            .iter()
            .rev()
            .find(|a| a["name"].as_str() == Some(id))?["file"]
            .as_str()?
            .to_string();
        std::fs::read(root.join("artifacts").join(file)).ok()
    };
    let mut guide = String::from("# What is in this folder\n\n");
    let last = final_step(plan, &artifacts);
    if let Some(bytes) = last.as_deref().and_then(newest) {
        parts.insert("Final document.md".into(), bytes);
        guide.push_str("- **Final document**: the finished result, as Markdown, and as Word and PDF when those could be made.\n");
    }
    if code {
        guide.push_str("- **code/**: everything that was built.\n");
    }
    guide.push_str("- **sources/**: the files the work was based on.\n- **decision-log.md**: what the checkers said and what changed.\n\n## Each step's result\n\n");
    let mut n = 0;
    for st in &plan.stages {
        if let Some(bytes) = newest(&st.id) {
            n += 1;
            let path = format!("steps/{n:02}-{}.md", slug(&st.title));
            guide.push_str(&format!("{n}. {}: {path}\n", st.title));
            parts.insert(path, bytes);
        }
    }
    parts.insert("README.md".into(), guide.into_bytes());
    Ok(())
}

pub fn export(studio: &Studio, out: &Path, opts: &ExportOpts) -> Result<Manifest, ExportError> {
    if out.exists() {
        return Err(ExportError::Exists);
    }
    let plan = studio.plan().ok_or(ExportError::NoPlan)?;
    let root = &studio.project.root;
    let mut warnings: Vec<String> = Vec::new();
    let mut parts = Parts::new();
    parts.insert(
        "brief.md".into(),
        studio.project.brief().map_err(io)?.into_bytes(),
    );
    parts.insert(
        "plan.json".into(),
        serde_json::to_vec_pretty(plan).map_err(io)?,
    );
    parts.insert("decisions.md".into(), decisions(studio)?.into_bytes());
    parts.insert(
        "decision-log.md".into(),
        crate::changelog::build(studio, crate::clock::now())
            .map_err(io)?
            .into_bytes(),
    );
    parts.insert(
        "Run report.md".into(),
        crate::run_report::build(studio, crate::clock::now())
            .map_err(io)?
            .into_bytes(),
    );
    let incomplete = incomplete(studio)?;
    let mut run = format!("# How to run this package\n\n{}\n", opts.run_instructions);
    if !incomplete.is_empty() {
        run.push_str(
            "\n## This package is incomplete\n\nThese stages failed and did not finish:\n\n",
        );
        for i in &incomplete {
            run.push_str(&format!("- {i}\n"));
        }
    }
    parts.insert("RUN.md".into(), run.into_bytes());
    if let Some(ev) = opts.evidence {
        parts.insert(
            "evidence.json".into(),
            serde_json::to_vec_pretty(ev).map_err(io)?,
        );
    }
    walk(&root.join("sources"), "sources", &mut parts, &mut warnings)?;
    walk(
        &root.join("artifacts"),
        "artifacts",
        &mut parts,
        &mut warnings,
    )?;
    if let Ok(b) = std::fs::read(root.join("artifacts.json")) {
        parts.insert("artifacts.json".into(), b);
    }
    if let Some(p) = opts.prototype_dir {
        walk(p, "prototype", &mut parts, &mut warnings)?;
    }
    if let Some(c) = opts.code_dir {
        walk(c, "code", &mut parts, &mut warnings)?;
    }
    readable_parts(studio, plan, &mut parts, opts.code_dir.is_some())?;
    for (path, bytes) in opts.extras {
        parts.insert(path.clone(), bytes.clone());
    }

    // Scan before anything is written.
    let mut secrets: Vec<String> = Vec::new();
    for (path, bytes) in &parts {
        let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if !TEXT_EXT.contains(&ext) && std::str::from_utf8(bytes).is_err() {
            continue;
        }
        for f in scan(&String::from_utf8_lossy(bytes)) {
            match f.class {
                Class::Secret => secrets.push(format!("{path}: {} (line {})", f.kind, f.line)),
                Class::Privacy => warnings.push(format!("{path}: {}", f.kind)),
            }
        }
    }
    if !secrets.is_empty() {
        return Err(ExportError::Secrets(secrets));
    }

    let mut broken_links = Vec::new();
    // The numbered results and the final document are copies, and their originals are checked in `artifacts/`.
    let copy = |p: &str| p.starts_with("steps/") || p == "Final document.md";
    for (path, bytes) in parts.iter().filter(|(p, _)| p.ends_with(".md") && !copy(p)) {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for target in md_links(&String::from_utf8_lossy(bytes)) {
            let ok = join_rel(dir, &target).is_some_and(|r| parts.contains_key(&r));
            if !ok {
                broken_links.push(format!("{path} -> {target}"));
            }
        }
    }
    let has = |prefix: &str| parts.keys().any(|k| k.starts_with(prefix));
    let mut missing = Vec::new();
    if !has("sources/") {
        missing.push("sources".to_string());
    }
    if !has("artifacts/") {
        missing.push("artifacts".to_string());
    }

    let files = parts
        .iter()
        .map(|(path, bytes)| FileEntry {
            path: path.clone(),
            sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        })
        .collect();
    warnings.sort();
    warnings.dedup();
    let manifest = Manifest {
        files,
        warnings,
        broken_links,
        missing,
        incomplete,
    };

    let partial = out.with_extension("partial");
    let write = || -> Result<(), ExportError> {
        for (path, bytes) in &parts {
            let dest = partial.join(path);
            std::fs::create_dir_all(dest.parent().unwrap_or(&partial)).map_err(io)?;
            std::fs::write(&dest, bytes).map_err(io)?;
        }
        std::fs::write(
            partial.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(io)?,
        )
        .map_err(io)?;
        std::fs::rename(&partial, out).map_err(io)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ARTIFACT_FILE;
    use crate::lead::parse_plan;
    use crate::sources::SourceStore;
    use crate::workspace::Project;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    struct Fx {
        studio: Studio,
        out: PathBuf,
    }

    fn setup(tag: &str, artifact_text: &str, with_source: bool) -> Fx {
        let base = std::env::temp_dir().join(format!("bff-pkg-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let mut studio = Studio::open(
            Project::create(&base, "case", "Write a short note about the pilot.", 1).unwrap(),
        )
        .unwrap();
        let v = json!({"stages": [{"id": "draft", "title": "Draft", "owner_role": "writer"}],
            "team": [{"role": "writer", "route": "R1", "model": "gpt-6-luna", "effort": "low"}]});
        studio
            .set_plan(
                parse_plan("Write a short note about the pilot.", &v).unwrap(),
                2,
            )
            .unwrap();
        studio.approve("Nitish", 3).unwrap();
        let d = studio.dispatch("draft", 4).unwrap();
        std::fs::write(d.dir.join(ARTIFACT_FILE), artifact_text).unwrap();
        studio.complete(&d, ARTIFACT_FILE, "draft", &[], 5).unwrap();
        if with_source {
            let mut s = SourceStore::open(&studio.project.root).unwrap();
            s.import("notes.txt", b"The pilot costs $20 per month.")
                .unwrap();
        }
        let out = base.join("export");
        Fx { studio, out }
    }

    fn opts() -> ExportOpts<'static> {
        ExportOpts {
            run_instructions: "Open index.html in a browser.",
            evidence: None,
            prototype_dir: None,
            code_dir: None,
            extras: &[],
        }
    }

    fn sha(p: &Path) -> String {
        format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()))
    }

    #[test]
    fn a_package_has_the_brief_plan_sources_artifacts_decisions_and_run_instructions_with_true_hashes(
    ) {
        let fx = setup("full", "# Note\nThe pilot costs $20 per month. See [the source](../sources/src-1/snapshot.json).\n", true);
        let m = export(&fx.studio, &fx.out, &opts()).unwrap();
        for f in [
            "brief.md",
            "plan.json",
            "decisions.md",
            "RUN.md",
            "manifest.json",
            "sources/src-1/original.txt",
            "sources/src-1/snapshot.json",
            "artifacts.json",
        ] {
            assert!(fx.out.join(f).is_file(), "{f} missing");
        }
        assert!(std::fs::read_dir(fx.out.join("artifacts")).unwrap().count() >= 1);
        assert_eq!(
            std::fs::read_to_string(fx.out.join("brief.md")).unwrap(),
            "Write a short note about the pilot."
        );
        assert!(std::fs::read_to_string(fx.out.join("RUN.md"))
            .unwrap()
            .contains("Open index.html"));
        assert!(std::fs::read_to_string(fx.out.join("decisions.md"))
            .unwrap()
            .contains("approved by Nitish"));
        assert!(!m.files.is_empty());
        for f in &m.files {
            assert_eq!(f.sha256, sha(&fx.out.join(&f.path)), "{}", f.path);
            assert_eq!(
                f.bytes,
                std::fs::metadata(fx.out.join(&f.path)).unwrap().len()
            );
        }
        assert!(
            m.files.iter().all(|f| f.path != "manifest.json"),
            "the manifest does not list itself"
        );
        let on_disk: Manifest =
            serde_json::from_str(&std::fs::read_to_string(fx.out.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(on_disk, m);
        assert!(
            m.missing.is_empty() && m.warnings.is_empty() && m.broken_links.is_empty(),
            "{m:?}"
        );
    }

    #[test]
    fn one_secret_anywhere_stops_the_export_and_writes_nothing() {
        let fx = setup(
            "secret",
            "# Note\nkey sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789\n",
            true,
        );
        let err = export(&fx.studio, &fx.out, &opts()).unwrap_err();
        match err {
            ExportError::Secrets(list) => {
                assert!(list.iter().any(|l| l.contains("provider key")), "{list:?}");
                assert!(
                    list.iter().all(|l| !l.contains("0123456789")),
                    "the report must not repeat the secret"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(!fx.out.exists(), "nothing was created");
        assert!(!fx.out.with_extension("partial").exists());
    }

    #[test]
    fn a_secret_in_a_source_snapshot_or_the_run_instructions_or_evidence_also_stops_it() {
        let fx = setup("secret-src", "# Note\nclean\n", false);
        let mut s = SourceStore::open(&fx.studio.project.root).unwrap();
        s.import("leak.txt", b"password = hunter2hunter2").unwrap();
        assert!(matches!(
            export(&fx.studio, &fx.out, &opts()),
            Err(ExportError::Secrets(_))
        ));
        let fx = setup("secret-run", "# Note\nclean\n", false);
        let bad = ExportOpts {
            run_instructions: "export TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            evidence: None,
            prototype_dir: None,
            code_dir: None,
            extras: &[],
        };
        assert!(matches!(
            export(&fx.studio, &fx.out, &bad),
            Err(ExportError::Secrets(_))
        ));
        let fx = setup("secret-ev", "# Note\nclean\n", false);
        let ev = json!({"claims": [{"text": "AKIAIOSFODNN7EXAMPLE"}]});
        let bad = ExportOpts {
            run_instructions: "ok",
            evidence: Some(&ev),
            prototype_dir: None,
            code_dir: None,
            extras: &[],
        };
        assert!(matches!(
            export(&fx.studio, &fx.out, &bad),
            Err(ExportError::Secrets(_))
        ));
    }

    #[test]
    fn private_details_are_exported_with_a_warning_that_names_the_file_and_kind_only() {
        let fx = setup(
            "privacy",
            "# Note\nSaved at /Users/pat/Documents/plan.md\n",
            false,
        );
        let m = export(&fx.studio, &fx.out, &opts()).unwrap();
        assert!(
            m.warnings
                .iter()
                .any(|w| w.contains("home path") && w.contains("artifacts/")),
            "{:?}",
            m.warnings
        );
        assert!(
            m.warnings.iter().all(|w| !w.contains("nitish")),
            "{:?}",
            m.warnings
        );
    }

    #[test]
    fn an_existing_destination_is_never_overwritten() {
        let fx = setup("exists", "# Note\nclean\n", false);
        std::fs::create_dir_all(&fx.out).unwrap();
        std::fs::write(fx.out.join("keep.txt"), "mine").unwrap();
        assert_eq!(
            export(&fx.studio, &fx.out, &opts()),
            Err(ExportError::Exists)
        );
        assert_eq!(
            std::fs::read_to_string(fx.out.join("keep.txt")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn links_between_package_files_are_checked() {
        let fx = setup("links", "# Note\n[good](../brief.md) [bad](missing.md) [web](https://example.test) [anchor](#top)\n", false);
        let m = export(&fx.studio, &fx.out, &opts()).unwrap();
        assert_eq!(m.broken_links.len(), 1, "{:?}", m.broken_links);
        assert!(m.broken_links[0].contains("missing.md"));
    }

    #[test]
    fn empty_required_parts_are_named_in_the_manifest() {
        let fx = setup("missing", "# Note\nclean\n", false);
        let m = export(&fx.studio, &fx.out, &opts()).unwrap();
        assert!(m.missing.iter().any(|x| x == "sources"), "{:?}", m.missing);
        assert!(!m.missing.iter().any(|x| x == "artifacts"));
    }

    #[test]
    fn a_prototype_folder_is_copied_without_following_symlinks() {
        let fx = setup("proto", "# Note\nclean\n", false);
        let proto = fx.out.parent().unwrap().join("proto");
        std::fs::create_dir_all(proto.join("js")).unwrap();
        std::fs::write(proto.join("index.html"), "<h1>x</h1>").unwrap();
        std::fs::write(proto.join("js/app.js"), "1").unwrap();
        let outside = fx.out.parent().unwrap().join("outside.txt");
        std::fs::write(&outside, "OUTSIDE").unwrap();
        std::os::unix::fs::symlink(&outside, proto.join("link.txt")).unwrap();
        let o = ExportOpts {
            run_instructions: "r",
            evidence: None,
            prototype_dir: Some(&proto),
            code_dir: None,
            extras: &[],
        };
        let m = export(&fx.studio, &fx.out, &o).unwrap();
        assert!(
            fx.out.join("prototype/index.html").is_file()
                && fx.out.join("prototype/js/app.js").is_file()
        );
        assert!(!fx.out.join("prototype/link.txt").exists());
        assert!(
            m.warnings
                .iter()
                .any(|w| w.contains("link.txt") && w.contains("symlink")),
            "{:?}",
            m.warnings
        );
        assert!(!std::fs::read_to_string(fx.out.join("manifest.json"))
            .unwrap()
            .contains("OUTSIDE"));
    }

    #[test]
    fn the_manifest_is_deterministic_and_the_project_is_untouched() {
        let fx = setup("det", "# Note\nclean\n", true);
        let before = fx.studio.journal.events().unwrap().len();
        let a = export(&fx.studio, &fx.out, &opts()).unwrap();
        let b = export(&fx.studio, &fx.out.with_file_name("export2"), &opts()).unwrap();
        assert_eq!(a, b);
        assert_eq!(fx.studio.journal.events().unwrap().len(), before);
        let paths: Vec<&String> = a.files.iter().map(|f| &f.path).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "files are listed in order");
    }

    #[test]
    fn evidence_is_included_when_given() {
        let fx = setup("ev", "# Note\nclean\n", false);
        let ev = json!({"claims": [{"id": "c1", "class": "E2"}]});
        let o = ExportOpts {
            run_instructions: "r",
            evidence: Some(&ev),
            prototype_dir: None,
            code_dir: None,
            extras: &[],
        };
        export(&fx.studio, &fx.out, &o).unwrap();
        let back: Value =
            serde_json::from_str(&std::fs::read_to_string(fx.out.join("evidence.json")).unwrap())
                .unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn a_stage_with_an_earlier_success_is_not_incomplete_even_if_a_later_attempt_failed() {
        let mut fx = setup("partial", "# Note\nclean\n", false);
        // A second stage attempt fails.
        let d = fx.studio.dispatch("draft", 6).unwrap();
        fx.studio.fail(&d, "timed out", &[], 7).unwrap();
        let m = export(&fx.studio, &fx.out, &opts()).unwrap();
        assert!(
            m.incomplete.is_empty(),
            "the stage has an artifact: {:?}",
            m.incomplete
        );
    }

    #[test]
    fn an_unfinished_stage_is_named_in_the_manifest_and_the_run_notes() {
        let base = std::env::temp_dir().join(format!("bff-pkg-{}-unfinished", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let mut studio = Studio::open(Project::create(&base, "case", "b", 1).unwrap()).unwrap();
        let v = json!({"stages": [{"id": "draft", "title": "Draft", "owner_role": "writer"}],
            "team": [{"role": "writer", "route": "R1", "model": "gpt-6-luna", "effort": "low"}]});
        studio.set_plan(parse_plan("b", &v).unwrap(), 2).unwrap();
        studio.approve("Nitish", 3).unwrap();
        let d = studio.dispatch("draft", 4).unwrap();
        studio.fail(&d, "timed out", &[], 5).unwrap();
        let out = base.join("export");
        let m = export(&studio, &out, &opts()).unwrap();
        assert_eq!(m.incomplete, vec!["draft: timed out".to_string()]);
        let run = std::fs::read_to_string(out.join("RUN.md")).unwrap();
        assert!(
            run.contains("incomplete") && run.contains("draft: timed out"),
            "{run}"
        );
        // The same stage then succeeds on a later attempt: nothing is incomplete.
        let d = studio.dispatch("draft", 6).unwrap();
        std::fs::write(d.dir.join(ARTIFACT_FILE), "ok").unwrap();
        studio.complete(&d, ARTIFACT_FILE, "draft", &[], 7).unwrap();
        let m = export(&studio, &base.join("export2"), &opts()).unwrap();
        assert!(m.incomplete.is_empty(), "{:?}", m.incomplete);
    }

    /// A plan of steps with these ids and roles, on one writer's route.
    fn plan_of(steps: &[(&str, &str)]) -> crate::plan::Plan {
        let stages: Vec<Value> = steps
            .iter()
            .map(|(id, role)| json!({"id": id, "title": id, "owner_role": role}))
            .collect();
        parse_plan("b", &json!({"stages": stages, "team": []})).unwrap()
    }
    fn done(ids: &[&str]) -> Vec<Value> {
        ids.iter().map(|id| json!({"name": id})).collect()
    }

    #[test]
    fn the_final_document_is_the_polished_one_and_never_a_check_a_readme_or_code() {
        let p = plan_of(&[
            ("frame", "analyst"),
            ("review", "reviewer"),
            ("document", "editor"),
            ("polish_document", "humaniser"),
            ("prototype", "builder"),
            ("final", "reviewer"),
        ]);
        let all = done(&[
            "frame",
            "review",
            "document",
            "polish_document",
            "prototype",
            "final",
        ]);
        assert_eq!(final_step(&p, &all).as_deref(), Some("polish_document"));
        let no_polish = done(&["frame", "review", "document", "final"]);
        assert_eq!(final_step(&p, &no_polish).as_deref(), Some("document"));
        assert_eq!(
            final_step(&p, &done(&["frame", "review"])),
            None,
            "an early draft is not the final document of a project that also builds"
        );
        let written = plan_of(&[("frame", "analyst"), ("review", "reviewer")]);
        assert_eq!(
            final_step(&written, &done(&["frame", "review"])).as_deref(),
            Some("frame")
        );
        assert_eq!(final_step(&p, &done(&["review"])), None);
    }

    #[test]
    fn the_step_that_applied_the_last_check_is_the_final_document_and_final_deck() {
        let p = plan_of(&[
            ("document", "editor"),
            ("deck", "presenter"),
            ("polish_document", "humaniser"),
            ("polish_deck", "humaniser"),
            ("final", "reviewer"),
            ("finish_deck", "presenter"),
            ("finish_document", "editor"),
        ]);
        let all = done(&[
            "document",
            "deck",
            "polish_document",
            "polish_deck",
            "final",
            "finish_deck",
            "finish_document",
        ]);
        assert_eq!(final_step(&p, &all).as_deref(), Some("finish_document"));
        assert_eq!(final_deck(&p, &all).as_deref(), Some("finish_deck"));
        let older = done(&[
            "document",
            "deck",
            "polish_document",
            "polish_deck",
            "final",
        ]);
        assert_eq!(final_step(&p, &older).as_deref(), Some("polish_document"));
        assert_eq!(final_deck(&p, &older).as_deref(), Some("polish_deck"));
        // A deck is never the document, whichever step is the last one to write.
        let deck_only = done(&["deck", "polish_deck", "finish_deck"]);
        assert_eq!(final_step(&p, &deck_only), None);
        // With the deck taken out of the plan, a closing step left behind has nothing to finish.
        let no_deck = plan_of(&[
            ("document", "editor"),
            ("final", "reviewer"),
            ("finish_deck", "presenter"),
        ]);
        assert_eq!(
            final_deck(&no_deck, &done(&["document", "final", "finish_deck"])),
            None
        );
        assert_eq!(final_deck(&plan_of(&[("document", "editor")]), &all), None);
    }

    #[test]
    fn an_app_project_has_no_final_document_to_name() {
        let p = plan_of(&[
            ("spec", "architect"),
            ("build", "builder"),
            ("document", "docs"),
            ("review", "reviewer"),
        ]);
        let f = final_step(&p, &done(&["spec", "build", "document", "review"]));
        assert_eq!(
            f, None,
            "what it built is the result, not its README or specification"
        );
    }

    #[test]
    fn an_export_has_a_final_document_each_step_numbered_a_guide_the_code_and_extras() {
        let fx = setup("everything", "# The note\n\nFinal words.", true);
        let code = fx.out.parent().unwrap().join("the-code");
        std::fs::create_dir_all(code.join("src")).unwrap();
        std::fs::write(code.join("src/main.py"), "print('hi')").unwrap();
        let extras = vec![("Final document.docx".to_string(), vec![1u8, 2, 3])];
        let o = ExportOpts {
            run_instructions: "x",
            evidence: None,
            prototype_dir: None,
            code_dir: Some(&code),
            extras: &extras,
        };
        let m = export(&fx.studio, &fx.out, &o).unwrap();
        let read = |p: &str| std::fs::read_to_string(fx.out.join(p)).unwrap();
        assert!(read("Final document.md").contains("Final words."));
        assert!(read("steps/01-draft.md").contains("Final words."));
        assert_eq!(read("code/src/main.py"), "print('hi')");
        assert!(fx.out.join("Final document.docx").is_file());
        let guide = read("README.md");
        assert!(
            guide.contains("Final document")
                && guide.contains("steps/01-draft.md")
                && guide.contains("code/"),
            "{guide}"
        );
        assert!(
            m.files.iter().any(|f| f.path == "Final document.docx"),
            "extras are in the manifest"
        );
    }

    #[test]
    fn a_secret_in_a_step_result_still_stops_the_export() {
        let secret = format!("key sk-ant-api03-{}", "abcdefghij".repeat(4));
        let fx = setup("step-secret", &secret, true);
        assert!(matches!(
            export(&fx.studio, &fx.out, &opts()),
            Err(ExportError::Secrets(_))
        ));
        assert!(!fx.out.exists());
    }

    #[test]
    fn a_deck_made_from_a_document_has_a_slide_for_each_section_with_a_few_short_points() {
        let doc = "# Why the freight is late\n\nThe lede paragraph. It is skipped by the slides.\n\n## What happened\n\nPort delays hit in March. Nobody told the carriers.\n\nVolumes were up 12% on the year, e.g. in April. More text.\n\n- First point here. And more.\n- Second point\n- Third point\n- Fourth point\n\n## What it cost\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncode is not a point.\n```\n\nIt cost **$40k** in fees? Yes.\n\n## What to do\n\nSwitch carriers.\n\n### A detail\n\nAnd renegotiate rates.\n";
        let deck = deck_from_document(doc);
        assert!(deck.starts_with("# Why the freight is late\n"), "{deck}");
        let sections: Vec<&str> = deck.split("\n## ").skip(1).collect();
        assert_eq!(sections.len(), 3, "{deck}");
        for s in &sections {
            let points = s.lines().filter(|l| l.starts_with("- ")).count();
            assert!((1..=5).contains(&points), "{s}");
        }
        assert!(sections[0].starts_with("What happened"));
        assert!(
            sections[0].contains("- Port delays hit in March\n"),
            "first sentence only, no full stop: {deck}"
        );
        assert!(!sections[0].contains("Nobody told"));
        assert!(
            sections[0].contains("- Volumes were up 12% on the year, e.g. in April\n"),
            "{deck}"
        );
        assert!(sections[0].contains("- First point here\n") && !sections[0].contains("Fourth"));
        assert!(!deck.contains("lede") && !deck.contains("code is not") && !deck.contains('|'));
        assert!(
            sections[1].contains("- It cost **$40k** in fees?"),
            "{deck}"
        );
        assert!(
            sections[2].contains("- Switch carriers\n")
                && sections[2].contains("- And renegotiate rates\n")
        );
        // It is a deck the exporter reads: a title, and a slide for each section.
        let parsed = crate::office_export::parse("T", "", &deck).unwrap();
        assert_eq!(
            parsed
                .blocks
                .iter()
                .filter(|b| b.kind == "heading" && b.level == 2)
                .count(),
            3
        );
    }

    #[test]
    fn a_deck_made_from_a_document_stops_at_twelve_slides_and_leaves_out_what_is_for_the_team() {
        let mut doc = String::from("# Big\n\n");
        for n in 1..=15 {
            doc.push_str(&format!("## Part {n}\n\nPoint {n}. Extra.\n\n"));
        }
        let deck = deck_from_document(&doc);
        assert_eq!(deck.matches("\n## ").count(), 12);
        assert!(deck.contains("## Part 12") && !deck.contains("Part 13"));
        let held = "# T\n\n## One\n\nThe price is [NEEDED: the price per seat].\n\n## Source warnings\n\nFor the team only.\n\n## Two\n\nTooling used: x\n\nDone.\n";
        let deck = deck_from_document(held);
        assert!(
            !deck.contains("NEEDED") && !deck.contains("team only") && !deck.contains("Tooling"),
            "{deck}"
        );
        assert!(
            deck.contains("## One") && deck.contains("## Two") && !deck.contains("Source warnings")
        );
        // No sections: the opening points become one slide. A long plain sentence is cut at a word.
        let flat = format!("# Short\n\n{}\n", "word ".repeat(80));
        let deck = deck_from_document(&flat);
        assert!(deck.contains("## Summary"), "{deck}");
        let point = deck.lines().find(|l| l.starts_with("- ")).unwrap();
        assert!(
            point.chars().count() <= 165 && point.ends_with('…'),
            "{point}"
        );
    }
}
