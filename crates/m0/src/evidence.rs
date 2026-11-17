//! Redacted evidence from a Claude run. Raw lines stay in memory. What reaches disk has been
//! through an allowlist, and tool output is cut to a short scrubbed snippet.

use crate::claude_run::ClaudeRun;
use crate::codex_run::Turn;
use crate::common::home;
use bff_core::claude::{tool_results, tool_uses, Event};
use bff_core::ledger::{Baseline, ClaudeLedger, ClaudeReport, CodexTokenLedger};
use bff_core::redact::{redact_json, scrub_str};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const CLAUDE_EVENT_ALLOW: &[&str] = &[
    "type",
    "subtype",
    "session_id",
    "model",
    "tools",
    "mcp_servers.name",
    "mcp_servers.status",
    "apiKeySource",
    "claude_code_version",
    "permissionMode",
    "plugin_errors",
    "mcp_server_errors",
    "message.id",
    "message.model",
    "message.stop_reason",
    "message.usage.**",
    "message.content.type",
    "message.content.name",
    "message.content.id",
    "message.content.tool_use_id",
    "message.content.is_error",
    "is_error",
    "num_turns",
    "duration_ms",
    "total_cost_usd",
    "stop_reason",
    "usage.**",
    "modelUsage.**",
    "permission_denials.tool_name",
    "rate_limit_info.**",
    "event.type",
    "event.delta.type",
    "event.message.id",
];

pub fn snippet(s: &str) -> String {
    scrub_str(&s.chars().take(160).collect::<String>(), &home())
}

/// Only the number and boolean leaves of a value, keeping its shape. Strings and nulls are dropped.
fn numeric_leaves(v: &Value) -> Option<Value> {
    match v {
        Value::Number(_) | Value::Bool(_) => Some(v.clone()),
        Value::Object(m) => {
            let out: serde_json::Map<String, Value> = m
                .iter()
                .filter_map(|(k, x)| numeric_leaves(x).map(|n| (k.clone(), n)))
                .collect();
            (!out.is_empty()).then_some(Value::Object(out))
        }
        Value::Array(a) => {
            let out: Vec<Value> = a.iter().filter_map(numeric_leaves).collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        _ => None,
    }
}

fn merge(into: &mut Value, from: Value) {
    match (into, from) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                match a.get_mut(&k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        a.insert(k, v);
                    }
                }
            }
        }
        (slot, v) if slot.is_null() => *slot = v,
        _ => {}
    }
}

/// Every stdout line of a run, through the allowlist. Stream and non-init system events also keep
/// their numeric fields, which carry token counts, but never a string. Empty lines are dropped.
pub fn claude_events_fixture(raw: &[String]) -> Vec<Value> {
    raw.iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| {
            let mut kept = redact_json(&v, CLAUDE_EVENT_ALLOW, &home()).value;
            let numbers_ok =
                v["type"] == "stream_event" || (v["type"] == "system" && v["subtype"] != "init");
            if numbers_ok {
                if let Some(n) = numeric_leaves(&v) {
                    merge(&mut kept, n);
                }
            }
            kept
        })
        .filter(|v| v.as_object().is_some_and(|m| !m.is_empty()))
        .collect()
}

pub const CODEX_EVENT_ALLOW: &[&str] = &[
    "threadId",
    "turnId",
    "turn.id",
    "turn.status",
    "item.type",
    "item.id",
    "item.status",
    "item.phase",
    "item.exitCode",
    "item.tool",
    "item.server",
    "item.command",
    "tokenUsage.**",
    "rateLimits.**",
    "fromModel",
    "toModel",
    "reason",
    "authMode",
    "planType",
    "status",
];

/// Codex notifications as `{method, params}`, through the allowlist. Text deltas are left out, since
/// the counts are in the summary and the text is not evidence.
pub fn codex_events_fixture(notes: &[(String, Value)]) -> Vec<Value> {
    notes
        .iter()
        .filter(|(m, _)| !m.contains("Delta") && !m.contains("delta"))
        .map(|(m, p)| json!({"method": m, "params": redact_json(p, CODEX_EVENT_ALLOW, &home()).value}))
        .collect()
}

