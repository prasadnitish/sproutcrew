//! The one HTTP client. It talks HTTPS only, to a fixed list of hosts given when it is built, and to nothing
//! else: a URL with another host, a port, user information, or a look-alike name is refused before any
//! connection opens. It follows no redirect, ignores proxy settings from the environment, has a time limit
//! and a size limit on every answer, and sends only the headers it is given. It serves connectors (GET) and
//! the API routes (POST with JSON).

use crate::api_engine::{ApiTransport, PostFailure};
use crate::connector::{Request, Response, Transport, TransportError};
use serde_json::Value;
use std::time::Duration;

pub const MAX_BODY: u64 = 5 * 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(120);
/// For a short read the person is waiting on, which is better failed than hung.
pub const QUICK_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a model may take to answer. A step that writes up to 8,000 tokens on a slow model can need more than
/// two minutes, and a timeout after the request was sent leaves its charge unknown, which pauses all AI work.
pub const MODEL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct HttpClient {
    agent: ureq::Agent,
    hosts: Vec<String>,
    https_only: bool,
}

/// The host of an `https://host/path` URL, or why it is refused. The host must match one of `allowed` exactly.
pub fn checked_host<'a>(
    url: &str,
    allowed: &'a [String],
    https_only: bool,
) -> Result<&'a str, String> {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("the address has spaces or control characters".into());
    }
    let rest = match url.strip_prefix("https://") {
        Some(r) => r,
        None if !https_only => url
            .strip_prefix("http://")
            .ok_or("the address is not http or https")?,
        None => return Err("only https addresses are allowed".into()),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', '\\']) {
        return Err("the address has no usable host".into());
    }
    allowed
        .iter()
        .find(|h| h.as_str() == authority)
        .map(String::as_str)
        .ok_or_else(|| format!("the host {authority:?} is not on this client's list"))
}

/// The one PATCH the app makes: adding blocks at the end of a Notion page, which Notion offers only as a PATCH to
/// `https://api.notion.com/v1/blocks/<page id>/children`. Every other PATCH is refused, so nothing can be
/// changed in place anywhere else.
pub fn patch_allowed(url: &str) -> bool {
    url.strip_prefix("https://api.notion.com/v1/blocks/")
        .and_then(|r| r.strip_suffix("/children"))
        .is_some_and(|id| {
            id.len() == 36
                && id.chars().enumerate().all(|(i, c)| {
                    if [8, 13, 18, 23].contains(&i) {
                        c == '-'
                    } else {
                        c.is_ascii_hexdigit()
                    }
                })
        })
}

impl HttpClient {
    /// A client for these hosts only, over HTTPS only.
    pub fn new(hosts: &[&str]) -> HttpClient {
        HttpClient::build(hosts.iter().map(|h| h.to_string()).collect(), true, TIMEOUT)
    }

    /// A client for a quick read of public data, such as the model list: it gives up after [`QUICK_TIMEOUT`].
    pub fn quick(hosts: &[&str]) -> HttpClient {
        HttpClient::build(
            hosts.iter().map(|h| h.to_string()).collect(),
            true,
            QUICK_TIMEOUT,
        )
    }

    /// A client for a model provider, which waits up to [`MODEL_TIMEOUT`] for an answer.
    pub fn for_models(hosts: &[&str]) -> HttpClient {
        HttpClient::build(
            hosts.iter().map(|h| h.to_string()).collect(),
            true,
            MODEL_TIMEOUT,
        )
    }

    /// A client for the person's own AI service at a checked address: that host and port alone, over HTTPS unless
    /// the address is this Mac's own plain `http://localhost:<port>`.
    pub fn for_own_service(a: &crate::custom_ai::Address) -> HttpClient {
        HttpClient::build(
            vec![a.authority.clone()],
            a.base.starts_with("https://"),
            MODEL_TIMEOUT,
        )
    }

    fn build(hosts: Vec<String>, https_only: bool, timeout: Duration) -> HttpClient {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .https_only(https_only)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .proxy(None)
            .build();
        HttpClient {
            agent: ureq::Agent::new_with_config(config),
            hosts,
            https_only,
        }
    }

    fn read(
        &self,
        url: &str,
        method: &str,
        headers: &[(String, String)],
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        self.read_classified(url, method, headers, body)
            .map(|(status, text, _)| (status, text))
            .map_err(|f| f.note)
    }

