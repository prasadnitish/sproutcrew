//! Pins for MCP tool definitions. The full `tools/list` answer for a server, schemas and annotations
//! included, is hashed tool by tool, and a server whose tools differ from the approved pin must be approved
//! again before it is used. This catches a server that changes its tools or their descriptions between
//! sessions. It cannot catch one that changes behavior behind an unchanged definition.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// JSON with object keys in sorted order and no spaces, so the same data always gives the same bytes.
pub fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap_or_default(),
                        canonical(&m[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

fn digest(v: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(v).as_bytes()))
}

/// Per-tool hashes by name. A definition with no string `name` is refused.
pub fn hash_tools(defs: &[Value]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for d in defs {
        let name = d
            .get("name")
            .and_then(Value::as_str)
            .ok_or("a tool definition has no name")?;
        if out.insert(name.to_string(), digest(d)).is_some() {
            return Err(format!("the tool {name:?} is listed twice"));
        }
    }
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Unchanged,
    /// No pin yet. The server has never been approved.
    Unapproved,
    Changed {
        added: Vec<String>,
        removed: Vec<String>,
        modified: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pin {
    /// Hash over all the tool hashes.
    hash: String,
    tools: BTreeMap<String, String>,
}

pub struct Pins {
    path: PathBuf,
    servers: BTreeMap<String, Pin>,
}

impl Pins {
    pub fn open(root: &Path) -> Result<Pins, String> {
        let path = root.join("tool-pins.json");
        let servers = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("tool-pins.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Pins { path, servers })
    }

    fn pin_of(defs: &[Value]) -> Result<Pin, String> {
        let tools = hash_tools(defs)?;
        let joined: String = tools.iter().map(|(n, h)| format!("{n}={h};")).collect();
        Ok(Pin {
            hash: format!("{:x}", Sha256::digest(joined.as_bytes())),
            tools,
        })
    }

    pub fn check(&self, server: &str, defs: &[Value]) -> Result<Verdict, String> {
        let now = Self::pin_of(defs)?;
        let Some(pin) = self.servers.get(server) else {
            return Ok(Verdict::Unapproved);
        };
        if pin.hash == now.hash {
            return Ok(Verdict::Unchanged);
        }
        let added = now
            .tools
            .keys()
            .filter(|n| !pin.tools.contains_key(*n))
            .cloned()
            .collect();
        let removed = pin
            .tools
            .keys()
            .filter(|n| !now.tools.contains_key(*n))
            .cloned()
            .collect();
        let modified = now
            .tools
            .iter()
            .filter(|(n, h)| pin.tools.get(*n).is_some_and(|old| old != *h))
            .map(|(n, _)| n.clone())
            .collect();
        Ok(Verdict::Changed {
            added,
            removed,
            modified,
        })
    }

    /// Records these definitions as approved. A person calls this after seeing what changed.
    pub fn approve(&mut self, server: &str, defs: &[Value]) -> Result<(), String> {
        let pin = Self::pin_of(defs)?;
        self.servers.insert(server.to_string(), pin);
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.servers).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn approved_servers(&self) -> Vec<&str> {
        self.servers.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pins(tag: &str) -> Pins {
        let d = std::env::temp_dir().join(format!("bff-pins-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Pins::open(&d).unwrap()
    }

    fn tool(name: &str, desc: &str) -> Value {
        json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]},
               "annotations": {"readOnlyHint": true, "destructiveHint": false}})
    }

    #[test]
    fn canonical_json_ignores_key_order_and_nothing_else() {
        let a = json!({"b": 1, "a": {"y": [1, 2], "x": "é"}});
        let b = json!({"a": {"x": "é", "y": [1, 2]}, "b": 1});
        assert_eq!(canonical(&a), canonical(&b));
        assert_ne!(
            canonical(&json!({"a": [1, 2]})),
            canonical(&json!({"a": [2, 1]})),
            "array order matters"
        );
        assert_ne!(canonical(&json!({"a": 1})), canonical(&json!({"a": "1"})));
        assert_eq!(canonical(&json!({"a": null})), "{\"a\":null}");
    }

    #[test]
    fn a_server_never_approved_is_unapproved_and_an_approved_one_is_unchanged_in_any_order() {
        let mut p = pins("basic");
        let defs = vec![
            tool("search", "Search things"),
            tool("fetch", "Fetch a page"),
        ];
        assert_eq!(p.check("web", &defs).unwrap(), Verdict::Unapproved);
        p.approve("web", &defs).unwrap();
        assert_eq!(p.check("web", &defs).unwrap(), Verdict::Unchanged);
        let reordered = vec![
            tool("fetch", "Fetch a page"),
            tool("search", "Search things"),
        ];
        assert_eq!(
            p.check("web", &reordered).unwrap(),
            Verdict::Unchanged,
            "list order is not a change"
        );
        assert_eq!(
            p.check("other", &defs).unwrap(),
            Verdict::Unapproved,
            "pins are per server"
        );
        assert_eq!(p.approved_servers(), vec!["web"]);
    }

    #[test]
    fn a_changed_description_schema_or_annotation_is_a_change_that_names_the_tool() {
        let mut p = pins("changes");
        let base = vec![
            tool("search", "Search things"),
            tool("fetch", "Fetch a page"),
        ];
        p.approve("web", &base).unwrap();
        let mut desc = base.clone();
        desc[0]["description"] = json!("Search things, then send the results to the server");
        assert_eq!(
            p.check("web", &desc).unwrap(),
            Verdict::Changed {
                added: vec![],
                removed: vec![],
                modified: vec!["search".into()]
            }
        );
        let mut schema = base.clone();
        schema[1]["inputSchema"]["properties"]["extra"] = json!({"type": "string"});
        assert!(
            matches!(p.check("web", &schema).unwrap(), Verdict::Changed { modified, .. } if modified == vec!["fetch".to_string()])
        );
        let mut ann = base.clone();
        ann[0]["annotations"]["readOnlyHint"] = json!(false);
        assert!(
            matches!(p.check("web", &ann).unwrap(), Verdict::Changed { modified, .. } if modified == vec!["search".to_string()])
        );
    }

    #[test]
    fn added_and_removed_tools_are_named() {
        let mut p = pins("addrem");
        p.approve("web", &[tool("a", "A"), tool("b", "B")]).unwrap();
        let v = p.check("web", &[tool("b", "B"), tool("c", "C")]).unwrap();
        assert_eq!(
            v,
            Verdict::Changed {
                added: vec!["c".into()],
                removed: vec!["a".into()],
                modified: vec![]
            }
        );
        assert!(
            matches!(p.check("web", &[]).unwrap(), Verdict::Changed { removed, .. } if removed.len() == 2)
        );
    }

    #[test]
    fn approving_again_replaces_the_pin_and_pins_survive_a_reopen() {
        let mut p = pins("persist");
        let v1 = vec![tool("a", "one")];
        p.approve("web", &v1).unwrap();
        let v2 = vec![tool("a", "two")];
        assert!(matches!(
            p.check("web", &v2).unwrap(),
            Verdict::Changed { .. }
        ));
        p.approve("web", &v2).unwrap();
        assert_eq!(p.check("web", &v2).unwrap(), Verdict::Unchanged);
        assert!(
            matches!(p.check("web", &v1).unwrap(), Verdict::Changed { .. }),
            "the old version is now a change"
        );
        let again = Pins::open(p.path.parent().unwrap()).unwrap();
        assert_eq!(again.check("web", &v2).unwrap(), Verdict::Unchanged);
    }

    #[test]
    fn malformed_lists_are_refused_and_never_approved() {
        let mut p = pins("malformed");
        assert!(p
            .approve("web", &[json!({"description": "no name"})])
            .is_err());
        assert!(
            p.approve("web", &[tool("a", "x"), tool("a", "y")]).is_err(),
            "duplicate names"
        );
        assert!(p.check("web", &[json!(5)]).is_err());
        assert!(p.approved_servers().is_empty());
    }
}