pub fn codex_turn_summary(t: &Turn, ledger: &CodexTokenLedger, thread: &str) -> Value {
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for i in &t.items {
        *kinds.entry(i.kind.clone()).or_default() += 1;
    }
    let delta = ledger.delta(thread);
    json!({
        "status": t.status, "elapsed_ms": t.elapsed_ms, "interrupted_by_harness": t.interrupted_by_harness,
        "timed_out": t.timed_out, "interrupt_ack_ms": t.interrupt_ack_ms, "terminal_ms": t.terminal_ms,
        "methods": t.methods, "item_kinds": kinds, "parse_errors": t.parse_errors,
        "violations": t.violations.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        "items": t.items.iter().map(|i| json!({
            "kind": i.kind, "status": i.status, "phase": i.phase, "exit_code": i.exit_code,
            "command": i.command.as_deref().map(snippet),
            "text_chars": i.text.as_ref().map(|s| s.chars().count()),
            "text_sha256": i.text.as_ref().map(|s| crate::common::sha256_hex(s.as_bytes())),
        })).collect::<Vec<_>>(),
        "tokens": delta.map(|d| json!({"input": d.input, "cached_input": d.cached_input, "output": d.output,
            "reasoning_output": d.reasoning_output, "total": d.total})),
        "tokens_status": if delta.is_some() { "Reported (cumulative total minus baseline)" } else { "Unknown" },
        "model_split_uncertain": ledger.model_split_uncertain(thread),
    })
}

pub fn last_result(r: &ClaudeRun) -> Option<&Value> {
    r.events.iter().rev().find_map(|e| {
        if let Event::Result(v) = e {
            Some(v)
        } else {
            None
        }
    })
}

pub fn claude_summary(r: &ClaudeRun, baseline: Baseline) -> (Value, ClaudeReport) {
    let mut ledger = ClaudeLedger::new(baseline);
    for e in &r.events {
        match e {
            Event::Assistant {
                message_id, usage, ..
            } => ledger.on_assistant(message_id, usage),
            Event::Stream(ev) => ledger.on_stream_event(ev),
            Event::Result(v) => ledger.on_result(v),
            _ => {}
        }
    }
    let rep = ledger.report();
    let kinds: BTreeMap<String, usize> = r
        .raw
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .fold(BTreeMap::new(), |mut m, v| {
            *m.entry(format!(
                "{}/{}",
                v["type"].as_str().unwrap_or("?"),
                v["subtype"].as_str().unwrap_or("")
            ))
            .or_default() += 1;
            m
        });
    let init = r.events.iter().find_map(|e| {
        if let Event::Init(i) = e {
            Some(i)
        } else {
            None
        }
    });
    let res = last_result(r);
    let summary = json!({
        "process": {"pid": r.id.pid, "pgid": r.id.pgid, "start_time": r.id.start_time},
        "init": init.map(|i| json!({"model": i.model, "tools": i.tools, "api_key_source": i.api_key_source,
            "version": i.version, "permission_mode": i.permission_mode,
            "mcp_servers": i.mcp_servers.iter().map(|(n, s)| json!({"name": n, "status": s})).collect::<Vec<_>>(),
            "mcp_server_errors": i.mcp_server_errors, "plugin_errors": i.plugin_errors})),
        "init_before_prompt": r.init_before_prompt,
        "interrupted_by_harness": r.interrupted_by_harness,
        "timed_out": r.timed_out,
        "ended_by": r.ended_by.map(|e| format!("{e:?}")),
        "exit_code": r.exit_code,
        "violations": r.violations.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        "bad_lines": r.bad_lines,
        "stderr": {"lines": r.stderr.len(),
            "first_words": r.stderr.iter().map(|l| l.split_whitespace().next().unwrap_or("").chars().take(24).collect::<String>()).collect::<Vec<_>>()},
        "event_kinds": kinds,
        "tools_used": tool_uses(&r.raw).iter().map(|t| json!({"name": t.name, "path": t.path.as_deref().map(snippet)})).collect::<Vec<_>>(),
        "tool_results": tool_results(&r.raw).iter().map(|t| json!({"is_error": t.is_error, "text": snippet(&t.text)})).collect::<Vec<_>>(),
        "result": res.map(|v| json!({
            "subtype": v["subtype"], "is_error": v["is_error"], "num_turns": v["num_turns"],
            "stop_reason": v["stop_reason"], "duration_ms": v["duration_ms"],
            "permission_denials": v["permission_denials"].as_array().map(|a| a.iter().map(|d| d["tool_name"].clone()).collect::<Vec<_>>()),
            "has_structured_output": v.get("structured_output").is_some(),
        })),
        "usage": {
            "tokens": rep.tokens.map(|t| json!({"input": t.input, "output": t.output, "cache_creation": t.cache_creation, "cache_read": t.cache_read})),
            "tokens_status": format!("{:?}", rep.tokens_status),
            "cost_usd_estimated": rep.cost_usd, "cost_status": format!("{:?}", rep.cost_status),
            "per_model": rep.per_model.iter().map(|(m, t)| (m.clone(), json!({"input": t.input, "output": t.output, "cache_creation": t.cache_creation, "cache_read": t.cache_read}))).collect::<serde_json::Map<_, _>>(),
            "result_distrusted": rep.result_distrusted,
        },
    });
    (summary, rep)
}

