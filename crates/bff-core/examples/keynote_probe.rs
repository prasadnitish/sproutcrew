//! The computer-use spike: can SproutCrew build a slide in Keynote through its accessibility tree?
//!
//! Open a new, empty Keynote presentation first (a throwaway, never a real deck). Then run
//! `cargo run -p bff-core --example keynote_probe -- <path to agent-desktop> [--edit]`.
//! Without `--edit` it only reads Keynote. With it, one checked action types a test title into the first text
//! area, through the same loop and allowlist a computer-use step uses.

use bff_core::desktop::{data_of, drive, Desktop, End, Program, Scope};
use serde_json::{json, Value};

fn walk(node: &Value, out: &mut Vec<(String, String, String)>) {
    if let Some(r) = node["ref_id"].as_str() {
        out.push((
            r.to_string(),
            node["role"].as_str().unwrap_or("").to_string(),
            node["name"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect(),
        ));
    }
    for c in node["children"].as_array().into_iter().flatten() {
        walk(c, out);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("give the path of agent-desktop");
        std::process::exit(2);
    };
    let edit = args.iter().any(|a| a == "--edit");
    let mut d = Program { path: path.into() };
    let perms = d.run(&["permissions".into()]).unwrap_or_default();
    println!("accessibility: {}", perms["data"]["accessibility"]["state"]);
    let scope = Scope {
        app: "Keynote".into(),
        file: "a new, empty presentation".into(),
        max_actions: 2,
    };
    let snap = d.run(&[
        "snapshot".into(),
        "--app".into(),
        "Keynote".into(),
        "--compact".into(),
    ]);
    let tree = match snap
        .as_ref()
        .map_err(String::clone)
        .and_then(|a| data_of(a).cloned())
    {
        Ok(t) => t,
        Err(e) => {
            println!("Keynote could not be read: {e}");
            return;
        }
    };
    let mut refs = vec![];
    walk(&tree["tree"], &mut refs);
    println!(
        "snapshot {} has {} elements with refs",
        tree["snapshot_id"],
        refs.len()
    );
    let mut roles: std::collections::BTreeMap<&str, usize> = Default::default();
    for (_, role, _) in &refs {
        *roles.entry(role.as_str()).or_default() += 1;
    }
    println!("by role: {roles:?}");
    for (r, role, name) in refs.iter().filter(|x| x.1.contains("text")).take(10) {
        println!("  {r} {role} {name:?}");
    }
    if !edit {
        return;
    }
    let mut done = false;
    let end = drive(
        &mut d,
        &scope,
        &mut |t| {
            if done {
                return json!({"command": "done"});
            }
            done = true;
            // The slide's title box is a layout item named "Title, ..."; its text field is the target. The first
            // text field in the window is an inspector box, such as Scale, and must not be used.
            fn title_field(n: &Value) -> Option<String> {
                if n["role"] == "layoutitem"
                    && n["name"].as_str().is_some_and(|x| x.starts_with("Title"))
                {
                    return n["children"]
                        .as_array()?
                        .iter()
                        .find(|c| c["role"] == "textfield")?["ref_id"]
                        .as_str()
                        .map(str::to_string);
                }
                n["children"].as_array()?.iter().find_map(title_field)
            }
            match title_field(&t["tree"]) {
                Some(id) => json!({"command": "type", "ref": id, "value": "SproutCrew test title"}),
                None => json!({"command": "done"}),
            }
        },
        &|| false,
        &mut |rec| println!("action: {rec}"),
    );
    println!("ended: {end:?}");
    if end == End::Done {
        println!("check Keynote: the first text area should now say \"SproutCrew test title\"");
    }
}
