//! App-owned git worktrees and diff-first code review. No generated command is executed.
//! Worktree separation is not process/network isolation; dependency execution fails closed.
use crate::{
    engine::sha256_hex,
    flow::{Dispatch, Studio},
    policy::{Action, Permission, Tool},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

pub const BUNDLE_RULES: &str = "CODE_DRAFT_JSON_V1: Return only a JSON object with summary (string) and files (array of objects with path and content strings). Supply complete contents of each changed file, at most 50 files and 1 MB each. Paths are relative, no hidden metadata except .gitignore, no artifact.md, no deletion. Do not run commands or install dependencies. Tests are written, never executed. All changes land as a diff for human review; do not claim they were applied or tested.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub path: String,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub summary: String,
    pub files: Vec<File>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub task: String,
    pub stage: String,
    pub role: String,
    pub scope: String,
    pub base: String,
    pub patch_hash: String,
    pub patch: String,
    pub files: Vec<File>,
    pub status: String,
    pub summary: String,
}
fn meta(root: &Path) -> PathBuf {
    root.join("code-review")
}
fn repo(root: &Path) -> PathBuf {
    root.join("code")
}
fn path(root: &Path, task: &str) -> Result<PathBuf, String> {
    if !crate::workspace::valid_name(task) {
        return Err("Invalid code task id".into());
    }
    Ok(meta(root).join("drafts").join(format!("{task}.json")))
}
fn save(root: &Path, draft: &Draft) -> Result<(), String> {
    let p = path(root, &draft.task)?;
    std::fs::create_dir_all(p.parent().ok_or("Missing draft folder")?)
        .map_err(|e| e.to_string())?;
    crate::workspace::write_atomic(
        &p,
        &serde_json::to_vec_pretty(draft).map_err(|e| e.to_string())?,
    )
}
pub fn list(root: &Path) -> Result<Vec<Draft>, String> {
    let dir = meta(root).join("drafts");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            return Err("Code draft metadata must be a regular file".into());
        }
        let draft: Draft =
            serde_json::from_slice(&std::fs::read(entry.path()).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        out.push(draft);
    }
    out.sort_by(|a, b| a.task.cmp(&b.task));
    Ok(out)
}
fn load(root: &Path, task: &str) -> Result<Draft, String> {
    serde_json::from_slice(&std::fs::read(path(root, task)?).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
/// Fixed argv, no shell, no ambient Git configuration, hooks, pagers, textconv or external diff.
fn git(at: &Path, args: &[&str]) -> Result<String, String> {
    let result = Command::new("/usr/bin/git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "user.name=Sprout Crew",
            "-c",
            "user.email=local@sproutcrew.app",
        ])
        .args(args)
        .current_dir(at)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("Git unavailable: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "Git operation failed: {}",
            String::from_utf8_lossy(&result.stderr)
                .chars()
                .take(1000)
                .collect::<String>()
        ));
    }
    if result.stdout.len() > 25 * 1024 * 1024 {
        return Err("Code diff exceeds 25 MB".into());
    }
    String::from_utf8(result.stdout).map_err(|_| "Code diff is not UTF-8".into())
}
fn trusted(root: &Path) -> Result<(), String> {
    let expected = std::fs::read_to_string(meta(root).join("managed-config.sha256"))
        .map_err(|_| "Not an app-managed code repository")?;
    let config = std::fs::read(repo(root).join(".git/config")).map_err(|e| e.to_string())?;
    if expected != sha256_hex(&config) {
        return Err("Managed Git configuration changed; no command was run".into());
    }
    Ok(())
}
pub fn head(root: &Path) -> Result<String, String> {
    trusted(root)?;
    Ok(git(&repo(root), &["rev-parse", "HEAD"])?.trim().into())
}
fn clean(root: &Path) -> Result<(), String> {
    trusted(root)?;
    if !git(
        &repo(root),
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?
    .is_empty()
    {
        return Err(
            "Project code has unreviewed local changes; reconcile them before continuing".into(),
        );
    }
    Ok(())
}
fn initialize(root: &Path) -> Result<(), String> {
    let repository = repo(root);
    if meta(root).join("managed-config.sha256").exists() {
        return clean(root);
    }
    if repository.join(".git").exists() {
        return Err(
            "Existing Git metadata cannot be adopted; the user's checkout stays untouched".into(),
        );
    }
    if std::fs::symlink_metadata(&repository).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Code repository cannot be a symbolic link".into());
    }
    std::fs::create_dir_all(&repository).map_err(|e| e.to_string())?;
    let template = meta(root).join("empty-template");
    std::fs::create_dir_all(&template).map_err(|e| e.to_string())?;
    git(
        &repository,
        &[
            "init",
            "--quiet",
            "--template",
            template.to_str().ok_or("Non-UTF8 template")?,
        ],
    )?;
    let files = crate::code::files(root)?;
    for (p, _) in files {
        git(&repository, &["add", "--force", "--", &p])?;
    }
    git(
        &repository,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-gpg-sign",
            "-m",
            "Initial approved project code",
        ],
    )?;
    crate::workspace::write_atomic(
        &meta(root).join("managed-config.sha256"),
        sha256_hex(&std::fs::read(repository.join(".git/config")).map_err(|e| e.to_string())?)
            .as_bytes(),
    )?;
    clean(root)
}

