//! Writing to Notion and GitHub, one approved proposal at a time. Nothing is written until a person has
//! approved the exact content, and the approval is a hash of it, checked again at the moment of sending.
//! Notion gets one new page under a page the person names. GitHub gets new files on a new branch (`bff/...`),
//! never on the default branch, with no pull request and no merge. The content is scanned for secrets
//! first. The keys are their own Keychain items (`notion-write`, `github-write`), separate from the
//! read-only ones, and a repeat send of the same approved proposal is refused.

use crate::connector::Credentials;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const NOTION_CREDENTIAL: &str = "notion-write";
pub const GITHUB_CREDENTIAL: &str = "github-write";
const MAX_FILES: usize = 200;
const MAX_FILE: usize = 1024 * 1024;
const MAX_TOTAL: usize = 10 * 1024 * 1024;
const MAX_BODY: usize = 100_000;

/// Sends one request. The host is fixed by the caller, and the answer is the status and the JSON body.
pub trait WriteTransport {
    fn send(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<(u16, Value), String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    /// A new page under `parent` (a Notion page id) with a title and a body written in light markdown. With
    /// `append`, nothing new is made: the title becomes a heading and the body is added at the end of the
    /// existing page `parent`. Nothing already on that page is changed or removed.
    Notion {
        parent: String,
        title: String,
        body: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        append: bool,
    },
    /// New files on a new branch of `repo` (`owner/name`).
    Github {
        repo: String,
        branch: String,
        files: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Proposed,
    Approved {
        by: String,
        hash: String,
    },
    /// The send began and the outcome is not known. A person must check the destination.
    Sending {
        hash: String,
    },
    Done {
        url: String,
    },
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Write {
    pub id: String,
    pub kind: Kind,
    pub status: Status,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    Transient(String),
    Rejected(String),
}

pub trait Sender {
    /// Writes the content and returns where it is, as an address.
    fn send(&mut self, kind: &Kind) -> Result<String, SendError>;
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    items: Vec<Write>,
}

pub struct Writes {
    path: PathBuf,
    saved: Saved,
}

pub fn content_hash(kind: &Kind) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_string(kind).expect("plain data").as_bytes())
    )
}

/// The id of a Notion page from its address or the bare id, with dashes. `Err` says what to paste.
pub fn notion_page_id(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    let bad = || "paste the address of a Notion page, or its 32-character id".to_string();
    let last = match t.strip_prefix("https://") {
        Some(rest) => {
            let (host, path) = rest.split_once('/').ok_or_else(bad)?;
            if ![
                "notion.so",
                "www.notion.so",
                "app.notion.com",
                "www.notion.com",
            ]
            .contains(&host)
            {
                return Err("that is not a Notion address (notion.so or notion.com)".into());
            }
            path.split(['?', '#'])
                .next()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string()
        }
        None if t.contains("://") || t.contains('/') => return Err(bad()),
        None => t.to_string(),
    };
    let compact: String = last.chars().filter(|c| *c != '-').collect();
    let id: String = compact
        .chars()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let id = id.to_lowercase();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &id[..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..]
    ))
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// A file path inside a repository: relative, plain characters, no `..`, and not in `.git` or `.github`.
pub fn check_repo_path(p: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("{p:?} is not a usable repository path: {why}"));
    if p.is_empty() || p.len() > 200 {
        return bad("it is empty or longer than 200 characters");
    }
    if !p
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
    {
        return bad("use letters, digits, dot, dash, underscore, and slash only");
    }
    for seg in p.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return bad("it must be relative, with no empty or .. parts");
        }
        if seg.eq_ignore_ascii_case(".git") || seg.eq_ignore_ascii_case(".github") {
            return bad("it is inside .git or .github, which BFF does not write");
        }
    }
    Ok(())
}

fn check_branch(b: &str) -> Result<(), String> {
    let rest = b.strip_prefix("bff/").unwrap_or("");
    let ok = !rest.is_empty()
        && b.len() <= 80
        && b.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'/' | b'_' | b'.' | b'-')
        })
        && !b.contains("..")
        && !b.contains("//")
        && !b.ends_with(['/', '.']);
    if ok {
        Ok(())
    } else {
        Err(format!("the branch {b:?} must start with bff/ and use lowercase letters, digits, dot, dash, underscore, and slash"))
    }
}

fn check_repo(r: &str) -> Result<(), String> {
    let part = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    match r.split_once('/') {
        Some((o, n)) if part(o) && part(n) => Ok(()),
        _ => Err(format!(
            "{r:?} is not a repository. Use the form owner/name"
        )),
    }
}

fn no_secret(what: &str, text: &str) -> Result<(), String> {
    let found = crate::secrets::scan(text);
    match found
        .iter()
        .find(|f| f.class == crate::secrets::Class::Secret)
    {
        Some(f) => Err(format!(
            "{what} contains what looks like a secret ({}, line {}). Remove it first",
            f.kind, f.line
        )),
        None => Ok(()),
    }
}

