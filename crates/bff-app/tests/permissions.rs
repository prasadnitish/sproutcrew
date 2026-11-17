//! Every command the window can call must be declared in `build.rs` and allowed in the capability file.
//! Without both, the command works in a browser preview and fails in the real window with "not allowed".

use std::collections::BTreeSet;

fn read(p: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p)).expect(p)
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> &'a str {
    let a = s.find(open).unwrap_or_else(|| panic!("no {open}")) + open.len();
    &s[a..a + s[a..].find(close).unwrap_or_else(|| panic!("no {close}"))]
}

#[test]
fn every_command_in_the_handler_is_declared_and_allowed() {
    let main = read("src/main.rs");
    let handler: BTreeSet<String> = between(&main, "generate_handler![", "]")
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    let build = read("build.rs");
    let declared: BTreeSet<String> = between(&build, "const COMMANDS: &[&str] = &[", "];")
        .split(',')
        .map(|c| c.trim().trim_matches('"').to_string())
        .filter(|c| !c.is_empty())
        .collect();
    let caps = read("capabilities/default.json");
    let allowed: BTreeSet<String> = caps
        .split('"')
        .filter_map(|t| t.strip_prefix("allow-"))
        .map(|t| t.replace('-', "_"))
        .collect();
    assert!(handler.len() > 40, "{handler:?}");
    let missing_decl: Vec<_> = handler.difference(&declared).collect();
    let missing_allow: Vec<_> = handler.difference(&allowed).collect();
    let stale: Vec<_> = declared.difference(&handler).collect();
    assert!(
        missing_decl.is_empty(),
        "not declared in build.rs: {missing_decl:?}"
    );
    assert!(
        missing_allow.is_empty(),
        "not allowed in capabilities/default.json: {missing_allow:?}"
    );
    assert!(stale.is_empty(), "declared but no command: {stale:?}");
}
