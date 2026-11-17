//! Redaction by allowlist, for anything durable built from outside data. The JSON and stderr redactors are
//! tested here; the app writes no raw provider events or stderr to disk today, and the local log
//! (`applog.rs`) and presentation mode use `scrub_str` / `scrub_value`.
//!
//! A field survives only if its path matches an allowlist pattern. Everything else is
//! dropped, keys included. Arrays add no path segment. A pattern segment `*` matches any
//! key, and a trailing `.**` allows a whole subtree. Strings that survive are scrubbed a
//! second time for home paths, email addresses, and token-shaped runs.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Keys that never survive, even under a `.**` pattern.
const NEVER: &[&str] = &[
    "email",
    "authorization",
    "cookie",
    "password",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apiKey",
    "headers",
    "workspaceName",
    "env",
];

#[derive(Debug, PartialEq)]
pub struct Redacted {
    pub value: Value,
    pub dropped: usize,
}

pub fn redact_json(v: &Value, allow: &[&str], home: &str) -> Redacted {
    let mut dropped = 0;
    let value = walk(v, &mut Vec::new(), allow, home, &mut dropped)
        .unwrap_or_else(|| Value::Object(Default::default()));
    Redacted { value, dropped }
}

fn allowed(path: &[String], allow: &[&str]) -> bool {
    allow.iter().any(|pat| {
        let segs: Vec<&str> = pat.split('.').collect();
        let (segs, subtree) = match segs.split_last() {
            Some((&"**", rest)) => (rest.to_vec(), true),
            _ => (segs, false),
        };
        let len_ok = if subtree {
            path.len() > segs.len()
        } else {
            path.len() == segs.len()
        };
        len_ok && segs.iter().zip(path).all(|(s, p)| *s == "*" || s == p)
    })
}