/// Notion blocks for light markdown: headings, bullets, numbers, dividers, and paragraphs.
pub fn notion_blocks(body: &str) -> Vec<Value> {
    let rich = |t: &str| {
        let chars: Vec<char> = t.chars().collect();
        Value::Array(
            chars
                .chunks(2000)
                .map(|c| json!({"type": "text", "text": {"content": c.iter().collect::<String>()}}))
                .collect(),
        )
    };
    let block = |kind: &str, t: &str| json!({"object": "block", "type": kind, kind: {"rich_text": rich(t)}});
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            if let Some(t) = l.strip_prefix("### ") {
                block("heading_3", t)
            } else if let Some(t) = l.strip_prefix("## ") {
                block("heading_2", t)
            } else if let Some(t) = l.strip_prefix("# ") {
                block("heading_1", t)
            } else if let Some(t) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
                block("bulleted_list_item", t)
            } else if l == "---" {
                json!({"object": "block", "type": "divider", "divider": {}})
            } else if let Some((n, t)) = l
                .split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            {
                let _ = n;
                block("numbered_list_item", t)
            } else {
                block("paragraph", l)
            }
        })
        .collect()
}

fn validate(kind: &Kind) -> Result<Kind, String> {
    match kind {
        Kind::Notion {
            parent,
            title,
            body,
            append,
        } => {
            let parent = notion_page_id(parent)?;
            let title = title.trim().to_string();
            if title.is_empty() || title.chars().count() > 200 {
                return Err("the page needs a title of up to 200 characters".into());
            }
            if body.trim().is_empty() || body.len() > MAX_BODY {
                return Err(format!(
                    "the page needs text, and not more than {MAX_BODY} characters"
                ));
            }
            if notion_blocks(body).len() + usize::from(*append) > 100 {
                return Err("the page has more than 100 blocks, which is more than one Notion request takes. Split it".into());
            }
            no_secret("the title", &title)?;
            no_secret("the text", body)?;
            Ok(Kind::Notion {
                parent,
                title,
                body: body.clone(),
                append: *append,
            })
        }
        Kind::Github {
            repo,
            branch,
            files,
        } => {
            check_repo(repo)?;
            check_branch(branch)?;
            if files.is_empty() || files.len() > MAX_FILES {
                return Err(format!("a proposal needs between 1 and {MAX_FILES} files"));
            }
            let mut total = 0;
            for (i, (path, content)) in files.iter().enumerate() {
                check_repo_path(path)?;
                if files[..i].iter().any(|(p, _)| p == path) {
                    return Err(format!("{path} appears twice"));
                }
                if content.len() > MAX_FILE {
                    return Err(format!("{path} is larger than 1 MB"));
                }
                total += content.len();
                no_secret(path, content)?;
            }
            if total > MAX_TOTAL {
                return Err("the files add up to more than 10 MB".into());
            }
            Ok(kind.clone())
        }
    }
}

/// Held while one change to `writes.json` is read, made and saved, so two handles cannot both act on the same
/// approval. It is removed when dropped.
struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn find_in<'a>(saved: &'a mut Saved, id: &str) -> Result<&'a mut Write, String> {
    saved
        .items
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| format!("no write called {id}"))
}

fn read_saved(path: &Path) -> Result<Saved, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Saved::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

