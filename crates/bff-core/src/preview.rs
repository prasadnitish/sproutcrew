//! The prototype preview server. It serves one folder, read-only, on 127.0.0.1 at a random port, under a
//! per-session path token, and it checks the `Host` header. It refuses traversal (plain, percent-encoded,
//! double-encoded, backslash), symlinks, directory listings, every method but GET and HEAD, and it has no
//! write or control endpoint. Every response carries a restrictive CSP.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'";
const MAX_FILE: u64 = 20 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Why a path was refused. The server answers all of these with the same plain 404 so a probe learns
/// nothing about what exists, except a directory without an index, which is a 403.
#[derive(Debug, PartialEq, Eq)]
pub enum Deny {
    Traversal,
    Encoding,
    Symlink,
    Outside,
    Listing,
    Missing,
    TooBig,
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decodes `%XX` once. A stray `%` is kept as it is.
fn percent_decode(s: &str) -> Result<String, Deny> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).map_err(|_| Deny::Encoding)
}

fn has_escape(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| {
        b[i] == b'%'
            && b.get(i + 1).and_then(|c| hex_val(*c)).is_some()
            && b.get(i + 2).and_then(|c| hex_val(*c)).is_some()
    })
}

/// Maps the part of the URL after the token to a file inside `root`. Pure with respect to the network.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, Deny> {
    let decoded = percent_decode(rel)?;
    if decoded.contains('\0') || has_escape(&decoded) {
        return Err(Deny::Encoding);
    }
    if decoded.contains('\\') || decoded.starts_with('/') {
        return Err(Deny::Traversal);
    }
    let root = root.canonicalize().map_err(|_| Deny::Missing)?;
    let trailing = decoded.is_empty() || decoded.ends_with('/');
    let trimmed = decoded.strip_suffix('/').unwrap_or(&decoded);
    let mut cur = root.clone();
    if !trimmed.is_empty() {
        for seg in trimmed.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." {
                return Err(Deny::Traversal);
            }
            cur = cur.join(seg);
            let meta = std::fs::symlink_metadata(&cur).map_err(|_| Deny::Missing)?;
            if meta.file_type().is_symlink() {
                return Err(Deny::Symlink);
            }
        }
    }
    let meta = std::fs::symlink_metadata(&cur).map_err(|_| Deny::Missing)?;
    if meta.is_dir() {
        let index = cur.join("index.html");
        return match std::fs::symlink_metadata(&index) {
            Ok(m) if m.file_type().is_symlink() => Err(Deny::Symlink),
            Ok(m) if m.is_file() => Ok(index),
            _ => Err(Deny::Listing),
        };
    }
    if trailing {
        return Err(Deny::Missing);
    }
    if !meta.is_file() {
        return Err(Deny::Missing);
    }
    if meta.len() > MAX_FILE {
        return Err(Deny::TooBig);
    }
    if !cur
        .canonicalize()
        .map_err(|_| Deny::Missing)?
        .starts_with(&root)
    {
        return Err(Deny::Outside);
    }
    Ok(cur)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" | "md" | "csv" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn plain(status: u16) -> Response {
    Response {
        status,
        content_type: "text/plain; charset=utf-8".into(),
        body: Vec::new(),
    }
}

/// Answers one request. `host_ok` says whether the `Host` header names this server.
pub fn respond(
    root: &Path,
    token: &str,
    host_ok: &dyn Fn(&str) -> bool,
    method: &str,
    target: &str,
    host: &str,
) -> Response {
    if method != "GET" && method != "HEAD" {
        return plain(405);
    }
    if !host_ok(host) {
        return plain(403);
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    let Some(after) = path.strip_prefix('/') else {
        return plain(404);
    };
    let (first, rest) = match after.split_once('/') {
        Some((f, r)) => (f, r),
        None => return plain(404),
    };
    if first != token {
        return plain(404);
    }
    match resolve(root, rest) {
        Ok(file) => match std::fs::read(&file) {
            Ok(bytes) => Response {
                status: 200,
                content_type: content_type(&file).into(),
                body: if method == "HEAD" { Vec::new() } else { bytes },
            },
            Err(_) => plain(404),
        },
        Err(Deny::Listing) => plain(403),
        Err(Deny::TooBig) => plain(413),
        Err(_) => plain(404),
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    }
}

fn write_response(c: &mut TcpStream, r: &Response) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Security-Policy: {CSP}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        r.status,
        reason(r.status),
        r.content_type,
        r.body.len()
    );
    let _ = c.write_all(head.as_bytes());
    let _ = c.write_all(&r.body);
}

fn handle(mut c: TcpStream, root: &Path, token: &str, port: u16) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 8192 {
        match c.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let parts: Vec<&str> = first.split(' ').collect();
    if parts.len() != 3 || !parts[2].starts_with("HTTP/1.") {
        write_response(&mut c, &plain(400));
        return;
    }
    let host = lines
        .take_while(|l| !l.is_empty())
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("host").then(|| v.trim().to_string())
        })
        .unwrap_or_default();
    let ok = move |h: &str| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}");
    write_response(
        &mut c,
        &respond(root, token, &ok, parts[0], parts[1], &host),
    );
}

