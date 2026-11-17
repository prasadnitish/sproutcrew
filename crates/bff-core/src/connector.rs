//! Read-only connectors (GitHub, Notion, Drive) for source material. The transport can only GET, a
//! credential is used only if every scope it carries is on the connector's read allowlist, an answer of
//! 401 or 403 marks that connector blocked until a person unblocks it, and one connector failing blocks
//! only the tasks that need it. Everything here runs against injected fakes: no key is read and no
//! network call is made by this module. What a connector returns is untrusted text.

use crate::capability::{Origin, Tainted};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    Network(String),
}

/// There is no method argument: this trait can only read.
pub trait Transport {
    fn get(&mut self, req: &Request) -> Result<Response, TransportError>;
}

#[derive(Clone)]
pub struct Credential {
    token: String,
    pub scopes: Vec<String>,
}

impl Credential {
    pub fn new(token: &str, scopes: &[&str]) -> Credential {
        Credential {
            token: token.to_string(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Credential {
    /// The secret itself. Used only at the moment a request is sent.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credential {{ token: ***, scopes: {:?} }}", self.scopes)
    }
}

/// Where credentials come from. Looked up by name when a request is made, never held by a connector.
pub trait Credentials {
    fn get(&self, name: &str) -> Option<Credential>;
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub id: &'static str,
    /// The only host this connector talks to.
    pub host: &'static str,
    pub credential: &'static str,
    /// The scopes a credential may carry. Anything else is treated as able to write.
    pub read_scopes: &'static [&'static str],
    /// Headers every request to this host needs, such as an API version.
    pub extra_headers: &'static [(&'static str, &'static str)],
}

/// GitHub: a fine-grained token with Contents: read and Metadata: read.
pub fn github() -> Spec {
    Spec {
        id: "github",
        host: "api.github.com",
        credential: "github",
        read_scopes: &["contents:read", "metadata:read"],
        extra_headers: &[
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ],
    }
}

/// Notion: an internal integration with the Read content capability only.
pub fn notion() -> Spec {
    Spec {
        id: "notion",
        host: "api.notion.com",
        credential: "notion",
        read_scopes: &["read_content"],
        extra_headers: &[("Notion-Version", "2026-03-11")],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    UnknownConnector(String),
    /// The connector is blocked and says why. Nothing is sent.
    Blocked(String),
    NoCredential,
    /// The credential carries this scope, which is not on the read allowlist.
    NotReadOnly(String),
    BadPath(String),
    /// 401 or 403. The connector is now blocked.
    Revoked(u16),
    RateLimited,
    Unavailable(u16),
    NotFound,
    Network(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Healthy,
    Blocked(String),
}

#[derive(Default)]
pub struct Connectors {
    specs: BTreeMap<&'static str, Spec>,
    state: BTreeMap<&'static str, State>,
}

impl Connectors {
    pub fn register(&mut self, spec: Spec) {
        self.state.insert(spec.id, State::Healthy);
        self.specs.insert(spec.id, spec);
    }

    pub fn state(&self, id: &str) -> Option<&State> {
        self.state.get(id)
    }

    /// A person fixed the credential. Clears the block.
    pub fn unblock(&mut self, id: &str) -> bool {
        match self.state.get_mut(id) {
            Some(s) => {
                *s = State::Healthy;
                true
            }
            None => false,
        }
    }

    fn block(&mut self, id: &'static str, why: String) {
        self.state.insert(id, State::Blocked(why));
    }

    /// Reads `path` (starting with `/`) from the connector's own host.
    pub fn read(
        &mut self,
        creds: &dyn Credentials,
        transport: &mut dyn Transport,
        id: &str,
        path: &str,
    ) -> Result<Tainted<String>, Failure> {
        let spec = self
            .specs
            .get(id)
            .cloned()
            .ok_or_else(|| Failure::UnknownConnector(id.to_string()))?;
        if let Some(State::Blocked(why)) = self.state.get(spec.id) {
            return Err(Failure::Blocked(why.clone()));
        }
        check_path(path)?;
        let cred = creds.get(spec.credential).ok_or(Failure::NoCredential)?;
        if let Some(bad) = cred
            .scopes
            .iter()
            .find(|s| !spec.read_scopes.contains(&s.as_str()))
        {
            self.block(
                spec.id,
                format!(
                    "the credential carries {bad}, which can write. Use a read-only credential"
                ),
            );
            return Err(Failure::NotReadOnly(bad.clone()));
        }
        let req = Request {
            url: format!("https://{}{}", spec.host, path),
            headers: {
                let mut h = vec![(
                    "Authorization".to_string(),
                    format!("Bearer {}", cred.token),
                )];
                if !spec
                    .extra_headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("accept"))
                {
                    h.push((
                        "Accept".to_string(),
                        "application/json, text/plain".to_string(),
                    ));
                }
                h.extend(
                    spec.extra_headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string())),
                );
                h
            },
        };
        let resp = transport
            .get(&req)
            .map_err(|TransportError::Network(m)| Failure::Network(m))?;
        match resp.status {
            200..=299 => Ok(Tainted::new(resp.body, Origin::Web)),
            s @ (401 | 403) => {
                self.block(
                    spec.id,
                    format!("the credential was refused with status {s}. Replace it and unblock"),
                );
                Err(Failure::Revoked(s))
            }
            404 => Err(Failure::NotFound),
            429 => Err(Failure::RateLimited),
            s => Err(Failure::Unavailable(s)),
        }
    }

    /// For each task that needs a blocked connector, why it cannot run. Other tasks are not listed.
    pub fn blocked_tasks<'a>(&self, needs: &[(&'a str, &[&str])]) -> Vec<(&'a str, String)> {
        needs
            .iter()
            .filter_map(|(task, ids)| {
                ids.iter().find_map(|id| match self.state.get(id) {
                    Some(State::Blocked(why)) => Some((*task, format!("{id} is blocked: {why}"))),
                    _ => None,
                })
            })
            .collect()
    }
}

