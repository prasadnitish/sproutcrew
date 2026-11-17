//! The synthetic brief for C1 and the deterministic checks on the artifact. One seeded error shows the
//! mechanism works. It does not show review quality.

use serde_json::{json, Value};

/// The brief. Line 6 is wrong on purpose: 4 weeks of 5 working days is 20 days, not 25.
pub const BRIEF: &str = "# Brief: beta plan for a small team (synthetic)\n\nFacts to use:\n- The team has 3 engineers.\n- The project runs for 4 weeks, with 5 working days per week.\n- Total working days available: 25.\n- Each engineer completes 2 tasks per week.\n- The backlog has 30 tasks.\n";

pub const WRITER_PROMPT: &str = "Read brief.md in the current directory with the shell. Then create artifact.md in the current directory with exactly these headings: Scope, Capacity, Risks, Decision. Under Capacity, copy the stated facts verbatim and state the total working days and the total task capacity. Do not change any number from the brief. Reply done.";

/// The schema Claude's review must satisfy.
pub fn review_schema() -> Value {
    json!({"type": "object", "additionalProperties": false, "required": ["reviewed_sha256", "findings"],
        "properties": {
            "reviewed_sha256": {"type": "string"},
            "findings": {"type": "array", "items": {"type": "object", "additionalProperties": false,
                "required": ["id", "quote", "problem", "fix"],
                "properties": {"id": {"type": "string"}, "quote": {"type": "string"}, "problem": {"type": "string"}, "fix": {"type": "string"}}}}}})
}

pub fn reviewer_prompt(sha: &str) -> String {
    format!("Read artifact.md in the current directory with the Read tool. Its SHA-256 is {sha}. Copy that value into reviewed_sha256. Report each factual or arithmetic inconsistency in the document as a finding with id F1, F2 and so on, an exact quote from the document, the problem, and a precise fix. Report only real errors.")
}

pub fn applier_prompt(id: &str, quote: &str, problem: &str, fix: &str) -> String {
    format!("Apply exactly one review finding to artifact.md in the current directory, and change nothing else. Finding {id}. Quote: \"{quote}\". Problem: {problem}. Fix: {fix}. Edit artifact.md so the quoted text is corrected as the fix says, and leave every other line as it is. Reply done.")
}

#[derive(Debug, PartialEq, Eq)]
pub struct LineDiff {
    pub changed: usize,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// A line-by-line comparison. Lines are compared by position, which is enough for a one-line edit.
pub fn line_diff(before: &str, after: &str) -> LineDiff {
    let (b, a): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    let mut d = LineDiff {
        changed: 0,
        removed: vec![],
        added: vec![],
    };
    for i in 0..b.len().max(a.len()) {
        let (x, y) = (b.get(i), a.get(i));
        if x != y {
            d.changed += 1;
            d.removed.extend(x.map(|s| s.to_string()));
            d.added.extend(y.map(|s| s.to_string()));
        }
    }
    d
}

/// Does the text still carry the seeded error, and does it carry the corrected figure?
pub fn seeded_error_state(text: &str) -> (bool, bool) {
    let has = |n: &str| {
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|tok| tok == n)
    };
    (has("25"), has("20"))
}

/// Pulls the first finding out of a review, if there is one.
pub fn first_finding(review: &Value) -> Option<(String, String, String, String)> {
    let f = review.get("findings")?.as_array()?.first()?;
    let s = |k: &str| f.get(k).and_then(Value::as_str).map(str::to_string);
    Some((s("id")?, s("quote")?, s("problem")?, s("fix")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brief_really_contains_the_seeded_error() {
        let (has_25, has_20) = seeded_error_state(BRIEF);
        assert!(has_25 && !has_20);
        assert!(BRIEF.contains("4 weeks") && BRIEF.contains("5 working days per week"));
    }

    #[test]
    fn a_one_line_edit_is_one_changed_line() {
        let d = line_diff(
            "a\nTotal working days: 25.\nc\n",
            "a\nTotal working days: 20.\nc\n",
        );
        assert_eq!(
            d,
            LineDiff {
                changed: 1,
                removed: vec!["Total working days: 25.".into()],
                added: vec!["Total working days: 20.".into()]
            }
        );
    }

    #[test]
    fn identical_text_has_no_changes_and_added_lines_count() {
        assert_eq!(line_diff("a\nb\n", "a\nb\n").changed, 0);
        let d = line_diff("a\n", "a\nb\nc\n");
        assert_eq!(
            (d.changed, d.added),
            (2, vec!["b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn the_seeded_state_distinguishes_the_error_from_the_fix() {
        assert_eq!(seeded_error_state("Total working days: 20."), (false, true));
        assert_eq!(seeded_error_state("total 25 working days"), (true, false));
        assert_eq!(seeded_error_state("nothing"), (false, false));
        // 125 or 250 are not the seeded number.
        assert_eq!(
            seeded_error_state("about 250 items and 120 hours"),
            (false, false)
        );
    }

    #[test]
    fn the_first_finding_is_extracted_and_an_empty_review_has_none() {
        let r = json!({"reviewed_sha256": "x", "findings": [{"id": "F1", "quote": "q", "problem": "p", "fix": "f"}, {"id": "F2", "quote": "q2", "problem": "p2", "fix": "f2"}]});
        assert_eq!(
            first_finding(&r),
            Some(("F1".into(), "q".into(), "p".into(), "f".into()))
        );
        assert_eq!(
            first_finding(&json!({"reviewed_sha256": "x", "findings": []})),
            None
        );
        assert_eq!(first_finding(&json!({})), None);
    }

    #[test]
    fn prompts_carry_the_hash_and_the_finding_but_no_instruction_to_change_other_lines() {
        assert!(reviewer_prompt("abc123").contains("abc123"));
        let p = applier_prompt("F1", "25", "wrong", "use 20");
        assert!(p.contains("F1") && p.contains("use 20") && p.contains("every other line"));
    }

    #[test]
    fn the_review_schema_is_a_valid_use_of_the_validator() {
        let ok = json!({"reviewed_sha256": "x", "findings": [{"id": "F1", "quote": "q", "problem": "p", "fix": "f"}]});
        assert!(bff_core::schema::validate(&review_schema(), &ok).is_empty());
        assert!(!bff_core::schema::validate(&review_schema(), &json!({"findings": []})).is_empty());
    }
}