/// The text a Claude run ended with: the `result` field, else the last assistant text. In memory only.
pub fn final_text(r: &ClaudeRun) -> String {
    if let Some(s) = last_result(r).and_then(|v| v["result"].as_str()) {
        return s.to_string();
    }
    r.raw
        .iter()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "assistant")
        .find_map(|v| {
            v["message"]["content"].as_array().and_then(|a| {
                a.iter()
                    .find_map(|b| b["text"].as_str().map(str::to_string))
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_fixture_keeps_ids_and_counts_and_drops_text() {
        let raw = vec![
            json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "SECRET WORDS"}], "usage": {"input_tokens": 3}}}).to_string(),
            json!({"type": "result", "subtype": "success", "result": "SECRET WORDS", "num_turns": 1}).to_string(),
            json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "SECRET"}}}).to_string(),
        ];
        let f = claude_events_fixture(&raw);
        let dumped = serde_json::to_string(&f).unwrap();
        assert!(!dumped.contains("SECRET"), "{dumped}");
        assert_eq!(f[0]["message"]["id"], "m1");
        assert_eq!(f[0]["message"]["usage"]["input_tokens"], 3);
        assert_eq!(f[1]["num_turns"], 1);
        assert_eq!(f[2]["event"]["delta"]["type"], "text_delta");
    }

    #[test]
    fn stream_and_system_events_keep_their_numbers_but_never_their_text() {
        let raw = vec![
            json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "m1", "role": "assistant", "content": [],
                   "usage": {"input_tokens": 100, "cache_read_input_tokens": 5, "output_tokens": 1, "service_tier": "standard"}}}}).to_string(),
            json!({"type": "stream_event", "event": {"type": "message_delta", "delta": {"stop_reason": null, "text": "PRIVATE"}, "usage": {"output_tokens": 37}}}).to_string(),
            json!({"type": "system", "subtype": "thinking_tokens", "tokens": 12, "note": "PRIVATE", "done": true}).to_string(),
        ];
        let f = claude_events_fixture(&raw);
        let dumped = serde_json::to_string(&f).unwrap();
        assert!(
            !dumped.contains("PRIVATE") && !dumped.contains("standard"),
            "{dumped}"
        );
        assert_eq!(f[0]["event"]["message"]["id"], "m1");
        assert_eq!(f[0]["event"]["message"]["usage"]["input_tokens"], 100);
        assert_eq!(
            f[0]["event"]["message"]["usage"]["cache_read_input_tokens"],
            5
        );
        assert_eq!(f[1]["event"]["usage"]["output_tokens"], 37);
        assert_eq!(
            (
                f[2]["subtype"].as_str(),
                f[2]["tokens"].as_i64(),
                f[2]["done"].as_bool()
            ),
            (Some("thinking_tokens"), Some(12), Some(true))
        );
    }

    #[test]
    fn the_summary_ledger_recovers_usage_from_an_interrupted_stream() {
        let raw = vec![
            json!({"type": "system", "subtype": "init", "session_id": "s", "model": "m", "tools": []}).to_string(),
            json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "m1", "usage": {"input_tokens": 100, "output_tokens": 1}}}}).to_string(),
            json!({"type": "stream_event", "event": {"type": "message_delta", "usage": {"output_tokens": 37}}}).to_string(),
            json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "total_cost_usd": 0, "usage": {"input_tokens": 0, "output_tokens": 0}}).to_string(),
        ];
        let events: Vec<_> = raw
            .iter()
            .filter_map(|l| bff_core::claude::parse_line(l).unwrap())
            .collect();
        let r = crate::claude_run::ClaudeRun {
            id: bff_core::process::ProcId {
                pid: 1,
                pgid: 1,
                start_time: String::new(),
            },
            raw,
            events,
            stderr: vec![],
            bad_lines: 0,
            violations: vec![],
            exit_code: Some(0),
            timed_out: false,
            ended_by: None,
            init_before_prompt: None,
            interrupted_by_harness: true,
        };
        let (sum, rep) = claude_summary(&r, Baseline::default());
        assert_eq!(rep.tokens.unwrap().output, 37);
        assert_eq!(sum["usage"]["tokens_status"], "Estimated");
        assert_eq!(sum["usage"]["result_distrusted"], true);
    }

    #[test]
    fn snippets_are_short_and_scrubbed() {
        let long = format!("mail me@example.com {}", "x ".repeat(200));
        let s = snippet(&long);
        assert!(s.chars().count() <= 170 && s.contains("[email]") && !s.contains('@'));
    }
}
