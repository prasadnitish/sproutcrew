//! Codex `app-server` over stdio: message framing, notifications, policy checks, and the
//! section 12 launch contract. Types come from the schema of the pinned binary.

use crate::codex_types::{
    AccountRateLimitsUpdatedNotification, ModelReroutedNotification,
    ThreadTokenUsageUpdatedNotification, TurnCompletedNotification,
};
use serde_json::{json, Value};
use std::path::Path;

// ------------------------------------------------------------- framing

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A server-to-client request. It needs an answer.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Value,
    },
    Error {
        id: Value,
        code: i64,
        message: String,
    },
    Notification {
        method: String,
        params: Value,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct FrameError(pub String);

/// One JSON object per line. `Ok(None)` for a blank line. Errors never echo the line.
pub fn parse_message(line: &str) -> Result<Option<Message>, FrameError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(line).map_err(|_| FrameError("invalid json".into()))?;
    let obj = v
        .as_object()
        .ok_or_else(|| FrameError("not a json object".into()))?;
    let id = obj.get("id").cloned();
    let method = obj
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    match (id, method) {
        (Some(id), Some(method)) => Ok(Some(Message::Request { id, method, params })),
        (None, Some(method)) => Ok(Some(Message::Notification { method, params })),
        (Some(id), None) => match (obj.get("error"), obj.get("result")) {
            (Some(e), _) => Ok(Some(Message::Error {
                id,
                code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: e
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })),
            (None, Some(result)) => Ok(Some(Message::Response {
                id,
                result: result.clone(),
            })),
            _ => Err(FrameError("response without result or error".into())),
        },
        (None, None) => Err(FrameError("message has neither id nor method".into())),
    }
}

/// A client request as one line, newline included.
pub fn request_line(id: u64, method: &str, params: &Value) -> String {
    format!(
        "{}\n",
        json!({"id": id, "method": method, "params": params})
    )
}

/// A client notification as one line, newline included.
pub fn notification_line(method: &str, params: Option<&Value>) -> String {
    let mut v = json!({ "method": method });
    if let Some(p) = params {
        v["params"] = p.clone();
    }
    format!("{v}\n")
}

// ------------------------------------------------------- notifications

#[derive(Debug, Clone)]
pub enum Notification {
    TokenUsage(ThreadTokenUsageUpdatedNotification),
    RateLimits(AccountRateLimitsUpdatedNotification),
    TurnCompleted(TurnCompletedNotification),
    TurnStarted {
        thread_id: String,
        turn_id: String,
    },
    Rerouted(ModelReroutedNotification),
    ItemStarted(Value),
    ItemCompleted(Value),
    Hook,
    AccountUpdated {
        auth_mode: Option<String>,
    },
    /// `error`, `warning`, `configWarning`, `deprecationNotice`, safety buffering, and verification.
    Diagnostic {
        method: String,
    },
    Other {
        method: String,
    },
}

