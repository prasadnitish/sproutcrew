//! "Your own AI service": any service that answers in the common chat format first made by OpenAI, such as
//! Together, Groq or Fireworks on the internet, or Ollama and LM Studio running on this Mac. The person types its
//! address, and briefs, sources and results go there, so the address is checked strictly here:
//!
//! - HTTPS only. The one exception is this Mac itself (`localhost` or `127.0.0.1` with a port), where apps such
//!   as Ollama answer on plain `http://localhost:11434`.
//! - No user name or password inside the address, no `?` or `#` parts, no spaces.
//! - The address is saved in the request limits file, which every plan approval covers, so changing it later
//!   needs a new approval. Redirects are never followed by the client.
//!
//! Prices are not known, so the route counts tokens under the token limit, and cost shows as unknown.

/// A checked address: `base` is what requests are built from (no slash at the end), `authority` is the host and
/// port the client may talk to, and `local` says it is this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub base: String,
    pub authority: String,
    pub local: bool,
}

/// Endings people often paste by mistake, which the app adds itself.
const ENDINGS: &[&str] = &["/chat/completions", "/completions", "/models"];

fn bad(why: &str) -> String {
    format!("{why} {EXAMPLES}")
}

/// Shown with every address problem, because the people typing it are rarely developers.
pub const EXAMPLES: &str = "It usually looks like https://api.together.xyz/v1 or, for an app on this Mac, http://localhost:11434/v1. The service's own help pages call it the \"base URL\" or \"OpenAI-compatible endpoint\".";

/// Checks a typed address and returns it ready to use, or a plain sentence saying what to fix.
pub fn check_address(input: &str) -> Result<Address, String> {
    let mut t = input.trim().to_string();
    if t.is_empty() {
        return Err(bad("Type the service's address."));
    }
    if t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("The address has a space in it."));
    }
    let (rest, secure) = if let Some(r) = t.strip_prefix("https://") {
        (r.to_string(), true)
    } else if let Some(r) = t.strip_prefix("http://") {
        (r.to_string(), false)
    } else {
        return Err(bad("The address must start with https://."));
    };
    if rest.contains(['?', '#', '@', '\\']) {
        return Err(bad(
            "Use only the address, with no user name, password, ? or # parts.",
        ));
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (rest[..i].to_ascii_lowercase(), rest[i..].to_string()),
        None => (rest.to_ascii_lowercase(), String::new()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), Some(p.to_string())),
        None => (authority.clone(), None),
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !host_ok {
        return Err(bad("The part after https:// is not a usable server name."));
    }
    if let Some(p) = &port {
        if p.parse::<u16>().map_or(true, |n| n == 0) {
            return Err(bad("The number after the colon is not a usable port."));
        }
    }
    let local = host == "localhost" || host == "127.0.0.1";
    if !secure && !(local && port.is_some()) {
        return Err(bad(
            "The address must start with https://, so what is sent is encrypted. Plain http:// is allowed only for an app on this Mac, such as http://localhost:11434.",
        ));
    }
    if path
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || "/._-~".contains(c)))
    {
        return Err(bad("The address has characters that are not allowed."));
    }
    t = format!(
        "{}://{authority}{path}",
        if secure { "https" } else { "http" }
    );
    while t.ends_with('/') {
        t.pop();
    }
    for e in ENDINGS {
        if let Some(s) = t.strip_suffix(e) {
            t = s.to_string();
        }
    }
    Ok(Address {
        base: t,
        authority,
        local,
    })
}

/// A model name as the service spells it, such as `llama3.3`, `qwen3:8b` or `meta-llama/Llama-4-Scout`.
pub fn model_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/@+-".contains(c))
}

