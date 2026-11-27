//! PDF export through Typst, detected as an external tool and never bundled. The Markdown is converted to
//! Typst markup with every character that could start Typst code escaped, because Typst can read files and
//! import packages from markup, and model-written text is untrusted. The tool runs from an empty
//! environment with an argument array.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SPECIAL: &[char] = &['\\', '#', '$', '*', '_', '`', '<', '>', '@', '[', ']', '~'];

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if SPECIAL.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Inline text: `**bold**`, `__bold__`, and backticks lose their markers, then everything is escaped. A
/// line that would start a list or heading in Typst gets its first character escaped.
fn inline(line: &str) -> String {
    let plain = line.replace("**", "").replace("__", "").replace('`', "");
    let mut s = escape(&plain);
    if s.starts_with(['-', '+', '=', '/']) {
        s.insert(0, '\\');
    }
    s
}

/// Converts the Markdown subset BFF writes (headings, bullets, numbered lists, paragraphs, fenced code) into
/// Typst. No `#` in the output is ever left unescaped, so the result contains no Typst function call.
pub fn markdown_to_typst(md: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            out.push_str(if in_code {
                "#raw(block: true, \""
            } else {
                "\")\n"
            });
            continue;
        }
        if in_code {
            // Inside a raw string only the backslash and quote matter.
            out.push_str(
                &line
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n"),
            );
            out.push_str("\\n");
            continue;
        }
        let t = line.trim_start();
        let heading = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&heading) && t[heading..].starts_with(' ') {
            out.push_str(&format!(
                "{} {}\n",
                "=".repeat(heading),
                inline(t[heading..].trim())
            ));
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            out.push_str(&format!("- {}\n", inline(rest)));
        } else if t
            .split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            out.push_str(&format!(
                "+ {}\n",
                inline(t.split_once(". ").map_or("", |x| x.1))
            ));
        } else if t.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&format!("{}\n", inline(t)));
        }
    }
    if in_code {
        out.push_str("\")\n");
    }
    out
}

/// Where Typst may be, in order. It is not on a login-shell PATH for a GUI app, so common places are tried.
/// The copy bundled with the app comes first, so every Mac makes the same files.
pub fn candidate_paths(home: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = crate::tools::bundled("typst").into_iter().collect();
    paths.extend([
        PathBuf::from("/opt/homebrew/bin/typst"),
        PathBuf::from("/usr/local/bin/typst"),
        PathBuf::from(home).join(".cargo/bin/typst"),
    ]);
    paths
}

pub use crate::tools::detect_in;

/// The folders Typst is told to look in for fonts, besides the system's: the person's and the Mac's own font
/// folders, and the fonts Microsoft Office carries inside its apps (Aptos, the default in new Office documents,
/// is often only there). Typst skips a folder that does not exist.
pub fn font_dirs(home: &str) -> Vec<PathBuf> {
    let mut dirs = vec![];
    if !home.is_empty() {
        dirs.push(Path::new(home).join("Library/Fonts"));
    }
    dirs.push(PathBuf::from("/Library/Fonts"));
    for app in ["Word", "PowerPoint", "Excel", "Outlook"] {
        dirs.push(PathBuf::from(format!(
            "/Applications/Microsoft {app}.app/Contents/Resources/DFonts"
        )));
    }
    dirs
}

