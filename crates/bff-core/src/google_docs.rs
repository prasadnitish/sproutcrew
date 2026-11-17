//! Google Docs as a source: the person signs in to Google once in their browser, and a Doc they paste the link of
//! is read as Markdown. Read-only: the app asks only for permission to view documents, and never edits one.
//!
//! Sign-in follows Google's guidance for desktop apps: the app listens once on `127.0.0.1` at a random port, opens
//! Google's sign-in page in the browser with a PKCE challenge (a one-time code check, so an intercepted answer
//! cannot be used by anyone else) and a random state, and swaps the answer for a refresh token, which is kept in
//! the Keychain (item `google-docs`) and never shown. Each read swaps the refresh token for a short-lived access
//! token. The OAuth client is Nitish's own Google Cloud client; until its ID is set in [`CLIENT_ID`], Google Docs
//! is shown as not available.

use serde_json::Value;

/// Nitish's Google Cloud OAuth client, of type "Desktop app", read at build time from `google-client.json` next to
/// this crate's `Cargo.toml`. That file is not committed. Google does not treat a desktop client's secret as
/// confidential, so it is built into the app with the ID. `None` when the file is missing.
pub const CLIENT_ID: Option<&str> = option_env!("SPROUTCREW_GOOGLE_CLIENT_ID");
pub const CLIENT_SECRET: Option<&str> = option_env!("SPROUTCREW_GOOGLE_CLIENT_SECRET");
/// Permission to view Google Docs, and nothing else.
pub const SCOPE: &str = "https://www.googleapis.com/auth/documents.readonly";
/// The Keychain item that holds the refresh token.
pub const KEY_ITEM: &str = "google-docs";
pub const AUTH_HOST: &str = "accounts.google.com";
pub const TOKEN_HOST: &str = "oauth2.googleapis.com";
pub const DOCS_HOST: &str = "docs.googleapis.com";
const MAX_BLOCKS: usize = 5_000;

/// Whether Google Docs can be offered in this version.
pub fn available() -> bool {
    CLIENT_ID.is_some()
}

/// Percent-encoding for a URL query or form value.
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(v)) => {
                out.push(v);
                i += 2;
            }
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A random string from this Mac's random source, as URL-safe base64.
pub fn random_token() -> Result<String, String> {
    use base64::Engine;
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|_| "this Mac's random source could not be read".to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b))
}

/// The refresh token as kept in the Keychain: URL-safe base64, so it fits the Keychain's characters (Google's
/// tokens contain slashes).
pub fn to_store(token: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token.as_bytes())
}

/// The refresh token back from its Keychain form.
pub fn from_store(stored: &str) -> Option<String> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(stored)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}

/// The PKCE challenge for a verifier: the SHA-256 of it, as URL-safe base64.
pub fn challenge(verifier: &str) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()))
}

/// Google's sign-in page for this client, asking for view-only access to Docs and a refresh token.
pub fn auth_url(client_id: &str, redirect: &str, state: &str, verifier: &str) -> String {
    format!(
        "https://{AUTH_HOST}/o/oauth2/v2/auth?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        encode(client_id),
        encode(redirect),
        encode(SCOPE),
        encode(state),
        challenge(verifier)
    )
}

/// The code from the first line of the browser's request to the loopback address, such as
/// `GET /?state=..&code=.. HTTP/1.1`. The state must match the one sent, or the answer is refused.
pub fn code_from(request_line: &str, state: &str) -> Result<String, String> {
    let target = request_line
        .strip_prefix("GET ")
        .and_then(|r| r.split(' ').next())
        .ok_or("the sign-in answer was not readable")?;
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = None;
    let mut got_state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "code" => code = Some(decode(v)),
            "state" => got_state = Some(decode(v)),
            "error" => error = Some(decode(v)),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(if e == "access_denied" {
            "Google sign-in was cancelled".into()
        } else {
            format!("Google sign-in did not finish ({e})")
        });
    }
    if got_state.as_deref() != Some(state) {
        return Err("the sign-in answer did not match this request, so it was not used".into());
    }
    code.filter(|c| !c.is_empty())
        .ok_or("the sign-in answer had no code".into())
}

/// The state a request line carries, decoded, if it carries one.
fn state_in(request_line: &str) -> Option<String> {
    let target = request_line.strip_prefix("GET ")?.split(' ').next()?;
    let (_, query) = target.split_once('?')?;
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == "state")
        .map(|(_, v)| decode(v))
}

