//! A scan for secrets and private details in text that is about to leave the machine. Secrets block an
//! export. Private details (a home folder path, an email address) are reported as warnings. The scan is
//! pattern based and conservative about ordinary prose, and a finding shows a masked excerpt so the report
//! does not repeat the secret.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Secret,
    Privacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: &'static str,
    pub class: Class,
    /// 1-based.
    pub line: usize,
    /// The start of the match, then `***`. Never the whole value.
    pub excerpt: String,
}

fn mask(s: &str) -> String {
    format!("{}***", s.chars().take(4).collect::<String>())
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_uuid(w: &str) -> bool {
    w.len() == 36
        && w.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn name_chars(s: &str, extra: &[u8]) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

/// The kind of secret a single token is, if it has a recognizable shape on its own.
fn token_kind(t: &str) -> Option<&'static str> {
    let rest_ok = |r: &str| name_chars(r, b"_-");
    if t.starts_with("sk-ant-") && t.len() >= 20 {
        return Some("provider key");
    }
    if let Some(r) = t.strip_prefix("sk-") {
        if t.len() >= 32 && rest_ok(r) {
            return Some("provider key");
        }
    }
    for p in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"] {
        if t.starts_with(p) && t.len() >= 30 {
            return Some("github token");
        }
    }
    if t.len() >= 15
        && t.starts_with("xox")
        && t.as_bytes().get(3).is_some_and(|c| b"baprs".contains(c))
        && t.as_bytes().get(4) == Some(&b'-')
    {
        return Some("slack token");
    }
    if t.len() == 20
        && t.starts_with("AKIA")
        && t.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return Some("aws key");
    }
    if t.len() >= 39 && t.starts_with("AIza") && rest_ok(&t[4..]) {
        return Some("google key");
    }
    if t.starts_with("eyJ") && t.len() >= 30 {
        let parts: Vec<&str> = t.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty()) {
            return Some("jwt");
        }
    }
    let digest = [32, 40, 64, 128].contains(&t.len()) && is_hex(t);
    if t.len() >= 32
        && rest_ok(t)
        && !digest
        && !is_uuid(t)
        && t.bytes().any(|b| b.is_ascii_digit())
        && t.bytes().any(|b| b.is_ascii_alphabetic())
    {
        return Some("high-entropy token");
    }
    None
}

fn placeholder(v: &str) -> bool {
    let l = v.to_lowercase();
    v.starts_with(['<', '$', '{', '[', '%'])
        || [
            "your",
            "example",
            "xxxx",
            "redacted",
            "changeme",
            "placeholder",
            "****",
            "...",
        ]
        .iter()
        .any(|p| l.contains(p))
        || v.chars().all(|c| v.starts_with(c))
}

const SECRET_NAMES: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "private_key",
    "authorization",
];

/// `name: value` or `name = value` where the name says it is a secret and the value is not a placeholder.
fn assignments(line: &str) -> Vec<String> {
    let lower = line.to_lowercase();
    let b = lower.as_bytes();
    let mut out = Vec::new();
    for name in SECRET_NAMES {
        let mut from = 0;
        while let Some(i) = lower[from..].find(name) {
            let at = from + i;
            from = at + name.len();
            // Extend to the whole identifier that contains the name.
            let mut end = from;
            while end < b.len()
                && (b[end].is_ascii_alphanumeric() || b[end] == b'_' || b[end] == b'-')
            {
                end += 1;
            }
            let mut rest = line[end..].trim_start_matches([' ', '\t', '"', '\'']);
            let Some(r) = rest.strip_prefix([':', '=']) else {
                continue;
            };
            rest = r.trim_start_matches([' ', '\t', '"', '\'', '=']);
            let value: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && !"\"',;".contains(*c))
                .collect();
            if value.len() >= 8 && !placeholder(&value) {
                out.push(value);
            }
        }
    }
    out
}

pub fn scan(text: &str) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line_no = n + 1;
        let mut add = |kind: &'static str, class: Class, at: &str| {
            if !out
                .iter()
                .any(|f| f.line == line_no && f.kind == kind && f.excerpt == mask(at))
            {
                out.push(Finding {
                    kind,
                    class,
                    line: line_no,
                    excerpt: mask(at),
                });
            }
        };
        if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----") {
            add("private key", Class::Secret, "-----BEGIN");
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let mut reported: Vec<String> = Vec::new();
        for (i, w) in words.iter().enumerate() {
            let trimmed = w.trim_matches(|c: char| ",;()<>\"'".contains(c));
            let mut pieces = vec![trimmed];
            if let Some((_, v)) = trimmed.split_once('=') {
                pieces.push(v.trim_matches(|c: char| ",;()<>\"'".contains(c)));
            }
            for t in pieces {
                let after_bearer =
                    i > 0 && words[i - 1] == "Bearer" && t.len() >= 20 && !placeholder(t);
                let kind = match token_kind(t) {
                    Some("high-entropy token") | None if after_bearer => Some("bearer token"),
                    other => other,
                };
                if let Some(kind) = kind {
                    add(kind, Class::Secret, t);
                    reported.push(t.to_string());
                }
            }
            if let Some(start) = trimmed.find("/Users/").or_else(|| trimmed.find("/home/")) {
                let after = &trimmed[start..];
                let seg = after.splitn(3, '/').nth(2).unwrap_or("");
                if !seg.is_empty() && !seg.starts_with('.') {
                    add("home path", Class::Privacy, &trimmed[start..]);
                }
            }
            if let Some((local, domain)) = trimmed.split_once('@') {
                if !local.is_empty()
                    && domain.contains('.')
                    && !domain.ends_with('.')
                    && !local.contains('/')
                {
                    add("email address", Class::Privacy, trimmed);
                }
            }
        }
        for value in assignments(line) {
            if !reported.iter().any(|r| r == &value) {
                add("secret assignment", Class::Secret, &value);
            }
        }
    }
    out
}

