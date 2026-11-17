//! Checks on Mermaid text before it is rendered. Generated diagram text is untrusted input, so the check
//! allows a short list of diagram types and refuses directives that change the renderer's security level,
//! click handlers, links, and markup. The renderer itself is set to `securityLevel: strict` with
//! `htmlLabels: false`, which gives text-based SVG, and `loose` is never used.

use serde_json::{json, Value};

pub const MAX_BYTES: usize = 20_000;
pub const MAX_LINES: usize = 400;

const TYPES: &[&str] = &[
    "flowchart",
    "graph",
    "sequenceDiagram",
    "stateDiagram-v2",
    "stateDiagram",
    "classDiagram",
    "erDiagram",
    "gantt",
    "journey",
    "pie",
    "mindmap",
    "timeline",
];

/// The configuration the renderer must be started with.
pub fn render_config() -> Value {
    json!({"startOnLoad": false, "securityLevel": "strict", "htmlLabels": false, "flowchart": {"htmlLabels": false}})
}

/// Problems with a diagram's source. Empty means it may be rendered.
pub fn check(src: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if src.len() > MAX_BYTES {
        problems.push(format!("the diagram is longer than {MAX_BYTES} bytes"));
    }
    if src.lines().count() > MAX_LINES {
        problems.push(format!("the diagram has more than {MAX_LINES} lines"));
    }
    let body: Vec<&str> = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("%%") || l.starts_with("%%{"))
        .collect();
    let Some(first) = body.first() else {
        return vec!["the diagram is empty".into()];
    };
    let kind = first
        .split(|c: char| c.is_whitespace() || c == ';')
        .next()
        .unwrap_or("");
    if !TYPES.contains(&kind) {
        problems.push(format!("{kind:?} is not an allowed diagram type"));
    }
    let lower = src.to_lowercase();
    if lower.contains("%%{") {
        problems.push(
            "directives (%%{ }) are not allowed, because they can change the security level".into(),
        );
    }
    for (needle, why) in [
        ("click ", "click handlers are not allowed"),
        ("href", "links are not allowed"),
        ("javascript:", "javascript: addresses are not allowed"),
        ("<script", "script tags are not allowed"),
        ("<iframe", "frames are not allowed"),
        ("<img", "markup in labels is not allowed"),
        ("onerror", "event handlers are not allowed"),
        ("onclick", "event handlers are not allowed"),
        (
            "securitylevel",
            "security settings are not allowed in a diagram",
        ),
        ("call ", "callbacks are not allowed"),
    ] {
        if lower.contains(needle) {
            problems.push(why.to_string());
        }
    }
    if src.contains('<')
        && src.contains('>')
        && src.matches('<').count()
            > src.matches("<-").count()
                + src.matches("<|").count()
                + src.matches("<<").count()
                + src.matches("<=").count()
    {
        problems.push("angle brackets look like markup, which is not allowed in labels".into());
    }
    problems.sort();
    problems.dedup();
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_diagrams_pass() {
        for src in [
            "flowchart TD\n  A[Start] --> B{Choice}\n  B -->|yes| C[Do it]\n  B -->|no| D[Stop]",
            "graph LR; A-->B; B-->C",
            "sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi",
            "stateDiagram-v2\n  [*] --> Draft\n  Draft --> Review\n  Review --> [*]",
            "%% a comment\nerDiagram\n  USER ||--o{ ORDER : places",
            "pie title Split\n  \"A\" : 40\n  \"B\" : 60",
        ] {
            assert_eq!(check(src), Vec::<String>::new(), "{src}");
        }
    }

    #[test]
    fn dangerous_or_unknown_input_is_refused_with_a_reason() {
        let cases = [
            (
                "%%{init: {'securityLevel': 'loose'}}%%\nflowchart TD\n A-->B",
                "directives",
            ),
            ("flowchart TD\n A-->B\n click A callback", "click handlers"),
            (
                "flowchart TD\n A[<img src=x onerror=alert(1)>]-->B",
                "markup",
            ),
            (
                "flowchart TD\n A-->B\n click A href \"https://x.test\"",
                "links",
            ),
            (
                "flowchart TD\n A[\"<script>alert(1)</script>\"]",
                "script tags",
            ),
            (
                "flowchart TD\n A[x]-->B\n click A \"javascript:alert(1)\"",
                "javascript:",
            ),
            ("notADiagram\n A-->B", "not an allowed diagram type"),
            ("", "empty"),
            ("   \n\n", "empty"),
        ];
        for (src, want) in cases {
            let p = check(src).join(" | ");
            assert!(p.contains(want), "{src:?} -> {p:?}, wanted {want:?}");
        }
    }

    #[test]
    fn size_limits_apply() {
        let long = format!("flowchart TD\n{}", "  A-->B\n".repeat(500));
        assert!(check(&long)
            .iter()
            .any(|p| p.contains("more than 400 lines")));
        let wide = format!("flowchart TD\n  A[{}]-->B", "x".repeat(MAX_BYTES));
        assert!(check(&wide).iter().any(|p| p.contains("longer than")));
    }

    #[test]
    fn arrows_with_angle_brackets_are_not_mistaken_for_markup() {
        assert!(check("sequenceDiagram\n  A->>B: hi\n  B-->>A: ok").is_empty());
        assert!(check("classDiagram\n  Animal <|-- Dog").is_empty());
    }

    #[test]
    fn the_render_config_is_strict_and_never_loose() {
        let c = render_config();
        assert_eq!(c["securityLevel"], "strict");
        assert_eq!(c["htmlLabels"], false);
        assert_eq!(c["flowchart"]["htmlLabels"], false);
        assert_eq!(c["startOnLoad"], false);
        assert!(!c.to_string().contains("loose"));
    }
}