fn walk(
    v: &Value,
    path: &mut Vec<String>,
    allow: &[&str],
    home: &str,
    dropped: &mut usize,
) -> Option<Value> {
    match v {
        Value::Object(m) if !m.is_empty() => {
            let mut out = serde_json::Map::new();
            for (k, child) in m {
                if NEVER.contains(&k.as_str()) {
                    *dropped += 1;
                    continue;
                }
                path.push(k.clone());
                let kept = walk(child, path, allow, home, dropped);
                path.pop();
                if let Some(c) = kept {
                    out.insert(k.clone(), c);
                }
            }
            (!out.is_empty()).then_some(Value::Object(out))
        }
        Value::Array(a) if !a.is_empty() => {
            let out: Vec<Value> = a
                .iter()
                .filter_map(|c| walk(c, path, allow, home, dropped))
                .collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        // Scalars and empty containers are leaves.
        leaf if allowed(path, allow) => Some(match leaf {
            Value::String(s) => Value::String(scrub_str(s, home)),
            other => other.clone(),
        }),
        _ => {
            *dropped += 1;
            None
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct StderrRecord {
    pub kept: Vec<String>,
    pub dropped_lines: usize,
    pub bytes: usize,
    pub sha256: String,
}

/// Keeps only lines that start with a known safe prefix. Records the size and hash of the rest.
pub fn redact_stderr(text: &str, safe_prefixes: &[&str], home: &str) -> StderrRecord {
    let mut kept = Vec::new();
    let mut dropped_lines = 0;
    for line in text.lines() {
        if safe_prefixes.iter().any(|p| line.starts_with(p)) {
            kept.push(scrub_str(line, home));
        } else {
            dropped_lines += 1;
        }
    }
    let digest = Sha256::digest(text.as_bytes());
    StderrRecord {
        kept,
        dropped_lines,
        bytes: text.len(),
        sha256: format!("{digest:x}"),
    }
}

/// Backstop for values the harness builds itself: scrubs every string and drops `NEVER` keys.
pub fn scrub_value(v: &Value, home: &str) -> Value {
    match v {
        Value::String(s) => Value::String(scrub_str(s, home)),
        Value::Array(a) => Value::Array(a.iter().map(|x| scrub_value(x, home)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !NEVER.contains(&k.as_str()))
                .map(|(k, x)| (k.clone(), scrub_value(x, home)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn scrub_str(s: &str, home: &str) -> String {
    let s = if home.is_empty() {
        s.to_string()
    } else {
        s.replace(home, "~")
    };
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        out.push_str(&scrub_word(word));
        word.clear();
    };
    for c in s.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn scrub_word(w: &str) -> String {
    let core = w.trim_matches(|c: char| ",;:()<>\"'".contains(c));
    let replacement = if core.split_once('@').is_some_and(|(_, d)| d.contains('.')) {
        "[email]"
    } else if looks_like_token(core) {
        "[token]"
    } else {
        return w.to_string();
    };
    w.replacen(core, replacement, 1)
}

fn looks_like_token(w: &str) -> bool {
    let is_uuid = w.len() == 36
        && w.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    let is_digest = [40, 64].contains(&w.len()) && w.bytes().all(|b| b.is_ascii_hexdigit());
    w.len() >= 32
        && w.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !is_uuid
        && !is_digest
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const HOME: &str = "/Users/pat";

    #[test]
    fn drops_fields_that_are_not_allowlisted() {
        let v = json!({"type": "system", "apiKeySource": "none", "email": "a@b.co", "mcp": {"url": "https://x"}});
        let r = redact_json(&v, &["type", "apiKeySource"], HOME);
        assert_eq!(r.value, json!({"type": "system", "apiKeySource": "none"}));
        assert_eq!(r.dropped, 2);
    }

    #[test]
    fn wildcard_matches_dynamic_keys() {
        let v = json!({"modelUsage": {"claude-haiku-5-5": {"inputTokens": 5, "costUSD": 0.1, "secretish": 1}}});
        let r = redact_json(
            &v,
            &["modelUsage.*.inputTokens", "modelUsage.*.costUSD"],
            HOME,
        );
        assert_eq!(
            r.value,
            json!({"modelUsage": {"claude-haiku-5-5": {"inputTokens": 5, "costUSD": 0.1}}})
        );
    }

    #[test]
    fn arrays_add_no_path_segment() {
        let v = json!({"tools": ["Read", "Glob"], "mcp_servers": [{"name": "fake", "status": "connected", "command": "x"}]});
        let r = redact_json(
            &v,
            &["tools", "mcp_servers.name", "mcp_servers.status"],
            HOME,
        );
        assert_eq!(
            r.value,
            json!({"tools": ["Read", "Glob"], "mcp_servers": [{"name": "fake", "status": "connected"}]})
        );
    }

    #[test]
    fn subtree_pattern_allows_descendants() {
        let v = json!({"usage": {"input_tokens": 1, "nested": {"output_tokens": 2}}, "other": 1});
        let r = redact_json(&v, &["usage.**"], HOME);
        assert_eq!(
            r.value,
            json!({"usage": {"input_tokens": 1, "nested": {"output_tokens": 2}}})
        );
    }

    #[test]
    fn never_keys_are_dropped_even_under_a_subtree_pattern() {
        let v = json!({"account": {"type": "chatgpt", "email": "me@example.com", "workspaceName": "Team"}});
        let r = redact_json(&v, &["account.**"], HOME);
        assert_eq!(r.value, json!({"account": {"type": "chatgpt"}}));
    }

    #[test]
    fn surviving_strings_are_scrubbed() {
        let v = json!({"path": "/Users/pat/proj/x", "note": "mail me@example.com now",
                       "blob": "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd"});
        let r = redact_json(&v, &["path", "note", "blob"], HOME);
        assert_eq!(r.value["path"], "~/proj/x");
        assert_eq!(r.value["note"], "mail [email] now");
        assert_eq!(r.value["blob"], "[token]");
    }

    #[test]
    fn numbers_and_bools_pass_through_when_allowed() {
        let v = json!({"n": 3, "ok": true, "none": null});
        let r = redact_json(&v, &["n", "ok", "none"], HOME);
        assert_eq!(r.value, v);
    }

    #[test]
    fn empty_allowlist_leaves_an_empty_object() {
        let r = redact_json(&json!({"a": 1}), &[], HOME);
        assert_eq!(r.value, json!({}));
        assert_eq!(r.dropped, 1);
    }

    #[test]
    fn scrub_value_cleans_every_string_and_drops_never_keys() {
        let v =
            json!({"a": ["/Users/pat/x", {"note": "me@example.com", "email": "z@z.io"}], "n": 1});
        assert_eq!(
            scrub_value(&v, HOME),
            json!({"a": ["~/x", {"note": "[email]"}], "n": 1})
        );
    }

    #[test]
    fn stderr_keeps_only_safe_prefixes() {
        let text = "warning: deprecated flag\nerror: token sk-abc123 rejected\nWARN ok-line me@example.com\n";
        let r = redact_stderr(text, &["warning:", "WARN "], HOME);
        assert_eq!(
            r.kept,
            vec!["warning: deprecated flag", "WARN ok-line [email]"]
        );
        assert_eq!(r.dropped_lines, 1);
        assert_eq!(r.bytes, text.len());
        assert_eq!(r.sha256.len(), 64);
    }

    #[test]
    fn stderr_with_no_safe_lines_keeps_nothing() {
        let r = redact_stderr("secret stuff\nmore\n", &["warning:"], HOME);
        assert!(r.kept.is_empty());
        assert_eq!(r.dropped_lines, 2);
    }
}
