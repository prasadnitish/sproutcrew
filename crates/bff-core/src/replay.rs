//! The replay adapter: recorded, redacted event streams in, normalized events out. It powers contract
//! tests and interface work at no provider cost. Its output is labelled replay and is never used to
//! imply a live run. It invents nothing: a missing count stays missing.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Codex,
    Claude,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdapterEvent {
    Started,
    ItemStarted {
        kind: String,
    },
    ItemCompleted {
        kind: String,
    },
    /// Cumulative total for Codex, or the result's figure for Claude. `None` when the stream has none.
    Usage {
        total_tokens: Option<i64>,
    },
    Window {
        bucket: String,
        percent: Option<i64>,
    },
    Finished {
        status: String,
    },
    Other(String),
}

pub struct Replay {
    events: std::vec::IntoIter<AdapterEvent>,
}

/// The label an interface must show beside replayed output.
pub const LABEL: &str = "replay";

impl Replay {
    /// `fixture` is the JSON array a run wrote to `fixtures/redacted/runs/<ID>/events.json`.
    pub fn new(source: Source, fixture: &Value) -> Result<Replay, String> {
        let list = fixture
            .as_array()
            .ok_or("a fixture is a JSON array of events")?;
        let events: Vec<AdapterEvent> = list
            .iter()
            .flat_map(|e| match source {
                Source::Codex => codex(e),
                Source::Claude => claude(e),
            })
            .collect();
        Ok(Replay {
            events: events.into_iter(),
        })
    }

    pub fn label(&self) -> &'static str {
        LABEL
    }
}

fn str_at<'a>(v: &'a Value, path: &str) -> Option<&'a str> {
    v.pointer(path).and_then(Value::as_str)
}

fn codex(e: &Value) -> Vec<AdapterEvent> {
    let method = str_at(e, "/method").unwrap_or_default();
    let p = e.get("params").unwrap_or(&Value::Null);
    let kind = || str_at(p, "/item/type").unwrap_or_default().to_string();
    vec![match method {
        "turn/started" => AdapterEvent::Started,
        "item/started" => AdapterEvent::ItemStarted { kind: kind() },
        "item/completed" => AdapterEvent::ItemCompleted { kind: kind() },
        "thread/tokenUsage/updated" => AdapterEvent::Usage {
            total_tokens: p
                .pointer("/tokenUsage/total/totalTokens")
                .and_then(Value::as_i64),
        },
        "account/rateLimits/updated" => AdapterEvent::Window {
            bucket: str_at(p, "/rateLimits/limitId")
                .unwrap_or("default")
                .to_string(),
            percent: p
                .pointer("/rateLimits/primary/usedPercent")
                .and_then(Value::as_i64),
        },
        "turn/completed" => AdapterEvent::Finished {
            status: str_at(p, "/turn/status").unwrap_or("unknown").to_string(),
        },
        other => AdapterEvent::Other(other.to_string()),
    }]
}

fn claude(e: &Value) -> Vec<AdapterEvent> {
    match (
        str_at(e, "/type").unwrap_or_default(),
        str_at(e, "/subtype"),
    ) {
        ("system", Some("init")) => vec![AdapterEvent::Started],
        ("system", sub) => vec![AdapterEvent::Other(format!(
            "system/{}",
            sub.unwrap_or_default()
        ))],
        ("assistant", _) => vec![AdapterEvent::ItemCompleted {
            kind: "assistant".into(),
        }],
        ("result", sub) => {
            let n = |k: &str| e.pointer(&format!("/usage/{k}")).and_then(Value::as_i64);
            let parts = [
                "input_tokens",
                "output_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
            ]
            .map(n);
            let total = parts
                .iter()
                .all(Option::is_some)
                .then(|| parts.iter().flatten().sum::<i64>());
            // A zeroed result on an error is a crash artifact, so the count is unknown.
            let total = if e.get("is_error") == Some(&Value::Bool(true)) && total == Some(0) {
                None
            } else {
                total
            };
            vec![
                AdapterEvent::Usage {
                    total_tokens: total,
                },
                AdapterEvent::Finished {
                    status: sub.unwrap_or("unknown").to_string(),
                },
            ]
        }
        (other, _) => vec![AdapterEvent::Other(other.to_string())],
    }
}

impl Iterator for Replay {
    type Item = AdapterEvent;
    fn next(&mut self) -> Option<AdapterEvent> {
        self.events.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all(source: Source, v: Value) -> Vec<AdapterEvent> {
        Replay::new(source, &v).unwrap().collect()
    }

    #[test]
    fn codex_methods_are_normalized() {
        let ev = all(
            Source::Codex,
            json!([
                {"method": "turn/started", "params": {}},
                {"method": "item/started", "params": {"item": {"type": "agentMessage"}}},
                {"method": "item/completed", "params": {"item": {"type": "agentMessage"}}},
                {"method": "thread/tokenUsage/updated", "params": {"tokenUsage": {"total": {"totalTokens": 150}}}},
                {"method": "account/rateLimits/updated", "params": {"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 3}}}},
                {"method": "turn/completed", "params": {"turn": {"status": "completed"}}},
                {"method": "brand/new", "params": {}}
            ]),
        );
        assert_eq!(
            ev,
            vec![
                AdapterEvent::Started,
                AdapterEvent::ItemStarted {
                    kind: "agentMessage".into()
                },
                AdapterEvent::ItemCompleted {
                    kind: "agentMessage".into()
                },
                AdapterEvent::Usage {
                    total_tokens: Some(150)
                },
                AdapterEvent::Window {
                    bucket: "codex".into(),
                    percent: Some(3)
                },
                AdapterEvent::Finished {
                    status: "completed".into()
                },
                AdapterEvent::Other("brand/new".into()),
            ]
        );
    }

    #[test]
    fn claude_events_are_normalized_and_the_result_carries_usage() {
        let ev = all(
            Source::Claude,
            json!([
                {"type": "system", "subtype": "init"},
                {"type": "stream_event", "event": {"type": "message_start"}},
                {"type": "assistant", "message": {"id": "m1"}},
                {"type": "result", "subtype": "success", "usage": {"input_tokens": 5, "output_tokens": 2, "cache_creation_input_tokens": 10, "cache_read_input_tokens": 3}}
            ]),
        );
        assert_eq!(ev[0], AdapterEvent::Started);
        assert!(ev.contains(&AdapterEvent::ItemCompleted {
            kind: "assistant".into()
        }));
        assert!(ev.contains(&AdapterEvent::Usage {
            total_tokens: Some(20)
        }));
        assert_eq!(
            ev.last(),
            Some(&AdapterEvent::Finished {
                status: "success".into()
            })
        );
    }

    #[test]
    fn missing_counts_stay_missing() {
        let ev = all(
            Source::Codex,
            json!([{"method": "thread/tokenUsage/updated", "params": {}}]),
        );
        assert_eq!(ev, vec![AdapterEvent::Usage { total_tokens: None }]);
        let ev = all(
            Source::Claude,
            json!([{"type": "result", "subtype": "success"}]),
        );
        assert_eq!(ev[0], AdapterEvent::Usage { total_tokens: None });
    }

    #[test]
    fn it_is_labelled_replay_and_rejects_a_non_array() {
        assert_eq!(
            Replay::new(Source::Codex, &json!([])).unwrap().label(),
            "replay"
        );
        assert!(Replay::new(Source::Codex, &json!({"not": "an array"})).is_err());
    }
}