/// The font families Typst could not find, from its warnings, in lower case as it gives them, each once.
pub fn unknown_fonts(stderr: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for line in stderr.lines() {
        if let Some(name) = line.trim().strip_prefix("warning: unknown font family: ") {
            let name = name.trim().to_string();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Compiles Typst source to a PDF in `out`. `work` is an empty scratch folder this call owns.
pub fn render(typst: &Path, source: &str, work: &Path, out: &Path) -> Result<(), String> {
    render_noting(typst, source, work, out).map(|_| ())
}

/// As [`render`], and returns the font families Typst could not find. It sets the text in another font then.
pub fn render_noting(
    typst: &Path,
    source: &str,
    work: &Path,
    out: &Path,
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let input = work.join("doc.typ");
    std::fs::write(&input, source).map_err(|e| e.to_string())?;
    let home = std::env::var("HOME").unwrap_or_default();
    let mut cmd = Command::new(typst);
    cmd.arg("compile")
        .arg("--root")
        .arg(work)
        .arg(&input)
        .arg(out);
    for dir in font_dirs(&home).into_iter().filter(|d| d.is_dir()) {
        cmd.arg("--font-path").arg(dir);
    }
    let mut child = cmd
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", work)
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start typst: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("typst took longer than 60 seconds".into());
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        use std::io::Read;
        let _ = e.read_to_string(&mut err);
    }
    if !status.success() {
        return Err(format!(
            "typst failed: {}",
            err.lines()
                .find(|l| l.starts_with("error"))
                .or_else(|| err.lines().next())
                .unwrap_or("no message")
        ));
    }
    let head = std::fs::read(out).map_err(|e| format!("typst wrote no file: {e}"))?;
    if !head.starts_with(b"%PDF") {
        return Err("typst's output is not a PDF".into());
    }
    Ok(unknown_fonts(&err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_fonts_typst_could_not_find_are_named_once_each() {
        // QA Q2-B7: Typst set the template's Aptos in a serif font and said so only on stderr.
        let err = "warning: unknown font family: aptos\n  ┌─ doc.typ:2:17\nwarning: unknown font family: aptos display\nwarning: unknown font family: aptos\n";
        assert_eq!(
            unknown_fonts(err),
            vec!["aptos".to_string(), "aptos display".to_string()]
        );
        assert!(unknown_fonts("").is_empty());
    }

    #[test]
    fn typst_also_looks_in_the_macs_font_folders_and_offices_own() {
        let dirs = font_dirs("/Users/someone");
        assert!(dirs
            .iter()
            .any(|d| d == Path::new("/Users/someone/Library/Fonts")));
        assert!(dirs.iter().any(|d| d == Path::new("/Library/Fonts")));
        assert!(dirs
            .iter()
            .any(|d| d.ends_with("Microsoft Word.app/Contents/Resources/DFonts")));
    }

    fn fake(tag: &str, body: &str) -> (PathBuf, PathBuf) {
        let d = std::env::temp_dir().join(format!("bff-typst-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("typst");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        (p, d)
    }

    #[test]
    fn markdown_becomes_headings_lists_and_paragraphs() {
        let t = markdown_to_typst(
            "# Title\n\nSome **bold** text.\n\n## Part\n- one\n- two\n1. first\n",
        );
        assert_eq!(
            t,
            "= Title\n\nSome bold text.\n\n== Part\n- one\n- two\n+ first\n"
        );
    }

    #[test]
    fn nothing_in_the_text_can_become_typst_code() {
        let evil = "#read(\"/etc/passwd\")\n#import \"@preview/x:1.0.0\": *\n$ 1 + 1 $ and @ref and <label> and #{ 1 }\n- #include \"x.typ\"\n## #heading([x])\n    ";
        let t = markdown_to_typst(evil);
        for (i, c) in t.char_indices() {
            if c == '#' {
                assert!(t[..i].ends_with('\\'), "unescaped # at {i} in {t:?}");
            }
        }
        assert!(t.contains("\\#read"));
        assert!(!t.contains("$ 1 + 1 $"), "math mode is escaped: {t:?}");
        assert!(t.contains("\\$") && t.contains("\\@ref") && t.contains("\\<label\\>"));
    }

    #[test]
    fn a_line_that_looks_like_a_list_or_heading_marker_is_escaped_when_it_is_prose() {
        let t = markdown_to_typst("= not a heading\n+ not a list\n/ not a term\n-5 degrees");
        for line in t.lines() {
            assert!(line.starts_with('\\'), "{line:?}");
        }
    }

    #[test]
    fn fenced_code_becomes_a_raw_string_that_cannot_break_out() {
        let t = markdown_to_typst("```\nlet x = \"a\\b\";\n#read(\"/etc/passwd\")\n```\nafter");
        assert!(t.starts_with("#raw(block: true, \""));
        assert!(t.contains("\\\"a\\\\b\\\""), "{t:?}");
        assert!(t.contains("\")\nafter"));
        // An unclosed fence is closed.
        assert!(markdown_to_typst("```\ncode").ends_with("\")\n"));
    }

    #[test]
    fn detection_skips_missing_and_broken_tools_and_reports_the_version() {
        let (good, _) = fake("good", "echo 'typst 0.13.1 (abc)'");
        let (bad, _) = fake("bad", "exit 3");
        let missing = PathBuf::from("/nonexistent/typst");
        assert_eq!(detect_in(&[missing.clone(), bad.clone()]), None);
        let (p, v) = detect_in(&[missing, bad, good.clone()]).unwrap();
        assert_eq!(p, good);
        assert_eq!(v, "typst 0.13.1 (abc)");
        assert_eq!(candidate_paths("/Users/x").len(), 3);
    }

    #[test]
    fn render_runs_the_tool_with_an_argument_array_and_checks_for_a_pdf() {
        let (ok, d) = fake("ok", "[ \"$1\" = compile ] && [ \"$2\" = --root ] && cp \"$4\" \"$5.src\" && printf '%%PDF-1.7 fake' > \"$5\"");
        let out = d.join("out.pdf");
        render(&ok, "= Hi\n", &d.join("work"), &out).unwrap();
        assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
        assert_eq!(
            std::fs::read_to_string(d.join("out.pdf.src")).unwrap(),
            "= Hi\n"
        );
    }

    #[test]
    fn render_reports_failure_and_non_pdf_output_and_does_not_leak_the_environment() {
        let (fail, d) = fake("fail", "echo 'error: unknown font' >&2; exit 1");
        let e = render(&fail, "x", &d.join("w"), &d.join("o.pdf")).unwrap_err();
        assert!(e.contains("unknown font"), "{e}");
        let (junk, d2) = fake("junk", "echo hello > \"$5\"");
        let e = render(&junk, "x", &d2.join("w"), &d2.join("o.pdf")).unwrap_err();
        assert!(e.contains("not a PDF"), "{e}");
        let (env, d3) = fake("env", "env > \"$5.env\"; printf '%%PDF' > \"$5\"");
        std::env::set_var("BFF_TEST_SECRET", "leaky");
        render(&env, "x", &d3.join("w"), &d3.join("o.pdf")).unwrap();
        let seen = std::fs::read_to_string(d3.join("o.pdf.env")).unwrap();
        assert!(
            !seen.contains("leaky") && !seen.contains("BFF_TEST_SECRET"),
            "{seen}"
        );
        assert!(seen.contains("PATH=/usr/bin:/bin"));
    }

    #[test]
    fn a_hung_tool_is_not_waited_on_forever_in_the_error_path() {
        let (missing, d) = fake("nostart", "exit 0");
        std::fs::remove_file(&missing).unwrap();
        let e = render(&missing, "x", &d.join("w"), &d.join("o.pdf")).unwrap_err();
        assert!(e.contains("cannot start typst"), "{e}");
    }

    /// Runs against the real tool when it is installed, and passes without it.
    #[test]
    fn the_real_typst_renders_a_pdf_and_hostile_text_stays_text() {
        let home = std::env::var("HOME").unwrap_or_default();
        let Some((typst, _)) = detect_in(&candidate_paths(&home)) else {
            eprintln!("typst is not installed; skipping");
            return;
        };
        let md = "# Report\n\nTotal: $20 per month, see @ref and <label>.\n\n- one\n- two\n\n#read(\"/etc/hosts\")\n\n```\ncode #here\n```\n";
        let d = std::env::temp_dir().join(format!("bff-typst-real-{}", std::process::id()));
        let out = d.join("out.pdf");
        render(&typst, &markdown_to_typst(md), &d.join("work"), &out).unwrap();
        let doc = lopdf::Document::load(&out).unwrap();
        let text = doc.extract_text(&[1]).unwrap();
        assert!(text.contains("Report") && text.contains("Total"), "{text}");
        assert!(
            text.contains("#read"),
            "the call is printed as text, not run: {text}"
        );
        assert!(
            !text.contains("localhost"),
            "the contents of /etc/hosts were not read: {text}"
        );
    }
}