/// A path on the connector's host: starts with one `/`, no `..`, no empty segment, no scheme, no fragment,
/// and only characters that cannot break a request line.
fn check_path(p: &str) -> Result<(), Failure> {
    let bad = || Failure::BadPath(p.chars().take(40).collect());
    if !p.starts_with('/')
        || p.starts_with("//")
        || p.contains("://")
        || p.contains("..")
        || p.contains("//")
    {
        return Err(bad());
    }
    if !p
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/-_.~%:@,=&?+".contains(c))
    {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        responses: Vec<Result<Response, TransportError>>,
        seen: Vec<Request>,
    }
    impl Fake {
        fn new(responses: Vec<Result<Response, TransportError>>) -> Fake {
            Fake {
                responses,
                seen: vec![],
            }
        }
        fn ok(body: &str) -> Result<Response, TransportError> {
            Ok(Response {
                status: 200,
                body: body.into(),
            })
        }
        fn status(code: u16) -> Result<Response, TransportError> {
            Ok(Response {
                status: code,
                body: "denied".into(),
            })
        }
    }
    impl Transport for Fake {
        fn get(&mut self, req: &Request) -> Result<Response, TransportError> {
            self.seen.push(req.clone());
            if self.responses.is_empty() {
                return Err(TransportError::Network("no more responses".into()));
            }
            self.responses.remove(0)
        }
    }

    struct Creds(BTreeMap<String, Credential>);
    impl Credentials for Creds {
        fn get(&self, name: &str) -> Option<Credential> {
            self.0.get(name).cloned()
        }
    }

    fn creds(entries: &[(&str, &[&str])]) -> Creds {
        Creds(
            entries
                .iter()
                .map(|(n, s)| (n.to_string(), Credential::new("tok-SECRET-123", s)))
                .collect(),
        )
    }

    fn connectors() -> Connectors {
        let mut c = Connectors::default();
        c.register(Spec {
            id: "github",
            host: "api.github.example",
            credential: "gh",
            read_scopes: &["contents:read", "metadata:read"],
            extra_headers: &[],
        });
        c.register(Spec {
            id: "notion",
            host: "api.notion.example",
            credential: "nt",
            read_scopes: &["read_content"],
            extra_headers: &[],
        });
        c
    }

    fn all_creds() -> Creds {
        creds(&[
            ("gh", &["contents:read", "metadata:read"]),
            ("nt", &["read_content"]),
        ])
    }

    #[test]
    fn a_read_goes_to_the_connectors_own_host_with_the_credential_and_returns_untrusted_text() {
        let mut c = connectors();
        let mut t = Fake::new(vec![Fake::ok("# readme")]);
        let got = c
            .read(&all_creds(), &mut t, "github", "/repos/acme/app/readme")
            .unwrap();
        assert_eq!(got.display(), "# readme");
        assert!(
            got.is_tainted(),
            "what a connector returns is data, not instructions"
        );
        assert_eq!(
            t.seen[0].url,
            "https://api.github.example/repos/acme/app/readme"
        );
        assert!(t.seen[0]
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer tok-SECRET-123"));
        assert_eq!(c.state("github"), Some(&State::Healthy));
    }

    #[test]
    fn a_path_cannot_name_another_host_or_climb_out() {
        let mut c = connectors();
        for bad in [
            "https://evil.example/x",
            "//evil.example/x",
            "repos/no-slash",
            "/a/../b",
            "/a//b",
            "/a b",
            "/a\r\nHost: x",
            "",
            "/x#frag",
            "/\u{0}",
        ] {
            let mut t = Fake::new(vec![Fake::ok("x")]);
            let r = c.read(&all_creds(), &mut t, "github", bad);
            assert!(matches!(r, Err(Failure::BadPath(_))), "{bad:?} -> {r:?}");
            assert!(t.seen.is_empty(), "{bad:?} must not be sent");
        }
    }

    #[test]
    fn a_credential_that_can_write_is_refused_and_blocks_the_connector() {
        let mut c = connectors();
        let writer = creds(&[
            ("gh", &["contents:read", "contents:write"]),
            ("nt", &["read_content"]),
        ]);
        let mut t = Fake::new(vec![Fake::ok("x")]);
        let r = c.read(&writer, &mut t, "github", "/repos/a/b");
        assert_eq!(r, Err(Failure::NotReadOnly("contents:write".into())));
        assert!(t.seen.is_empty(), "the credential is not used at all");
        assert!(
            matches!(c.state("github"), Some(State::Blocked(m)) if m.contains("contents:write"))
        );
        // Even with a good credential afterwards, it stays blocked until a person unblocks it.
        let r = c.read(
            &all_creds(),
            &mut Fake::new(vec![Fake::ok("x")]),
            "github",
            "/repos/a/b",
        );
        assert!(matches!(r, Err(Failure::Blocked(_))));
        assert!(c.unblock("github"));
        assert!(c
            .read(
                &all_creds(),
                &mut Fake::new(vec![Fake::ok("x")]),
                "github",
                "/repos/a/b"
            )
            .is_ok());
    }

    #[test]
    fn a_classic_all_access_scope_counts_as_write() {
        let mut c = connectors();
        let broad = creds(&[("gh", &["repo"]), ("nt", &["read_content"])]);
        let r = c.read(&broad, &mut Fake::new(vec![]), "github", "/x");
        assert_eq!(r, Err(Failure::NotReadOnly("repo".into())));
    }

    #[test]
    fn a_revoked_credential_blocks_that_connector_only_and_the_credential_never_appears_in_errors()
    {
        let mut c = connectors();
        let mut t = Fake::new(vec![Fake::status(401)]);
        let r = c.read(&all_creds(), &mut t, "github", "/repos/a/b");
        assert_eq!(r, Err(Failure::Revoked(401)));
        assert!(matches!(c.state("github"), Some(State::Blocked(m)) if m.contains("401")));
        assert!(!format!("{r:?}{:?}", c.state("github")).contains("SECRET"));
        // The other connector keeps working.
        assert!(c
            .read(
                &all_creds(),
                &mut Fake::new(vec![Fake::ok("page")]),
                "notion",
                "/v1/pages/1"
            )
            .is_ok());
        // 403 blocks too.
        let mut c2 = connectors();
        assert_eq!(
            c2.read(
                &all_creds(),
                &mut Fake::new(vec![Fake::status(403)]),
                "notion",
                "/v1/pages/1"
            ),
            Err(Failure::Revoked(403))
        );
    }

    #[test]
    fn transient_failures_do_not_block() {
        let mut c = connectors();
        for (resp, want) in [
            (Fake::status(429), Failure::RateLimited),
            (Fake::status(503), Failure::Unavailable(503)),
            (Fake::status(500), Failure::Unavailable(500)),
            (Fake::status(404), Failure::NotFound),
            (
                Err(TransportError::Network("reset".into())),
                Failure::Network("reset".into()),
            ),
        ] {
            let r = c.read(&all_creds(), &mut Fake::new(vec![resp]), "github", "/x");
            assert_eq!(r, Err(want));
            assert_eq!(c.state("github"), Some(&State::Healthy));
        }
    }

    #[test]
    fn missing_credentials_and_unknown_connectors_are_named() {
        let mut c = connectors();
        let none = Creds(BTreeMap::new());
        assert_eq!(
            c.read(&none, &mut Fake::new(vec![]), "github", "/x"),
            Err(Failure::NoCredential)
        );
        assert_eq!(
            c.read(&none, &mut Fake::new(vec![]), "gitlab", "/x"),
            Err(Failure::UnknownConnector("gitlab".into()))
        );
        assert_eq!(c.state("gitlab"), None);
        assert!(!c.unblock("gitlab"));
    }

    #[test]
    fn one_connector_failing_blocks_only_the_tasks_that_need_it() {
        let mut c = connectors();
        let _ = c.read(
            &all_creds(),
            &mut Fake::new(vec![Fake::status(401)]),
            "github",
            "/x",
        );
        let needs: Vec<(&str, &[&str])> = vec![
            ("t1", &["github"]),
            ("t2", &["notion"]),
            ("t3", &[]),
            ("t4", &["notion", "github"]),
        ];
        let blocked = c.blocked_tasks(&needs);
        assert_eq!(
            blocked.iter().map(|b| b.0).collect::<Vec<_>>(),
            vec!["t1", "t4"]
        );
        assert!(blocked[0].1.contains("github") && blocked[0].1.contains("401"));
        c.unblock("github");
        assert!(c.blocked_tasks(&needs).is_empty());
    }

    #[test]
    fn the_credential_debug_output_hides_the_token() {
        let c = Credential::new("tok-SECRET-123", &["read"]);
        let shown = format!("{c:?}");
        assert!(!shown.contains("SECRET") && shown.contains("***"));
    }

    #[test]
    fn the_github_and_notion_specs_carry_their_hosts_scopes_and_version_headers() {
        let (g, n) = (github(), notion());
        assert_eq!(
            (g.host, g.credential, n.host, n.credential),
            ("api.github.com", "github", "api.notion.com", "notion")
        );
        assert!(
            g.read_scopes.iter().all(|s| s.ends_with(":read")),
            "{:?}",
            g.read_scopes
        );
        assert_eq!(n.read_scopes, &["read_content"]);
        let mut c = Connectors::default();
        c.register(g);
        c.register(n);
        let creds = creds(&[
            ("github", &["contents:read"]),
            ("notion", &["read_content"]),
        ]);
        let mut t = Fake::new(vec![Fake::ok("a"), Fake::ok("b")]);
        c.read(&creds, &mut t, "github", "/repos/a/b").unwrap();
        c.read(&creds, &mut t, "notion", "/v1/pages/1").unwrap();
        let has =
            |i: usize, k: &str, v: &str| t.seen[i].headers.iter().any(|(a, b)| a == k && b == v);
        assert!(
            has(0, "X-GitHub-Api-Version", "2022-11-28")
                && has(0, "Accept", "application/vnd.github+json")
        );
        assert!(
            has(1, "Notion-Version", "2026-03-11")
                && has(1, "Accept", "application/json, text/plain")
        );
        assert_eq!(
            t.seen[0]
                .headers
                .iter()
                .filter(|(k, _)| k == "Accept")
                .count(),
            1,
            "one Accept header"
        );
        assert!(
            t.seen[0].url.starts_with("https://api.github.com/")
                && t.seen[1].url.starts_with("https://api.notion.com/")
        );
    }
}