    /// [`HttpClient::read`] that also says whether a failure could have come after the provider got the request.
    fn read_classified(
        &self,
        url: &str,
        method: &str,
        headers: &[(String, String)],
        body: Option<&str>,
    ) -> Result<(u16, String, crate::api_limits::Hints), PostFailure> {
        checked_host(url, &self.hosts, self.https_only)?;
        let sent = match (method, body) {
            ("POST", Some(b)) => {
                let mut r = self.agent.post(url);
                for (k, v) in headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.send(b)
            }
            ("PUT", Some(b)) => {
                let mut r = self.agent.put(url);
                for (k, v) in headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.send(b)
            }
            ("PATCH", Some(b)) if patch_allowed(url) => {
                let mut r = self.agent.patch(url);
                for (k, v) in headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.send(b)
            }
            ("GET", _) => {
                let mut r = self.agent.get(url);
                for (k, v) in headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                r.call()
            }
            (other, _) => return Err(format!("{other} is not a request this client makes").into()),
        };
        let mut resp = sent.map_err(|e| PostFailure {
            note: format!("the request failed: {e}"),
            maybe_billed: !never_sent(&e),
        })?;
        let status = resp.status().as_u16();
        let hints = crate::api_limits::Hints::from_headers(
            resp.headers()
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str(), v))),
        );
        let text = resp
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map_err(|e| PostFailure {
                note: format!("the answer could not be read: {e}"),
                maybe_billed: true,
            })?;
        Ok((status, text, hints))
    }
}

