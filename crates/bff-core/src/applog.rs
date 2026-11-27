//! A small local log a person can export and attach to a bug report. Nothing is sent anywhere. Callers write short
//! kinds of event with fixed wording (an app start and its version, a step that stopped and why in one word, a
//! connection check and its status); they never pass prompts, briefs, results, file names or keys. Every line is
//! scrubbed again before it is written: home folders, email addresses and token-shaped words are replaced, and a
//! line that still looks like a secret is withheld. The file is capped and rotated once.

use std::io::Write;
use std::path::{Path, PathBuf};

pub const DIR: &str = "log";
pub const FILE: &str = "app.log";
/// The most the log holds before it is rotated, in bytes.
pub const MAX_BYTES: u64 = 256 * 1024;
const MAX_LINE: usize = 300;

/// The one word a failed step is logged under: the app's cap or the project's limit that refused it, the kind of
/// provider limit, or `other`. Only this word is logged, never the step's words.
pub fn failure_kind(note: &str) -> &'static str {
    if let Some(k) = crate::stop::cap_kind(note) {
        k
    } else if note.starts_with("paused:") && note.contains("spending limit") {
        "project_limit"
    } else {
        crate::api_limits::kind_of(note).map_or("other", |k| k.word())
    }
}

pub struct AppLog {
    dir: PathBuf,
    home: String,
}

impl AppLog {
    pub fn new(data: &Path, home: &str) -> AppLog {
        AppLog {
            dir: data.join(DIR),
            home: home.to_string(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    /// The line as it would be written: one line, scrubbed, cut to length, or withheld if it looks like a secret.
    pub fn line(&self, kind: &str, text: &str, now: u64) -> String {
        let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let scrubbed = crate::redact::scrub_str(&one, &self.home);
        let safe = if crate::secrets::has_secret(&crate::secrets::scan(&scrubbed)) {
            "[withheld: looked like a secret]".to_string()
        } else {
            scrubbed.chars().take(MAX_LINE).collect()
        };
        let kind: String = kind
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .take(40)
            .collect();
        format!("{now}\t{kind}\t{safe}\n")
    }

    /// Appends one line. Writing the log never stops the app, so a failure here is ignored by callers.
    pub fn write(&self, kind: &str, text: &str, now: u64) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.path();
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
            let _ = std::fs::rename(&path, self.dir.join(format!("{FILE}.1")));
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        f.write_all(self.line(kind, text, now).as_bytes())
            .map_err(|e| e.to_string())
    }

    /// Writes a copy a person can attach to a bug report: what the app is, then the older and newer log, then
    /// which connections are set up (never their keys). Returns where it was written.
    pub fn export(&self, header: &[(&str, String)], now: u64) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let mut text = String::from(
            "SproutCrew log for a bug report. It holds no briefs, results, file names or keys.\n\n",
        );
        for (k, v) in header {
            text.push_str(&format!(
                "{k}: {}\n",
                crate::redact::scrub_str(v, &self.home)
            ));
        }
        text.push('\n');
        for name in [format!("{FILE}.1"), FILE.to_string()] {
            if let Ok(t) = std::fs::read_to_string(self.dir.join(name)) {
                text.push_str(&t);
            }
        }
        if crate::secrets::has_secret(&crate::secrets::scan(&text)) {
            return Err("The log looked like it held a secret, so it was not exported".into());
        }
        let out = self.dir.join(format!("sproutcrew-log-{now}.txt"));
        std::fs::write(&out, text).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("applog-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_line_is_one_line_with_home_emails_and_tokens_scrubbed() {
        let log = AppLog::new(&dir("scrub"), "/Users/someone");
        let l = log.line(
            "step.failed",
            "at /Users/someone/x\nfor a@b.com with sk-or-v1-0123456789abcdef0123456789abcdef",
            7,
        );
        assert_eq!(l.matches('\n').count(), 1);
        assert!(l.starts_with("7\tstep.failed\t"));
        assert!(!l.contains("/Users/someone") && l.contains("~/x"), "{l}");
        assert!(!l.contains("a@b.com"), "{l}");
        assert!(!l.contains("0123456789abcdef0123456789abcdef"), "{l}");
    }

    #[test]
    fn a_failed_step_is_logged_by_the_cap_or_limit_that_refused_it() {
        // Q2-B9: a token-cap stop was logged only as "other".
        assert_eq!(
            failure_kind("stopped: the API token cap of 500,000 tokens is reached (1 used)"),
            "token_cap"
        );
        assert_eq!(
            failure_kind(
                "stopped: AI work is on hold. The API token cap of 500,000 tokens is reached."
            ),
            "token_cap"
        );
        assert_eq!(
            failure_kind("stopped: the API spending cap of $5.00 is reached"),
            "spending_cap"
        );
        assert_eq!(
            failure_kind("paused: this project has reached its spending limit of $5.00"),
            "project_limit"
        );
        assert_eq!(failure_kind("something else"), "other");
    }

    #[test]
    fn a_kind_keeps_only_plain_characters_and_a_line_is_cut_to_length() {
        let log = AppLog::new(&dir("cut"), "");
        let l = log.line("bad kind\t\n<x>", &"word ".repeat(200), 1);
        assert!(l.starts_with("1\tbadkindx\t"), "{l}");
        assert!(l.len() < MAX_LINE + 40);
    }

    #[test]
    fn writing_appends_rotates_once_and_the_export_holds_both_parts_and_no_keys() {
        let d = dir("rotate");
        let log = AppLog::new(&d, "");
        log.write("app.start", "version 2.0.6", 1).unwrap();
        let filler = "1\tapp.start\tversion 2.0.5\n";
        std::fs::write(
            log.path(),
            filler.repeat(MAX_BYTES as usize / filler.len() + 1),
        )
        .unwrap();
        log.write("step.failed", "daily_cap", 2).unwrap();
        assert!(d.join(DIR).join(format!("{FILE}.1")).exists());
        let out = log
            .export(
                &[("App", "2.0.6".into()), ("Connections", "R12 ready".into())],
                3,
            )
            .unwrap();
        let text = std::fs::read_to_string(out).unwrap();
        assert!(text.contains("App: 2.0.6") && text.contains("daily_cap"));
        assert!(text.starts_with("SproutCrew log for a bug report."));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
