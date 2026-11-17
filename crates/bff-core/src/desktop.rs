//! Computer use: a step that operates one app on this Mac (Keynote first) to build what the plan approved. The
//! app is driven through its accessibility tree by an outside program (agent-desktop), which has no allowlist
//! or confirmation of its own, so this module is that layer, and the loop is owned here:
//!
//! 1. Read the app's tree ([`Desktop::snapshot`]). The model sees it as fenced, untrusted data.
//! 2. The model proposes one action as data, `{"command": ..., "ref": ..., "value": ...}`, never a command line.
//! 3. [`check`] accepts it only if the command is on a short list, the element belongs to the latest snapshot,
//!    and any key is on a fixed list. [`args`] then builds the argument array from the checked fields.
//! 4. The action runs, the tree is read again, and the loop repeats, up to the scope's action limit, stopping
//!    at once when the stop latch is set.
//!
//! Every action is journaled with its command, the element and the result; typed text is recorded by length
//! only. Nothing here asks macOS for permission. Accessibility permission is asked for only when a person turns
//! computer use on for a step, never during setup.

use serde_json::Value;

/// The commands a step may use. Everything else (launching or closing apps, the clipboard, notifications,
/// system surfaces, batches, screenshots, and anything that moves the real pointer) is refused.
pub const ALLOWED: &[&str] = &[
    "snapshot",
    "get",
    "click",
    "type",
    "set-value",
    "select",
    "toggle",
    "check",
    "expand",
    "scroll",
    "press",
];
const NEEDS_REF: &[&str] = &[
    "get",
    "click",
    "type",
    "set-value",
    "select",
    "toggle",
    "check",
    "expand",
    "scroll",
];
const NEEDS_VALUE: &[&str] = &["type", "set-value", "select", "press", "scroll"];
/// Keys a step may press. No shortcuts that quit, close, switch apps or open other windows.
pub const KEYS: &[&str] = &[
    "return", "tab", "escape", "space", "delete", "up", "down", "left", "right", "home", "end",
    "pageup", "pagedown",
];
const SCROLLS: &[&str] = &["up", "down", "left", "right"];
const MAX_VALUE: usize = 4_000;

/// What a person approved for one step: which app, which file (a copy inside the project folder), and how many
/// actions at most.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub app: String,
    pub file: String,
    pub max_actions: u32,
}

impl Scope {
    /// The sentence shown on the review board for this permission.
    pub fn text(&self, role: &str) -> String {
        format!(
            "Let the {role} operate {} on {} for this step, up to {} actions.",
            self.app, self.file, self.max_actions
        )
    }
}

/// One checked action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub command: String,
    pub element: Option<String>,
    pub value: Option<String>,
}

impl Action {
    /// The journal's record of it: the typed text by length only.
    pub fn record(&self) -> Value {
        let value = match (self.command.as_str(), &self.value) {
            ("type" | "set-value", Some(v)) => {
                serde_json::json!(format!("{} characters", v.chars().count()))
            }
            (_, v) => serde_json::json!(v),
        };
        serde_json::json!({"command": self.command, "element": self.element, "value": value})
    }
}

/// An element reference from a snapshot, such as `@s8f3k2p9:e8`: the snapshot's id, then the element.
fn snapshot_of(element: &str) -> Option<&str> {
    let rest = element.strip_prefix('@')?;
    let (snap, el) = rest.split_once(':')?;
    let ok =
        |s: &str| !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric());
    (ok(snap)
        && el
            .strip_prefix('e')
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())))
    .then_some(snap)
}