/// Whether a failure to send happened before any request bytes could have left this Mac. Anything else, such as a
/// reset connection or a timeout while waiting, may have come after the provider got the request.
fn never_sent(e: &ureq::Error) -> bool {
    use ureq::Timeout;
    matches!(
        e,
        ureq::Error::BadUri(_)
            | ureq::Error::Http(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::InvalidProxyUrl
            | ureq::Error::Timeout(Timeout::Resolve | Timeout::Connect)
    )
}

impl Transport for HttpClient {
    fn get(&mut self, req: &Request) -> Result<Response, TransportError> {
        self.read(&req.url, "GET", &req.headers, None)
            .map(|(status, body)| Response { status, body })
            .map_err(TransportError::Network)
    }
}

impl crate::google_docs::FormPost for HttpClient {
    fn post_form(&mut self, url: &str, fields: &[(&str, &str)]) -> Result<(u16, String), String> {
        let body = fields
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    crate::google_docs::encode(k),
                    crate::google_docs::encode(v)
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        self.read(
            url,
            "POST",
            &[(
                "Content-Type".into(),
                "application/x-www-form-urlencoded".into(),
            )],
            Some(&body),
        )
    }
}

impl crate::writes::WriteTransport for HttpClient {
    fn send(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let text = body.map(Value::to_string);
        let (status, answer) = self.read(url, method, headers, text.as_deref())?;
        Ok((status, serde_json::from_str(&answer).unwrap_or(Value::Null)))
    }
}

impl ApiTransport for HttpClient {
    fn post(
        &mut self,
        url: &str,
        headers: &[(String, String)],
        body: &Value,
    ) -> Result<(u16, Value), PostFailure> {
        self.post_with_hints(url, headers, body)
            .map(|(status, json, _)| (status, json))
    }

    fn post_with_hints(
        &mut self,
        url: &str,
        headers: &[(String, String)],
        body: &Value,
    ) -> Result<(u16, Value, crate::api_limits::Hints), PostFailure> {
        let (status, text, hints) =
            self.read_classified(url, "POST", headers, Some(&body.to_string()))?;
        let json = match serde_json::from_str(&text) {
            Ok(json) => json,
            // A refusal is not billed, whatever its body looks like.
            Err(_) if (400..500).contains(&status) => Value::Null,
            Err(_) => {
                return Err(PostFailure {
                    note: format!("the answer (status {status}) was not JSON"),
                    maybe_billed: true,
                })
            }
        };
        Ok((status, json, hints))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A local server that answers each connection with the next canned reply and records the request text.
    struct Server {
        port: u16,
        seen: Arc<Mutex<Vec<String>>>,
    }

    fn serve(replies: Vec<&'static str>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut c, _)) = listener.accept() else {
                    return;
                };
                let mut buf = vec![0u8; 16384];
                let mut got = Vec::new();
                loop {
                    let n = c.read(&mut buf).unwrap_or(0);
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).into_owned();
                    let done = text.split_once("\r\n\r\n").is_some_and(|(h, b)| {
                        let want = h
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        b.len() >= want
                    });
                    if n == 0 || done {
                        break;
                    }
                }
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&got).into_owned());
                if reply != "HANG" {
                    let _ = c.write_all(reply.as_bytes());
                } else {
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        });
        Server { port, seen }
    }

    fn local(port: u16, timeout: Duration) -> HttpClient {
        HttpClient::build(vec![format!("127.0.0.1:{port}")], false, timeout)
    }

    const OK_TEXT: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
    const OK_JSON: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}";
    const REDIRECT: &str = "HTTP/1.1 302 Found\r\nLocation: http://evil.test/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const NOT_JSON: &str = "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope";
    const DENIED: &str =
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 6\r\nConnection: close\r\n\r\ndenied";

    fn req(port: u16, path: &str) -> Request {
        Request {
            url: format!("http://127.0.0.1:{port}{path}"),
            headers: vec![("Authorization".into(), "Bearer TEST-TOKEN".into())],
        }
    }

    #[test]
    fn a_get_returns_the_status_and_body_and_sends_only_the_headers_it_was_given() {
        let s = serve(vec![OK_TEXT]);
        let mut c = local(s.port, Duration::from_secs(5));
        let r = c.get(&req(s.port, "/x")).unwrap();
        assert_eq!((r.status, r.body.as_str()), (200, "hello"));
        let seen = s.seen.lock().unwrap()[0].to_lowercase();
        assert!(seen.starts_with("get /x http/1.1"), "{seen}");
        assert!(seen.contains("authorization: bearer test-token"));
        assert!(!seen.contains("cookie:") && !seen.contains("x-forwarded"));
    }

    #[test]
    fn a_rate_limit_answer_keeps_its_retry_hints_and_a_plain_text_refusal_is_not_billed() {
        const LIMITED: &str = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 9\r\nX-RateLimit-Limit: 20\r\nX-RateLimit-Reset: 1794355230000\r\nContent-Type: application/json\r\nContent-Length: 44\r\nConnection: close\r\n\r\n{\"error\":{\"code\":429,\"message\":\"slow down\"}}";
        const HTML_REFUSAL: &str = "HTTP/1.1 429 Too Many Requests\r\nContent-Type: text/html\r\nContent-Length: 11\r\nConnection: close\r\n\r\n<p>busy</p>";
        let s = serve(vec![LIMITED, HTML_REFUSAL]);
        let mut c = local(s.port, Duration::from_secs(5));
        let url = format!("http://127.0.0.1:{}/v1", s.port);
        let (status, body, hints) = c
            .post_with_hints(&url, &[], &serde_json::json!({}))
            .unwrap();
        assert_eq!(status, 429);
        assert_eq!(body["error"]["message"], "slow down");
        assert_eq!(
            hints,
            crate::api_limits::Hints {
                retry_after: Some(9),
                limit: Some(20),
                reset_ms: Some(1_794_355_230_000)
            }
        );
        let (status, body, _) = c
            .post_with_hints(&url, &[], &serde_json::json!({}))
            .unwrap();
        assert_eq!((status, body), (429, Value::Null));
    }

    #[test]
    fn error_statuses_are_returned_to_the_caller_not_raised() {
        let s = serve(vec![DENIED]);
        let r = local(s.port, Duration::from_secs(5))
            .get(&req(s.port, "/x"))
            .unwrap();
        assert_eq!((r.status, r.body.as_str()), (401, "denied"));
    }

    #[test]
    fn a_redirect_is_never_followed() {
        let s = serve(vec![REDIRECT, OK_TEXT]);
        let r = local(s.port, Duration::from_secs(5))
            .get(&req(s.port, "/x"))
            .unwrap();
        assert_eq!(r.status, 302, "the 302 comes back as it is");
        assert_eq!(
            s.seen.lock().unwrap().len(),
            1,
            "no second request was made"
        );
    }

    #[test]
    fn the_only_patch_allowed_adds_to_the_end_of_a_notion_page() {
        assert!(patch_allowed(
            "https://api.notion.com/v1/blocks/01234567-89ab-cdef-0123-456789abcdef/children"
        ));
        for no in [
            "https://api.notion.com/v1/pages/01234567-89ab-cdef-0123-456789abcdef",
            "https://api.notion.com/v1/blocks/01234567-89ab-cdef-0123-456789abcdef",
            "http://api.notion.com/v1/blocks/01234567-89ab-cdef-0123-456789abcdef/children",
            "https://api.notion.com/v1/blocks/../pages/x/children",
            "https://api.github.com/v1/blocks/01234567-89ab-cdef-0123-456789abcdef/children",
            "https://api.notion.com/v1/blocks/01234567-89ab-cdef-0123-456789abcdef/children?x=1",
        ] {
            assert!(!patch_allowed(no), "{no}");
        }
    }

    #[test]
    fn a_write_request_can_put_and_an_empty_or_unreadable_answer_is_null_and_other_methods_are_refused(
    ) {
        use crate::writes::WriteTransport;
        let s = serve(vec![OK_JSON, NOT_JSON]);
        let mut c = local(s.port, Duration::from_secs(5));
        let url = format!("http://127.0.0.1:{}/repos/o/r/contents/a.txt", s.port);
        let (status, v) = c
            .send(
                "PUT",
                &url,
                &[("Authorization".into(), "Bearer K".into())],
                Some(&serde_json::json!({"content": "eA=="})),
            )
            .unwrap();
        assert_eq!((status, v["ok"].as_bool()), (200, Some(true)));
        let seen = s.seen.lock().unwrap()[0].clone();
        assert!(
            seen.starts_with("PUT /repos/o/r/contents/a.txt HTTP/1.1"),
            "{seen}"
        );
        assert!(seen.ends_with("{\"content\":\"eA==\"}"), "{seen}");
        let (status, v) = c.send("GET", &url, &[], None).unwrap();
        assert_eq!((status, v), (200, serde_json::Value::Null));
        assert!(
            c.send("PATCH", &url, &[], Some(&serde_json::json!({})))
                .is_err(),
            "PATCH only to a Notion page's children"
        );
        for method in ["DELETE", "PATCH", "put"] {
            assert!(c.send(method, &url, &[], None).is_err(), "{method}");
        }
        assert_eq!(
            s.seen.lock().unwrap().len(),
            2,
            "the refused methods sent nothing"
        );
    }

    #[test]
    fn a_post_sends_the_json_body_and_returns_parsed_json() {
        let s = serve(vec![OK_JSON]);
        let mut c = local(s.port, Duration::from_secs(5));
        let (status, v) = c
            .post(
                &format!("http://127.0.0.1:{}/v1/x", s.port),
                &[
                    ("Content-Type".into(), "application/json".into()),
                    ("Authorization".into(), "Bearer K".into()),
                ],
                &serde_json::json!({"a": 1}),
            )
            .unwrap();
        assert_eq!((status, v["ok"].as_bool()), (200, Some(true)));
        let seen = s.seen.lock().unwrap()[0].clone();
        assert!(seen.starts_with("POST /v1/x HTTP/1.1"), "{seen}");
        assert!(seen.ends_with("{\"a\":1}"), "{seen}");
        assert!(seen
            .to_lowercase()
            .contains("content-type: application/json"));
    }

    #[test]
    fn an_answer_that_is_not_json_is_an_error_naming_the_status() {
        let s = serve(vec![NOT_JSON]);
        let e = local(s.port, Duration::from_secs(5))
            .post(
                &format!("http://127.0.0.1:{}/x", s.port),
                &[],
                &serde_json::json!({}),
            )
            .unwrap_err();
        assert!(
            e.note.contains("not JSON") && e.note.contains("200"),
            "{e:?}"
        );
        assert!(e.maybe_billed, "the answer arrived, so the call was made");
    }

    #[test]
    fn urls_for_other_hosts_are_refused_before_any_connection() {
        let s = serve(vec![OK_TEXT]);
        let mut c = HttpClient::new(&["api.github.com"]);
        for url in [
            "https://evil.test/x",
            "https://api.github.com.evil.test/x",
            "https://api.github.com@evil.test/x",
            "https://evil.test@api.github.com/x",
            "https://api.github.com:8443/x",
            "http://api.github.com/x",
            "ftp://api.github.com/x",
            "//api.github.com/x",
            "https://API.GITHUB.COM/x",
            "https://api.github.com\\@evil.test/x",
            "https:///x",
            "",
        ] {
            let r = c.get(&Request {
                url: url.into(),
                headers: vec![],
            });
            assert!(
                matches!(r, Err(TransportError::Network(_))),
                "{url:?} -> {r:?}"
            );
        }
        assert!(s.seen.lock().unwrap().is_empty());
        assert_eq!(
            checked_host(
                "https://api.github.com/repos/a/b",
                &["api.github.com".to_string()],
                true
            ),
            Ok("api.github.com")
        );
        assert_eq!(
            checked_host(
                "https://api.github.com",
                &["api.github.com".to_string()],
                true
            ),
            Ok("api.github.com")
        );
    }

    #[test]
    fn a_production_client_will_not_speak_plain_http_even_to_an_allowed_host() {
        let mut c = HttpClient::new(&["127.0.0.1"]);
        let r = c.get(&Request {
            url: "http://127.0.0.1/x".into(),
            headers: vec![],
        });
        assert!(matches!(r, Err(TransportError::Network(m)) if m.contains("https")));
    }

    #[test]
    fn an_answer_over_the_size_limit_is_an_error() {
        let big: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                MAX_BODY + 10,
                "x".repeat((MAX_BODY + 10) as usize)
            )
            .into_boxed_str(),
        );
        let s = serve(vec![big]);
        let r = local(s.port, Duration::from_secs(20)).get(&req(s.port, "/x"));
        assert!(
            matches!(r, Err(TransportError::Network(_))),
            "{:?}",
            r.map(|x| x.body.len())
        );
    }

    #[test]
    fn a_server_that_never_answers_is_given_up_on() {
        let s = serve(vec!["HANG"]);
        let started = std::time::Instant::now();
        let r = local(s.port, Duration::from_millis(700)).get(&req(s.port, "/x"));
        assert!(matches!(r, Err(TransportError::Network(_))));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
    }
}