/// Waits for Google to send the browser back to `listener`, answers with a short page telling the person to
/// return to SproutCrew, and returns the code. Gives up after `timeout`.
pub fn wait_for_code(
    listener: &std::net::TcpListener,
    state: &str,
    timeout: std::time::Duration,
) -> Result<String, String> {
    use std::io::{BufRead, Read, Write};
    let until = std::time::Instant::now() + timeout;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut line = String::new();
                let _ = std::io::BufReader::new((&stream).take(8192)).read_line(&mut line);
                // A browser may ask for a favicon first, and another program on this Mac may knock on the port;
                // only the answer that carries our state counts, so nothing else can end the sign-in.
                if state_in(line.trim_end()).as_deref() != Some(state) {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let r = code_from(line.trim_end(), state);
                let page = if r.is_ok() {
                    "Signed in to Google Docs. You can close this tab and go back to SproutCrew."
                } else {
                    "Google sign-in did not finish. Go back to SproutCrew to try again."
                };
                let body = format!("<!doctype html><meta charset=utf-8><title>SproutCrew</title><p style=\"font:16px -apple-system,sans-serif;margin:3em\">{page}</p>");
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
                return r;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= until {
                    return Err("Google sign-in timed out. Try again, and finish it in the browser within three minutes".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// A form POST, which Google's token endpoint takes.
pub trait FormPost {
    fn post_form(&mut self, url: &str, fields: &[(&str, &str)]) -> Result<(u16, String), String>;
}

/// The form fields for swapping the sign-in code for tokens.
pub fn exchange_fields<'a>(
    code: &'a str,
    verifier: &'a str,
    redirect: &'a str,
    id: &'a str,
    secret: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect),
        ("client_id", id),
        ("client_secret", secret),
    ]
}

/// Swaps the sign-in code for a refresh token.
pub fn exchange(
    post: &mut dyn FormPost,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<String, String> {
    let (id, secret) = client()?;
    let (status, body) = post.post_form(
        &format!("https://{TOKEN_HOST}/token"),
        &exchange_fields(code, verifier, redirect, id, secret),
    )?;
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if status != 200 {
        return Err(format!(
            "Google did not accept the sign-in ({})",
            v["error"].as_str().unwrap_or("no reason given")
        ));
    }
    v["refresh_token"]
        .as_str()
        .map(str::to_string)
        .ok_or("Google gave no lasting sign-in. Remove SproutCrew's access in your Google account and sign in again".into())
}

fn client() -> Result<(&'static str, &'static str), String> {
    match (CLIENT_ID, CLIENT_SECRET) {
        (Some(i), Some(s)) => Ok((i, s)),
        _ => Err("Google Docs is not available in this version yet".into()),
    }
}

/// A short-lived access token from the saved refresh token.
pub fn access_token(post: &mut dyn FormPost, refresh: &str) -> Result<String, String> {
    let (id, secret) = client()?;
    let (status, body) = post.post_form(
        &format!("https://{TOKEN_HOST}/token"),
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", id),
            ("client_secret", secret),
        ],
    )?;
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    match (status, v["access_token"].as_str()) {
        (200, Some(t)) => Ok(t.to_string()),
        (400 | 401, _) if v["error"] == "invalid_grant" => {
            Err("Your Google sign-in has ended. Connect Google Docs again in Settings".into())
        }
        _ => Err(format!(
            "Google sign-in could not be renewed (status {status})"
        )),
    }
}

/// The document id from a Google Docs link, such as `https://docs.google.com/document/d/<id>/edit`.
pub fn doc_id(link: &str) -> Option<String> {
    let rest = link
        .trim()
        .strip_prefix("https://docs.google.com/document/")?;
    let rest = rest
        .strip_prefix("u/")
        .map_or(rest, |r| r.split_once('/').map_or(r, |(_, x)| x));
    let id = rest.strip_prefix("d/")?.split(['/', '?', '#']).next()?;
    (id.len() >= 20
        && id.len() <= 100
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| id.to_string())
}

/// A Google Doc as Markdown: headings, list items, paragraphs and tables, with its title. Pictures and drawings
/// are left out, and the notes say so.
pub fn to_markdown(doc: &Value) -> (String, String, Vec<String>) {
    let title = doc["title"].as_str().unwrap_or("Google Doc").to_string();
    let mut out = String::new();
    let mut notes = vec![];
    let mut left_out = false;
    let mut n = 0;
    fn text_of(p: &Value, left_out: &mut bool) -> String {
        let mut t = String::new();
        for e in p["elements"].as_array().into_iter().flatten() {
            if let Some(s) = e["textRun"]["content"].as_str() {
                t.push_str(s);
            } else if e.get("inlineObjectElement").is_some() {
                *left_out = true;
            }
        }
        t.trim_end_matches('\n').to_string()
    }
    for el in doc["body"]["content"].as_array().into_iter().flatten() {
        n += 1;
        if n > MAX_BLOCKS {
            notes.push(format!(
                "Only the first {MAX_BLOCKS} parts of the document were read."
            ));
            break;
        }
        if let Some(p) = el.get("paragraph") {
            let t = text_of(p, &mut left_out);
            if t.trim().is_empty() {
                continue;
            }
            let style = p["paragraphStyle"]["namedStyleType"].as_str().unwrap_or("");
            let line = if let Some(h) = style.strip_prefix("HEADING_") {
                format!(
                    "{} {t}",
                    "#".repeat(h.parse::<usize>().unwrap_or(1).clamp(1, 6))
                )
            } else if style == "TITLE" {
                format!("# {t}")
            } else if p.get("bullet").is_some() {
                let depth = p["bullet"]["nestingLevel"].as_u64().unwrap_or(0) as usize;
                format!("{}- {t}", "  ".repeat(depth.min(5)))
            } else {
                t
            };
            out.push_str(&line);
            out.push_str(if p.get("bullet").is_some() {
                "\n"
            } else {
                "\n\n"
            });
        } else if let Some(table) = el.get("table") {
            let rows: Vec<Vec<String>> = table["tableRows"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    r["tableCells"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|c| {
                            c["content"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|x| {
                                    x.get("paragraph").map(|p| text_of(p, &mut left_out))
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                                .replace('|', "\\|")
                        })
                        .collect()
                })
                .collect();
            if let Some(first) = rows.first() {
                out.push_str(&format!(
                    "\n| {} |\n|{}\n",
                    first.join(" | "),
                    " --- |".repeat(first.len())
                ));
                for r in &rows[1..] {
                    out.push_str(&format!("| {} |\n", r.join(" | ")));
                }
                out.push('\n');
            }
        }
    }
    if left_out {
        notes.push("Pictures and drawings in the document were left out.".into());
    }
    (title, out.trim_end().to_string() + "\n", notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_sign_in_link_asks_only_to_view_docs_with_a_pkce_challenge() {
        let u = auth_url(
            "abc.apps.googleusercontent.com",
            "http://127.0.0.1:4321",
            "st8",
            "verifier",
        );
        assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdocuments.readonly&"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A4321&"));
        assert!(u.contains("code_challenge_method=S256") && u.contains(&challenge("verifier")));
        assert!(
            !u.contains("verifier&"),
            "the verifier itself is never sent in the link"
        );
        // Checked against Python's hashlib and base64.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mJ0kHZL6eEEnx7EYtgcNDOsdAKqEo"),
            "P1TSMTx_7iVH4jn9scuZzfp-xPzyFJbfIWvDHDbz6Bw"
        );
        assert_ne!(random_token().unwrap(), random_token().unwrap());
    }

    #[test]
    fn a_refresh_token_is_kept_in_characters_the_keychain_accepts() {
        let t = "1//0gAbC-def_GHI/jkl+mno=";
        let stored = to_store(t);
        assert!(crate::keychain::valid_secret(&stored), "{stored}");
        assert_eq!(from_store(&stored).as_deref(), Some(t));
    }

    #[test]
    fn the_answer_is_used_only_when_its_state_matches() {
        assert_eq!(
            code_from("GET /?state=s1&code=4%2F0Ab HTTP/1.1", "s1").unwrap(),
            "4/0Ab"
        );
        assert!(code_from("GET /?state=other&code=x HTTP/1.1", "s1")
            .unwrap_err()
            .contains("did not match"));
        assert!(
            code_from("GET /?error=access_denied&state=s1 HTTP/1.1", "s1")
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(code_from("GET /?state=s1 HTTP/1.1", "s1").is_err());
    }

    #[test]
    fn a_request_without_our_state_cannot_end_the_sign_in() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            for knock in [
                &b"GET /?error=x HTTP/1.1\r\n\r\n"[..],
                &b"GET /?state=other&code=theirs HTTP/1.1\r\n\r\n"[..],
            ] {
                let mut a = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                a.write_all(knock).unwrap();
                let mut s = String::new();
                let _ = a.read_to_string(&mut s);
                assert!(s.starts_with("HTTP/1.1 404"), "{s}");
            }
            let mut b = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            b.write_all(b"GET /?state=s1&code=c0de HTTP/1.1\r\n\r\n")
                .unwrap();
            let mut s = String::new();
            let _ = b.read_to_string(&mut s);
        });
        assert_eq!(
            wait_for_code(&l, "s1", std::time::Duration::from_secs(5)).unwrap(),
            "c0de"
        );
        t.join().unwrap();
    }

    #[test]
    fn the_loopback_listener_takes_one_answer_and_ignores_other_requests() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut a = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            a.write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n").unwrap();
            let mut s = String::new();
            let _ = a.read_to_string(&mut s);
            let mut b = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            b.write_all(b"GET /?state=s1&code=c0de HTTP/1.1\r\n\r\n")
                .unwrap();
            let mut s = String::new();
            let _ = b.read_to_string(&mut s);
            s
        });
        assert_eq!(
            wait_for_code(&l, "s1", std::time::Duration::from_secs(5)).unwrap(),
            "c0de"
        );
        assert!(t.join().unwrap().contains("go back to SproutCrew"));
    }

    #[test]
    fn a_docs_link_gives_its_id_and_other_links_give_none() {
        let id = "1AbCdEfGhIjKlMnOpQrStUvWxYz_0123456789-ab";
        for ok in [
            format!("https://docs.google.com/document/d/{id}/edit"),
            format!("https://docs.google.com/document/d/{id}/edit?usp=sharing"),
            format!("https://docs.google.com/document/u/0/d/{id}/edit"),
            format!(" https://docs.google.com/document/d/{id} "),
        ] {
            assert_eq!(doc_id(&ok).as_deref(), Some(id), "{ok}");
        }
        for no in [
            "https://docs.google.com/spreadsheets/d/1AbCdEfGhIjKlMnOpQrStUvWx/edit",
            "https://evil.com/document/d/1AbCdEfGhIjKlMnOpQrStUvWx",
            "https://docs.google.com/document/d/short/edit",
        ] {
            assert!(doc_id(no).is_none(), "{no}");
        }
    }

    #[test]
    fn a_doc_becomes_markdown_with_headings_lists_and_tables() {
        let p = |style: &str, text: &str| json!({"paragraph": {"paragraphStyle": {"namedStyleType": style}, "elements": [{"textRun": {"content": format!("{text}\n")}}]}});
        let doc = json!({"title": "Plan", "body": {"content": [
            p("TITLE", "Q3 plan"),
            p("HEADING_2", "Goals"),
            {"paragraph": {"paragraphStyle": {"namedStyleType": "NORMAL_TEXT"}, "bullet": {"listId": "a"}, "elements": [{"textRun": {"content": "Ship it\n"}}]}},
            p("NORMAL_TEXT", "Body text."),
            {"paragraph": {"elements": [{"inlineObjectElement": {}}]}},
            {"table": {"tableRows": [
                {"tableCells": [{"content": [p("NORMAL_TEXT", "Name")]}, {"content": [p("NORMAL_TEXT", "Cost")]}]},
                {"tableCells": [{"content": [p("NORMAL_TEXT", "A|B")]}, {"content": [p("NORMAL_TEXT", "$5")]}]}
            ]}}
        ]}});
        let (title, md, notes) = to_markdown(&doc);
        assert_eq!(title, "Plan");
        assert!(
            md.starts_with("# Q3 plan\n\n## Goals\n\n- Ship it\nBody text."),
            "{md}"
        );
        assert!(
            md.contains("| Name | Cost |\n| --- | --- |\n| A\\|B | $5 |"),
            "{md}"
        );
        assert_eq!(
            notes,
            ["Pictures and drawings in the document were left out."]
        );
    }

    struct Form(u16, &'static str, Vec<(String, Vec<(String, String)>)>);
    impl FormPost for Form {
        fn post_form(
            &mut self,
            url: &str,
            fields: &[(&str, &str)],
        ) -> Result<(u16, String), String> {
            self.2.push((
                url.into(),
                fields
                    .iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect(),
            ));
            Ok((self.0, self.1.into()))
        }
    }

    #[test]
    fn without_a_client_nothing_is_sent_and_google_docs_is_not_offered() {
        if CLIENT_ID.is_none() {
            assert!(!available());
            let mut f = Form(200, "{}", vec![]);
            assert!(access_token(&mut f, "r")
                .unwrap_err()
                .contains("not available"));
            assert!(f.2.is_empty());
        }
        let fields = exchange_fields("c", "v", "http://127.0.0.1:1", "id", "s");
        assert!(
            fields.contains(&("code_verifier", "v"))
                && fields.contains(&("grant_type", "authorization_code"))
        );
    }
}
