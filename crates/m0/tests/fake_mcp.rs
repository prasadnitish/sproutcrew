//! The fake MCP server used by A4, spoken to over stdio the way Claude would.

use bff_core::codex::{parse_message, Message};
use bff_core::process::spawn;
use m0::fake::marker;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

fn ask(sup: &mut bff_core::process::Supervised, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let stdin = sup.stdin.as_mut().unwrap();
    writeln!(stdin, "{line}").unwrap();
    stdin.flush().unwrap();
    loop {
        match sup
            .lines
            .recv_timeout(Duration::from_secs(5))
            .expect("server answers")
        {
            bff_core::process::Line::Out(s) => {
                if let Ok(Some(Message::Response { id: got, result })) = parse_message(&s) {
                    if got == json!(id) {
                        return result;
                    }
                }
            }
            bff_core::process::Line::Err(_) => {}
        }
    }
}

#[test]
fn speaks_enough_mcp_for_a_client_to_list_and_call_its_tool() {
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let mut sup = spawn(
        Path::new(env!("CARGO_BIN_EXE_m0")),
        &["__fake-mcp".into()],
        &env,
        Path::new("/private/tmp"),
    )
    .unwrap();

    let init = ask(
        &mut sup,
        1,
        "initialize",
        json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
    );
    assert_eq!(init["serverInfo"]["name"], "fake");
    assert!(init["capabilities"]["tools"].is_object());
    writeln!(
        sup.stdin.as_mut().unwrap(),
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();

    let tools = ask(&mut sup, 2, "tools/list", json!({}));
    assert_eq!(tools["tools"][0]["name"], "echo_marker");
    assert_eq!(tools["tools"][0]["inputSchema"]["required"][0], "text");

    let call = ask(
        &mut sup,
        3,
        "tools/call",
        json!({"name": "echo_marker", "arguments": {"text": "abc"}}),
    );
    assert_eq!(call["content"][0]["text"], marker("abc"));
    sup.cancel(
        |_| {},
        bff_core::process::Graces {
            after_interrupt: Duration::from_millis(300),
            after_term: Duration::from_millis(300),
        },
    );
}

#[test]
fn the_marker_is_deterministic_and_input_dependent() {
    assert_eq!(marker("abc"), marker("abc"));
    assert_ne!(marker("abc"), marker("abd"));
    assert!(marker("abc").starts_with("marker:"));
    assert_eq!(marker("abc").len(), "marker:".len() + 8);
}
