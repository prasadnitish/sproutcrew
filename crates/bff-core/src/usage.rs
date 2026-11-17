//! Usage as rows a person can read: one row for each request recorded in a project's journal, with when it ran,
//! which step and role it was for, which provider and model answered, and its tokens and cost. A figure that was
//! not reported stays `None` and is never read as zero. The screen groups these rows by day, month, role or model;
//! the same rows are what the CSV export writes.

use crate::journal::Event;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    /// Seconds since 1970, when the usage was recorded.
    pub at: u64,
    /// The run's task, such as `t4`, or `plan` for the coordinator's plan.
    pub task: String,
    /// The plan step the task ran, when the journal says.
    pub stage: Option<String>,
    /// The role that owns that step in the plan.
    pub role: Option<String>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub tokens: Option<f64>,
    pub dollars: Option<f64>,
    /// How the dollar figure is known: `reported`, `estimated`, `reconciled` or `unknown`.
    pub dollars_status: String,
}

/// One row for each task that recorded usage, in the order it was recorded. `roles` maps a plan step to its role.
pub fn rows(events: &[Event], roles: &BTreeMap<String, String>) -> Vec<Row> {
    let mut stage_of: BTreeMap<&str, &str> = BTreeMap::new();
    let mut ran: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for e in events {
        let task = e.payload["task"].as_str();
        match (e.kind.as_str(), task) {
            ("worker.dispatched", Some(t)) => {
                if let Some(s) = e.payload["stage"].as_str() {
                    stage_of.insert(t, s);
                }
            }
            ("worker.timing", Some(t)) => {
                if let (Some(r), Some(m)) =
                    (e.payload["route"].as_str(), e.payload["model"].as_str())
                {
                    ran.insert(t, (r, m));
                }
            }
            _ => {}
        }
    }
    let mut out: Vec<Row> = Vec::new();
    for e in events.iter().filter(|e| e.kind == "usage") {
        let Some(task) = e.payload["step"].as_str() else {
            continue;
        };
        let meter = e.payload["meter"].as_str().unwrap_or("");
        if meter != "tokens" && meter != "api_dollars" {
            continue;
        }
        let i = match out.iter().rposition(|r| r.task == task) {
            Some(i) => i,
            None => {
                let stage = stage_of.get(task).map(|s| s.to_string());
                let (route, model) = match ran.get(task) {
                    Some((r, m)) => (Some(r.to_string()), Some(m.to_string())),
                    None => (None, None),
                };
                out.push(Row {
                    at: e.at,
                    task: task.to_string(),
                    role: stage.as_ref().and_then(|s| roles.get(s).cloned()),
                    stage,
                    route,
                    model,
                    tokens: None,
                    dollars: None,
                    dollars_status: "unknown".into(),
                });
                out.len() - 1
            }
        };
        let value = e.payload["value"].as_f64();
        let r = &mut out[i];
        match meter {
            "tokens" => {
                if let Some(v) = value {
                    *r.tokens.get_or_insert(0.0) += v;
                }
                if r.model.is_none() {
                    r.model = e.payload["detail"].as_str().map(str::to_string);
                }
            }
            _ => {
                if let Some(v) = value {
                    *r.dollars.get_or_insert(0.0) += v;
                    r.dollars_status = e.payload["status"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                }
            }
        }
    }
    out
}

/// Totals of the rows recorded at or after `since`: known dollars, requests with no known cost, known tokens.
pub fn totals_since(rows: &[Row], since: u64) -> (f64, usize, f64) {
    rows.iter()
        .filter(|r| r.at >= since)
        .fold((0.0, 0, 0.0), |(d, u, t), r| {
            (
                d + r.dollars.unwrap_or(0.0),
                u + usize::from(r.dollars.is_none()),
                t + r.tokens.unwrap_or(0.0),
            )
        })
}

/// The menu bar's line for today: "Today: $0.42 · 12,300 tokens", with any unknown costs said in words.
pub fn today_line(dollars: f64, unknown: usize, tokens: f64) -> String {
    let d = if dollars == 0.0 {
        "$0".to_string()
    } else if dollars < 0.01 {
        "under $0.01".to_string()
    } else {
        format!("${dollars:.2}")
    };
    let digits = (tokens.round() as u64).to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let more = if unknown > 0 {
        format!(" + {unknown} unknown")
    } else {
        String::new()
    };
    format!("Today: {d}{more} · {grouped} tokens")
}

fn csv_field(s: &str) -> String {
    // A leading = + - @ would be read as a formula by a spreadsheet, so such a field is quoted with a leading '.
    let s = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) || s.starts_with('\'') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// The rows as CSV with a header. Unknown figures are empty cells with the word `unknown` beside them, never 0.
pub fn csv(project: &str, rows: &[Row]) -> String {
    let mut out = String::from(
        "project,recorded_at_utc,task,step,role,route,model,tokens,dollars,dollars_status\n",
    );
    for r in rows {
        let cells = [
            csv_field(project),
            crate::changelog::utc(r.at),
            csv_field(&r.task),
            csv_field(r.stage.as_deref().unwrap_or("")),
            csv_field(r.role.as_deref().unwrap_or("")),
            csv_field(r.route.as_deref().unwrap_or("")),
            csv_field(r.model.as_deref().unwrap_or("")),
            r.tokens.map_or("unknown".into(), |t| format!("{t}")),
            r.dollars.map_or(String::new(), |d| format!("{d:.6}")),
            r.dollars_status.clone(),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(id: i64, kind: &str, payload: serde_json::Value, at: u64) -> Event {
        Event {
            id,
            kind: kind.into(),
            payload,
            at,
        }
    }

    fn journal() -> Vec<Event> {
        vec![
            ev(
                1,
                "usage",
                json!({"step": "plan", "meter": "tokens", "value": 900.0, "status": "reported", "detail": "anthropic/claude-haiku-5.5"}),
                100,
            ),
            ev(
                2,
                "worker.dispatched",
                json!({"task": "t1", "stage": "frame", "kind": "stage"}),
                200,
            ),
            ev(
                3,
                "worker.timing",
                json!({"task": "t1", "route": "R12", "model": "anthropic/claude-haiku-5.5", "secs": 4.0}),
                210,
            ),
            ev(
                4,
                "usage",
                json!({"step": "t1", "meter": "tokens", "value": 1500.0, "status": "reported", "detail": "anthropic/claude-haiku-5.5"}),
                210,
            ),
            ev(
                5,
                "usage",
                json!({"step": "t1", "meter": "api_dollars", "value": 0.0021, "status": "reported", "detail": "api"}),
                210,
            ),
            ev(
                6,
                "usage",
                json!({"step": "t1", "meter": "plan_window", "value": null, "status": "unknown", "detail": "api"}),
                210,
            ),
            ev(
                7,
                "worker.dispatched",
                json!({"task": "t2", "stage": "customers", "kind": "stage"}),
                300,
            ),
            ev(
                8,
                "worker.timing",
                json!({"task": "t2", "route": "R12", "model": "qwen/qwen3.8-omni-flash", "secs": 300.0}),
                600,
            ),
            ev(
                9,
                "usage",
                json!({"step": "t2", "meter": "tokens", "value": null, "status": "unknown", "detail": "qwen/qwen3.8-omni-flash"}),
                600,
            ),
            ev(
                10,
                "usage",
                json!({"step": "t2", "meter": "api_dollars", "value": null, "status": "unknown", "detail": "api"}),
                600,
            ),
        ]
    }

    #[test]
    fn each_task_becomes_one_row_with_its_step_role_and_model_and_unknown_stays_unknown() {
        let roles = BTreeMap::from([("frame".to_string(), "facilitator".to_string())]);
        let r = rows(&journal(), &roles);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].task, "plan");
        assert_eq!(r[0].model.as_deref(), Some("anthropic/claude-haiku-5.5"));
        assert_eq!((r[0].stage.as_deref(), r[0].role.as_deref()), (None, None));
        assert_eq!(r[1].stage.as_deref(), Some("frame"));
        assert_eq!(r[1].role.as_deref(), Some("facilitator"));
        assert_eq!(r[1].route.as_deref(), Some("R12"));
        assert_eq!((r[1].tokens, r[1].dollars), (Some(1500.0), Some(0.0021)));
        assert_eq!(r[1].dollars_status, "reported");
        assert_eq!(r[1].at, 210);
        assert_eq!(
            (r[2].tokens, r[2].dollars),
            (None, None),
            "unknown is never zero"
        );
        assert_eq!(r[2].dollars_status, "unknown");
    }

    #[test]
    fn today_counts_only_rows_since_the_start_of_the_day_and_says_unknown_in_words() {
        let r = rows(&journal(), &BTreeMap::new());
        let (d, u, t) = totals_since(&r, 200);
        assert!((d - 0.0021).abs() < 1e-12);
        assert_eq!((u, t), (1, 1500.0));
        assert_eq!(
            today_line(d, u, t),
            "Today: under $0.01 + 1 unknown · 1,500 tokens"
        );
        assert_eq!(today_line(0.0, 0, 0.0), "Today: $0 · 0 tokens");
        assert_eq!(
            today_line(1.5, 0, 1234567.0),
            "Today: $1.50 · 1,234,567 tokens"
        );
    }

    #[test]
    fn the_csv_has_a_header_writes_unknown_in_words_and_cannot_start_a_formula() {
        let r = rows(&journal(), &BTreeMap::new());
        let text = csv("=cmd|calc", &r);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "project,recorded_at_utc,task,step,role,route,model,tokens,dollars,dollars_status"
        );
        assert_eq!(lines.len(), 4);
        assert!(lines[2].contains(",1500,0.002100,reported"), "{}", lines[2]);
        assert!(lines[3].ends_with(",unknown,,unknown"), "{}", lines[3]);
        assert!(lines[1].starts_with("\"'=cmd|calc\","), "{}", lines[1]);
    }
}
