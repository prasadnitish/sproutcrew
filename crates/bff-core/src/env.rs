//! Child environment builder. Plan section 12: start empty, pass only an allowlist.

use std::path::PathBuf;

/// Settings BFF itself may add to a child. Anything else is refused, which keeps
/// API keys, cloud selectors, gateways, and custom providers out by construction.
pub const ALLOWED_EXTRA: &[&str] = &[
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
    "CLAUDE_CODE_DISABLE_CLAUDE_MDS",
    "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS",
];

#[derive(Debug, PartialEq, Eq)]
pub enum EnvError {
    ExtraNotAllowed(String),
    PathNotExplicit(String),
    TmpdirNotAbsolute,
}

#[derive(Debug, Clone)]
pub struct EnvSpec {
    pub home: String,
    pub user: String,
    pub lang: String,
    /// Explicit PATH. Every entry must be an absolute directory.
    pub path: String,
    /// Per-task temp directory.
    pub tmpdir: PathBuf,
    pub extra: Vec<(String, String)>,
}

impl EnvSpec {
    /// Reads HOME, USER, and LANG from the ambient environment and nothing else.
    pub fn from_ambient(path: &str, tmpdir: PathBuf) -> EnvSpec {
        let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        EnvSpec {
            home: get("HOME", ""),
            user: get("USER", ""),
            lang: get("LANG", "en_US.UTF-8"),
            path: path.to_string(),
            tmpdir,
            extra: Vec::new(),
        }
    }
}

pub fn build_child_env(spec: &EnvSpec) -> Result<Vec<(String, String)>, EnvError> {
    if !spec.tmpdir.is_absolute() {
        return Err(EnvError::TmpdirNotAbsolute);
    }
    if spec.path.is_empty() || spec.path.split(':').any(|p| !p.starts_with('/')) {
        return Err(EnvError::PathNotExplicit(spec.path.clone()));
    }
    if let Some((k, _)) = spec
        .extra
        .iter()
        .find(|(k, _)| !ALLOWED_EXTRA.contains(&k.as_str()))
    {
        return Err(EnvError::ExtraNotAllowed(k.clone()));
    }
    let mut env = vec![
        ("HOME".to_string(), spec.home.clone()),
        ("USER".to_string(), spec.user.clone()),
        ("LOGNAME".to_string(), spec.user.clone()),
        ("LANG".to_string(), spec.lang.clone()),
        ("PATH".to_string(), spec.path.clone()),
        (
            "TMPDIR".to_string(),
            spec.tmpdir.to_string_lossy().into_owned(),
        ),
    ];
    env.extend(spec.extra.iter().cloned());
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> EnvSpec {
        EnvSpec {
            home: "/Users/test".into(),
            user: "test".into(),
            lang: "en_US.UTF-8".into(),
            path: "/opt/homebrew/bin:/usr/bin:/bin".into(),
            tmpdir: PathBuf::from("/private/tmp/bff-task-1"),
            extra: vec![],
        }
    }

    fn keys(env: &[(String, String)]) -> Vec<&str> {
        let mut k: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        k.sort();
        k
    }

    #[test]
    fn passes_only_the_allowlist() {
        let env = build_child_env(&spec()).unwrap();
        assert_eq!(
            keys(&env),
            vec!["HOME", "LANG", "LOGNAME", "PATH", "TMPDIR", "USER"]
        );
    }

    #[test]
    fn ambient_route_variables_do_not_leak() {
        std::env::set_var("ANTHROPIC_API_KEY", "x");
        std::env::set_var("CLAUDE_CODE_USE_BEDROCK", "1");
        std::env::set_var("OPENAI_BASE_URL", "http://gateway");
        let s = EnvSpec::from_ambient("/usr/bin:/bin", PathBuf::from("/private/tmp/t"));
        let env = build_child_env(&s).unwrap();
        for k in keys(&env) {
            assert!(
                ["HOME", "LANG", "LOGNAME", "PATH", "TMPDIR", "USER"].contains(&k),
                "leaked {k}"
            );
        }
    }

    #[test]
    fn extra_must_be_on_the_bff_list() {
        let mut s = spec();
        s.extra = vec![("ANTHROPIC_API_KEY".into(), "sk".into())];
        assert_eq!(
            build_child_env(&s),
            Err(EnvError::ExtraNotAllowed("ANTHROPIC_API_KEY".into()))
        );
        s.extra = vec![("CLAUDE_CODE_USE_BEDROCK".into(), "1".into())];
        assert!(matches!(
            build_child_env(&s),
            Err(EnvError::ExtraNotAllowed(_))
        ));
    }

    #[test]
    fn allowed_extra_is_passed() {
        let mut s = spec();
        s.extra = vec![("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into())];
        let env = build_child_env(&s).unwrap();
        assert!(env.contains(&("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into())));
    }

    #[test]
    fn path_entries_must_be_absolute() {
        let mut s = spec();
        s.path = "/usr/bin:relative/bin".into();
        assert!(matches!(
            build_child_env(&s),
            Err(EnvError::PathNotExplicit(_))
        ));
        s.path = "/usr/bin::/bin".into();
        assert!(matches!(
            build_child_env(&s),
            Err(EnvError::PathNotExplicit(_))
        ));
        s.path = String::new();
        assert!(matches!(
            build_child_env(&s),
            Err(EnvError::PathNotExplicit(_))
        ));
    }

    #[test]
    fn tmpdir_must_be_absolute() {
        let mut s = spec();
        s.tmpdir = PathBuf::from("relative/tmp");
        assert_eq!(build_child_env(&s), Err(EnvError::TmpdirNotAbsolute));
    }
}