impl Writes {
    pub fn open(root: &Path) -> Result<Writes, String> {
        let path = root.join("writes.json");
        let saved = read_saved(&path)?;
        Ok(Writes { path, saved })
    }

    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.saved).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    fn lock(&self) -> Result<Lock, String> {
        let path = self.path.with_extension("lock");
        for _ in 0..150 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Lock(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A lock left by a crash is older than any change takes.
                    let old = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(30));
                    if old {
                        let _ = std::fs::remove_file(&path);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Err("another change to the writes is in progress. Try again".into())
    }

    /// Reads the file again, makes one change to it, and saves, all while holding the lock. Another handle's change
    /// made since this one was opened is never overwritten, and an approval can be acted on once.
    fn edit<R>(
        &mut self,
        change: impl FnOnce(&mut Saved) -> Result<R, String>,
    ) -> Result<R, String> {
        let _held = self.lock()?;
        self.saved = read_saved(&self.path)?;
        let out = change(&mut self.saved)?;
        self.save()?;
        Ok(out)
    }

    pub fn items(&self) -> &[Write] {
        &self.saved.items
    }

    pub fn propose(&mut self, kind: Kind) -> Result<String, String> {
        let kind = validate(&kind)?;
        self.edit(|saved| {
            let next = saved
                .items
                .iter()
                .filter_map(|w| w.id.trim_start_matches('w').parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                + 1;
            let id = format!("w{next}");
            saved.items.push(Write {
                id: id.clone(),
                kind,
                status: Status::Proposed,
            });
            Ok(id)
        })
    }

    pub fn approve(&mut self, id: &str, by: &str) -> Result<(), String> {
        if by.trim().is_empty() {
            return Err("say who approves".into());
        }
        self.edit(|saved| {
            let w = find_in(saved, id)?;
            if w.status != Status::Proposed {
                return Err(format!("{id} is not waiting for approval"));
            }
            w.status = Status::Approved {
                by: by.trim().into(),
                hash: content_hash(&w.kind),
            };
            Ok(())
        })
    }

    pub fn cancel(&mut self, id: &str) -> Result<(), String> {
        self.edit(|saved| {
            let w = find_in(saved, id)?;
            match w.status {
                Status::Done { .. } => {
                    return Err(format!("{id} was already written and cannot be cancelled"))
                }
                Status::Sending { .. } => {
                    return Err(format!(
                        "{id} is being sent. Check the destination, then say whether it arrived"
                    ))
                }
                _ => w.status = Status::Cancelled,
            }
            Ok(())
        })
    }

    /// A failed write goes back to proposed, so it has to be approved again.
    pub fn retry(&mut self, id: &str) -> Result<(), String> {
        self.edit(|saved| {
            let w = find_in(saved, id)?;
            if !matches!(w.status, Status::Failed(_)) {
                return Err(format!("{id} has not failed"));
            }
            w.status = Status::Proposed;
            Ok(())
        })
    }

    /// The person checked the destination after a send was interrupted, and says what they found. A write that did
    /// arrive is recorded as written, with the address they give. One that did not goes back to proposed, so it
    /// needs a fresh approval before it can be sent.
    pub fn resolve_interrupted(&mut self, id: &str, arrived: Option<&str>) -> Result<(), String> {
        self.edit(|saved| {
            let w = find_in(saved, id)?;
            if !matches!(w.status, Status::Sending { .. }) {
                return Err(format!("{id} is not waiting for a check"));
            }
            w.status = match arrived {
                Some(url) if !url.trim().is_empty() => Status::Done {
                    url: url.trim().to_string(),
                },
                Some(_) => return Err("give the address of what was written".into()),
                None => Status::Proposed,
            };
            Ok(())
        })
    }

    pub fn send(&mut self, id: &str, to: &mut dyn Sender) -> Result<String, String> {
        // The approval is spent in one step under the lock, so a second send of the same approval finds it
        // already sending and is refused before it contacts anything.
        let kind = self.edit(|saved| {
            let w = find_in(saved, id)?;
            let hash = match &w.status {
                Status::Approved { hash, .. } => hash.clone(),
                Status::Done { .. } => return Err(format!("{id} was already written")),
                Status::Sending { .. } => return Err(format!("An earlier send of {id} was interrupted and may have gone through. Check the destination, then say whether it arrived")),
                Status::Failed(m) => return Err(format!("{id} failed earlier ({m}). Retry it and approve it again")),
                Status::Cancelled => return Err(format!("{id} was cancelled")),
                Status::Proposed => return Err(format!("{id} is not approved")),
            };
            if hash != content_hash(&w.kind) {
                return Err(format!(
                    "{id} changed since it was approved. Nothing was sent"
                ));
            }
            let kind = w.kind.clone();
            w.status = Status::Sending { hash };
            Ok(kind)
        })?;
        let result = to.send(&kind);
        // A failed send is an answer too, so it is saved before the error is returned.
        self.edit(|saved| {
            let w = find_in(saved, id)?;
            Ok(match result {
                Ok(url) => {
                    w.status = Status::Done { url: url.clone() };
                    Ok(url)
                }
                Err(SendError::Transient(m) | SendError::Rejected(m)) => {
                    w.status = Status::Failed(m.clone());
                    Err(m)
                }
            })
        })?
    }
}

/// Sends to the real services through a transport and the keys in the Keychain.
pub struct Remote<'a> {
    pub transport: &'a mut dyn WriteTransport,
    pub creds: &'a dyn Credentials,
}

fn rejected(m: impl Into<String>) -> SendError {
    SendError::Rejected(m.into())
}

/// The failure for an answer that was not the one expected, in plain words. `who` is Notion or GitHub.
fn answer(who: &str, status: u16, body: &Value, not_found: &str) -> SendError {
    match status {
        401 | 403 => rejected(format!("the {who} key was refused (status {status}). Check the Keychain item and what it is allowed to do")),
        404 => rejected(not_found),
        429 => SendError::Transient(format!("{who} rate limit reached (status 429)")),
        500..=599 => SendError::Transient(format!("{who} is unavailable (status {status})")),
        _ => rejected(match body["message"].as_str() {
            Some(m) => format!("{who} refused the request (status {status}): {m}"),
            None => format!("{who} answered status {status}"),
        }),
    }
}

