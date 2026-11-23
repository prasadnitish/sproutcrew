//! The static prototype kit: a small vetted set of HTML, CSS, and JavaScript files stored in the app. A
//! prototype is scaffolded from it with no package install, shows a banner that says the data and
//! integrations are mocked, and loads only its own files, so it runs under the preview server's CSP. The
//! smoke check refuses remote assets, inline scripts, event-handler attributes, and `eval`. A Builder step
//! writes a prototype as `prototype/` among the project's code, under the rules in [`BUILD_RULES`], and
//! [`ready`] is the one check the preview makes before it serves one. An app built with its page at the top of the
//! code, or in a folder of its own, opens the same way. It is not a prototype, so it has no banner to show, and it
//! passes the same safety checks.

use std::path::{Path, PathBuf};

const INDEX: &str = include_str!("../kit/index.html");
const STYLE: &str = include_str!("../kit/style.css");
const DATA: &str = include_str!("../kit/data.js");
const APP: &str = include_str!("../kit/app.js");

pub const MOCK_BANNER: &str = "Prototype with sample data. Integrations are mocked.";

/// What a code-writing step is told, so what it builds passes [`smoke`]. The banner is repeated word for word.
pub const BUILD_RULES: &str = " If this step asks for a web prototype, put it in a folder named prototype, opening at prototype/index.html, as plain HTML, CSS, and JavaScript with no build step and no packages. It must work offline: keep every file local with relative links, and use nothing remote. Put scripts in .js files, with no inline scripts and no inline event handlers such as onclick. Use sample data, and show this sentence on the first screen, word for word: Prototype with sample data. Integrations are mocked.";

fn page(dir: &Path) -> bool {
    dir.join("index.html").is_file()
}

/// Where a project's web page is: a prototype a Builder wrote among the code, the top of the code, the first folder
/// in the code that has a page, or else the older `prototype/` folder.
pub fn folder(project_root: &Path) -> PathBuf {
    let code = project_root.join(crate::code::DIR);
    let built = code.join("prototype");
    if page(&built) {
        return built;
    }
    if page(&code) {
        return code;
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&code)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| page(p))
        .collect();
    folders.sort();
    folders
        .into_iter()
        .next()
        .unwrap_or_else(|| project_root.join("prototype"))
}

/// Whether the project has a web page to open. A command-line app has none.
pub fn has_page(project_root: &Path) -> bool {
    page(&folder(project_root))
}