pub fn has_secret(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.class == Class::Secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_key_shapes_are_secrets() {
        for (text, kind) in [
            (
                "key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
                "provider key",
            ),
            (
                "OPENAI=sk-proj1234567890abcdefghijklmnopqrstuvwx",
                "provider key",
            ),
            (
                "token ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                "github token",
            ),
            (
                "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
                "github token",
            ),
            ("slack xoxb-1234567890-abcdefghij", "slack token"),
            ("aws AKIAIOSFODNN7EXAMPLE", "aws key"),
            (
                "google AIzaSyA1234567890abcdefghijklmnopqrstuv",
                "google key",
            ),
            (
                "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnop",
                "jwt",
            ),
            (
                "Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345",
                "bearer token",
            ),
            (
                "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----",
                "private key",
            ),
            ("-----BEGIN OPENSSH PRIVATE KEY-----", "private key"),
        ] {
            let f = scan(text);
            assert!(
                f.iter().any(|x| x.kind == kind && x.class == Class::Secret),
                "{text:?} -> {f:?}"
            );
        }
    }

    #[test]
    fn assignments_to_secret_names_are_secrets_unless_they_are_placeholders() {
        for text in [
            "password = hunter2hunter2",
            "api_key: \"abcd1234efgh5678\"",
            "client_secret=Zm9vYmFyYmF6cXV4",
            "ACCESS_TOKEN: 9f8e7d6c5b4a3210",
        ] {
            assert!(has_secret(&scan(text)), "{text:?}");
        }
        for text in [
            "password = <your password>",
            "api_key: ${API_KEY}",
            "token = REDACTED",
            "secret: your-secret-here",
            "api_key = xxxxxxxxxxxx",
            "password: example-password",
            "token: $TOKEN",
        ] {
            assert!(
                !has_secret(&scan(text)),
                "{text:?} is a placeholder: {:?}",
                scan(text)
            );
        }
    }

    #[test]
    fn long_random_looking_strings_are_suspected_but_digests_and_ids_are_not() {
        assert!(has_secret(&scan(
            "value Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYg"
        )));
        let sha = "a".repeat(64);
        let uuid = "123e4567-e89b-12d3-a456-426614174000";
        let sha40 = "0123456789abcdef0123456789abcdef01234567";
        assert!(scan(&format!("sha256 {sha} and {uuid} and {sha40}")).is_empty());
    }

    #[test]
    fn ordinary_prose_about_passwords_and_tokens_is_clean() {
        let prose = "The password reset flow sends a one-time token by email. Users who forgot their password \
                     can request a reset. Our API key rotation policy is quarterly. Bearer of bad news: the pilot slips.";
        assert!(scan(prose).is_empty(), "{:?}", scan(prose));
        assert!(scan("Total: $20 per month for 4 licenses. See src-1#p2.").is_empty());
        assert!(scan("").is_empty());
    }

    #[test]
    fn home_paths_and_emails_are_warnings_not_secrets() {
        let f = scan("saved to /Users/pat/Documents/plan.md, ask pat.lee@example.com");
        assert!(f
            .iter()
            .any(|x| x.kind == "home path" && x.class == Class::Privacy));
        assert!(f
            .iter()
            .any(|x| x.kind == "email address" && x.class == Class::Privacy));
        assert!(!has_secret(&f));
        assert!(scan("/home/alex/project")
            .iter()
            .any(|x| x.kind == "home path"));
        assert!(scan("a path like /usr/local/bin and ~/notes is fine").is_empty());
    }

    #[test]
    fn findings_carry_the_line_and_a_masked_excerpt_never_the_whole_secret() {
        let secret = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
        let f = scan(&format!("line one\nline two {secret} end"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].line, 2);
        assert!(f[0].excerpt.ends_with("***"));
        assert!(!f[0].excerpt.contains("0123456789"), "{}", f[0].excerpt);
        assert!(f[0].excerpt.len() <= 12);
    }

    #[test]
    fn one_secret_is_reported_once_even_when_two_patterns_match_it() {
        let f = scan("Authorization: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(
            f.iter().filter(|x| x.class == Class::Secret).count(),
            1,
            "{f:?}"
        );
    }
}
