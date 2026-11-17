//! Credentials from the macOS Keychain, through `/usr/bin/security`. Items live under the service name `sproutcrew`,
//! one per account (`openai-api`, `github`, ...). The secret is the item's password, and the scopes a
//! credential is meant to carry are written in its comment field, so a connector can refuse a credential
//! that was given more than read access. Reading the secret happens only when a request is about to be
//! sent. A presence check reads the item's attributes and never its password. A key the person pastes in Settings
//! is saved through `security`'s standard input, so it never appears in a command line another program can list.

use crate::connector::{Credential, Credentials};
use std::process::{Command, Stdio};

/// V2's own service. V1 kept its items under `bff`, and V2 never reads them.
pub const SERVICE: &str = "sproutcrew";
const SECURITY: &str = "/usr/bin/security";

pub trait Runner: Send {
    /// Runs `security` with these arguments. Returns the exit code and standard output.
    fn run(&self, args: &[&str]) -> Result<(i32, String), String>;
    /// Runs `security -i` with these commands on its standard input. Standard output is discarded.
    fn run_input(&self, commands: &str) -> Result<i32, String>;
}

pub struct RealSecurity;

impl Runner for RealSecurity {
    fn run(&self, args: &[&str]) -> Result<(i32, String), String> {
        let out = Command::new(SECURITY)
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run security: {e}"))?;
        Ok((
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ))
    }

    fn run_input(&self, commands: &str) -> Result<i32, String> {
        use std::io::Write;
        let mut child = Command::new(SECURITY)
            .arg("-i")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run security: {e}"))?;
        let written = child
            .stdin
            .take()
            .map(|mut i| i.write_all(commands.as_bytes()))
            .unwrap_or_else(|| Err(std::io::Error::other("no input")));
        let status = child.wait().map_err(|e| e.to_string())?;
        written.map_err(|e| format!("cannot pass the key to security: {e}"))?;
        Ok(status.code().unwrap_or(-1))
    }
}