/// The folder to serve, or what stands in the way, in words a person can act on.
pub fn ready(project_root: &Path) -> Result<PathBuf, String> {
    let dir = folder(project_root);
    if !page(&dir) {
        return Err("This project has no prototype yet. Add a step for the Builder that asks for one, in Shape the steps before you approve the plan, or ask for a change to add it.".into());
    }
    let problems = if dir.ends_with("prototype") {
        smoke(&dir)
    } else {
        safety(&dir)
    };
    if problems.is_empty() {
        Ok(dir)
    } else {
        Err(format!(
            "the prototype does not pass its checks: {}",
            problems.join("; ")
        ))
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Writes the kit into `dir`. The folder must be empty or new.
pub fn scaffold(dir: &Path, title: &str, hypothesis: &str) -> Result<(), String> {
    if dir.exists()
        && std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err(format!("{} is not empty", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let index = INDEX
        .replace("{{TITLE}}", &escape_html(title))
        .replace("{{HYPOTHESIS}}", &escape_html(hypothesis));
    for (name, text) in [
        ("index.html", index.as_str()),
        ("style.css", STYLE),
        ("data.js", DATA),
        ("app.js", APP),
    ] {
        std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn attr_values<'a>(html: &'a str, attr: &str) -> Vec<&'a str> {
    let needle = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(&needle) {
        rest = &rest[i + needle.len()..];
        if let Some(j) = rest.find('"') {
            out.push(&rest[..j]);
            rest = &rest[j..];
        }
    }
    out
}

/// What is wrong with a prototype: its banner, and everything in [`safety`]. Empty means it passes.
pub fn smoke(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let index = std::fs::read_to_string(dir.join("index.html")).unwrap_or_default();
    if !index.contains(MOCK_BANNER) && page(dir) {
        problems.push("the mock banner is missing".into());
    }
    problems.extend(safety(dir));
    problems
}

/// Problems that would stop a page from running offline under the preview CSP. Empty means it passes.
pub fn safety(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let Ok(index) = std::fs::read_to_string(dir.join("index.html")) else {
        return vec!["index.html is missing".into()];
    };
    for attr in ["src", "href"] {
        for v in attr_values(&index, attr) {
            if v.contains("://") || v.starts_with("//") || v.starts_with('/') || v.contains("..") {
                problems.push(format!("{attr}=\"{v}\" is not a local relative file"));
            } else if !dir.join(v).is_file() {
                problems.push(format!("{attr}=\"{v}\" does not exist"));
            }
        }
    }
    // An inline script has a body between its tags. External ones are empty.
    let mut rest = index.as_str();
    while let Some(i) = rest.find("<script") {
        rest = &rest[i..];
        let Some(end) = rest.find('>') else { break };
        let after = &rest[end + 1..];
        let close = after.find("</script>").unwrap_or(0);
        if !after[..close].trim().is_empty() {
            problems.push("index.html has an inline script".into());
        }
        rest = &rest[end..];
    }
    let lower = index.to_lowercase();
    if [
        " onclick=",
        " onload=",
        " onerror=",
        " onsubmit=",
        " onchange=",
    ]
    .iter()
    .any(|h| lower.contains(h))
    {
        problems.push("index.html has an inline event handler".into());
    }
    for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = f.path();
        if path.extension().is_some_and(|e| e == "js") {
            // A test file is not part of the page unless the page loads it.
            let name = f.file_name().to_string_lossy().into_owned();
            if name.ends_with(".test.js") && !index.contains(&name) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for bad in [
                "eval(",
                "new Function",
                "document.write",
                "importScripts",
                "WebSocket(",
                "XMLHttpRequest",
            ] {
                if text.contains(bad) {
                    problems.push(format!("{} uses {bad}", f.file_name().to_string_lossy()));
                }
            }
            for remote in ["http://", "https://"] {
                if text.contains(remote) {
                    problems.push(format!(
                        "{} refers to a remote address",
                        f.file_name().to_string_lossy()
                    ));
                    break;
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::Preview;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bff-kit-{}-{tag}", std::process::id()))
    }

    fn project(tag: &str) -> std::path::PathBuf {
        let d = dir(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_project_with_no_prototype_is_told_how_to_get_one() {
        let e = ready(&project("none")).unwrap_err();
        assert!(
            e.contains("no prototype yet") && e.contains("Builder"),
            "{e}"
        );
    }

    #[test]
    fn an_app_built_at_the_top_of_the_code_opens_without_the_prototype_banner() {
        let root = project("app-top");
        let code = root.join(crate::code::DIR);
        std::fs::create_dir_all(&code).unwrap();
        std::fs::write(
            code.join("index.html"),
            "<html><body><script src=\"app.js\"></script></body></html>",
        )
        .unwrap();
        std::fs::write(code.join("app.js"), "document.title = 'hi';").unwrap();
        assert_eq!(ready(&root).unwrap(), code);
    }

    #[test]
    fn an_app_in_a_folder_of_its_own_opens_too() {
        let root = project("app-sub");
        let web = root.join(crate::code::DIR).join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("index.html"), "<html></html>").unwrap();
        std::fs::write(root.join(crate::code::DIR).join("README.md"), "x").unwrap();
        assert_eq!(ready(&root).unwrap(), web);
    }

    #[test]
    fn an_app_still_has_to_pass_the_safety_checks_and_a_prototype_still_needs_its_banner() {
        let root = project("app-unsafe");
        let code = root.join(crate::code::DIR);
        std::fs::create_dir_all(&code).unwrap();
        std::fs::write(
            code.join("index.html"),
            "<html><script>alert(1)</script></html>",
        )
        .unwrap();
        assert!(ready(&root).unwrap_err().contains("inline script"));
        let root = project("proto-banner");
        let proto = root.join(crate::code::DIR).join("prototype");
        std::fs::create_dir_all(&proto).unwrap();
        std::fs::write(proto.join("index.html"), "<html></html>").unwrap();
        assert!(ready(&root).unwrap_err().contains("mock banner"));
    }

    #[test]
    fn a_test_file_the_page_does_not_load_does_not_stop_the_app_from_opening() {
        let root = project("app-test-file");
        let code = root.join(crate::code::DIR);
        std::fs::create_dir_all(&code).unwrap();
        std::fs::write(code.join("index.html"), "<html></html>").unwrap();
        std::fs::write(code.join("app.test.js"), "eval('1'); // http://example.com").unwrap();
        assert!(ready(&root).is_ok());
        std::fs::write(
            code.join("index.html"),
            "<html><script src=\"app.test.js\"></script></html>",
        )
        .unwrap();
        assert!(ready(&root).unwrap_err().contains("eval("));
    }

    #[test]
    fn a_project_with_only_a_command_line_app_has_no_page() {
        let root = project("app-cli");
        let code = root.join(crate::code::DIR);
        std::fs::create_dir_all(&code).unwrap();
        std::fs::write(code.join("main.py"), "print('hi')").unwrap();
        assert!(!has_page(&root));
    }

    #[test]
    fn a_prototype_a_builder_wrote_reaches_the_preview_through_the_projects_code() {
        let (root, task) = (project("built"), project("built-task"));
        scaffold(&task.join("prototype"), "Reorder", "One tap").unwrap();
        crate::code::harvest(&root, &task).unwrap();
        let served = ready(&root).unwrap();
        assert_eq!(served, root.join("code/prototype"));
        assert!(served.join("index.html").is_file());
    }

    #[test]
    fn the_builders_prototype_wins_over_an_older_folder_and_an_older_folder_still_opens() {
        let root = project("both");
        scaffold(&root.join("prototype"), "Old", "x").unwrap();
        assert_eq!(ready(&root).unwrap(), root.join("prototype"));
        scaffold(&root.join("code/prototype"), "New", "x").unwrap();
        assert_eq!(ready(&root).unwrap(), root.join("code/prototype"));
    }

    #[test]
    fn a_prototype_that_breaks_the_rules_is_not_served_and_the_problem_is_named() {
        let root = project("rulebreak");
        std::fs::create_dir_all(root.join("code/prototype")).unwrap();
        std::fs::write(
            root.join("code/prototype/index.html"),
            "<html><body>no banner</body></html>",
        )
        .unwrap();
        let e = ready(&root).unwrap_err();
        assert!(e.contains("does not pass") && e.contains("banner"), "{e}");
    }

    #[test]
    fn the_rules_a_builder_is_given_are_the_ones_the_check_enforces() {
        assert!(BUILD_RULES.contains(MOCK_BANNER));
        for must in ["prototype/index.html", "inline", "remote"] {
            assert!(BUILD_RULES.contains(must), "{must}");
        }
    }

    #[test]
    fn the_kit_scaffolds_and_passes_its_own_smoke_check() {
        let d = dir("ok");
        scaffold(
            &d,
            "Pilot planner",
            "Teams adopt a tool faster with a checklist",
        )
        .unwrap();
        for f in ["index.html", "style.css", "data.js", "app.js"] {
            assert!(d.join(f).is_file(), "{f}");
        }
        assert_eq!(smoke(&d), Vec::<String>::new());
        let html = std::fs::read_to_string(d.join("index.html")).unwrap();
        assert!(
            html.contains("<title>Pilot planner</title>")
                && html.contains("Hypothesis: Teams adopt")
        );
        assert!(html.contains(MOCK_BANNER));
    }

    #[test]
    fn titles_are_escaped_and_a_non_empty_folder_is_refused() {
        let d = dir("esc");
        scaffold(&d, "<script>alert(1)</script>", "a \"quote\" & more").unwrap();
        let html = std::fs::read_to_string(d.join("index.html")).unwrap();
        assert!(!html.contains("<script>alert"), "{html}");
        assert!(html.contains("&lt;script&gt;"));
        assert!(
            smoke(&d).is_empty(),
            "an escaped title is not an inline script"
        );
        assert!(scaffold(&d, "x", "y").is_err(), "the folder now has files");
    }

    #[test]
    fn the_smoke_check_names_each_problem() {
        let d = dir("bad");
        scaffold(&d, "T", "H").unwrap();
        std::fs::write(
            d.join("index.html"),
            "<html><link rel=\"stylesheet\" href=\"https://cdn.example/x.css\"><script src=\"missing.js\"></script><script>alert(1)</script><button onclick=\"x()\">b</button></html>",
        )
        .unwrap();
        std::fs::write(d.join("app.js"), "eval('1'); fetch('https://x.test');").unwrap();
        let p = smoke(&d).join("\n");
        for want in [
            "mock banner",
            "not a local relative file",
            "missing.js\" does not exist",
            "inline script",
            "inline event handler",
            "uses eval(",
            "remote address",
        ] {
            assert!(p.contains(want), "missing {want:?} in:\n{p}");
        }
        let empty = dir("none");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(smoke(&empty), vec!["index.html is missing".to_string()]);
    }

    #[test]
    fn the_scaffolded_prototype_is_served_by_the_preview_server_with_every_file() {
        let d = dir("serve");
        scaffold(&d, "Served", "H").unwrap();
        let p = Preview::start(&d).unwrap();
        for (file, ctype) in [
            ("", "text/html"),
            ("app.js", "text/javascript"),
            ("data.js", "text/javascript"),
            ("style.css", "text/css"),
        ] {
            let mut c = TcpStream::connect(("127.0.0.1", p.port())).unwrap();
            c.write_all(
                format!(
                    "GET /{}/{file} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                    p.token(),
                    p.port()
                )
                .as_bytes(),
            )
            .unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).unwrap();
            assert!(out.starts_with("HTTP/1.1 200"), "{file}: {out}");
            assert!(out.contains(ctype), "{file}");
        }
        p.stop();
    }
}