pub fn prepare(studio: &Studio, d: &Dispatch, role: &str, now: u64) -> Result<(), String> {
    let owner = studio
        .plan()
        .and_then(|p| p.stages.iter().find(|s| s.id == d.stage))
        .ok_or("Unknown code step")?;
    if owner.owner_role != role {
        return Err("Code worktree role does not own this step".into());
    }
    let probe = Action {
        stage: d.stage.clone(),
        tool: Tool::Write,
        target: "draft-files".into(),
        payload_hash: String::new(),
    };
    if crate::policy::evaluate(&studio.policy()?, role, &probe) == Permission::Deny {
        return Err("Code writes are denied for this role".into());
    }
    initialize(&studio.project.root)?;
    let base = head(&studio.project.root)?;
    if std::fs::read_dir(&d.dir)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("Code worktree needs a fresh empty task folder".into());
    }
    git(
        &repo(&studio.project.root),
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            d.dir.to_str().ok_or("Non-UTF8 task path")?,
            &base,
        ],
    )?;
    let draft = Draft {
        task: d.task.clone(),
        stage: d.stage.clone(),
        role: role.into(),
        scope: studio.approval_hash()?,
        base,
        patch_hash: String::new(),
        patch: String::new(),
        files: vec![],
        status: "drafting".into(),
        summary: String::new(),
    };
    save(&studio.project.root, &draft)?;
    studio.journal.append(
        "code.worktree",
        &json!({"task":draft.task,"stage":draft.stage,"base":draft.base,"role":role}),
        now,
    )?;
    Ok(())
}
fn validate(bundle: &Bundle) -> Result<(), String> {
    if bundle.summary.trim().is_empty() || bundle.summary.len() > 16000 || bundle.files.len() > 50 {
        return Err("Invalid code bundle summary or file count".into());
    }
    let mut seen = BTreeSet::new();
    let mut size = 0;
    for f in &bundle.files {
        crate::writes::check_repo_path(&f.path)?;
        if f.path == "artifact.md"
            || f.path
                .split('/')
                .any(|p| p.starts_with('.') && p != ".gitignore")
            || !seen.insert(&f.path)
            || f.content.len() > 1024 * 1024
        {
            return Err("Code bundle has a reserved, duplicate, hidden or oversized file".into());
        }
        size += f.content.len();
        if crate::secrets::has_secret(&crate::secrets::scan(&f.content)) {
            return Err("Code draft blocked: possible secret".into());
        }
    }
    if size > 10 * 1024 * 1024 {
        return Err("Code bundle exceeds 10 MB".into());
    }
    Ok(())
}
fn write_action(draft: &Draft) -> Action {
    Action {
        stage: draft.stage.clone(),
        tool: Tool::Write,
        target: "draft-files".into(),
        payload_hash: sha256_hex(
            serde_json::to_string(&draft.files)
                .unwrap_or_default()
                .as_bytes(),
        ),
    }
}
/// Parse model output as bounded data; no commands, destinations or permission changes can be encoded.
pub fn finish(studio: &Studio, task: &str, strict_json: bool, now: u64) -> Result<bool, String> {
    let root = &studio.project.root;
    let mut draft = load(root, task)?;
    if draft.scope != studio.approval_hash()? || studio.gate() != crate::plan::Gate::Allowed {
        return Err("Code draft approval is stale".into());
    }
    let dir = root.join("tasks").join(task);
    let output = std::fs::read_to_string(dir.join("artifact.md")).map_err(|e| e.to_string())?;
    let bundle = match serde_json::from_str::<Bundle>(&output) {
        Ok(b) => b,
        Err(_) if !strict_json => {
            let files = crate::code::candidate_files(&dir)?
                .into_iter()
                .map(|(path, _)| {
                    let content = std::fs::read_to_string(dir.join(&path))
                        .map_err(|_| "Code fixture file must be UTF-8".to_string())?;
                    Ok(File { path, content })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Bundle {
                summary: output.chars().take(16000).collect(),
                files,
            }
        }
        Err(_) => {
            return Err(
                "Code writer did not return the required JSON file bundle; no code was applied"
                    .into(),
            )
        }
    };
    validate(&bundle)?;
    draft.files = bundle.files;
    draft.summary = bundle.summary;
    draft.status = "needs_permission".into();
    save(root, &draft)?;
    stage(studio, task, now)
}
pub fn stage(studio: &Studio, task: &str, now: u64) -> Result<bool, String> {
    let root = &studio.project.root;
    let mut draft = load(root, task)?;
    if draft.status != "needs_permission" || draft.scope != studio.approval_hash()? {
        return Err("Code draft is not waiting for file permission in this scope".into());
    }
    match studio.authorize_action(&write_action(&draft), now) {
        Ok(()) => {}
        Err(e) => {
            let p = crate::policy::evaluate(&studio.policy()?, &draft.role, &write_action(&draft));
            if p == Permission::Ask {
                return Ok(false);
            }
            return Err(e);
        }
    }
    validate(&Bundle {
        summary: draft.summary.clone(),
        files: draft.files.clone(),
    })?;
    let dir = root.join("tasks").join(task);
    for f in &draft.files {
        crate::policy::confined(&dir, &f.path)?;
    }
    for f in &draft.files {
        let dest = dir.join(&f.path);
        std::fs::create_dir_all(dest.parent().ok_or("Missing file parent")?)
            .map_err(|e| e.to_string())?;
        crate::workspace::write_atomic(&dest, f.content.as_bytes())?;
        git(&dir, &["add", "--force", "--", &f.path])?;
    }
    draft.patch = git(
        &dir,
        &[
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--binary",
            &draft.base,
            "--",
        ],
    )?;
    draft.patch_hash = sha256_hex(draft.patch.as_bytes());
    draft.status = if draft.patch.is_empty() {
        "no_changes"
    } else {
        "pending"
    }
    .into();
    save(root, &draft)?;
    studio.journal.append("code.proposed",&json!({"task":task,"stage":draft.stage,"base":draft.base,"patch_hash":draft.patch_hash,"status":draft.status,"role":draft.role}),now)?;
    Ok(true)
}
pub fn summary(root: &Path) -> Result<Value, String> {
    Ok(json!(list(root)?
        .into_iter()
        .map(|mut d| {
            d.files.clear();
            d
        })
        .collect::<Vec<_>>()))
}
/// A human approves this exact diff. Every base is checked again before an index or checkout changes.
pub fn decide(
    studio: &Studio,
    task: &str,
    patch_hash: &str,
    approve: bool,
    by: &str,
    now: u64,
) -> Result<(), String> {
    if by.trim().is_empty() {
        return Err("Name the code reviewer".into());
    }
    let root = &studio.project.root;
    let mut draft = load(root, task)?;
    if !approve {
        if !matches!(
            draft.status.as_str(),
            "pending" | "stale" | "needs_permission" | "drafting"
        ) || draft.patch_hash != patch_hash
        {
            return Err("This code proposal cannot be rejected in its current state".into());
        }
        draft.status = "rejected".into();
        save(root, &draft)?;
        studio.journal.append(
            "code.rejected",
            &json!({"task":task,"by":by,"patch_hash":patch_hash}),
            now,
        )?;
        return Ok(());
    }
    if draft.status != "pending"
        || draft.patch_hash != patch_hash
        || sha256_hex(draft.patch.as_bytes()) != patch_hash
    {
        return Err("Code proposal changed or is not pending".into());
    }
    if draft.scope != studio.approval_hash()? || studio.gate() != crate::plan::Gate::Allowed {
        return Err("Approve the current plan before reviewing code".into());
    }
    clean(root)?;
    if head(root)? != draft.base {
        draft.status = "stale".into();
        save(root, &draft)?;
        return Err(
            "Stale base: reconcile against the current code; nothing was overwritten".into(),
        );
    }
    // File ownership is journal-derived. A different role must ask the owning role to reconcile it.
    for e in studio
        .journal
        .events()?
        .iter()
        .filter(|e| e.kind == "code.applied")
    {
        if e.payload["role"] != draft.role {
            for f in &draft.files {
                if e.payload["paths"]
                    .as_array()
                    .is_some_and(|ps| ps.iter().any(|p| p == &f.path))
                {
                    return Err(format!(
                        "{} belongs to another writer; reconcile through its owner",
                        f.path
                    ));
                }
            }
        }
    }
    let action = Action {
        stage: draft.stage.clone(),
        tool: Tool::Write,
        target: "../code".into(),
        payload_hash: sha256_hex(json!([draft.base, draft.patch_hash]).to_string().as_bytes()),
    };
    let permission = crate::policy::evaluate(&studio.policy()?, &draft.role, &action);
    if let Err(e) = studio.authorize_action(&action, now) {
        if permission != Permission::Ask {
            return Err(e);
        }
        studio.decide_actions(&[action.id(&studio.approval_hash()?)], true, by, now)?;
        studio.authorize_action(&action, now)?;
    }
    let patch = meta(root).join(format!("{task}.patch"));
    crate::workspace::write_atomic(&patch, draft.patch.as_bytes())?;
    git(
        &repo(root),
        &[
            "apply",
            "--check",
            "--index",
            "--",
            patch.to_str().ok_or("Non-UTF8 patch path")?,
        ],
    )?;
    draft.status = "applying".into();
    save(root, &draft)?;
    studio.journal.append(
        "code.apply_started",
        &json!({"task":task,"by":by,"base":draft.base,"patch_hash":patch_hash,"role":draft.role}),
        now,
    )?;
    git(
        &repo(root),
        &[
            "apply",
            "--index",
            "--",
            patch.to_str().ok_or("Non-UTF8 patch path")?,
        ],
    )?;
    git(
        &repo(root),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "Apply explicitly reviewed code draft",
        ],
    )?;
    let sha = head(root)?;
    draft.status = "applied".into();
    save(root, &draft)?;
    studio.journal.append("code.applied",&json!({"task":task,"stage":draft.stage,"role":draft.role,"by":by,"base":draft.base,"head":sha,"patch_hash":patch_hash,"paths":draft.files.iter().map(|f|&f.path).collect::<Vec<_>>()}),now)?;
    Ok(())
}
/// Reconciliation is explicit replacement data against the CURRENT base, never an overwrite/retry.
pub fn reconcile(
    studio: &Studio,
    task: &str,
    current_base: &str,
    bundle: Bundle,
    now: u64,
) -> Result<String, String> {
    validate(&bundle)?;
    let root = &studio.project.root;
    let mut old = load(root, task)?;
    if old.status != "stale" || head(root)? != current_base {
        return Err("Reconciliation needs the current exact base and a stale proposal".into());
    }
    let mut n = 1;
    let next = loop {
        let name = format!("{task}-r{n}");
        if !root.join("tasks").join(&name).exists() {
            break name;
        }
        n += 1;
    };
    let dir = root.join("tasks").join(&next);
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let d = Dispatch {
        task: next.clone(),
        stage: old.stage.clone(),
        dir: dir.clone(),
        checkpoint: 0,
    };
    prepare(studio, &d, &old.role, now)?;
    std::fs::write(
        dir.join("artifact.md"),
        serde_json::to_vec(&bundle).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    finish(studio, &next, true, now)?;
    old.status = "superseded".into();
    save(root, &old)?;
    studio.journal.append(
        "code.reconciled",
        &json!({"from":task,"task":next,"base":current_base}),
        now,
    )?;
    Ok(next)
}
pub fn dependency_install(studio: &Studio, task: &str, now: u64) -> Result<(), String> {
    let d = load(&studio.project.root, task)?;
    let action = Action {
        stage: d.stage,
        tool: Tool::DependencyInstall,
        target: format!("tasks/{task}: dependency installation"),
        payload_hash: sha256_hex(b"dependency-execution-disabled-without-isolation"),
    };
    studio.authorize_action(&action, now)?;
    Err("Dependency execution is blocked: no tested process/filesystem/network isolation backend; no command was run".into())
}