/// Checks the model's proposed action against the allowlist and the latest snapshot. The proposal is data
/// from a model, so every field is checked and nothing else in it is used.
pub fn check(proposal: &Value, latest_snapshot: &str) -> Result<Action, String> {
    let command = proposal["command"]
        .as_str()
        .ok_or("the proposal names no command")?;
    if !ALLOWED.contains(&command) {
        return Err(format!("{command:?} is not an action this step may take"));
    }
    if let Some(o) = proposal.as_object() {
        if let Some(k) = o
            .keys()
            .find(|k| !["command", "ref", "value"].contains(&k.as_str()))
        {
            return Err(format!(
                "the proposal has a field this step does not take: {k:?}"
            ));
        }
    }
    let element = proposal["ref"].as_str().map(str::to_string);
    if NEEDS_REF.contains(&command) {
        let e = element
            .as_deref()
            .ok_or("the action needs an element from the latest snapshot")?;
        if snapshot_of(e) != Some(latest_snapshot) {
            return Err("the element is not from the latest snapshot; read the app again".into());
        }
    } else if element.is_some() {
        return Err(format!("{command} takes no element"));
    }
    let value = proposal["value"].as_str().map(str::to_string);
    if NEEDS_VALUE.contains(&command) && value.is_none() {
        return Err(format!("{command} needs a value"));
    }
    if let Some(v) = &value {
        if v.chars().count() > MAX_VALUE
            || v.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err("the value is too long or has control characters".into());
        }
        if command == "press" && !KEYS.contains(&v.as_str()) {
            return Err(format!("{v:?} is not a key this step may press"));
        }
        if command == "scroll" && !SCROLLS.contains(&v.as_str()) {
            return Err("scroll goes up, down, left or right".into());
        }
        if !NEEDS_VALUE.contains(&command) {
            return Err(format!("{command} takes no value"));
        }
    }
    Ok(Action {
        command: command.to_string(),
        element,
        value,
    })
}

/// The argument array for a checked action, scoped to the approved app. It is passed to the program as an
/// array, never through a shell, and `--` ends the options so a value cannot become a flag.
pub fn args(a: &Action, scope: &Scope) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut v = vec![a.command.clone()];
    match a.command.as_str() {
        // The app is named for reading and for keys; a ref already names its own app and snapshot.
        "snapshot" => v.extend([s("--app"), scope.app.clone(), s("-i"), s("--compact")]),
        "press" => v.extend([s("--app"), scope.app.clone()]),
        "scroll" => v.extend([s("--direction"), a.value.clone().unwrap_or_default()]),
        _ => {}
    }
    if a.command != "snapshot" {
        v.push(s("--"));
        v.extend(a.element.iter().cloned());
        if a.command != "scroll" {
            v.extend(a.value.iter().cloned());
        }
    }
    v
}

/// The `data` of a JSON answer from the program, or its error code and message. Every answer is one envelope,
/// `{"ok": true, "data": ...}` or `{"ok": false, "error": {"code": ..., "message": ...}}`.
pub fn data_of(answer: &Value) -> Result<&Value, String> {
    if answer["ok"] == Value::Bool(true) {
        return Ok(&answer["data"]);
    }
    Err(format!(
        "{}: {}",
        answer["error"]["code"].as_str().unwrap_or("ERROR"),
        answer["error"]["message"].as_str().unwrap_or("no message")
    ))
}

/// The program that reads and operates the app. A fake stands in for it in tests.
pub trait Desktop {
    /// Runs one command and returns its JSON answer.
    fn run(&mut self, args: &[String]) -> Result<Value, String>;
}

/// The real program, run once per command from an empty environment with only `HOME` and a fixed `PATH`, with
/// the arguments as an array and never through a shell.
pub struct Program {
    pub path: std::path::PathBuf,
}

impl Desktop for Program {
    fn run(&mut self, args: &[String]) -> Result<Value, String> {
        let out = std::process::Command::new(&self.path)
            .args(args)
            .env_clear()
            .env("HOME", std::env::var_os("HOME").unwrap_or_default())
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("the desktop program could not start: {e}"))?;
        // Every answer is one JSON envelope on standard output, also for a refusal.
        serde_json::from_slice(&out.stdout).map_err(|_| {
            format!(
                "the desktop program gave no readable answer (exit {:?})",
                out.status.code()
            )
        })
    }
}

