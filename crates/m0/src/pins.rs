//! Version pins. The plan was verified against these exact builds. A mismatch blocks live runs.

pub const CLAUDE_PIN: &str = "2.1.316";
pub const CODEX_PIN: &str = "0.166.0";

/// "2.1.316 (Claude Code)" -> "2.1.316". "codex-cli 0.166.0" -> "0.166.0".
pub fn version_of(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|w| {
            w.split('.').count() == 3
                && w.split('.')
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_string)
}

#[derive(Debug, PartialEq, Eq)]
pub struct PinCheck {
    pub claude: Option<String>,
    pub codex: Option<String>,
}

impl PinCheck {
    pub fn ok(&self) -> bool {
        self.claude.as_deref() == Some(CLAUDE_PIN) && self.codex.as_deref() == Some(CODEX_PIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_first_dotted_version() {
        assert_eq!(
            version_of("2.1.316 (Claude Code)\n"),
            Some("2.1.316".into())
        );
        assert_eq!(version_of("codex-cli 0.166.0\n"), Some("0.166.0".into()));
        assert_eq!(version_of("no version here"), None);
        assert_eq!(version_of(""), None);
    }

    #[test]
    fn both_pins_must_match_exactly() {
        let ok = PinCheck {
            claude: Some(CLAUDE_PIN.into()),
            codex: Some(CODEX_PIN.into()),
        };
        assert!(ok.ok());
        let newer = PinCheck {
            claude: Some("2.1.317".into()),
            codex: Some(CODEX_PIN.into()),
        };
        assert!(!newer.ok());
        let missing = PinCheck {
            claude: Some(CLAUDE_PIN.into()),
            codex: None,
        };
        assert!(!missing.ok());
    }
}
