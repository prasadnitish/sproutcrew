//! The decided list (DESIGN_SPEC.md §10, §14): what the person decided, read back from each project's own journal,
//! newest first. It only reads; nothing here decides anything.
use crate::journal::Event;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decided {
    pub project: String,
    pub at: u64,
    pub kind: String,
    /// What was decided, as one sentence.
    pub what: String,
    /// Who decided, when the journal names them. Finding decisions made before 3.0 do not.
    pub by: Option<String>,
    /// The version the decision covered, when it was bound to one.
    pub version: Option<String>,
}

fn text(e: &Event, key: &str) -> String {
    e.payload[key].as_str().unwrap_or_default().to_string()
}

fn opt(e: &Event, key: &str) -> Option<String> {
    e.payload[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// One line for each decision in a project's journal, newest first. Other entries are left out.
pub fn rows(project: &str, events: &[Event]) -> Vec<Decided> {
    let mut out: Vec<Decided> = events
        .iter()
        .filter_map(|e| {
            let (what, version) = match e.kind.as_str() {
                "plan.approved" => ("Approved the plan".to_string(), opt(e, "hash")),
                "result.reviewed" => ("Marked the results reviewed".to_string(), opt(e, "version")),
                "policy.approved" => ("Allowed an action".to_string(), opt(e, "id")),
                "policy.rejected" => ("Refused an action".to_string(), opt(e, "id")),
                "code.applied" => ("Applied a code diff".to_string(), opt(e, "patch_hash")),
                "code.rejected" => ("Rejected a code draft".to_string(), opt(e, "patch_hash")),
                "coordinator.applied" => {
                    let summary = text(e, "summary");
                    (
                        if summary.is_empty() {
                            "Applied a plan change".to_string()
                        } else {
                            format!("Applied a plan change: {summary}")
                        },
                        None,
                    )
                }
                "review.decided" => {
                    let verb = if text(e, "decision") == "accept" {
                        "Accepted"
                    } else {
                        "Rejected"
                    };
                    let reason = text(e, "reason");
                    (
                        format!(
                            "{verb} finding {} on {}{}",
                            text(e, "finding"),
                            text(e, "name"),
                            if reason.is_empty() {
                                String::new()
                            } else {
                                format!(" ({reason})")
                            }
                        ),
                        None,
                    )
                }
                _ => return None,
            };
            Some(Decided {
                project: project.into(),
                at: e.at,
                kind: e.kind.clone(),
                what,
                by: opt(e, "by"),
                version,
            })
        })
        .collect();
    out.sort_by_key(|d| std::cmp::Reverse(d.at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::Journal;
    use serde_json::json;

    #[test]
    fn each_decision_is_one_line_newest_first_with_who_and_the_version_it_covered() {
        let j = Journal::open_in_memory().unwrap();
        j.append("plan.proposed", &json!({}), 1).unwrap();
        j.append(
            "plan.approved",
            &json!({"hash": "9f3c21ab", "by": "Ana Ruiz"}),
            2,
        )
        .unwrap();
        j.append(
            "review.decided",
            &json!({"name": "draft", "finding": "f2", "decision": "reject", "reason": "No source"}),
            3,
        )
        .unwrap();
        j.append(
            "policy.rejected",
            &json!({"id": "act-1", "by": "Ana Ruiz"}),
            4,
        )
        .unwrap();
        j.append("note.added", &json!({"text": "Keep it short"}), 5)
            .unwrap();
        j.append(
            "result.reviewed",
            &json!({"version": "77c1", "by": "Ana Ruiz"}),
            6,
        )
        .unwrap();
        let r = rows("case", &j.events().unwrap());
        let lines: Vec<(&str, u64)> = r.iter().map(|d| (d.what.as_str(), d.at)).collect();
        assert_eq!(
            lines,
            [
                ("Marked the results reviewed", 6),
                ("Refused an action", 4),
                ("Rejected finding f2 on draft (No source)", 3),
                ("Approved the plan", 2),
            ]
        );
        assert_eq!(r[3].by.as_deref(), Some("Ana Ruiz"));
        assert_eq!(r[3].version.as_deref(), Some("9f3c21ab"));
        assert_eq!(r[2].by, None);
        assert!(r.iter().all(|d| d.project == "case"));
    }

    #[test]
    fn a_journal_with_no_decisions_gives_an_empty_list() {
        let j = Journal::open_in_memory().unwrap();
        j.append("plan.proposed", &json!({}), 1).unwrap();
        assert!(rows("case", &j.events().unwrap()).is_empty());
    }
}