fn random_token() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub struct Preview {
    addr: SocketAddr,
    token: String,
    stop: Arc<AtomicBool>,
}

impl Preview {
    pub fn start(root: &Path) -> io::Result<Preview> {
        let root = root.canonicalize()?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let token = random_token();
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, tok) = (stop.clone(), token.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(c) = conn {
                    let (root, tok) = (root.clone(), tok.clone());
                    std::thread::spawn(move || handle(c, &root, &tok, addr.port()));
                }
            }
        });
        Ok(Preview { addr, token, stop })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.addr.port(), self.token)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Site {
        base: PathBuf,
        root: PathBuf,
    }

    /// `base/root` is served. `base/secret.txt` and `base/outside/` are not.
    fn site(tag: &str) -> Site {
        let base = std::env::temp_dir().join(format!("bff-preview-{}-{tag}", std::process::id()));
        let root = base.join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("emptydir")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(root.join("index.html"), "<h1>proto</h1>").unwrap();
        std::fs::write(root.join("app.js"), "console.log(1)").unwrap();
        std::fs::write(root.join("style.css"), "body{}").unwrap();
        std::fs::write(root.join("sub/page.html"), "<p>sub</p>").unwrap();
        std::fs::write(root.join("sub/index.html"), "<p>sub index</p>").unwrap();
        std::fs::write(base.join("secret.txt"), "TOP SECRET").unwrap();
        std::fs::write(base.join("outside/also.txt"), "ALSO SECRET").unwrap();
        std::os::unix::fs::symlink(base.join("secret.txt"), root.join("link-file")).unwrap();
        std::os::unix::fs::symlink(base.join("outside"), root.join("link-dir")).unwrap();
        std::os::unix::fs::symlink(root.join("app.js"), root.join("link-inside")).unwrap();
        Site { base, root }
    }

    fn get(s: &Site, rel: &str) -> Response {
        respond(
            &s.root,
            "tok",
            &|h| h == "127.0.0.1:1234",
            "GET",
            &format!("/tok/{rel}"),
            "127.0.0.1:1234",
        )
    }

    fn text(r: &Response) -> String {
        String::from_utf8_lossy(&r.body).into_owned()
    }

    #[test]
    fn it_serves_files_inside_the_folder_with_their_types() {
        let s = site("serve");
        let r = get(&s, "app.js");
        assert_eq!((r.status, text(&r).as_str()), (200, "console.log(1)"));
        assert!(r.content_type.starts_with("text/javascript"));
        assert!(get(&s, "style.css").content_type.starts_with("text/css"));
        assert!(
            get(&s, "").content_type.starts_with("text/html"),
            "the folder's index.html"
        );
        assert_eq!(text(&get(&s, "")), "<h1>proto</h1>");
        assert_eq!(text(&get(&s, "sub/page.html")), "<p>sub</p>");
        assert_eq!(text(&get(&s, "sub/")), "<p>sub index</p>");
    }

    #[test]
    fn traversal_in_every_spelling_is_refused_and_serves_nothing_outside() {
        let s = site("trav");
        for rel in [
            "../secret.txt",
            "sub/../../secret.txt",
            "%2e%2e/secret.txt",
            "%2E%2E/secret.txt",
            "%2e%2e%2fsecret.txt",
            "..%2fsecret.txt",
            "%252e%252e%252fsecret.txt",
            "%252e%252e/secret.txt",
            "..\\secret.txt",
            "..%5csecret.txt",
            "sub%2f..%2f..%2fsecret.txt",
            "/etc/passwd",
            "%2fetc%2fpasswd",
            "app.js%00.png",
            "./../secret.txt",
        ] {
            let r = get(&s, rel);
            assert_ne!(r.status, 200, "{rel} was served");
            assert!(!text(&r).contains("SECRET"), "{rel} leaked");
        }
        assert_eq!(resolve(&s.root, "../secret.txt"), Err(Deny::Traversal));
        assert_eq!(resolve(&s.root, "%2e%2e/secret.txt"), Err(Deny::Traversal));
        assert_eq!(
            resolve(&s.root, "%252e%252e/secret.txt"),
            Err(Deny::Encoding)
        );
    }

    #[test]
    fn symlinks_are_refused_whether_they_point_outside_or_inside() {
        let s = site("sym");
        for rel in ["link-file", "link-dir/also.txt", "link-dir/", "link-inside"] {
            let r = get(&s, rel);
            assert_ne!(r.status, 200, "{rel} was served");
            assert!(!text(&r).contains("SECRET"), "{rel} leaked");
        }
        assert_eq!(resolve(&s.root, "link-file"), Err(Deny::Symlink));
        assert_eq!(resolve(&s.root, "link-dir/also.txt"), Err(Deny::Symlink));
        assert_eq!(resolve(&s.root, "link-inside"), Err(Deny::Symlink));
    }

    #[test]
    fn a_directory_without_an_index_is_not_listed() {
        let s = site("list");
        let r = get(&s, "emptydir/");
        assert_eq!(r.status, 403);
        assert!(!text(&r).contains("emptydir") || text(&r).is_empty() || r.status == 403);
        // A directory with a listing-worthy name set is never enumerated.
        std::fs::remove_file(s.root.join("sub/index.html")).unwrap();
        let r = get(&s, "sub/");
        assert_eq!(r.status, 403);
        assert!(!text(&r).contains("page.html"), "file names must not leak");
        assert_eq!(resolve(&s.root, "sub"), Err(Deny::Listing));
    }

    #[test]
    fn the_session_token_and_host_are_required() {
        let s = site("tok");
        let go = |target: &str, host: &str| {
            respond(
                &s.root,
                "tok",
                &|h| h == "127.0.0.1:1234",
                "GET",
                target,
                host,
            )
            .status
        };
        assert_eq!(go("/tok/app.js", "127.0.0.1:1234"), 200);
        assert_eq!(go("/app.js", "127.0.0.1:1234"), 404, "no token");
        assert_eq!(go("/wrong/app.js", "127.0.0.1:1234"), 404, "wrong token");
        assert_eq!(go("/", "127.0.0.1:1234"), 404);
        assert_eq!(go("/tok/app.js", "evil.test"), 403, "a rebinding host");
        assert_eq!(go("/tok/app.js", ""), 403);
        assert_eq!(
            go("/tokx/app.js", "127.0.0.1:1234"),
            404,
            "a token prefix is not the token"
        );
    }

    #[test]
    fn only_get_and_head_are_answered_and_there_is_no_control_endpoint() {
        let s = site("method");
        for m in [
            "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE", "CONNECT",
        ] {
            let r = respond(&s.root, "tok", &|_| true, m, "/tok/app.js", "h");
            assert_eq!(r.status, 405, "{m}");
        }
        let head = respond(&s.root, "tok", &|_| true, "HEAD", "/tok/app.js", "h");
        assert_eq!(head.status, 200);
        assert!(head.body.is_empty(), "HEAD has no body");
        for ctl in ["__stop", "__shutdown", "api/write", "admin"] {
            assert_eq!(get(&s, ctl).status, 404, "{ctl}");
        }
    }

    #[test]
    fn a_query_or_fragment_does_not_change_which_file_is_served() {
        let s = site("query");
        assert_eq!(text(&get(&s, "app.js?v=2#x")), "console.log(1)");
        assert_ne!(get(&s, "app.js?/../../secret.txt").status, 0);
        assert!(!text(&get(&s, "app.js?/../../secret.txt")).contains("SECRET"));
    }

    #[test]
    fn a_real_server_enforces_the_same_rules_over_tcp_and_stops_on_request() {
        let s = site("tcp");
        let p = Preview::start(&s.root).unwrap();
        let ask = |raw: String| -> String {
            let mut c = TcpStream::connect(("127.0.0.1", p.port())).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            c.write_all(raw.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = c.read_to_string(&mut out);
            out
        };
        let host = format!("127.0.0.1:{}", p.port());
        let ok = ask(format!(
            "GET /{}/app.js HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
            p.token()
        ));
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(
            ok.contains("Content-Security-Policy: default-src 'none'"),
            "{ok}"
        );
        assert!(
            ok.contains("X-Content-Type-Options: nosniff")
                && ok.contains("Cache-Control: no-store")
        );
        assert!(ok.ends_with("console.log(1)"));
        let trav = ask(format!(
            "GET /{}/../secret.txt HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
            p.token()
        ));
        assert!(
            !trav.starts_with("HTTP/1.1 200") && !trav.contains("SECRET"),
            "{trav}"
        );
        let evil = ask(format!(
            "GET /{}/app.js HTTP/1.1\r\nHost: evil.test\r\nConnection: close\r\n\r\n",
            p.token()
        ));
        assert!(evil.starts_with("HTTP/1.1 403"), "{evil}");
        let post = ask(format!("POST /{}/app.js HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", p.token()));
        assert!(post.starts_with("HTTP/1.1 405"), "{post}");
        let junk = ask("NOT HTTP AT ALL\r\n\r\n".to_string());
        assert!(junk.starts_with("HTTP/1.1 400"), "{junk}");
        assert!(p.url().starts_with("http://127.0.0.1:") && p.url().contains(p.token()));
        p.stop();
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            TcpStream::connect_timeout(
                &SocketAddr::from(([127, 0, 0, 1], p.port())),
                Duration::from_millis(300)
            )
            .is_err()
                || ask(format!(
                    "GET /{}/app.js HTTP/1.1\r\nHost: {host}\r\n\r\n",
                    p.token()
                ))
                .is_empty()
        );
    }

    #[test]
    fn the_server_listens_on_loopback_only_with_a_fresh_token_each_time() {
        let s = site("loop");
        let (a, b) = (
            Preview::start(&s.root).unwrap(),
            Preview::start(&s.root).unwrap(),
        );
        assert!(a.addr.ip().is_loopback());
        assert_ne!(a.token(), b.token());
        assert_ne!(a.port(), b.port());
        assert!(a.token().len() >= 32);
        a.stop();
        b.stop();
        let _ = &s.base;
    }
}
