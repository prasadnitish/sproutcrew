//! Scripted stand-ins used only by tests: a fake `codex app-server` and (later) a fake MCP server.

use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn send(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

pub fn fake_codex(mode: &str) {
    if mode.starts_with("turn") {
        return fake_turn(mode);
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut last_reply_was_error = false;
    while let Some(Ok(line)) = lines.next() {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (Some(id), Some(method)) = (
            v.get("id").cloned(),
            v.get("method").and_then(Value::as_str),
        ) else {
            continue;
        };
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        match mode {
            "silent" => {}
            "exit" => return,
            "error" => send(&json!({"id": id, "error": {"code": -32600, "message": "nope"}})),
            "notify-first" => {
                send(&json!({"method": "account/updated", "params": {}}));
                send(&json!({"method": "warning", "params": {"message": "m", "threadId": "t"}}));
                send(&json!({"id": id, "result": {}}));
            }
            "server-request" => match method {
                "turn/start" => {
                    send(
                        &json!({"id": "srv-1", "method": "item/commandExecution/requestApproval", "params": {}}),
                    );
                    if let Some(Ok(reply)) = lines.next() {
                        let r: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
                        last_reply_was_error = r.get("error").is_some();
                    }
                    send(&json!({"id": id, "result": {"ok": true}}));
                }
                _ => send(&json!({"id": id, "result": {"refusalWasError": last_reply_was_error}})),
            },
            "stream" => {
                send(&json!({"id": id, "result": {}}));
                send(
                    &json!({"method": "item/started", "params": {"item": {"type": "agentMessage", "id": "i"}}}),
                );
                send(&json!({"method": "turn/completed", "params": {}}));
            }
            "stderr" => {
                eprintln!("diagnostic: something happened");
                std::thread::sleep(std::time::Duration::from_millis(80));
                send(&json!({"id": id, "result": {}}));
            }
            _ => {
                send(&json!({"id": id, "result": {"echoedMethod": method, "echoedParams": params}}))
            }
        }
    }
}

/// What the fake MCP tool returns: a short digest of its input.
pub fn marker(text: &str) -> String {
    format!(
        "marker:{}",
        &crate::common::sha256_hex(text.as_bytes())[..8]
    )
}

/// A minimal MCP server over stdio with one tool, `echo_marker`.
pub fn fake_mcp() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(method) = v.get("method").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = v.get("id").cloned() else {
            continue;
        }; // notifications get no reply
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => json!({
                "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-03-26")),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "fake", "version": "0"}
            }),
            "ping" => json!({}),
            "tools/list" => json!({"tools": [{
                "name": "echo_marker",
                "description": "Returns a short marker computed from the given text.",
                "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}
            }]}),
            "tools/call" => {
                let text = params["arguments"]["text"].as_str().unwrap_or_default();
                json!({"content": [{"type": "text", "text": marker(text)}]})
            }
            _ => {
                send(
                    &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown method"}}),
                );
                continue;
            }
        };
        send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }
}

fn usage(total: i64) -> Value {
    let b = json!({"inputTokens": total - 10, "cachedInputTokens": 0, "outputTokens": 10, "reasoningOutputTokens": 0, "totalTokens": total});
    json!({"threadId": "t1", "turnId": "u1", "tokenUsage": {"total": b, "last": b}})
}

fn note(method: &str, params: Value) {
    send(&json!({"method": method, "params": params}));
}

fn completed(status: &str) {
    note(
        "turn/completed",
        json!({"threadId": "t1", "turn": {"id": "u1", "items": [], "status": status}}),
    );
}

/// Scripted turns for the runner tests. A reader thread lets the script notice `turn/interrupt` mid-stream.
fn fake_turn(mode: &str) {
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let answer_interrupt = |v: &Value| send(&json!({"id": v["id"], "result": {}}));
    while let Ok(v) = rx.recv() {
        let (Some(id), Some(method)) = (
            v.get("id").cloned(),
            v.get("method").and_then(Value::as_str),
        ) else {
            continue;
        };
        if method != "turn/start" {
            send(&json!({"id": id, "result": {}}));
            continue;
        }
        send(
            &json!({"id": id, "result": {"turn": {"id": "u1", "items": [], "status": "inProgress"}}}),
        );
        note(
            "turn/started",
            json!({"threadId": "t1", "turn": {"id": "u1", "items": [], "status": "inProgress"}}),
        );
        let wait_for_interrupt = |then: &dyn Fn()| {
            while let Ok(m) = rx.recv() {
                if m["method"] == "turn/interrupt" {
                    answer_interrupt(&m);
                    then();
                    completed("interrupted");
                    return;
                }
            }
        };
        match mode {
            "turn" => {
                note(
                    "item/started",
                    json!({"threadId": "t1", "turnId": "u1", "item": {"type": "agentMessage", "id": "i1"}}),
                );
                note("thread/tokenUsage/updated", usage(100));
                note(
                    "item/completed",
                    json!({"threadId": "t1", "turnId": "u1", "item": {"type": "agentMessage", "id": "i1", "text": "hello", "phase": "final_answer"}}),
                );
                note("thread/tokenUsage/updated", usage(150));
                completed("completed");
            }
            "turn-websearch" => {
                note(
                    "item/started",
                    json!({"threadId": "t1", "turnId": "u1", "item": {"type": "webSearch", "id": "w1"}}),
                );
                wait_for_interrupt(&|| {});
            }
            "turn-hang" => wait_for_interrupt(&|| {}),
            "turn-hot" => {
                note(
                    "account/rateLimits/updated",
                    json!({"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 55}}}),
                );
                wait_for_interrupt(&|| {});
            }
            "turn-badusage" => {
                note("thread/tokenUsage/updated", json!({"threadId": "t1"}));
                completed("completed");
            }
            "turn-long" => {
                for _ in 0..300 {
                    note(
                        "item/agentMessage/delta",
                        json!({"threadId": "t1", "turnId": "u1", "itemId": "i1", "delta": "x"}),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    if let Ok(m) = rx.try_recv() {
                        if m["method"] == "turn/interrupt" {
                            answer_interrupt(&m);
                            // Usage reported after the interrupt, as the real server may.
                            note("thread/tokenUsage/updated", usage(77));
                            completed("interrupted");
                            return;
                        }
                    }
                }
                completed("completed");
            }
            _ => completed("completed"),
        }
    }
}