/// How a computer-use step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The model said the work is done.
    Done,
    /// The action limit in the scope was reached.
    Limit,
    /// The stop latch was set.
    Stopped,
    /// An action was refused or failed; the step stops rather than guess.
    Refused(String),
}

/// Drives the app: read it, ask the model for one action, check and run it, and repeat. `next` is the model:
/// given the latest tree, it returns a proposal, or `{"command": "done"}`. `log` receives each action's record.
pub fn drive(
    desktop: &mut dyn Desktop,
    scope: &Scope,
    next: &mut dyn FnMut(&Value) -> Value,
    stopped: &dyn Fn() -> bool,
    log: &mut dyn FnMut(Value),
) -> End {
    let snap = Action {
        command: "snapshot".into(),
        element: None,
        value: None,
    };
    for _ in 0..scope.max_actions {
        if stopped() {
            return End::Stopped;
        }
        let answer = desktop.run(&args(&snap, scope));
        let tree = match answer
            .as_ref()
            .map_err(String::clone)
            .and_then(|a| data_of(a).cloned())
        {
            Ok(t) => t,
            Err(e) => return End::Refused(format!("the app could not be read: {e}")),
        };
        let Some(id) = tree["snapshot_id"].as_str().map(str::to_string) else {
            return End::Refused("the app's tree came back without a snapshot id".into());
        };
        let proposal = next(&tree);
        if proposal["command"] == "done" {
            return End::Done;
        }
        let action = match check(&proposal, &id) {
            Ok(a) => a,
            Err(e) => return End::Refused(e),
        };
        if stopped() {
            return End::Stopped;
        }
        let result = desktop
            .run(&args(&action, scope))
            .and_then(|a| data_of(&a).map(|_| ()));
        let mut rec = action.record();
        rec["ok"] = Value::Bool(result.is_ok());
        log(rec);
        if let Err(e) = result {
            return End::Refused(format!("the action failed: {e}"));
        }
    }
    End::Limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> Scope {
        Scope {
            app: "Keynote".into(),
            file: "deck-copy.key".into(),
            max_actions: 3,
        }
    }

    #[test]
    fn only_listed_actions_on_elements_of_the_latest_snapshot_are_accepted() {
        let ok = check(&json!({"command": "click", "ref": "@s1:e8"}), "s1").unwrap();
        assert_eq!(ok.element.as_deref(), Some("@s1:e8"));
        assert!(check(
            &json!({"command": "type", "ref": "@s1:e2", "value": "Q3 results"}),
            "s1"
        )
        .is_ok());
        assert!(check(&json!({"command": "press", "value": "return"}), "s1").is_ok());
        for (bad, why) in [
            (
                json!({"command": "launch", "value": "Terminal"}),
                "not an action",
            ),
            (json!({"command": "close-app"}), "not an action"),
            (json!({"command": "clipboard-read"}), "not an action"),
            (json!({"command": "screenshot"}), "not an action"),
            (json!({"command": "batch", "value": "[]"}), "not an action"),
            (
                json!({"command": "click", "ref": "@s0:e8"}),
                "latest snapshot",
            ),
            (json!({"command": "click", "ref": "e8"}), "latest snapshot"),
            (json!({"command": "click"}), "needs an element"),
            (json!({"command": "press", "value": "cmd+q"}), "not a key"),
            (
                json!({"command": "scroll", "ref": "@s1:e3", "value": "sideways"}),
                "scroll goes",
            ),
            (json!({"command": "type", "ref": "@s1:e2"}), "needs a value"),
            (
                json!({"command": "click", "ref": "@s1:e8", "headed": true}),
                "does not take",
            ),
            (
                json!({"command": "type", "ref": "@s1:e2", "value": "a\u{1b}[2J"}),
                "control",
            ),
        ] {
            let e = check(&bad, "s1").unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
    }

    #[test]
    fn arguments_are_an_array_scoped_to_the_approved_app_with_values_after_the_end_of_options() {
        let a = check(
            &json!({"command": "type", "ref": "@s1:e2", "value": "--headed"}),
            "s1",
        )
        .unwrap();
        assert_eq!(args(&a, &scope()), ["type", "--", "@s1:e2", "--headed"]);
        let snap = Action {
            command: "snapshot".into(),
            element: None,
            value: None,
        };
        assert_eq!(
            args(&snap, &scope()),
            ["snapshot", "--app", "Keynote", "-i", "--compact"]
        );
        let p = check(&json!({"command": "press", "value": "return"}), "s1").unwrap();
        assert_eq!(
            args(&p, &scope()),
            ["press", "--app", "Keynote", "--", "return"]
        );
        let sc = check(
            &json!({"command": "scroll", "ref": "@s1:e3", "value": "down"}),
            "s1",
        )
        .unwrap();
        assert_eq!(
            args(&sc, &scope()),
            ["scroll", "--direction", "down", "--", "@s1:e3"]
        );
        assert!(
            data_of(&json!({"ok": false, "error": {"code": "STALE_REF", "message": "gone"}}))
                .unwrap_err()
                .starts_with("STALE_REF")
        );
        assert_eq!(
            scope().text("Builder"),
            "Let the Builder operate Keynote on deck-copy.key for this step, up to 3 actions."
        );
        assert_eq!(
            a.record()["value"],
            "8 characters",
            "typed text is logged by length only"
        );
    }

    struct Fake {
        calls: Vec<Vec<String>>,
        snap: u32,
    }
    impl Desktop for Fake {
        fn run(&mut self, args: &[String]) -> Result<Value, String> {
            self.calls.push(args.to_vec());
            if args[0] == "snapshot" {
                self.snap += 1;
                return Ok(
                    json!({"ok": true, "data": {"snapshot_id": format!("s{}", self.snap), "tree": {}}}),
                );
            }
            Ok(json!({"ok": true, "data": {}}))
        }
    }

    #[test]
    fn the_loop_reads_again_after_every_action_and_stops_at_done_the_limit_a_refusal_or_the_latch()
    {
        let mut d = Fake {
            calls: vec![],
            snap: 0,
        };
        let mut log = vec![];
        let mut n = 0;
        let end = drive(
            &mut d,
            &scope(),
            &mut |tree| {
                n += 1;
                let id = tree["snapshot_id"].as_str().unwrap().to_string();
                if n < 3 {
                    json!({"command": "click", "ref": format!("@{id}:e1")})
                } else {
                    json!({"command": "done"})
                }
            },
            &|| false,
            &mut |r| log.push(r),
        );
        assert_eq!(end, End::Done);
        let kinds: Vec<&str> = d.calls.iter().map(|c| c[0].as_str()).collect();
        assert_eq!(
            kinds,
            ["snapshot", "click", "snapshot", "click", "snapshot"]
        );
        assert_eq!(log.len(), 2);
        // The model keeps acting: the scope's limit ends it.
        let mut d = Fake {
            calls: vec![],
            snap: 0,
        };
        let end = drive(
            &mut d,
            &scope(),
            &mut |t| json!({"command": "click", "ref": format!("@{}:e1", t["snapshot_id"].as_str().unwrap())}),
            &|| false,
            &mut |_| {},
        );
        assert_eq!(end, End::Limit);
        // A stale element is refused and nothing is run.
        let mut d = Fake {
            calls: vec![],
            snap: 0,
        };
        let end = drive(
            &mut d,
            &scope(),
            &mut |_| json!({"command": "click", "ref": "@old:e1"}),
            &|| false,
            &mut |_| {},
        );
        assert!(matches!(end, End::Refused(_)));
        assert_eq!(d.calls.len(), 1);
        // The latch stops it before anything is read.
        let mut d = Fake {
            calls: vec![],
            snap: 0,
        };
        assert_eq!(
            drive(&mut d, &scope(), &mut |_| json!({}), &|| true, &mut |_| {}),
            End::Stopped
        );
        assert!(d.calls.is_empty());
    }
}