/// Asks the service for its list of models, which costs nothing, to confirm the address and key work before they
/// are saved. Returns the model names it lists (at most 50), so the person can pick one. The key is sent only to
/// this address and is never returned.
pub fn check_service(
    a: &Address,
    key: Option<&str>,
    transport: &mut dyn crate::connector::Transport,
) -> Result<Vec<String>, String> {
    let mut headers = vec![];
    if let Some(k) = key {
        headers.push(("Authorization".to_string(), format!("Bearer {k}")));
    }
    let req = crate::connector::Request {
        url: format!("{}/models", a.base),
        headers,
    };
    match transport.get(&req) {
        Ok(r) if (200..300).contains(&r.status) => {
            let v: serde_json::Value = serde_json::from_str(&r.body).map_err(|_| {
                "The address answered, but not like an AI service. Check it ends where the service's help says, often with /v1.".to_string()
            })?;
            let names = v["data"]
                .as_array()
                .or_else(|| v["models"].as_array())
                .map(|l| {
                    l.iter()
                        .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
                        .filter(|n| model_ok(n))
                        .take(50)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            Ok(names)
        }
        Ok(r) if r.status == 401 || r.status == 403 => Err(if key.is_some() {
            "The service refused this key. Check you copied all of it.".into()
        } else {
            "The service needs a key. Paste the key from the service's website.".into()
        }),
        Ok(r) if r.status == 404 => Err(format!(
            "The address answered, but has no AI service there (status 404). {EXAMPLES}"
        )),
        Ok(r) => Err(format!(
            "The service could not be checked right now (status {}). Try again in a minute.",
            r.status
        )),
        Err(crate::connector::TransportError::Network(_)) => Err(if a.local {
            "Nothing answered at that address on this Mac. Check the app (such as Ollama or LM Studio) is open and its server is switched on, and that the port number matches.".into()
        } else {
            "That address could not be reached. Check it is typed correctly and that this Mac is online.".into()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Answer(u16, &'static str, Vec<crate::connector::Request>);
    impl crate::connector::Transport for Answer {
        fn get(
            &mut self,
            req: &crate::connector::Request,
        ) -> Result<crate::connector::Response, crate::connector::TransportError> {
            self.2.push(req.clone());
            Ok(crate::connector::Response {
                status: self.0,
                body: self.1.into(),
            })
        }
    }

    #[test]
    fn the_check_lists_models_at_the_address_and_sends_the_key_only_when_there_is_one() {
        let a = check_address("http://localhost:11434/v1").unwrap();
        let mut t = Answer(
            200,
            r#"{"data": [{"id": "qwen3:8b"}, {"id": "bad name"}]}"#,
            vec![],
        );
        assert_eq!(check_service(&a, None, &mut t).unwrap(), ["qwen3:8b"]);
        assert_eq!(t.2[0].url, "http://localhost:11434/v1/models");
        assert!(t.2[0].headers.is_empty());
        let mut t = Answer(401, "{}", vec![]);
        assert!(check_service(&a, None, &mut t)
            .unwrap_err()
            .contains("needs a key"));
        assert!(check_service(&a, Some("k"), &mut t)
            .unwrap_err()
            .contains("refused this key"));
        assert_eq!(t.2[1].headers[0].1, "Bearer k");
        let mut t = Answer(200, "<html>", vec![]);
        assert!(check_service(&a, None, &mut t).unwrap_err().contains("/v1"));
    }

    #[test]
    fn https_addresses_are_accepted_and_tidied() {
        let a = check_address(" https://API.Together.xyz/v1/ ").unwrap();
        assert_eq!(a.base, "https://api.together.xyz/v1");
        assert_eq!(a.authority, "api.together.xyz");
        assert!(!a.local);
        // A pasted full endpoint is cut back to the base, since the app adds the ending itself.
        assert_eq!(
            check_address("https://api.groq.com/openai/v1/chat/completions")
                .unwrap()
                .base,
            "https://api.groq.com/openai/v1"
        );
        assert_eq!(
            check_address("https://ai.example.com:8443")
                .unwrap()
                .authority,
            "ai.example.com:8443"
        );
    }

    #[test]
    fn plain_http_is_allowed_only_for_this_mac_with_a_port() {
        let a = check_address("http://localhost:11434/v1").unwrap();
        assert!(a.local);
        assert_eq!(a.authority, "localhost:11434");
        assert!(check_address("http://127.0.0.1:1234/v1").unwrap().local);
        for no in [
            "http://localhost/v1",
            "http://example.com/v1",
            "http://192.168.1.20:8080/v1",
        ] {
            let e = check_address(no).unwrap_err();
            assert!(
                e.contains("https://") && e.contains("localhost:11434"),
                "{no}: {e}"
            );
        }
        assert!(check_address("https://localhost:8443/v1").unwrap().local);
    }

    #[test]
    fn addresses_that_hide_something_or_are_not_addresses_are_refused_with_an_example() {
        for no in [
            "",
            "api.together.xyz/v1",
            "ftp://x.example",
            "https://user:pw@x.example/v1",
            "https://x.example/v1?key=1",
            "https://x.example/v1#a",
            "https://x .example",
            "https://-x.example",
            "https://x.example:0",
            "https://x.example:99999",
            "https://x..example",
            "https://x.example/v1%2e",
        ] {
            let e = check_address(no).unwrap_err();
            assert!(e.contains("base URL"), "{no}: {e}");
        }
    }

    #[test]
    fn model_names_are_checked_by_shape() {
        for ok in [
            "llama3.3",
            "qwen3:8b",
            "meta-llama/Llama-4-Scout-17B",
            "gpt-oss-20b",
        ] {
            assert!(model_ok(ok), "{ok}");
        }
        for no in ["", "a b", "x;rm", &"m".repeat(129)] {
            assert!(!model_ok(no), "{no}");
        }
    }
}