impl Remote<'_> {
    fn call(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<(u16, Value), SendError> {
        self.transport
            .send(method, url, headers, body)
            .map_err(SendError::Transient)
    }

    fn notion(&mut self, parent: &str, title: &str, body: &str) -> Result<String, SendError> {
        let parent = notion_page_id(parent).map_err(rejected)?;
        let key = self.creds.get(NOTION_CREDENTIAL).ok_or_else(|| {
            rejected(format!(
                "no key: add the {NOTION_CREDENTIAL} item to the Keychain (see docs/KEYS.md)"
            ))
        })?;
        let headers = vec![
            (
                "Authorization".to_string(),
                format!("Bearer {}", key.token()),
            ),
            ("Notion-Version".to_string(), "2022-06-28".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let payload = json!({
            "parent": {"page_id": parent},
            "properties": {"title": {"title": [{"text": {"content": title}}]}},
            "children": notion_blocks(body),
        });
        let (status, answer_body) = self.call(
            "POST",
            "https://api.notion.com/v1/pages",
            &headers,
            Some(&payload),
        )?;
        if status == 200 {
            return Ok(answer_body["url"]
                .as_str()
                .unwrap_or("https://www.notion.so/")
                .to_string());
        }
        Err(answer("Notion", status, &answer_body, "Notion cannot see that page. Share it with the integration: open the page in Notion, choose Connections, and add it"))
    }

    /// Adds the title as a heading and the body at the end of an existing page. Notion's only way to add to a page
    /// is a PATCH to the page's children, which the client allows for that address alone.
    fn notion_append(&mut self, page: &str, title: &str, body: &str) -> Result<String, SendError> {
        let page = notion_page_id(page).map_err(rejected)?;
        let key = self.creds.get(NOTION_CREDENTIAL).ok_or_else(|| {
            rejected(format!(
                "no key: add the {NOTION_CREDENTIAL} item to the Keychain (see docs/KEYS.md)"
            ))
        })?;
        let headers = vec![
            (
                "Authorization".to_string(),
                format!("Bearer {}", key.token()),
            ),
            ("Notion-Version".to_string(), "2022-06-28".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let mut children = vec![
            json!({"object": "block", "type": "heading_2", "heading_2": {"rich_text": [{"type": "text", "text": {"content": title}}]}}),
        ];
        children.extend(notion_blocks(body));
        let (status, answer_body) = self.call(
            "PATCH",
            &format!("https://api.notion.com/v1/blocks/{page}/children"),
            &headers,
            Some(&json!({"children": children})),
        )?;
        if status == 200 {
            return Ok(format!("https://www.notion.so/{}", page.replace('-', "")));
        }
        Err(answer("Notion", status, &answer_body, "Notion cannot see that page. Share it with the integration: open the page in Notion, choose Connections, and add it"))
    }

    fn github(
        &mut self,
        repo: &str,
        branch: &str,
        files: &[(String, String)],
    ) -> Result<String, SendError> {
        let key = self.creds.get(GITHUB_CREDENTIAL).ok_or_else(|| {
            rejected(format!(
                "no key: add the {GITHUB_CREDENTIAL} item to the Keychain (see docs/KEYS.md)"
            ))
        })?;
        let headers = vec![
            (
                "Authorization".to_string(),
                format!("Bearer {}", key.token()),
            ),
            (
                "Accept".to_string(),
                "application/vnd.github+json".to_string(),
            ),
            ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
            ("User-Agent".to_string(), "BFF".to_string()),
        ];
        let api = format!("https://api.github.com/repos/{repo}");
        let missing = format!("The repository {repo} was not found or the key cannot see it");
        let (status, info) = self.call("GET", &api, &headers, None)?;
        if status != 200 {
            return Err(answer("GitHub", status, &info, &missing));
        }
        let default = info["default_branch"]
            .as_str()
            .ok_or_else(|| rejected("GitHub did not name the default branch"))?
            .to_string();
        let (status, reference) = self.call(
            "GET",
            &format!("{api}/git/ref/heads/{default}"),
            &headers,
            None,
        )?;
        if matches!(status, 404 | 409) {
            return Err(rejected(
                "The repository has no commits yet. Add a README on GitHub first, then try again",
            ));
        }
        if status != 200 {
            return Err(answer("GitHub", status, &reference, &missing));
        }
        let sha = reference["object"]["sha"]
            .as_str()
            .ok_or_else(|| rejected("GitHub did not give the commit to branch from"))?;
        let (status, made) = self.call(
            "POST",
            &format!("{api}/git/refs"),
            &headers,
            Some(&json!({"ref": format!("refs/heads/{branch}"), "sha": sha})),
        )?;
        match status {
            201 => {}
            422 => return Err(rejected(format!("The branch {branch} already exists in {repo}. Propose again with another branch name"))),
            _ => return Err(answer("GitHub", status, &made, &missing)),
        }
        for (i, (path, content)) in files.iter().enumerate() {
            let put = json!({"message": format!("Add {path} (written by BFF)"), "content": base64(content.as_bytes()), "branch": branch});
            let (status, got) = self.call(
                "PUT",
                &format!("{api}/contents/{path}"),
                &headers,
                Some(&put),
            )?;
            if !matches!(status, 200 | 201) {
                let why = match answer("GitHub", status, &got, &missing) {
                    SendError::Transient(m) | SendError::Rejected(m) => m,
                };
                return Err(rejected(format!(
                    "Only {i} of {} files were written to {branch}: {why}",
                    files.len()
                )));
            }
        }
        Ok(format!("https://github.com/{repo}/tree/{branch}"))
    }
}

impl Sender for Remote<'_> {
    fn send(&mut self, kind: &Kind) -> Result<String, SendError> {
        match kind {
            Kind::Notion {
                parent,
                title,
                body,
                append: false,
            } => self.notion(parent, title, body),
            Kind::Notion {
                parent,
                title,
                body,
                append: true,
            } => self.notion_append(parent, title, body),
            Kind::Github {
                repo,
                branch,
                files,
            } => self.github(repo, branch, files),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::Credential;
    use std::collections::BTreeMap;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-writes-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn notion(title: &str, body: &str) -> Kind {
        Kind::Notion {
            parent: "0123456789abcdef0123456789abcdef".into(),
            title: title.into(),
            body: body.into(),
            append: false,
        }
    }
    fn github(files: &[(&str, &str)]) -> Kind {
        Kind::Github {
            repo: "nitish/stopwatch".into(),
            branch: "bff/stopwatch-1".into(),
            files: files
                .iter()
                .map(|(p, c)| (p.to_string(), c.to_string()))
                .collect(),
        }
    }

    struct Keys(BTreeMap<String, Credential>);
    impl Credentials for Keys {
        fn get(&self, name: &str) -> Option<Credential> {
            self.0.get(name).cloned()
        }
    }
    fn keys() -> Keys {
        let mut m = BTreeMap::new();
        m.insert(
            NOTION_CREDENTIAL.to_string(),
            Credential::new("NOTION-SECRET-1", &[]),
        );
        m.insert(
            GITHUB_CREDENTIAL.to_string(),
            Credential::new("GITHUB-SECRET-2", &[]),
        );
        Keys(m)
    }

    type Sent = Vec<(String, String, Vec<(String, String)>, Option<Value>)>;
    struct Fake {
        sent: Sent,
        replies: Vec<Result<(u16, Value), String>>,
    }
    impl WriteTransport for Fake {
        fn send(
            &mut self,
            method: &str,
            url: &str,
            headers: &[(String, String)],
            body: Option<&Value>,
        ) -> Result<(u16, Value), String> {
            self.sent
                .push((method.into(), url.into(), headers.to_vec(), body.cloned()));
            if self.replies.is_empty() {
                return Err("no more replies".into());
            }
            self.replies.remove(0)
        }
    }
    fn fake(replies: Vec<(u16, Value)>) -> Fake {
        Fake {
            sent: vec![],
            replies: replies.into_iter().map(Ok).collect(),
        }
    }

    struct Records(Vec<Kind>, Result<String, SendError>);
    impl Sender for Records {
        fn send(&mut self, kind: &Kind) -> Result<String, SendError> {
            self.0.push(kind.clone());
            match &self.1 {
                Ok(u) => Ok(u.clone()),
                Err(SendError::Transient(m)) => Err(SendError::Transient(m.clone())),
                Err(SendError::Rejected(m)) => Err(SendError::Rejected(m.clone())),
            }
        }
    }

    #[test]
    fn a_notion_address_or_a_bare_id_gives_the_page_id_with_dashes() {
        let want = "01234567-89ab-cdef-0123-456789abcdef";
        for ok in [
            "https://www.notion.so/BFF-drafts-0123456789abcdef0123456789abcdef",
            "https://www.notion.so/workspace/BFF-drafts-0123456789abcdef0123456789abcdef?pvs=4",
            "0123456789abcdef0123456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdef",
            "  https://notion.so/0123456789ABCDEF0123456789ABCDEF  ",
            "https://app.notion.com/p/0123456789abcdef0123456789abcdef?pvs=204",
        ] {
            assert_eq!(notion_page_id(ok).unwrap().to_lowercase(), want, "{ok}");
        }
        for bad in [
            "",
            "hello",
            "https://example.com/page",
            "https://www.notion.so/short-123",
            "https://evil.com/notion.so/0123456789abcdef0123456789abcdef",
        ] {
            assert!(notion_page_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        assert_eq!(base64(&[0xfb, 0xff, 0xfe]), "+//+");
    }

    #[test]
    fn repository_paths_must_be_plain_relative_and_outside_git_and_workflows() {
        for ok in [
            "README.md",
            "src/main.py",
            "tests/test_timer.py",
            "a-b_c.d/e.txt",
        ] {
            assert!(check_repo_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            ".git/config",
            "x/.git/hooks/pre-commit",
            ".github/workflows/ci.yml",
            ".GITHUB/workflows/x.yml",
            "a b.txt",
            "a\\b",
            "a/",
            "é.txt",
            &"a".repeat(300),
        ] {
            assert!(check_repo_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_proposal_is_checked_before_it_is_kept() {
        let mut w = Writes::open(&dir("propose")).unwrap();
        for bad in [
            notion("", "body"),
            notion("T", "   "),
            notion(&"t".repeat(300), "body"),
            notion("T", &"x".repeat(MAX_BODY + 1)),
            Kind::Notion {
                parent: "not a page".into(),
                title: "T".into(),
                body: "b".into(),
                append: false,
            },
            notion(
                "T",
                "key = sk-proj-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH",
            ),
            github(&[]),
            github(&[("../x", "a")]),
            github(&[("a.txt", "a"), ("a.txt", "b")]),
            github(&[("a.txt", &"x".repeat(MAX_FILE + 1))]),
            github(&[(
                "config.py",
                "TOKEN = 'ghp_abcdefghijklmnopqrstuvwxyz0123456789'",
            )]),
            Kind::Github {
                repo: "no-slash".into(),
                branch: "bff/x".into(),
                files: vec![("a".into(), "b".into())],
            },
            Kind::Github {
                repo: "a/b/c".into(),
                branch: "bff/x".into(),
                files: vec![("a".into(), "b".into())],
            },
            Kind::Github {
                repo: "o/r".into(),
                branch: "main".into(),
                files: vec![("a".into(), "b".into())],
            },
            Kind::Github {
                repo: "o/r".into(),
                branch: "bff/../main".into(),
                files: vec![("a".into(), "b".into())],
            },
            Kind::Github {
                repo: "o/r".into(),
                branch: "bff/".into(),
                files: vec![("a".into(), "b".into())],
            },
        ] {
            assert!(w.propose(bad.clone()).is_err(), "{bad:?}");
        }
        assert!(w.items().is_empty());
        let id = w.propose(notion("T", "body")).unwrap();
        assert_eq!(w.items()[0].id, id);
        assert_eq!(w.items()[0].status, Status::Proposed);
    }

    #[test]
    fn nothing_is_sent_without_approval_and_an_approved_proposal_is_sent_once() {
        let mut w = Writes::open(&dir("approve")).unwrap();
        let id = w.propose(notion("Drafts", "Post 1")).unwrap();
        let mut to = Records(vec![], Ok("https://notion.so/x".into()));
        assert!(w.send(&id, &mut to).unwrap_err().contains("not approved"));
        assert!(to.0.is_empty());
        assert!(w.approve("nope", "Nitish").is_err());
        w.approve(&id, "Nitish").unwrap();
        assert_eq!(w.send(&id, &mut to).unwrap(), "https://notion.so/x");
        assert_eq!(to.0.len(), 1);
        assert_eq!(
            w.items()[0].status,
            Status::Done {
                url: "https://notion.so/x".into()
            }
        );
        assert!(w.send(&id, &mut to).unwrap_err().contains("already"));
        assert!(
            w.cancel(&id).is_err(),
            "a finished write cannot be cancelled"
        );
        assert_eq!(to.0.len(), 1);
    }

    #[test]
    fn changed_content_after_approval_is_not_sent() {
        let d = dir("tamper");
        let mut w = Writes::open(&d).unwrap();
        let id = w.propose(notion("Drafts", "Post 1")).unwrap();
        w.approve(&id, "Nitish").unwrap();
        // Someone edits the saved file between approval and send.
        let path = d.join("writes.json");
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("Post 1", "Post 1 and a link to something else");
        std::fs::write(&path, text).unwrap();
        let mut w = Writes::open(&d).unwrap();
        let mut to = Records(vec![], Ok("u".into()));
        assert!(w.send(&id, &mut to).unwrap_err().contains("changed"));
        assert!(to.0.is_empty());
    }

    #[test]
    fn a_failed_send_is_recorded_and_a_retry_needs_a_new_approval() {
        let mut w = Writes::open(&dir("fail")).unwrap();
        let id = w.propose(notion("Drafts", "Post 1")).unwrap();
        w.approve(&id, "Nitish").unwrap();
        let mut to = Records(
            vec![],
            Err(SendError::Rejected("Notion cannot see that page".into())),
        );
        assert!(w.send(&id, &mut to).unwrap_err().contains("cannot see"));
        assert!(matches!(&w.items()[0].status, Status::Failed(m) if m.contains("cannot see")));
        let mut again = Records(vec![], Ok("u".into()));
        assert!(
            w.send(&id, &mut again).is_err(),
            "a failed write is not sent again by itself"
        );
        w.retry(&id).unwrap();
        assert_eq!(w.items()[0].status, Status::Proposed);
        assert!(w
            .send(&id, &mut again)
            .unwrap_err()
            .contains("not approved"));
        w.approve(&id, "Nitish").unwrap();
        assert!(w.send(&id, &mut again).is_ok());
    }

    #[test]
    fn a_send_that_was_interrupted_is_not_repeated_by_itself() {
        let d = dir("crash");
        let mut w = Writes::open(&d).unwrap();
        let id = w.propose(notion("Drafts", "Post 1")).unwrap();
        w.approve(&id, "Nitish").unwrap();
        // The program stopped after marking the write as sending.
        let path = d.join("writes.json");
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hash = content_hash(&w.items()[0].kind);
        v["items"][0]["status"] = json!({"Sending": {"hash": hash}});
        std::fs::write(&path, v.to_string()).unwrap();
        let mut w = Writes::open(&d).unwrap();
        assert!(
            matches!(w.items()[0].status, Status::Sending { .. }),
            "{:?}",
            w.items()[0].status
        );
        let mut to = Records(vec![], Ok("u".into()));
        assert!(w.send(&id, &mut to).unwrap_err().contains("Check"));
        assert!(to.0.is_empty());
        assert!(
            w.cancel(&id).is_err(),
            "it cannot be cancelled until someone has checked"
        );
    }

    #[test]
    fn writes_are_kept_across_a_restart() {
        let d = dir("persist");
        let id = {
            let mut w = Writes::open(&d).unwrap();
            let id = w.propose(github(&[("README.md", "# Stopwatch")])).unwrap();
            w.approve(&id, "Nitish").unwrap();
            id
        };
        let w = Writes::open(&d).unwrap();
        assert!(matches!(&w.items()[0].status, Status::Approved { by, .. } if by == "Nitish"));
        assert_eq!(w.items()[0].id, id);
    }

    fn header<'a>(h: &'a [(String, String)], k: &str) -> Option<&'a str> {
        h.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn adding_to_an_existing_notion_page_is_one_patch_of_its_children_with_the_title_as_a_heading()
    {
        let mut t = fake(vec![(200, json!({"object": "list", "results": []}))]);
        let mut r = Remote {
            transport: &mut t,
            creds: &keys(),
        };
        let mut k = notion("Update for November", "First line.\n\n- a point");
        if let Kind::Notion { append, .. } = &mut k {
            *append = true;
        }
        let url = r.send(&k).unwrap();
        assert_eq!(
            url,
            "https://www.notion.so/0123456789abcdef0123456789abcdef"
        );
        let (method, u, headers, sent) = &t.sent[0];
        assert_eq!(
            (method.as_str(), u.as_str()),
            (
                "PATCH",
                "https://api.notion.com/v1/blocks/01234567-89ab-cdef-0123-456789abcdef/children"
            )
        );
        assert!(crate::http::patch_allowed(u));
        assert_eq!(
            header(headers, "Authorization"),
            Some("Bearer NOTION-SECRET-1")
        );
        let kids = sent.as_ref().unwrap()["children"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(kids[0]["type"], "heading_2");
        assert_eq!(
            kids[0]["heading_2"]["rich_text"][0]["text"]["content"],
            "Update for November"
        );
        assert_eq!(kids.len(), 3);
        // A record from before this existed reads as a new page, and a new-page record is written as before.
        let old: Kind = serde_json::from_value(
            json!({"kind": "notion", "parent": "p", "title": "t", "body": "b"}),
        )
        .unwrap();
        assert!(matches!(old, Kind::Notion { append: false, .. }));
        assert!(!serde_json::to_string(&notion("t", "b"))
            .unwrap()
            .contains("append"));
    }

    #[test]
    fn a_notion_page_is_one_request_to_notion_with_the_key_only_in_the_header() {
        let long = "word ".repeat(700);
        let body = format!("# Post 1\n\nFirst line of copy.\n\n## Hashtags\n- #claims\n- #insurance\n\n1. Step one\n\n{long}");
        let mut t = fake(vec![(
            200,
            json!({"url": "https://www.notion.so/Drafts-abc"}),
        )]);
        let mut r = Remote {
            transport: &mut t,
            creds: &keys(),
        };
        let url = r.send(&notion("Claims dashboard drafts", &body)).unwrap();
        assert_eq!(url, "https://www.notion.so/Drafts-abc");
        let (method, u, headers, sent) = &t.sent[0];
        assert_eq!(
            (method.as_str(), u.as_str()),
            ("POST", "https://api.notion.com/v1/pages")
        );
        assert_eq!(
            header(headers, "Authorization"),
            Some("Bearer NOTION-SECRET-1")
        );
        assert!(header(headers, "Notion-Version").is_some());
        let sent = sent.as_ref().unwrap();
        assert!(!sent.to_string().contains("SECRET") && !u.contains("SECRET"));
        assert_eq!(
            sent["parent"]["page_id"],
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(
            sent["properties"]["title"]["title"][0]["text"]["content"],
            "Claims dashboard drafts"
        );
        let kids = sent["children"].as_array().unwrap();
        let kinds: Vec<&str> = kids.iter().map(|k| k["type"].as_str().unwrap()).collect();
        assert_eq!(
            &kinds[..6],
            [
                "heading_1",
                "paragraph",
                "heading_2",
                "bulleted_list_item",
                "bulleted_list_item",
                "numbered_list_item"
            ]
        );
        assert_eq!(
            kids[3]["bulleted_list_item"]["rich_text"][0]["text"]["content"],
            "#claims"
        );
        // A paragraph longer than Notion allows in one text object is split.
        let para = &kids[6]["paragraph"]["rich_text"];
        assert!(para.as_array().unwrap().len() >= 2);
        assert!(para.as_array().unwrap().iter().all(|t| t["text"]["content"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 2000));
    }

    #[test]
    fn notion_answers_become_plain_words() {
        for (status, body, want, transient) in [
            (401, json!({}), "key was refused", false),
            (404, json!({}), "Share it with the integration", false),
            (
                400,
                json!({"message": "body failed validation"}),
                "body failed validation",
                false,
            ),
            (429, json!({}), "rate limit", true),
            (503, json!({}), "unavailable", true),
        ] {
            let mut t = fake(vec![(status, body)]);
            let e = Remote {
                transport: &mut t,
                creds: &keys(),
            }
            .send(&notion("T", "b"))
            .unwrap_err();
            let (m, tr) = match e {
                SendError::Rejected(m) => (m, false),
                SendError::Transient(m) => (m, true),
            };
            assert!(m.contains(want), "{status}: {m}");
            assert_eq!(tr, transient, "{status}");
        }
        let none = Keys(BTreeMap::new());
        let mut t = fake(vec![]);
        let e = Remote {
            transport: &mut t,
            creds: &none,
        }
        .send(&notion("T", "b"))
        .unwrap_err();
        assert!(matches!(e, SendError::Rejected(m) if m.contains("notion-write")));
        assert!(t.sent.is_empty());
    }

    #[test]
    fn github_files_go_to_a_new_branch_and_never_to_the_default_branch() {
        let mut t = fake(vec![
            (200, json!({"default_branch": "main"})),
            (200, json!({"object": {"sha": "abc123"}})),
            (201, json!({})),
            (201, json!({})),
            (201, json!({})),
        ]);
        let url = Remote {
            transport: &mut t,
            creds: &keys(),
        }
        .send(&github(&[
            ("README.md", "# Stopwatch\n"),
            ("src/main.py", "print('hi')\n"),
        ]))
        .unwrap();
        assert_eq!(
            url,
            "https://github.com/nitish/stopwatch/tree/bff/stopwatch-1"
        );
        let calls: Vec<(&str, &str)> = t
            .sent
            .iter()
            .map(|s| (s.0.as_str(), s.1.as_str()))
            .collect();
        assert_eq!(
            calls,
            vec![
                ("GET", "https://api.github.com/repos/nitish/stopwatch"),
                (
                    "GET",
                    "https://api.github.com/repos/nitish/stopwatch/git/ref/heads/main"
                ),
                (
                    "POST",
                    "https://api.github.com/repos/nitish/stopwatch/git/refs"
                ),
                (
                    "PUT",
                    "https://api.github.com/repos/nitish/stopwatch/contents/README.md"
                ),
                (
                    "PUT",
                    "https://api.github.com/repos/nitish/stopwatch/contents/src/main.py"
                ),
            ]
        );
        assert_eq!(
            t.sent[2].3.as_ref().unwrap(),
            &json!({"ref": "refs/heads/bff/stopwatch-1", "sha": "abc123"})
        );
        let put = t.sent[3].3.as_ref().unwrap();
        assert_eq!(put["branch"], "bff/stopwatch-1");
        assert_eq!(put["content"], base64(b"# Stopwatch\n"));
        for s in &t.sent {
            assert_eq!(
                header(&s.2, "Authorization"),
                Some("Bearer GITHUB-SECRET-2")
            );
            assert!(!s
                .3
                .as_ref()
                .map(|b| b.to_string())
                .unwrap_or_default()
                .contains("SECRET"));
        }
        assert!(t
            .sent
            .iter()
            .filter(|s| s.0 == "PUT")
            .all(|s| s.3.as_ref().unwrap()["branch"] != "main"));
    }

    #[test]
    fn github_problems_are_explained_and_a_partial_write_says_how_far_it_got() {
        let kind = github(&[("a.txt", "a"), ("b.txt", "b")]);
        let run = |replies: Vec<(u16, Value)>| {
            let mut t = fake(replies);
            let e = Remote {
                transport: &mut t,
                creds: &keys(),
            }
            .send(&kind)
            .unwrap_err();
            (e, t.sent.len())
        };
        let (e, n) = run(vec![(404, json!({}))]);
        assert!(
            matches!(&e, SendError::Rejected(m) if m.contains("not found or the key cannot see it")),
            "{e:?}"
        );
        assert_eq!(n, 1);
        let (e, _) = run(vec![
            (200, json!({"default_branch": "main"})),
            (409, json!({"message": "Git Repository is empty."})),
        ]);
        assert!(
            matches!(&e, SendError::Rejected(m) if m.contains("no commits")),
            "{e:?}"
        );
        let (e, _) = run(vec![
            (200, json!({"default_branch": "main"})),
            (200, json!({"object": {"sha": "s"}})),
            (422, json!({"message": "Reference already exists"})),
        ]);
        assert!(
            matches!(&e, SendError::Rejected(m) if m.contains("already exists")),
            "{e:?}"
        );
        let (e, n) = run(vec![(401, json!({}))]);
        assert!(
            matches!(&e, SendError::Rejected(m) if m.contains("key was refused")),
            "{e:?}"
        );
        assert_eq!(n, 1);
        let (e, n) = run(vec![
            (200, json!({"default_branch": "main"})),
            (200, json!({"object": {"sha": "s"}})),
            (201, json!({})),
            (201, json!({})),
            (422, json!({"message": "nope"})),
        ]);
        assert!(
            matches!(&e, SendError::Rejected(m) if m.contains("1 of 2 files") && m.contains("bff/stopwatch-1")),
            "{e:?}"
        );
        assert_eq!(n, 5);
    }

    #[test]
    fn one_approval_is_sent_once_even_from_two_handles_at_the_same_time() {
        let d = dir("two-handles");
        let mut w = Writes::open(&d).unwrap();
        let id = w.propose(notion("Drafts", "Post 1")).unwrap();
        w.approve(&id, "Nitish").unwrap();
        // Both handles were opened while the write was approved.
        let (mut one, mut two) = (Writes::open(&d).unwrap(), Writes::open(&d).unwrap());
        struct Slow(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Sender for Slow {
            fn send(&mut self, _: &Kind) -> Result<String, SendError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(150));
                Ok("https://notion.so/once".into())
            }
        }
        let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = sent.clone();
        let id2 = id.clone();
        let first = std::thread::spawn(move || one.send(&id2, &mut Slow(counter)));
        std::thread::sleep(std::time::Duration::from_millis(40));
        let second = two.send(&id, &mut Slow(sent.clone()));
        let first = first.join().unwrap();
        assert!(first.is_ok(), "{first:?}");
        assert!(second.is_err(), "the same approval cannot be spent twice");
        assert_eq!(
            sent.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one request went out"
        );
    }

    #[test]
    fn an_interrupted_send_is_resolved_by_saying_whether_it_arrived() {
        let d = dir("interrupted");
        let mut w = Writes::open(&d).unwrap();
        let id = w.propose(notion("Drafts", "Post 1")).unwrap();
        w.approve(&id, "Nitish").unwrap();
        // The app stopped after the write was marked as sending.
        let path = d.join("writes.json");
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hash = doc["items"][0]["status"]["Approved"]["hash"].clone();
        doc["items"][0]["status"] = serde_json::json!({"Sending": {"hash": hash}});
        std::fs::write(&path, doc.to_string()).unwrap();

        let mut w = Writes::open(&d).unwrap();
        assert!(w.cancel(&id).is_err() && w.retry(&id).is_err());
        assert!(
            w.resolve_interrupted(&id, Some("  ")).is_err(),
            "an address is needed"
        );
        // It did not arrive: back to proposed, and it needs a fresh approval before it can be sent.
        w.resolve_interrupted(&id, None).unwrap();
        let mut to = Records(vec![], Ok("u".into()));
        assert!(w.send(&id, &mut to).is_err());
        w.approve(&id, "Nitish").unwrap();
        assert!(w.send(&id, &mut to).is_ok());
        assert!(
            w.resolve_interrupted(&id, None).is_err(),
            "only a write waiting for a check"
        );

        // It did arrive: recorded as written, with the address the person gives.
        let id2 = w.propose(notion("Drafts", "Post 2")).unwrap();
        w.approve(&id2, "Nitish").unwrap();
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hash = doc["items"][1]["status"]["Approved"]["hash"].clone();
        doc["items"][1]["status"] = serde_json::json!({"Sending": {"hash": hash}});
        std::fs::write(&path, doc.to_string()).unwrap();
        w.resolve_interrupted(&id2, Some("https://notion.so/found"))
            .unwrap();
        assert!(matches!(&w.items()[1].status, Status::Done { url } if url.ends_with("/found")));
    }
}
