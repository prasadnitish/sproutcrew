//! Claude `--output-format stream-json` events, and the section 12 checks on them.
//!
//! Parsing is tolerant: every optional field may be absent. A model's reply proves
//! nothing, so policy is judged from `system/init` and `tool_use` blocks only.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Init {
    pub session_id: String,
    pub model: String,
    pub tools: Vec<String>,
    pub mcp_servers: Vec<(String, String)>,
    pub api_key_source: Option<String>,
    pub version: Option<String>,
    pub permission_mode: Option<String>,
    pub mcp_server_errors: usize,
    pub plugin_errors: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitInfo {
    pub status: String,
    pub kind: Option<String>,
    pub resets_at: Option<i64>,
    pub utilization: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Init(Init),
    Assistant {
        message_id: String,
        model: Option<String>,
        usage: Value,
        tool_uses: Vec<ToolUse>,
    },
    User,
    Result(Value),
    RateLimit(RateLimitInfo),
    Hook {
        subtype: String,
    },
    Stream(Value),
    Other {
        kind: String,
        subtype: Option<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

/// `Ok(None)` for a blank line. Errors never echo the line, which may hold sensitive text.
pub fn parse_line(line: &str) -> Result<Option<Event>, ParseError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(line)
        .map_err(|e| ParseError(format!("invalid json: {}", e.classify_name())))?;
    if !v.is_object() {
        return Err(ParseError("not a json object".into()));
    }
    let kind = str_of(&v, "type").unwrap_or_default();
    let subtype = str_of(&v, "subtype");
    Ok(Some(match (kind.as_str(), subtype.as_deref()) {
        ("system", Some("init")) => Event::Init(parse_init(&v)),
        ("system", Some(s)) if s.starts_with("hook") => Event::Hook {
            subtype: s.to_string(),
        },
        ("assistant", _) => parse_assistant(&v),
        ("user", _) => Event::User,
        ("result", _) => Event::Result(v),
        ("rate_limit_event", _) => Event::RateLimit(parse_rate_limit(&v)),
        ("stream_event", _) => Event::Stream(v.get("event").cloned().unwrap_or(Value::Null)),
        _ => Event::Other { kind, subtype },
    }))
}

trait ClassifyName {
    fn classify_name(&self) -> &'static str;
}

impl ClassifyName for serde_json::Error {
    fn classify_name(&self) -> &'static str {
        match self.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "eof",
        }
    }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn count_of(v: &Value, key: &str) -> usize {
    v.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn parse_init(v: &Value) -> Init {
    let strings = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let servers = v
        .get("mcp_servers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        str_of(s, "name").unwrap_or_default(),
                        str_of(s, "status").unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Init {
        session_id: str_of(v, "session_id").unwrap_or_default(),
        model: str_of(v, "model").unwrap_or_default(),
        tools: strings("tools"),
        mcp_servers: servers,
        api_key_source: str_of(v, "apiKeySource"),
        version: str_of(v, "claude_code_version"),
        permission_mode: str_of(v, "permissionMode"),
        mcp_server_errors: count_of(v, "mcp_server_errors"),
        plugin_errors: count_of(v, "plugin_errors"),
    }
}

fn parse_assistant(v: &Value) -> Event {
    let m = v.get("message").cloned().unwrap_or(Value::Null);
    let tool_uses = m
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                .map(|b| ToolUse {
                    id: str_of(b, "id").unwrap_or_default(),
                    name: str_of(b, "name").unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    Event::Assistant {
        message_id: str_of(&m, "id").unwrap_or_default(),
        model: str_of(&m, "model"),
        usage: m.get("usage").cloned().unwrap_or(Value::Null),
        tool_uses,
    }
}

fn parse_rate_limit(v: &Value) -> RateLimitInfo {
    let i = v.get("rate_limit_info").cloned().unwrap_or(Value::Null);
    RateLimitInfo {
        status: str_of(&i, "status").unwrap_or_default(),
        kind: str_of(&i, "rateLimitType"),
        resets_at: i.get("resetsAt").and_then(Value::as_i64),
        utilization: i.get("utilization").and_then(Value::as_f64),
    }
}

/// What a run is allowed to have.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// Role tool list, such as `Read,Glob,Grep`.
    pub tools: Vec<String>,
    /// MCP servers the run expects. Their `mcp__<name>__*` tools are allowed.
    pub mcp_servers: Vec<String>,
    /// Tools the CLI adds for a flag the run set, such as `StructuredOutput` for `--json-schema`.
    pub implicit: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Delegation(String),
    ToolOutsideAllowlist(String),
    HookFired(String),
    ApiKeySource(String),
    ApiKeySourceMissing,
    McpServerErrors(usize),
    PluginErrors(usize),
    ExpectedMcpMissing(String),
    /// An MCP server the run did not ask for. Under `--strict-mcp-config` none should appear.
    UnexpectedMcp(String),
}

/// Checks at `system/init`. Plan login must report an `apiKeySource` of `none`.
pub fn check_init(init: &Init, policy: &Policy) -> Vec<Violation> {
    let mut out = Vec::new();
    match init.api_key_source.as_deref() {
        Some("none") => {}
        Some(other) => out.push(Violation::ApiKeySource(other.to_string())),
        None => out.push(Violation::ApiKeySourceMissing),
    }
    out.extend(init.tools.iter().filter_map(|t| tool_violation(t, policy)));
    if init.mcp_server_errors > 0 {
        out.push(Violation::McpServerErrors(init.mcp_server_errors));
    }
    if init.plugin_errors > 0 {
        out.push(Violation::PluginErrors(init.plugin_errors));
    }
    for (name, _) in &init.mcp_servers {
        if !policy.mcp_servers.iter().any(|s| s == name) {
            out.push(Violation::UnexpectedMcp(name.clone()));
        }
    }
    for want in &policy.mcp_servers {
        if !init.mcp_servers.iter().any(|(name, _)| name == want) {
            out.push(Violation::ExpectedMcpMissing(want.clone()));
        }
    }
    out
}

/// The delegation tool appears as `Task` in the init list and as `Agent` in `tool_use` blocks.
const DELEGATION: &[&str] = &["Agent", "Task", "Skill", "Workflow"];

fn tool_violation(name: &str, policy: &Policy) -> Option<Violation> {
    let listed = policy
        .tools
        .iter()
        .chain(&policy.implicit)
        .any(|t| t == name);
    let mcp = policy
        .mcp_servers
        .iter()
        .any(|s| name.starts_with(&format!("mcp__{s}__")));
    if listed || mcp {
        None
    } else if DELEGATION.contains(&name) {
        Some(Violation::Delegation(name.to_string()))
    } else {
        Some(Violation::ToolOutsideAllowlist(name.to_string()))
    }
}

/// Checks on every later event: tool use outside the allowlist, delegation, and hooks.
pub fn check_event(event: &Event, policy: &Policy) -> Vec<Violation> {
    match event {
        Event::Assistant { tool_uses, .. } => tool_uses
            .iter()
            .filter_map(|t| tool_violation(&t.name, policy))
            .collect(),
        Event::Hook { subtype } => vec![Violation::HookFired(subtype.clone())],
        _ => vec![],
    }
}

// ------------------------------------------------- tool attempts, from events

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolUseRec {
    pub id: String,
    pub name: String,
    /// `file_path` of the input, when the tool has one.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultRec {
    pub tool_use_id: String,
    pub is_error: bool,
    pub text: String,
}

fn content_blocks(line: &str, kind: &str) -> Vec<Value> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    if v["type"] != kind {
        return vec![];
    }
    v["message"]["content"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// Every `tool_use` block in `assistant` events, in order. Partial-message duplicates are not included.
pub fn tool_uses(raw: &[String]) -> Vec<ToolUseRec> {
    raw.iter()
        .flat_map(|l| content_blocks(l, "assistant"))
        .filter(|b| b["type"] == "tool_use")
        .map(|b| ToolUseRec {
            id: str_of(&b, "id").unwrap_or_default(),
            name: str_of(&b, "name").unwrap_or_default(),
            path: str_of(&b["input"], "file_path"),
        })
        .collect()
}

/// Every `tool_result` block in `user` events, in order.
pub fn tool_results(raw: &[String]) -> Vec<ToolResultRec> {
    raw.iter()
        .flat_map(|l| content_blocks(l, "user"))
        .filter(|b| b["type"] == "tool_result")
        .map(|b| ToolResultRec {
            tool_use_id: str_of(&b, "tool_use_id").unwrap_or_default(),
            is_error: b["is_error"].as_bool().unwrap_or(false),
            text: match &b["content"] {
                Value::String(s) => s.clone(),
                Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            },
        })
        .collect()
}

// ----------------------------------------------------- launch contract

pub enum Session<'a> {
    New(&'a str),
    Resume(&'a str),
}

pub enum SystemPrompt<'a> {
    None,
    File(&'a str),
    Inline(&'a str),
}

/// Inputs to the section 12 `claude -p` contract for route R2.
pub struct Launch<'a> {
    pub tools: &'a [&'a str],
    pub settings_path: &'a str,
    pub mcp_config_path: &'a str,
    pub model: &'a str,
    pub effort: &'a str,
    pub max_turns: u32,
    pub system_prompt: SystemPrompt<'a>,
    pub json_schema: Option<&'a str>,
    pub session: Session<'a>,
    /// Use `--input-format stream-json`, so the prompt is a JSON message sent after init.
    pub stream_input: bool,
}

/// Arguments after the `claude` program name. The prompt is never an argument: it goes in on stdin.
pub fn launch_args(l: &Launch) -> Vec<String> {
    let tools = l.tools.join(",");
    let mut a: Vec<String> = [
        "-p",
        "--restricted",
        "--tools",
        &tools,
        "--allowedTools",
        &tools,
        "--disallowedTools",
        "Agent Task Skill Workflow Bash",
        "--disable-slash-commands",
        "--settings",
        l.settings_path,
        "--strict-mcp-config",
        "--mcp-config",
        l.mcp_config_path,
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--model",
        l.model,
        "--effort",
        l.effort,
        "--max-turns",
        &l.max_turns.to_string(),
    ]
    .map(String::from)
    .to_vec();
    match l.system_prompt {
        SystemPrompt::File(p) => a.extend(["--append-system-prompt-file".into(), p.into()]),
        SystemPrompt::Inline(s) => a.extend(["--append-system-prompt".into(), s.into()]),
        SystemPrompt::None => {}
    }
    if let Some(s) = l.json_schema {
        a.extend(["--json-schema".into(), s.into()]);
    }
    a.extend(
        [
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--include-hook-events",
        ]
        .map(String::from),
    );
    if l.stream_input {
        a.extend(["--input-format".into(), "stream-json".into()]);
    }
    match l.session {
        Session::New(id) => a.extend(["--session-id".into(), id.into()]),
        Session::Resume(id) => a.extend(["--resume".into(), id.into()]),
    }
    a
}

/// The per-run settings file: hooks off and delegation or network tools denied.
pub fn settings_json() -> Value {
    settings_for(&[])
}

/// The settings for a run. Web tools stay denied unless the run's own tool list has them.
pub fn settings_for(tools: &[&str]) -> Value {
    let deny: Vec<&str> = [
        "Agent",
        "Task",
        "Skill",
        "Workflow",
        "Bash",
        "WebFetch",
        "WebSearch",
    ]
    .into_iter()
    .filter(|t| !tools.contains(t))
    .collect();
    json!({
        "disableAllHooks": true,
        "permissions": {"deny": deny}
    })
}

/// The per-run MCP config: nothing, or one stdio server.
pub fn mcp_config_json(server: Option<(&str, &str, &[&str])>) -> Value {
    match server {
        None => json!({"mcpServers": {}}),
        Some((name, command, args)) => {
            json!({"mcpServers": {name: {"command": command, "args": args}}})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Event {
        parse_line(&v.to_string()).unwrap().unwrap()
    }

    fn policy() -> Policy {
        Policy {
            tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
            mcp_servers: vec![],
            ..Default::default()
        }
    }

    fn init_json() -> Value {
        json!({"type": "system", "subtype": "init", "session_id": "s-1", "model": "claude-haiku-5-5",
               "tools": ["Read", "Glob", "Grep"], "mcp_servers": [], "apiKeySource": "none",
               "claude_code_version": "2.1.316", "permissionMode": "dontAsk",
               "plugin_errors": [], "slash_commands": []})
    }

    fn init_of(v: Value) -> Init {
        match parse(v) {
            Event::Init(i) => i,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_init() {
        let i = init_of(init_json());
        assert_eq!(i.session_id, "s-1");
        assert_eq!(i.model, "claude-haiku-5-5");
        assert_eq!(i.tools, vec!["Read", "Glob", "Grep"]);
        assert_eq!(i.api_key_source.as_deref(), Some("none"));
        assert_eq!(i.version.as_deref(), Some("2.1.316"));
        assert_eq!((i.mcp_server_errors, i.plugin_errors), (0, 0));
    }

    #[test]
    fn init_with_absent_optional_fields_still_parses() {
        let i =
            init_of(json!({"type": "system", "subtype": "init", "session_id": "s", "model": "m"}));
        assert!(i.tools.is_empty());
        assert_eq!(i.api_key_source, None);
    }

    #[test]
    fn parses_mcp_servers_and_error_counts() {
        let mut v = init_json();
        v["mcp_servers"] = json!([{"name": "fake", "status": "connected"}]);
        v["mcp_server_errors"] = json!([{"name": "x", "error": "boom"}]);
        v["plugin_errors"] = json!([{}, {}]);
        let i = init_of(v);
        assert_eq!(
            i.mcp_servers,
            vec![("fake".to_string(), "connected".to_string())]
        );
        assert_eq!((i.mcp_server_errors, i.plugin_errors), (1, 2));
    }

    #[test]
    fn parses_assistant_with_tool_use_and_message_id() {
        let e = parse(json!({"type": "assistant", "message": {
            "id": "msg_1", "model": "claude-haiku-5-5",
            "content": [{"type": "text", "text": "hi"}, {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {}}],
            "usage": {"input_tokens": 5, "output_tokens": 1}}}));
        match e {
            Event::Assistant {
                message_id,
                tool_uses,
                usage,
                ..
            } => {
                assert_eq!(message_id, "msg_1");
                assert_eq!(
                    tool_uses,
                    vec![ToolUse {
                        id: "tu_1".into(),
                        name: "Read".into()
                    }]
                );
                assert_eq!(usage["input_tokens"], 5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_result_as_raw_value() {
        let e = parse(
            json!({"type": "result", "subtype": "success", "is_error": false, "structured_output": {"a": 1}}),
        );
        match e {
            Event::Result(v) => assert_eq!(v["structured_output"]["a"], 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rate_limit_event_fields_are_optional() {
        let full = parse(json!({"type": "rate_limit_event", "rate_limit_info": {
            "status": "allowed_warning", "resetsAt": 1790000000, "rateLimitType": "five_hour", "utilization": 0.42}}));
        let bare =
            parse(json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed"}}));
        assert_eq!(
            full,
            Event::RateLimit(RateLimitInfo {
                status: "allowed_warning".into(),
                kind: Some("five_hour".into()),
                resets_at: Some(1790000000),
                utilization: Some(0.42)
            })
        );
        assert_eq!(
            bare,
            Event::RateLimit(RateLimitInfo {
                status: "allowed".into(),
                kind: None,
                resets_at: None,
                utilization: None
            })
        );
    }

    #[test]
    fn hook_stream_user_and_unknown_events() {
        assert_eq!(
            parse(json!({"type": "system", "subtype": "hook_started"})),
            Event::Hook {
                subtype: "hook_started".into()
            }
        );
        assert_eq!(
            parse(json!({"type": "system", "subtype": "hook_response"})),
            Event::Hook {
                subtype: "hook_response".into()
            }
        );
        assert_eq!(
            parse(json!({"type": "stream_event", "event": {"type": "message_start"}})),
            Event::Stream(json!({"type": "message_start"}))
        );
        assert_eq!(parse(json!({"type": "user", "message": {}})), Event::User);
        assert_eq!(
            parse(json!({"type": "brand_new", "subtype": "x"})),
            Event::Other {
                kind: "brand_new".into(),
                subtype: Some("x".into())
            }
        );
    }

    #[test]
    fn blank_line_is_skipped_and_garbage_is_an_error() {
        assert_eq!(parse_line("   "), Ok(None));
        assert!(parse_line("not json").is_err());
        assert!(parse_line("[1,2]").is_err());
    }

    #[test]
    fn clean_init_has_no_violations() {
        assert_eq!(check_init(&init_of(init_json()), &policy()), vec![]);
    }

    #[test]
    fn init_flags_extra_tools_including_task() {
        let mut v = init_json();
        v["tools"] = json!(["Read", "Bash", "Task"]);
        let got = check_init(&init_of(v), &policy());
        assert!(got.contains(&Violation::ToolOutsideAllowlist("Bash".into())));
        assert!(got.contains(&Violation::Delegation("Task".into())));
    }

    #[test]
    fn init_flags_wrong_api_key_source() {
        let mut v = init_json();
        v["apiKeySource"] = json!("ANTHROPIC_API_KEY");
        assert_eq!(
            check_init(&init_of(v), &policy()),
            vec![Violation::ApiKeySource("ANTHROPIC_API_KEY".into())]
        );
        let mut v = init_json();
        v.as_object_mut().unwrap().remove("apiKeySource");
        assert_eq!(
            check_init(&init_of(v), &policy()),
            vec![Violation::ApiKeySourceMissing]
        );
    }

    #[test]
    fn init_flags_error_arrays_and_missing_servers() {
        let mut v = init_json();
        v["mcp_server_errors"] = json!([{}]);
        v["plugin_errors"] = json!([{}, {}]);
        let mut p = policy();
        p.mcp_servers = vec!["fake".into()];
        let got = check_init(&init_of(v), &p);
        assert!(got.contains(&Violation::McpServerErrors(1)));
        assert!(got.contains(&Violation::PluginErrors(2)));
        assert!(got.contains(&Violation::ExpectedMcpMissing("fake".into())));
    }

    #[test]
    fn tool_uses_and_results_are_read_from_events_in_order() {
        let raw = vec![
            json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "text", "text": "hi"},
                {"type": "tool_use", "id": "t1", "name": "Write", "input": {"file_path": "/a/b.txt", "content": "probe"}},
                {"type": "tool_use", "id": "t2", "name": "Read", "input": {"file_path": "/a/c.txt"}}]}}).to_string(),
            json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "Permission to use Write has been denied."},
                {"type": "tool_result", "tool_use_id": "t2", "content": [{"type": "text", "text": "file text"}]}]}}).to_string(),
            "not json".to_string(),
        ];
        assert_eq!(
            tool_uses(&raw),
            vec![
                ToolUseRec {
                    id: "t1".into(),
                    name: "Write".into(),
                    path: Some("/a/b.txt".into())
                },
                ToolUseRec {
                    id: "t2".into(),
                    name: "Read".into(),
                    path: Some("/a/c.txt".into())
                },
            ]
        );
        assert_eq!(
            tool_results(&raw),
            vec![
                ToolResultRec {
                    tool_use_id: "t1".into(),
                    is_error: true,
                    text: "Permission to use Write has been denied.".into()
                },
                ToolResultRec {
                    tool_use_id: "t2".into(),
                    is_error: false,
                    text: "file text".into()
                },
            ]
        );
    }

    #[test]
    fn a_tool_without_a_file_path_has_none_and_other_events_are_ignored() {
        let raw = vec![
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t", "name": "Glob", "input": {"pattern": "*"}}]}}).to_string(),
            json!({"type": "stream_event", "event": {}}).to_string(),
        ];
        assert_eq!(tool_uses(&raw)[0].path, None);
        assert!(tool_results(&raw).is_empty());
    }

    #[test]
    fn an_implicit_tool_is_allowed_only_when_the_policy_names_it_and_only_by_that_name() {
        let mut v = init_json();
        v["tools"] = json!(["Read", "StructuredOutput"]);
        let init = init_of(v);
        assert_eq!(
            check_init(&init, &policy()),
            vec![Violation::ToolOutsideAllowlist("StructuredOutput".into())]
        );
        let mut p = policy();
        p.implicit = vec!["StructuredOutput".into()];
        assert_eq!(check_init(&init, &p), vec![]);
        let call = |name: &str| Event::Assistant {
            message_id: "m".into(),
            model: None,
            usage: json!({}),
            tool_uses: vec![ToolUse {
                id: "t".into(),
                name: name.into(),
            }],
        };
        assert_eq!(check_event(&call("StructuredOutput"), &p), vec![]);
        assert_eq!(
            check_event(&call("Bash"), &p),
            vec![Violation::ToolOutsideAllowlist("Bash".into())]
        );
        assert_eq!(
            check_event(&call("Agent"), &p),
            vec![Violation::Delegation("Agent".into())]
        );
    }

    #[test]
    fn an_mcp_server_the_run_did_not_ask_for_is_a_violation() {
        let mut v = init_json();
        v["mcp_servers"] = json!([{"name": "surprise", "status": "connected"}, {"name": "fake", "status": "connected"}]);
        let mut p = policy();
        p.mcp_servers = vec!["fake".into()];
        assert_eq!(
            check_init(&init_of(v), &p),
            vec![Violation::UnexpectedMcp("surprise".into())]
        );
    }

    #[test]
    fn expected_mcp_tools_are_allowed_by_prefix() {
        let mut v = init_json();
        v["tools"] = json!(["Read", "mcp__fake__echo"]);
        v["mcp_servers"] = json!([{"name": "fake", "status": "connected"}]);
        let mut p = policy();
        p.mcp_servers = vec!["fake".into()];
        assert_eq!(check_init(&init_of(v), &p), vec![]);
    }

    #[test]
    fn tool_use_outside_allowlist_and_delegation_are_flagged() {
        let e = |name: &str| Event::Assistant {
            message_id: "m".into(),
            model: None,
            usage: json!({}),
            tool_uses: vec![ToolUse {
                id: "t".into(),
                name: name.into(),
            }],
        };
        assert_eq!(check_event(&e("Read"), &policy()), vec![]);
        assert_eq!(
            check_event(&e("Write"), &policy()),
            vec![Violation::ToolOutsideAllowlist("Write".into())]
        );
        assert_eq!(
            check_event(&e("Agent"), &policy()),
            vec![Violation::Delegation("Agent".into())]
        );
        assert_eq!(
            check_event(&e("Skill"), &policy()),
            vec![Violation::Delegation("Skill".into())]
        );
    }

    #[test]
    fn any_hook_event_is_a_violation() {
        assert_eq!(
            check_event(
                &Event::Hook {
                    subtype: "hook_started".into()
                },
                &policy()
            ),
            vec![Violation::HookFired("hook_started".into())]
        );
        assert_eq!(check_event(&Event::Stream(json!({})), &policy()), vec![]);
    }

    fn base<'a>(tools: &'a [&'a str]) -> Launch<'a> {
        Launch {
            tools,
            settings_path: "/run/settings.json",
            mcp_config_path: "/run/mcp.json",
            model: "claude-haiku-5-5",
            effort: "low",
            max_turns: 4,
            system_prompt: SystemPrompt::File("/run/role.md"),
            json_schema: None,
            session: Session::New("11111111-1111-1111-1111-111111111111"),
            stream_input: false,
        }
    }

    fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].as_str())
    }

    #[test]
    fn launch_args_follow_section_12() {
        let a = launch_args(&base(&["Read", "Glob", "Grep"]));
        for flag in [
            "-p",
            "--restricted",
            "--disable-slash-commands",
            "--strict-mcp-config",
            "--verbose",
            "--include-partial-messages",
            "--include-hook-events",
        ] {
            assert!(a.contains(&flag.to_string()), "{flag}");
        }
        assert_eq!(value_of(&a, "--tools"), Some("Read,Glob,Grep"));
        assert_eq!(value_of(&a, "--allowedTools"), Some("Read,Glob,Grep"));
        assert_eq!(
            value_of(&a, "--disallowedTools"),
            Some("Agent Task Skill Workflow Bash")
        );
        assert_eq!(value_of(&a, "--permission-mode"), Some("dontAsk"));
        assert_eq!(value_of(&a, "--permission-prompts"), Some("none"));
        assert_eq!(value_of(&a, "--output-format"), Some("stream-json"));
        assert_eq!(value_of(&a, "--settings"), Some("/run/settings.json"));
        assert_eq!(value_of(&a, "--mcp-config"), Some("/run/mcp.json"));
        assert_eq!(value_of(&a, "--model"), Some("claude-haiku-5-5"));
        assert_eq!(value_of(&a, "--effort"), Some("low"));
        assert_eq!(value_of(&a, "--max-turns"), Some("4"));
        assert_eq!(
            value_of(&a, "--append-system-prompt-file"),
            Some("/run/role.md")
        );
        assert_eq!(
            value_of(&a, "--session-id"),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert!(!a.contains(&"--input-format".to_string()));
        assert!(!a.contains(&"--json-schema".to_string()));
    }

    #[test]
    fn launch_args_never_carry_a_bypass_or_a_bare_mode_or_the_prompt() {
        let a = launch_args(&base(&["Read"]));
        for banned in [
            "--bare",
            "--dangerously-skip-permissions",
            "--allow-dangerously-skip-permissions",
            "--add-dir",
            "--agents",
            "--plugin-dir",
        ] {
            assert!(!a.contains(&banned.to_string()), "{banned}");
        }
        assert!(
            a.iter().all(|x| !x.starts_with("Reply") && x.len() < 200),
            "no prompt text in argv"
        );
    }

    #[test]
    fn resume_json_schema_stream_input_and_inline_prompt_variants() {
        let mut l = base(&["Read"]);
        l.session = Session::Resume("abc");
        l.json_schema = Some("{\"type\":\"object\"}");
        l.stream_input = true;
        l.system_prompt = SystemPrompt::Inline("be brief");
        let a = launch_args(&l);
        assert_eq!(value_of(&a, "--resume"), Some("abc"));
        assert!(!a.contains(&"--session-id".to_string()));
        assert_eq!(value_of(&a, "--json-schema"), Some("{\"type\":\"object\"}"));
        assert_eq!(value_of(&a, "--input-format"), Some("stream-json"));
        assert_eq!(value_of(&a, "--append-system-prompt"), Some("be brief"));
        assert!(!a.contains(&"--append-system-prompt-file".to_string()));
        l.system_prompt = SystemPrompt::None;
        assert!(launch_args(&l)
            .iter()
            .all(|x| !x.starts_with("--append-system-prompt")));
    }

    #[test]
    fn an_mcp_tool_in_the_role_list_is_passed_through() {
        let a = launch_args(&base(&["Read", "mcp__fake__echo_marker"]));
        assert_eq!(value_of(&a, "--tools"), Some("Read,mcp__fake__echo_marker"));
    }

    #[test]
    fn settings_turn_hooks_off_and_deny_delegation_and_network() {
        let s = settings_json();
        assert_eq!(s["disableAllHooks"], true);
        let deny: Vec<&str> = s["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for d in [
            "Agent",
            "Task",
            "Skill",
            "Workflow",
            "Bash",
            "WebFetch",
            "WebSearch",
        ] {
            assert!(deny.contains(&d), "{d}");
        }
    }

    #[test]
    fn mcp_config_is_empty_or_one_stdio_server() {
        assert_eq!(mcp_config_json(None), json!({"mcpServers": {}}));
        let c = mcp_config_json(Some(("fake", "/bin/m0", &["__fake-mcp"])));
        assert_eq!(c["mcpServers"]["fake"]["command"], "/bin/m0");
        assert_eq!(c["mcpServers"]["fake"]["args"], json!(["__fake-mcp"]));
    }

    #[test]
    fn web_tools_are_denied_unless_the_run_is_given_them() {
        let deny = |v: &Value| v["permissions"]["deny"].as_array().unwrap().clone();
        let closed = deny(&settings_for(&["Read"]));
        assert!(closed.iter().any(|x| x == "WebSearch") && closed.iter().any(|x| x == "WebFetch"));
        let open = deny(&settings_for(&["Read", "WebSearch", "WebFetch"]));
        assert!(!open.iter().any(|x| x == "WebSearch" || x == "WebFetch"));
        assert!(open.iter().any(|x| x == "Bash") && open.iter().any(|x| x == "Agent"));
        assert_eq!(settings_json(), settings_for(&[]));
    }
}
