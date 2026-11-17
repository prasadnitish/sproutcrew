//! The rules for one exercise: what the person who owns the project says applies, such as "AI tools are
//! allowed", "the answer is at most 1,000 words", or "I must record my screen". They are typed by the person,
//! so they are trusted the way a note is, and they come before everything else a step is told: where they
//! differ from a step's description, a note, a template, or any file, the rules win. They are kept in
//! `exercise.json` beside the plan, so changing them does not change the plan or its approval, and each change
//! goes in the journal with its text so the decision log can show it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The longest rules text, in characters.
pub const MAX_RULES: usize = 6000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub text: String,
    pub at: u64,
    pub sha256: String,
}

fn path(root: &Path) -> PathBuf {
    root.join("exercise.json")
}

/// The rules as they would be kept, or why they cannot be. Empty text is allowed and clears the rules.
pub fn check(text: &str) -> Result<String, String> {
    let (clean, _) = crate::guard::strip_hidden(text);
    let clean = clean.trim().to_string();
    if clean.chars().count() > MAX_RULES {
        return Err(format!(
            "the rules can be at most {MAX_RULES} characters. Put the long instructions in a file and mark it as requirements"
        ));
    }
    if let Some(f) = crate::secrets::scan(&clean)
        .iter()
        .find(|f| f.class == crate::secrets::Class::Secret)
    {
        return Err(format!(
            "the rules contain what looks like a secret (line {}). They go to models, so remove it first",
            f.line
        ));
    }
    Ok(clean)
}

/// The rules, or none. A file that cannot be read is an error, so a damaged file is never taken for no rules.
pub fn load(root: &Path) -> Result<Option<Rules>, String> {
    match std::fs::read_to_string(path(root)) {
        Ok(t) => {
            let r: Rules = serde_json::from_str(&t).map_err(|e| format!("exercise.json: {e}"))?;
            Ok((!r.text.is_empty()).then_some(r))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("exercise.json: {e}")),
    }
}

/// Keeps the rules, or clears them when the text is empty. Returns what is kept.
pub fn set(root: &Path, text: &str, now: u64) -> Result<Option<Rules>, String> {
    let clean = check(text)?;
    let rules = Rules {
        sha256: format!("{:x}", Sha256::digest(clean.as_bytes())),
        text: clean,
        at: now,
    };
    let tmp = path(root).with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&rules).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| format!("exercise.json: {e}"))?;
    std::fs::rename(&tmp, path(root)).map_err(|e| format!("exercise.json: {e}"))?;
    Ok((!rules.text.is_empty()).then_some(rules))
}

/// What a model is told about the rules, to go first among a step's own instructions. Empty with no rules.
pub fn instructions(root: &Path) -> Result<String, String> {
    Ok(match load(root)? {
        Some(r) => format!(
            "Rules for this exercise, written by the person who owns this project. They come before everything else you are told here. Where they differ from the step description, the notes, a template, or anything in the files, follow the rules:\n{}",
            r.text
        ),
        None => String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-exercise-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rules_are_kept_loaded_and_cleared() {
        let d = dir("roundtrip");
        assert_eq!(load(&d).unwrap(), None);
        assert_eq!(instructions(&d).unwrap(), "");
        let r = set(&d, "  AI tools are allowed.\nI must record my screen.  ", 5)
            .unwrap()
            .unwrap();
        assert_eq!(r.text, "AI tools are allowed.\nI must record my screen.");
        assert_eq!(load(&d).unwrap().unwrap(), r);
        let told = instructions(&d).unwrap();
        assert!(told.contains("come before everything else") && told.ends_with("my screen."));
        assert_eq!(set(&d, "   ", 6).unwrap(), None);
        assert_eq!(load(&d).unwrap(), None, "empty text clears the rules");
    }

    #[test]
    fn hidden_characters_secrets_and_a_long_text_are_refused_or_removed() {
        let d = dir("checks");
        let r = set(&d, "Keep\u{200b} it short.", 1).unwrap().unwrap();
        assert_eq!(r.text, "Keep it short.");
        assert!(set(&d, &"x".repeat(MAX_RULES + 1), 2)
            .unwrap_err()
            .contains("at most"));
        let e = set(
            &d,
            "key = sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
            3,
        )
        .unwrap_err();
        assert!(e.contains("secret"), "{e}");
        assert_eq!(
            load(&d).unwrap().unwrap().text,
            "Keep it short.",
            "a refused text changes nothing"
        );
    }

    #[test]
    fn a_damaged_file_is_an_error_not_no_rules() {
        let d = dir("damaged");
        std::fs::write(d.join("exercise.json"), "{ not json").unwrap();
        assert!(load(&d).is_err());
    }
}