/// A key a person may paste: letters, digits, and `.`, `_`, `-`, as every supported provider issues them. Anything
/// else is refused, which also keeps it from being read as more than one value by `security -i`.
pub fn valid_secret(secret: &str) -> bool {
    (16..=512).contains(&secret.len())
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub struct Keychain {
    runner: Box<dyn Runner>,
}

/// Account names are lowercase letters, digits, and hyphens.
pub fn valid_account(name: &str) -> bool {
    !name.starts_with('-')
        && !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The comment field from `security find-generic-password` output: `"icmt"<blob>="..."`.
pub fn scopes_from_attributes(dump: &str) -> Vec<String> {
    dump.lines()
        .find_map(|l| l.trim().strip_prefix("\"icmt\"<blob>="))
        .and_then(|v| v.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
        .map(|c| {
            c.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl Keychain {
    pub fn new() -> Keychain {
        Keychain {
            runner: Box::new(RealSecurity),
        }
    }

    pub fn with_runner(runner: Box<dyn Runner>) -> Keychain {
        Keychain { runner }
    }

    /// Whether an item exists. Reads attributes only, so no password is read and no prompt appears.
    pub fn present(&self, account: &str) -> bool {
        valid_account(account)
            && matches!(
                self.runner
                    .run(&["find-generic-password", "-s", SERVICE, "-a", account]),
                Ok((0, _))
            )
    }

    /// Saves a key the person pasted, replacing any earlier one, and checks it is there afterwards.
    pub fn save(&self, account: &str, secret: &str) -> Result<(), String> {
        if !valid_account(account) {
            return Err("that is not a key name SproutCrew uses".into());
        }
        if !valid_secret(secret) {
            return Err(
                "That does not look like an API key. Paste the whole key, with no spaces.".into(),
            );
        }
        let commands = format!("add-generic-password -U -s {SERVICE} -a {account} -w {secret}\n");
        self.runner.run_input(&commands)?;
        if self.present(account) {
            Ok(())
        } else {
            Err("The Keychain did not keep the key.".into())
        }
    }

    /// Deletes the saved key. The key still works at the provider until it is revoked there.
    pub fn remove(&self, account: &str) -> Result<(), String> {
        if !valid_account(account) {
            return Err("that is not a key name SproutCrew uses".into());
        }
        match self
            .runner
            .run(&["delete-generic-password", "-s", SERVICE, "-a", account])
        {
            Ok((0 | 44, _)) => Ok(()),
            Ok((code, _)) => Err(format!(
                "The Keychain could not remove the key (code {code})."
            )),
            Err(e) => Err(e),
        }
    }

    /// The scopes written in the item's comment, if it exists.
    pub fn declared_scopes(&self, account: &str) -> Option<Vec<String>> {
        if !valid_account(account) {
            return None;
        }
        match self
            .runner
            .run(&["find-generic-password", "-s", SERVICE, "-a", account])
        {
            Ok((0, dump)) => Some(scopes_from_attributes(&dump)),
            _ => None,
        }
    }
}

impl Default for Keychain {
    fn default() -> Self {
        Keychain::new()
    }
}

impl Credentials for Keychain {
    fn get(&self, name: &str) -> Option<Credential> {
        let scopes = self.declared_scopes(name)?;
        let (code, out) = self
            .runner
            .run(&["find-generic-password", "-s", SERVICE, "-a", name, "-w"])
            .ok()?;
        let token = out.trim_end_matches(['\n', '\r']);
        if code != 0 || token.is_empty() {
            return None;
        }
        let scopes: Vec<&str> = scopes.iter().map(String::as_str).collect();
        Some(Credential::new(token, &scopes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const DUMP: &str = "keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\nversion: 512\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"github\"\n    \"icmt\"<blob>=\"contents:read, metadata:read\"\n    \"svce\"<blob>=\"bff\"\n";

    struct Fake {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
        items: Vec<(&'static str, &'static str, &'static str)>,
    }
    impl Runner for Fake {
        fn run_input(&self, commands: &str) -> Result<i32, String> {
            self.calls
                .lock()
                .unwrap()
                .push(vec!["-i".into(), commands.into()]);
            Ok(0)
        }

        fn run(&self, args: &[&str]) -> Result<(i32, String), String> {
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect());
            let account = args
                .iter()
                .position(|a| *a == "-a")
                .map(|i| args[i + 1])
                .unwrap_or("");
            let Some((_, pw, comment)) = self.items.iter().find(|(a, _, _)| *a == account) else {
                return Ok((44, String::new()));
            };
            if args.contains(&"-w") {
                Ok((0, format!("{pw}\n")))
            } else {
                Ok((0, DUMP.replace("contents:read, metadata:read", comment)))
            }
        }
    }

    fn keychain(
        items: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (Keychain, Arc<Mutex<Vec<Vec<String>>>>) {
        let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
        (
            Keychain::with_runner(Box::new(Fake {
                calls: calls.clone(),
                items,
            })),
            calls,
        )
    }

    #[test]
    fn a_pasted_key_goes_through_standard_input_and_a_bad_one_is_refused() {
        let (k, calls) = keychain(vec![("openai-api", "sk-abc", "")]);
        k.save("openai-api", "sk-proj-AbC_12.3-xyzxyzxyz").unwrap();
        let c = calls.lock().unwrap().clone();
        assert_eq!(
            c[0][0], "-i",
            "the key is passed as input, not as an argument"
        );
        assert_eq!(
            c[0][1],
            "add-generic-password -U -s sproutcrew -a openai-api -w sk-proj-AbC_12.3-xyzxyzxyz\n"
        );
        for bad in [
            "short",
            "has a space in it 1234",
            "sk-abc\nadd-generic-password",
            "sk-'quoted'-1234567890",
        ] {
            assert!(k.save("openai-api", bad).is_err(), "{bad}");
        }
        assert!(k.save("-s", "sk-proj-AbC_12.3-xyzxyzxyz").is_err());
        let (k, _) = keychain(vec![]);
        assert!(
            k.save("anthropic-api", "sk-ant-api03-abcdefghijklmnop")
                .is_err(),
            "a save that left nothing behind is reported"
        );
        assert!(
            k.remove("anthropic-api").is_ok(),
            "removing a missing key is fine"
        );
    }

    #[test]
    fn the_scopes_come_from_the_comment_field() {
        assert_eq!(
            scopes_from_attributes(DUMP),
            vec!["contents:read".to_string(), "metadata:read".to_string()]
        );
        assert!(scopes_from_attributes("    \"icmt\"<blob>=<NULL>\n").is_empty());
        assert!(scopes_from_attributes("nothing here").is_empty());
        assert_eq!(
            scopes_from_attributes("\"icmt\"<blob>=\" a , ,b \""),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn a_credential_has_the_password_and_the_declared_scopes() {
        let (k, _) = keychain(vec![(
            "github",
            "ghp-SECRET",
            "contents:read,metadata:read",
        )]);
        let c = k.get("github").unwrap();
        assert_eq!(c.token(), "ghp-SECRET");
        assert_eq!(
            c.scopes,
            vec!["contents:read".to_string(), "metadata:read".to_string()]
        );
        assert!(!format!("{c:?}").contains("SECRET"));
    }

    #[test]
    fn a_missing_item_or_a_bad_account_name_gives_nothing_and_asks_the_keychain_nothing_unsafe() {
        let (k, calls) = keychain(vec![("github", "x", "")]);
        assert!(k.get("notion").is_none() && !k.present("notion"));
        for bad in ["", "Git Hub", "a/../b", "-w", "x;y", "UPPER"] {
            assert!(
                k.get(bad).is_none() && !k.present(bad) && k.declared_scopes(bad).is_none(),
                "{bad:?}"
            );
        }
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| c.iter().any(|a| a == "notion")
                    || c.iter().any(|a| a == "github")
                    || c.is_empty()),
            "bad names never reach security"
        );
        let (k2, calls2) = keychain(vec![]);
        k2.get("a b");
        assert!(calls2.lock().unwrap().is_empty());
    }

    #[test]
    fn checking_presence_never_asks_for_the_password() {
        let (k, calls) = keychain(vec![("openai-api", "sk-SECRET", "")]);
        assert!(k.present("openai-api"));
        assert!(k.declared_scopes("openai-api").is_some());
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| !c.contains(&"-w".to_string())),
            "no -w, so no secret is read"
        );
        k.get("openai-api");
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.contains(&"-w".to_string())),
            "reading the secret is a separate call"
        );
    }

    #[test]
    fn an_empty_password_is_no_credential() {
        let (k, _) = keychain(vec![("openai-api", "", "")]);
        assert!(k.get("openai-api").is_none());
    }

    #[test]
    fn the_item_is_looked_up_under_the_sproutcrew_service() {
        let (k, calls) = keychain(vec![("openai-api", "k", "")]);
        k.get("openai-api");
        for c in calls.lock().unwrap().iter() {
            let i = c.iter().position(|a| a == "-s").unwrap();
            assert_eq!(c[i + 1], "sproutcrew");
        }
    }
}