/// Unparseable params for a method BFF depends on are an error, never a silent skip.
pub fn parse_notification(method: &str, params: &Value) -> Result<Notification, FrameError> {
    fn typed<T: serde::de::DeserializeOwned>(
        method: &str,
        params: &Value,
    ) -> Result<T, FrameError> {
        serde_json::from_value(params.clone())
            .map_err(|_| FrameError(format!("unparseable params for {method}")))
    }
    let item = || {
        params
            .get("item")
            .cloned()
            .ok_or_else(|| FrameError(format!("{method} has no item")))
    };
    Ok(match method {
        "thread/tokenUsage/updated" => Notification::TokenUsage(typed(method, params)?),
        "account/rateLimits/updated" => Notification::RateLimits(typed(method, params)?),
        "turn/completed" => Notification::TurnCompleted(typed(method, params)?),
        "model/rerouted" => Notification::Rerouted(typed(method, params)?),
        "turn/started" => {
            let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
            match (s(params.get("threadId")), s(params.pointer("/turn/id"))) {
                (Some(thread_id), Some(turn_id)) => {
                    Notification::TurnStarted { thread_id, turn_id }
                }
                _ => return Err(FrameError("unparseable params for turn/started".into())),
            }
        }
        "item/started" => Notification::ItemStarted(item()?),
        "item/completed" => Notification::ItemCompleted(item()?),
        "hook/started" | "hook/completed" => Notification::Hook,
        "account/updated" => Notification::AccountUpdated {
            auth_mode: params
                .get("authMode")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "error"
        | "warning"
        | "configWarning"
        | "deprecationNotice"
        | "model/safetyBuffering/updated"
        | "model/verification" => Notification::Diagnostic {
            method: method.to_string(),
        },
        _ => Notification::Other {
            method: method.to_string(),
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Delegation(String),
    WebSearch,
    McpToolCall,
    DynamicToolCall,
    ImageGeneration,
    HookFired,
    AccountNotChatgpt(String),
    NoAccount,
}

/// Tool activity that fails the curated route. Shell and file changes are recorded, not violations.
pub fn check_notification(n: &Notification) -> Vec<Violation> {
    match n {
        Notification::Hook => vec![Violation::HookFired],
        Notification::AccountUpdated { auth_mode: Some(m) } if m != "chatgpt" => {
            vec![Violation::AccountNotChatgpt(m.clone())]
        }
        Notification::ItemStarted(item) | Notification::ItemCompleted(item) => {
            let ty = item.get("type").and_then(Value::as_str).unwrap_or_default();
            match ty {
                "collabAgentToolCall" | "subAgentActivity" => {
                    vec![Violation::Delegation(ty.to_string())]
                }
                "webSearch" => vec![Violation::WebSearch],
                "mcpToolCall" => vec![Violation::McpToolCall],
                "dynamicToolCall" => vec![Violation::DynamicToolCall],
                "imageGeneration" => vec![Violation::ImageGeneration],
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub kind: String,
    pub plan_type: Option<String>,
}

/// Reduces `account/read` to its type and plan. The email and workspace name are dropped here.
pub fn check_account(resp: &Value) -> Result<AccountSummary, Violation> {
    let account = resp
        .get("account")
        .filter(|a| a.is_object())
        .ok_or(Violation::NoAccount)?;
    let kind = account
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind != "chatgpt" {
        return Err(Violation::AccountNotChatgpt(kind.to_string()));
    }
    Ok(AccountSummary {
        kind: kind.to_string(),
        plan_type: account
            .get("planType")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

// ----------------------------------------------------- launch contract

/// Section 12 overrides, as `(dotted key, TOML literal)`. `config/read` is checked against the same list.
/// The effort every Codex thread starts with unless a role sets another for a turn.
pub const DEFAULT_CODEX_EFFORT: &str = "medium";

pub const CURATED: &[(&str, &str)] = &[
    ("forced_login_method", "\"chatgpt\""),
    ("model_provider", "\"openai\""),
    ("service_tier", "\"default\""),
    ("model_reasoning_effort", "\"medium\""),
    ("agents.enabled", "false"),
    ("features.multi_agent", "false"),
    ("features.multi_agent_v2", "false"),
    ("features.hooks", "false"),
    ("notify", "[]"),
    ("features.memories", "false"),
    ("memories.use_memories", "false"),
    ("memories.generate_memories", "false"),
    ("features.apps", "false"),
    ("features.plugins", "false"),
    ("features.computer_use", "false"),
    ("features.browser_use", "false"),
    ("features.browser_use_external", "false"),
    ("features.in_app_browser", "false"),
    ("features.image_generation", "false"),
    ("features.goals", "false"),
    ("features.tool_suggest", "false"),
    ("features.skill_search", "false"),
    ("features.skill_mcp_dependency_install", "false"),
    ("skills.include_instructions", "false"),
    ("skills.bundled.enabled", "false"),
    ("web_search", "\"disabled\""),
    ("project_root_markers", "[]"),
    ("shell_environment_policy.inherit", "\"core\""),
    ("shell_environment_policy.set", "{}"),
];

/// Arguments after the `codex` program name. Never includes `--experimental`.
pub fn launch_args(
    mcp_servers: &[String],
    shell_set_keys: &[String],
    strict_config: bool,
) -> Vec<String> {
    let mut args: Vec<String> = ["app-server", "--listen", "stdio://"]
        .map(String::from)
        .to_vec();
    let overrides = CURATED
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .chain(
            mcp_servers
                .iter()
                .map(|n| format!("mcp_servers.{}.enabled=false", key_segment(n))),
        )
        // Config tables merge, so `set={}` cannot remove entries the user defined. Each is blanked by name.
        .chain(
            shell_set_keys
                .iter()
                .map(|k| format!("shell_environment_policy.set.{}=\"\"", key_segment(k))),
        );
    for o in overrides {
        args.push("-c".into());
        args.push(o);
    }
    if strict_config {
        args.push("--strict-config".into());
    }
    args
}

/// Key names under `[shell_environment_policy.set]`. Values are never read.
pub fn shell_set_names(config_toml: &str) -> Vec<String> {
    let mut in_set = false;
    let mut names = Vec::new();
    for line in config_toml.lines().map(str::trim) {
        if line.starts_with('[') {
            in_set = line == "[shell_environment_policy.set]";
        } else if in_set && !line.starts_with('#') {
            if let Some((k, _)) = line.split_once('=') {
                let k = k.trim().trim_matches(['"', '\'']);
                if !k.is_empty() {
                    names.push(k.to_string());
                }
            }
        }
    }
    names
}

/// Names from `[mcp_servers.<name>]` table headers. Values are never read.
pub fn mcp_server_names(config_toml: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in config_toml.lines() {
        let Some(rest) = line.trim().strip_prefix("[mcp_servers.") else {
            continue;
        };
        let name = if let Some(q) = rest.strip_prefix('"') {
            q.split('"').next().unwrap_or_default()
        } else {
            rest.split(['.', ']']).next().unwrap_or_default()
        };
        if !name.is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigStatus {
    Match,
    /// The effective value differs. Only booleans are quoted, since other values may be sensitive.
    Mismatch {
        actual: String,
    },
    /// `config/read` did not show the key, so the override is unverified.
    NotReported,
}

pub fn check_effective_config(
    config: &Value,
    mcp_servers: &[String],
    shell_set_keys: &[String],
) -> Vec<(String, ConfigStatus)> {
    let expected = CURATED
        .iter()
        .map(|(k, lit)| (k.to_string(), expected_json(lit)))
        .map(|(k, want)| {
            let blanked: serde_json::Map<String, Value> = shell_set_keys
                .iter()
                .map(|n| (n.clone(), json!("")))
                .collect();
            if k == "shell_environment_policy.set" && !blanked.is_empty() {
                (k, Value::Object(blanked))
            } else {
                (k, want)
            }
        })
        .chain(
            mcp_servers
                .iter()
                .map(|n| (format!("mcp_servers.{n}.enabled"), json!(false))),
        );
    expected
        .map(|(key, want)| {
            let mut cur = Some(config);
            for seg in key.split('.') {
                cur = cur.and_then(|c| c.get(seg));
            }
            let status = match cur {
                None => ConfigStatus::NotReported,
                Some(got) if *got == want => ConfigStatus::Match,
                Some(Value::Bool(b)) => ConfigStatus::Mismatch {
                    actual: b.to_string(),
                },
                Some(other) => ConfigStatus::Mismatch {
                    actual: type_name(other).into(),
                },
            };
            (key, status)
        })
        .collect()
}

fn key_segment(name: &str) -> String {
    if !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn expected_json(literal: &str) -> Value {
    match literal {
        "false" => json!(false),
        "true" => json!(true),
        "[]" => json!([]),
        "{}" => json!({}),
        s => json!(s.trim_matches('"')),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ----------------------------------------------------------- requests

pub fn initialize_params() -> Value {
    json!({
        "clientInfo": {"name": "bff-m0", "version": env!("CARGO_PKG_VERSION")},
        "capabilities": {"experimentalApi": false}
    })
}

pub fn writer_policy(task_dir: &Path) -> Value {
    json!({
        "type": "workspaceWrite",
        "writableRoots": [task_dir.to_string_lossy()],
        "networkAccess": false,
        "excludeTmpdirEnvVar": true,
        "excludeSlashTmp": true
    })
}

pub fn read_only_policy() -> Value {
    json!({"type": "readOnly", "networkAccess": false})
}

pub fn thread_start_params(cwd: &Path, model: &str, instructions: &str) -> Value {
    json!({
        "cwd": cwd.to_string_lossy(),
        "model": model,
        "serviceTier": "default",
        "approvalPolicy": "never",
        "approvalsReviewer": "user",
        "developerInstructions": instructions
    })
}

pub fn thread_resume_params(thread_id: &str, cwd: &Path, model: &str) -> Value {
    json!({
        "threadId": thread_id,
        "cwd": cwd.to_string_lossy(),
        "model": model,
        "serviceTier": "default",
        "approvalPolicy": "never",
        "approvalsReviewer": "user"
    })
}

pub fn turn_start_params(
    thread_id: &str,
    cwd: &Path,
    effort: &str,
    text: &str,
    output_schema: Option<&Value>,
    policy: &Value,
) -> Value {
    let mut v = json!({
        "threadId": thread_id,
        "cwd": cwd.to_string_lossy(),
        "effort": effort,
        "input": [{"type": "text", "text": text}],
        "sandboxPolicy": policy
    });
    if let Some(s) = output_schema {
        v["outputSchema"] = s.clone();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_types::{
        InitializeParams, ThreadStartParams, TurnInterruptParams, TurnStartParams,
    };

    fn n(method: &str, params: Value) -> Notification {
        parse_notification(method, &params).unwrap()
    }

    // ---- framing

    #[test]
    fn classifies_the_four_message_shapes() {
        let p = |v: Value| parse_message(&v.to_string()).unwrap().unwrap();
        assert!(matches!(
            p(json!({"id": 1, "method": "item/commandExecution/requestApproval", "params": {}})),
            Message::Request { .. }
        ));
        assert!(matches!(
            p(json!({"id": "a", "result": {"x": 1}})),
            Message::Response { .. }
        ));
        assert_eq!(
            p(json!({"id": 2, "error": {"code": -32600, "message": "bad"}})),
            Message::Error {
                id: json!(2),
                code: -32600,
                message: "bad".into()
            }
        );
        assert!(matches!(
            p(json!({"method": "turn/started", "params": {}})),
            Message::Notification { .. }
        ));
    }

    #[test]
    fn jsonrpc_header_is_optional_and_blank_lines_skip() {
        let m = parse_message(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#)
            .unwrap()
            .unwrap();
        assert!(matches!(m, Message::Response { .. }));
        assert_eq!(parse_message("  "), Ok(None));
    }

    #[test]
    fn garbage_and_shapeless_objects_are_errors() {
        assert!(parse_message("nope").is_err());
        assert!(parse_message("[]").is_err());
        assert!(parse_message(r#"{"foo": 1}"#).is_err());
    }

    #[test]
    fn request_lines_are_single_line_json_with_a_newline() {
        let l = request_line(7, "account/read", &json!({"refreshToken": false}));
        assert!(l.ends_with('\n') && l.matches('\n').count() == 1);
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(
            (v["id"].as_u64(), v["method"].as_str()),
            (Some(7), Some("account/read"))
        );
        let nl = notification_line("initialized", None);
        let v: Value = serde_json::from_str(&nl).unwrap();
        assert_eq!(v["method"], "initialized");
        assert!(v.get("id").is_none());
    }

    // ---- notifications

    #[test]
    fn parses_token_usage_and_rate_limit_notifications() {
        let usage = json!({"threadId": "t", "turnId": "u", "tokenUsage": {
            "total": {"inputTokens": 5, "cachedInputTokens": 1, "outputTokens": 2, "reasoningOutputTokens": 0, "totalTokens": 7},
            "last": {"inputTokens": 5, "cachedInputTokens": 1, "outputTokens": 2, "reasoningOutputTokens": 0, "totalTokens": 7}}});
        match n("thread/tokenUsage/updated", usage) {
            Notification::TokenUsage(u) => assert_eq!(u.token_usage.total.total_tokens, 7),
            other => panic!("{other:?}"),
        }
        let rl = json!({"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 3}}});
        assert!(matches!(
            n("account/rateLimits/updated", rl),
            Notification::RateLimits(_)
        ));
    }

    #[test]
    fn broken_params_for_a_known_method_are_errors() {
        assert!(
            parse_notification("thread/tokenUsage/updated", &json!({"threadId": "t"})).is_err()
        );
    }

    #[test]
    fn unknown_methods_and_diagnostics_are_kept_apart() {
        assert!(matches!(
            n("brand/new", json!({})),
            Notification::Other { .. }
        ));
        for m in [
            "error",
            "warning",
            "configWarning",
            "deprecationNotice",
            "model/safetyBuffering/updated",
            "model/verification",
        ] {
            assert!(
                matches!(n(m, json!({})), Notification::Diagnostic { .. }),
                "{m}"
            );
        }
    }

    #[test]
    fn account_updated_exposes_the_auth_mode() {
        match n(
            "account/updated",
            json!({"authMode": "apikey", "planType": null}),
        ) {
            Notification::AccountUpdated { auth_mode } => {
                assert_eq!(auth_mode.as_deref(), Some("apikey"))
            }
            other => panic!("{other:?}"),
        }
        match n("account/updated", json!({})) {
            Notification::AccountUpdated { auth_mode } => assert_eq!(auth_mode, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_auth_mode_other_than_chatgpt_fails_the_route_and_a_missing_one_does_not() {
        let up = |m: Value| check_notification(&n("account/updated", json!({ "authMode": m })));
        assert_eq!(up(json!("chatgpt")), vec![]);
        assert_eq!(up(json!(null)), vec![]);
        assert_eq!(
            up(json!("apikey")),
            vec![Violation::AccountNotChatgpt("apikey".into())]
        );
        assert_eq!(
            up(json!("chatgptAuthTokens")),
            vec![Violation::AccountNotChatgpt("chatgptAuthTokens".into())]
        );
    }

    #[test]
    fn model_rerouted_carries_both_models() {
        let r = n(
            "model/rerouted",
            json!({"fromModel": "gpt-6-luna", "toModel": "other", "reason": "highRiskCyberActivity", "threadId": "t", "turnId": "u"}),
        );
        match r {
            Notification::Rerouted(m) => assert_eq!(
                (m.from_model.as_str(), m.to_model.as_str()),
                ("gpt-6-luna", "other")
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn turn_started_carries_ids() {
        match n(
            "turn/started",
            json!({"threadId": "t1", "turn": {"id": "u1", "items": [], "status": "inProgress"}}),
        ) {
            Notification::TurnStarted { thread_id, turn_id } => {
                assert_eq!((thread_id.as_str(), turn_id.as_str()), ("t1", "u1"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tool_activity_fails_the_route_but_shell_and_messages_do_not() {
        let item = |ty: &str| {
            n(
                "item/started",
                json!({"threadId": "t", "turnId": "u", "item": {"type": ty, "id": "i"}}),
            )
        };
        assert_eq!(check_notification(&item("agentMessage")), vec![]);
        assert_eq!(check_notification(&item("commandExecution")), vec![]);
        assert_eq!(check_notification(&item("fileChange")), vec![]);
        assert_eq!(
            check_notification(&item("collabAgentToolCall")),
            vec![Violation::Delegation("collabAgentToolCall".into())]
        );
        assert_eq!(
            check_notification(&item("subAgentActivity")),
            vec![Violation::Delegation("subAgentActivity".into())]
        );
        assert_eq!(
            check_notification(&item("webSearch")),
            vec![Violation::WebSearch]
        );
        assert_eq!(
            check_notification(&item("mcpToolCall")),
            vec![Violation::McpToolCall]
        );
        assert_eq!(
            check_notification(&item("dynamicToolCall")),
            vec![Violation::DynamicToolCall]
        );
        assert_eq!(
            check_notification(&item("imageGeneration")),
            vec![Violation::ImageGeneration]
        );
        assert_eq!(
            check_notification(&n(
                "hook/started",
                json!({"run": {}, "threadId": "t", "turnId": "u"})
            )),
            vec![Violation::HookFired]
        );
    }

    #[test]
    fn completed_items_are_checked_too() {
        let done = n(
            "item/completed",
            json!({"threadId": "t", "turnId": "u", "item": {"type": "webSearch", "id": "i"}}),
        );
        assert_eq!(check_notification(&done), vec![Violation::WebSearch]);
    }

    // ---- account

    #[test]
    fn chatgpt_account_passes_and_the_summary_has_no_email() {
        let r = json!({"account": {"type": "chatgpt", "email": "me@example.com", "planType": "prolite", "workspaceName": "W"},
                       "requiresOpenaiAuth": true});
        let s = check_account(&r).unwrap();
        assert_eq!(
            s,
            AccountSummary {
                kind: "chatgpt".into(),
                plan_type: Some("prolite".into())
            }
        );
        assert!(!format!("{s:?}").contains("example.com"));
    }

    #[test]
    fn api_key_missing_or_other_accounts_fail() {
        assert_eq!(
            check_account(&json!({"account": {"type": "apiKey"}, "requiresOpenaiAuth": true})),
            Err(Violation::AccountNotChatgpt("apiKey".into()))
        );
        assert_eq!(
            check_account(&json!({"account": null, "requiresOpenaiAuth": true})),
            Err(Violation::NoAccount)
        );
        assert_eq!(
            check_account(&json!({"requiresOpenaiAuth": true})),
            Err(Violation::NoAccount)
        );
    }

    // ---- launch contract

    #[test]
    fn launch_args_pass_each_override_as_its_own_argument() {
        let args = launch_args(&["alpha".into(), "beta-2".into()], &[], false);
        assert_eq!(&args[..3], ["app-server", "--listen", "stdio://"]);
        assert!(args
            .iter()
            .all(|a| a != "--experimental" && a != "--strict-config"));
        let pairs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "-c")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(pairs.len(), CURATED.len() + 2);
        assert!(pairs.contains(&"forced_login_method=\"chatgpt\""));
        assert!(pairs.contains(&"features.multi_agent_v2=false"));
        assert!(pairs.contains(&"notify=[]"));
        assert!(pairs.contains(&"mcp_servers.alpha.enabled=false"));
        assert!(pairs.contains(&"mcp_servers.beta-2.enabled=false"));
    }

    #[test]
    fn server_names_with_odd_characters_are_quoted() {
        let args = launch_args(&["we ird".into()], &[], false);
        assert!(args.contains(&"mcp_servers.\"we ird\".enabled=false".to_string()));
    }

    #[test]
    fn user_defined_shell_env_entries_are_overridden_to_empty_one_by_one() {
        let args = launch_args(&[], &["FOO".into(), "BAR-1".into()], false);
        assert!(args.contains(&"shell_environment_policy.set.FOO=\"\"".to_string()));
        assert!(args.contains(&"shell_environment_policy.set.BAR-1=\"\"".to_string()));
    }

    #[test]
    fn shell_set_names_come_from_the_set_table_header_only() {
        let toml =
            "[shell_environment_policy.set]\nFOO = \"secret\"\n\"BAR\" = 'x'\n[other]\nBAZ = 1\n";
        assert_eq!(shell_set_names(toml), vec!["FOO", "BAR"]);
        assert!(shell_set_names("[other]\nA = 1\n").is_empty());
    }

    #[test]
    fn a_cleared_shell_set_table_matches_and_a_live_value_does_not() {
        let keys = vec!["FOO".to_string()];
        let status = |cfg: Value| {
            check_effective_config(&cfg, &[], &keys)
                .into_iter()
                .find(|(k, _)| k == "shell_environment_policy.set")
                .unwrap()
                .1
        };
        let cleared = json!({"shell_environment_policy": {"set": {"FOO": ""}}});
        assert_eq!(status(cleared), ConfigStatus::Match);
        let live = json!({"shell_environment_policy": {"set": {"FOO": "still-set"}}});
        assert_eq!(
            status(live),
            ConfigStatus::Mismatch {
                actual: "object".into()
            }
        );
        let extra = json!({"shell_environment_policy": {"set": {"FOO": "", "NEW": ""}}});
        assert_eq!(
            status(extra),
            ConfigStatus::Mismatch {
                actual: "object".into()
            }
        );
    }

    #[test]
    fn the_thread_default_effort_is_pinned_to_medium_and_checked_in_the_effective_config() {
        let args = launch_args(&[], &[], false);
        assert!(args.contains(&"model_reasoning_effort=\"medium\"".to_string()));
        assert_eq!(DEFAULT_CODEX_EFFORT, "medium");
        let status = |v: Value| {
            check_effective_config(&json!({ "model_reasoning_effort": v }), &[], &[])
                .into_iter()
                .find(|(k, _)| k == "model_reasoning_effort")
                .unwrap()
                .1
        };
        assert_eq!(status(json!("medium")), ConfigStatus::Match);
        assert_eq!(
            status(json!("xhigh")),
            ConfigStatus::Mismatch {
                actual: "string".into()
            }
        );
    }

    #[test]
    fn strict_config_is_opt_in() {
        assert!(launch_args(&[], &[], true).contains(&"--strict-config".to_string()));
    }

    #[test]
    fn mcp_names_come_from_table_headers_only() {
        let toml = "model = \"x\"\n[mcp_servers.a]\ncommand = \"c\"\n[mcp_servers.a.env]\nTOKEN = \"secret\"\n[mcp_servers.b-c]\nurl = \"u\"\n[other]\n";
        assert_eq!(mcp_server_names(toml), vec!["a", "b-c"]);
    }

    #[test]
    fn effective_config_reports_match_mismatch_and_not_reported() {
        let cfg = json!({"forced_login_method": "chatgpt", "model_provider": "azure", "features": {"hooks": true},
                         "mcp_servers": {"a": {"enabled": false}}});
        let got: std::collections::HashMap<_, _> =
            check_effective_config(&cfg, &["a".into(), "b".into()], &[])
                .into_iter()
                .collect();
        assert_eq!(got["forced_login_method"], ConfigStatus::Match);
        assert_eq!(
            got["model_provider"],
            ConfigStatus::Mismatch {
                actual: "string".into()
            }
        );
        assert_eq!(
            got["features.hooks"],
            ConfigStatus::Mismatch {
                actual: "true".into()
            }
        );
        assert_eq!(got["features.multi_agent"], ConfigStatus::NotReported);
        assert_eq!(got["mcp_servers.a.enabled"], ConfigStatus::Match);
        assert_eq!(got["mcp_servers.b.enabled"], ConfigStatus::NotReported);
    }

    // ---- requests conform to the generated schema types

    #[test]
    fn requests_deserialize_into_the_generated_types() {
        serde_json::from_value::<InitializeParams>(initialize_params()).unwrap();
        let dir = Path::new("/private/tmp/bff-task");
        serde_json::from_value::<ThreadStartParams>(thread_start_params(
            dir,
            "gpt-6-luna",
            "be brief",
        ))
        .unwrap();
        let schema = json!({"type": "object"});
        let t = turn_start_params("t1", dir, "low", "hi", Some(&schema), &writer_policy(dir));
        serde_json::from_value::<TurnStartParams>(t).unwrap();
        serde_json::from_value::<TurnStartParams>(turn_start_params(
            "t1",
            dir,
            "low",
            "hi",
            None,
            &read_only_policy(),
        ))
        .unwrap();
        serde_json::from_value::<TurnInterruptParams>(json!({"threadId": "t", "turnId": "u"}))
            .unwrap();
    }

    #[test]
    fn thread_resume_params_are_typed_never_experimental_and_never_auto_approve() {
        let dir = Path::new("/private/tmp/bff-task");
        let r = thread_resume_params("t1", dir, "gpt-6-luna");
        serde_json::from_value::<crate::codex_types::ThreadResumeParams>(r.clone()).unwrap();
        assert_eq!(
            (
                r["threadId"].as_str(),
                r["approvalPolicy"].as_str(),
                r["approvalsReviewer"].as_str(),
                r["serviceTier"].as_str()
            ),
            (Some("t1"), Some("never"), Some("user"), Some("default"))
        );
        assert_eq!(r["cwd"], "/private/tmp/bff-task");
        assert!(r.get("experimentalApi").is_none());
    }

    #[test]
    fn writer_policy_matches_section_12() {
        assert_eq!(
            writer_policy(Path::new("/private/tmp/bff-task")),
            json!({"type": "workspaceWrite", "writableRoots": ["/private/tmp/bff-task"], "networkAccess": false,
                   "excludeTmpdirEnvVar": true, "excludeSlashTmp": true})
        );
    }

    #[test]
    fn thread_start_is_never_experimental_and_never_auto_approves() {
        let v = thread_start_params(Path::new("/private/tmp/t"), "gpt-6-luna", "x");
        assert_eq!(v["approvalPolicy"], "never");
        assert_eq!(v["approvalsReviewer"], "user");
        assert_eq!(v["serviceTier"], "default");
        assert_eq!(v["cwd"], "/private/tmp/t");
        assert!(v.get("experimentalApi").is_none());
        assert_eq!(
            initialize_params()["capabilities"]["experimentalApi"],
            false
        );
    }
}
