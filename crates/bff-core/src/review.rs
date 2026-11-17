//! The reviewer's report and what happens to it. A report names the exact version it reviewed, so a report
//! on an old version is refused. A clean report is a result ("reviewed, no change needed"), a rejected
//! finding keeps the reason it was rejected, and the same findings coming back unchanged stop the loop.

use crate::artifact::{Refusal as ArtifactRefusal, Registry, ReviewState};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub claim: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub reviewed_sha256: String,
    pub findings: Vec<Finding>,
}

/// The structured output the reviewer must return.
pub fn report_schema() -> Value {
    let s = json!({"type": "string"});
    json!({"type": "object", "additionalProperties": false, "required": ["reviewed_sha256", "findings"], "properties": {
        "reviewed_sha256": s,
        "findings": {"type": "array", "items": {"type": "object", "additionalProperties": false,
            "required": ["id", "claim", "evidence"], "properties": {"id": s, "claim": s, "evidence": s}}}}})
}

/// The prompt for a reviewer. The artifact's hash is given so the reviewer can echo it back.
pub fn review_prompt(brief: &str, artifact_file: &str, sha256: &str) -> String {
    format!(
        "You are a reviewer on a team. The brief the author worked from is:\n{brief}\n\nRead the file {artifact_file} in the current directory. Its SHA-256 is {sha256}; copy that value into reviewed_sha256 exactly.\nList each concrete problem you find as a finding with a short id, a claim, and the evidence (a quote or line). Report only problems you can point to. If you find none, return an empty findings list.\nDo not edit or change any file. Reply only with the report."
    )
}

/// Builds a report from the reviewer's JSON. Shape errors are reported, not guessed at.
pub fn parse_report(v: &Value) -> Result<Report, String> {
    let text = |x: &Value, k: &str| {
        x.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    let reviewed_sha256 = text(v, "reviewed_sha256").ok_or("the report has no reviewed_sha256")?;
    let list = v
        .get("findings")
        .and_then(Value::as_array)
        .ok_or("the report has no findings list")?;
    let mut seen = BTreeSet::new();
    let mut findings = Vec::new();
    for item in list {
        let f = Finding {
            id: text(item, "id").ok_or("a finding has no id")?,
            claim: text(item, "claim").ok_or("a finding has no claim")?,
            evidence: text(item, "evidence").ok_or("a finding has no evidence")?,
        };
        if !seen.insert(f.id.clone()) {
            return Err(format!("the finding id {:?} is used twice", f.id));
        }
        findings.push(f);
    }
    Ok(Report {
        reviewed_sha256,
        findings,
    })
}

/// A finding's identity across rounds: the claim with case and spacing removed.
fn fingerprint(f: &Finding) -> String {
    f.claim
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The reviewer found nothing. A result, not a gap.
    ReviewedNoChange,
    /// These findings need a decision from the author.
    Open(Vec<String>),
    /// The same findings came back unchanged after a round. Stop and hand it to a person.
    NoProgress,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The report is about a version that is not the current one.
    StaleReport {
        reviewed: String,
        head: String,
    },
    Artifact(ArtifactRefusal),
    UnknownFinding(String),
    /// A rejection needs a reason.
    NoReason(String),
    AlreadyDecided(String),
}

impl From<ArtifactRefusal> for Refusal {
    fn from(a: ArtifactRefusal) -> Self {
        Refusal::Artifact(a)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject { reason: String },
}

#[derive(Default)]
pub struct Review {
    open: Vec<Finding>,
    decisions: BTreeMap<String, Decision>,
    previous: Option<BTreeSet<String>>,
}

impl Review {
    /// A report arrives. It is checked against the artifact's current version, recorded as that reviewer's
    /// review, and compared with the last round.
    pub fn receive(
        &mut self,
        reg: &mut Registry,
        artifact: &str,
        reviewer: &str,
        report: Report,
    ) -> Result<Outcome, Refusal> {
        let head = reg.head(artifact)?.sha256.clone();
        if report.reviewed_sha256 != head {
            return Err(Refusal::StaleReport {
                reviewed: report.reviewed_sha256,
                head,
            });
        }
        reg.record_review(artifact, reviewer, &head)?;
        self.decisions.clear();
        if report.findings.is_empty() {
            self.open.clear();
            self.previous = None;
            return Ok(Outcome::ReviewedNoChange);
        }
        let prints: BTreeSet<String> = report.findings.iter().map(fingerprint).collect();
        let repeated = self.previous.as_ref() == Some(&prints);
        self.previous = Some(prints);
        let ids = report.findings.iter().map(|f| f.id.clone()).collect();
        self.open = report.findings;
        Ok(if repeated {
            Outcome::NoProgress
        } else {
            Outcome::Open(ids)
        })
    }

    pub fn open_findings(&self) -> &[Finding] {
        &self.open
    }

    pub fn decide(&mut self, id: &str, d: Decision) -> Result<(), Refusal> {
        if !self.open.iter().any(|f| f.id == id) {
            return Err(Refusal::UnknownFinding(id.to_string()));
        }
        if self.decisions.contains_key(id) {
            return Err(Refusal::AlreadyDecided(id.to_string()));
        }
        if let Decision::Reject { reason } = &d {
            if reason.trim().is_empty() {
                return Err(Refusal::NoReason(id.to_string()));
            }
        }
        self.decisions.insert(id.to_string(), d);
        Ok(())
    }

    pub fn decision(&self, id: &str) -> Option<&Decision> {
        self.decisions.get(id)
    }

    /// Findings the author has not decided yet.
    pub fn undecided(&self) -> Vec<&Finding> {
        self.open
            .iter()
            .filter(|f| !self.decisions.contains_key(&f.id))
            .collect()
    }

    /// The accepted findings, which the author must address with a change.
    pub fn accepted(&self) -> Vec<&Finding> {
        self.open
            .iter()
            .filter(|f| self.decisions.get(&f.id) == Some(&Decision::Accept))
            .collect()
    }

    /// Whether the reviewer's last review still covers the artifact, or a re-review is needed.
    pub fn needs_rereview(reg: &Registry, artifact: &str, reviewer: &str) -> Result<bool, Refusal> {
        Ok(reg.review_state(artifact, reviewer)? != ReviewState::Valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reg() -> Registry {
        let mut r = Registry::default();
        r.create("note", "writer", "h1").unwrap();
        r
    }

    fn f(id: &str, claim: &str) -> Finding {
        Finding {
            id: id.into(),
            claim: claim.into(),
            evidence: "line 3".into(),
        }
    }

    fn rep(hash: &str, findings: Vec<Finding>) -> Report {
        Report {
            reviewed_sha256: hash.into(),
            findings,
        }
    }

    #[test]
    fn a_clean_report_is_reviewed_with_no_change_needed_and_counts_as_a_valid_review() {
        let (mut r, mut rv) = (reg(), Review::default());
        let out = rv
            .receive(&mut r, "note", "critic", rep("h1", vec![]))
            .unwrap();
        assert_eq!(out, Outcome::ReviewedNoChange);
        assert_eq!(
            r.review_state("note", "critic").unwrap(),
            ReviewState::Valid
        );
        assert!(rv.open_findings().is_empty());
        assert!(!Review::needs_rereview(&r, "note", "critic").unwrap());
    }

    #[test]
    fn a_report_on_an_old_version_is_refused_and_records_no_review() {
        let (mut r, mut rv) = (reg(), Review::default());
        r.change("note", "writer", "h1", "h2", true).unwrap();
        let err = rv
            .receive(
                &mut r,
                "note",
                "critic",
                rep("h1", vec![f("a", "wrong total")]),
            )
            .unwrap_err();
        assert_eq!(
            err,
            Refusal::StaleReport {
                reviewed: "h1".into(),
                head: "h2".into()
            }
        );
        assert_eq!(
            r.review_state("note", "critic").unwrap(),
            ReviewState::NoReview
        );
        assert!(rv.open_findings().is_empty());
    }

    #[test]
    fn a_report_on_a_hash_that_was_never_a_version_is_refused() {
        let (mut r, mut rv) = (reg(), Review::default());
        let err = rv
            .receive(&mut r, "note", "critic", rep("invented", vec![]))
            .unwrap_err();
        assert!(matches!(err, Refusal::StaleReport { .. }));
    }

    #[test]
    fn findings_open_until_the_author_decides_and_a_rejection_keeps_its_reason() {
        let (mut r, mut rv) = (reg(), Review::default());
        let out = rv
            .receive(
                &mut r,
                "note",
                "critic",
                rep("h1", vec![f("a", "total is 25"), f("b", "typo")]),
            )
            .unwrap();
        assert_eq!(out, Outcome::Open(vec!["a".into(), "b".into()]));
        assert_eq!(rv.undecided().len(), 2);
        rv.decide("a", Decision::Accept).unwrap();
        rv.decide(
            "b",
            Decision::Reject {
                reason: "intended spelling in the brief".into(),
            },
        )
        .unwrap();
        assert!(rv.undecided().is_empty());
        assert_eq!(
            rv.accepted()
                .iter()
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert_eq!(
            rv.decision("b"),
            Some(&Decision::Reject {
                reason: "intended spelling in the brief".into()
            })
        );
    }

    #[test]
    fn a_rejection_without_a_reason_and_a_decision_on_nothing_are_refused() {
        let (mut r, mut rv) = (reg(), Review::default());
        rv.receive(&mut r, "note", "critic", rep("h1", vec![f("a", "x")]))
            .unwrap();
        assert_eq!(
            rv.decide(
                "a",
                Decision::Reject {
                    reason: "  ".into()
                }
            ),
            Err(Refusal::NoReason("a".into()))
        );
        assert!(
            rv.decision("a").is_none(),
            "a refused decision leaves no trace"
        );
        assert_eq!(
            rv.decide("zzz", Decision::Accept),
            Err(Refusal::UnknownFinding("zzz".into()))
        );
        rv.decide("a", Decision::Accept).unwrap();
        assert_eq!(
            rv.decide("a", Decision::Accept),
            Err(Refusal::AlreadyDecided("a".into()))
        );
    }

    #[test]
    fn an_accepted_finding_leads_to_a_material_change_and_the_review_needs_repeating() {
        let (mut r, mut rv) = (reg(), Review::default());
        rv.receive(
            &mut r,
            "note",
            "critic",
            rep("h1", vec![f("a", "total is 25")]),
        )
        .unwrap();
        rv.decide("a", Decision::Accept).unwrap();
        r.change("note", "writer", "h1", "h2", true).unwrap();
        assert!(Review::needs_rereview(&r, "note", "critic").unwrap());
        // The re-review of the new version comes back clean.
        let out = rv
            .receive(&mut r, "note", "critic", rep("h2", vec![]))
            .unwrap();
        assert_eq!(out, Outcome::ReviewedNoChange);
        assert!(!Review::needs_rereview(&r, "note", "critic").unwrap());
        assert!(
            rv.open_findings().is_empty(),
            "a new round replaces the old findings"
        );
    }

    #[test]
    fn the_same_findings_coming_back_unchanged_stop_the_loop() {
        let (mut r, mut rv) = (reg(), Review::default());
        rv.receive(
            &mut r,
            "note",
            "critic",
            rep("h1", vec![f("a", "Total is 25")]),
        )
        .unwrap();
        rv.decide("a", Decision::Accept).unwrap();
        // The author changes the file but not the thing the reviewer objects to.
        r.change("note", "writer", "h1", "h2", true).unwrap();
        // Same claim, different id and different spacing and case: still the same finding.
        let out = rv
            .receive(
                &mut r,
                "note",
                "critic",
                rep("h2", vec![f("z9", "  total is   25 ")]),
            )
            .unwrap();
        assert_eq!(out, Outcome::NoProgress);
    }

    #[test]
    fn a_different_finding_in_the_next_round_is_progress() {
        let (mut r, mut rv) = (reg(), Review::default());
        rv.receive(
            &mut r,
            "note",
            "critic",
            rep("h1", vec![f("a", "total is 25")]),
        )
        .unwrap();
        r.change("note", "writer", "h1", "h2", true).unwrap();
        let out = rv
            .receive(
                &mut r,
                "note",
                "critic",
                rep("h2", vec![f("a", "heading is missing")]),
            )
            .unwrap();
        assert_eq!(out, Outcome::Open(vec!["a".into()]));
    }

    #[test]
    fn a_reviewer_cannot_change_the_artifact_through_the_report_path() {
        // Findings are data. Applying one is a change by the owner, and the reviewer is refused.
        let (mut r, mut rv) = (reg(), Review::default());
        rv.receive(&mut r, "note", "critic", rep("h1", vec![f("a", "x")]))
            .unwrap();
        assert!(r.change("note", "critic", "h1", "h2", true).is_err());
        assert_eq!(r.head("note").unwrap().sha256, "h1");
    }

    #[test]
    fn parse_report_reads_the_schema_shape_and_rejects_the_rest() {
        let ok = json!({"reviewed_sha256": "h1", "findings": [{"id": "a", "claim": "c", "evidence": "e"}]});
        let rep = parse_report(&ok).unwrap();
        assert_eq!(rep.reviewed_sha256, "h1");
        assert_eq!(
            rep.findings,
            vec![Finding {
                id: "a".into(),
                claim: "c".into(),
                evidence: "e".into()
            }]
        );
        assert!(parse_report(&json!({"findings": []})).is_err(), "no hash");
        assert!(
            parse_report(&json!({"reviewed_sha256": "h1"})).is_err(),
            "no findings list"
        );
        assert!(
            parse_report(&json!({"reviewed_sha256": "", "findings": []})).is_err(),
            "blank hash"
        );
        let blank = json!({"reviewed_sha256": "h1", "findings": [{"id": "a", "claim": " ", "evidence": "e"}]});
        assert!(parse_report(&blank).is_err(), "blank claim");
        let dup = json!({"reviewed_sha256": "h1", "findings": [
            {"id": "a", "claim": "c", "evidence": "e"}, {"id": "a", "claim": "d", "evidence": "e"}]});
        assert!(parse_report(&dup).is_err(), "duplicate ids");
    }

    #[test]
    fn the_review_prompt_names_the_file_and_the_hash_and_forbids_edits() {
        let p = review_prompt("write a note", "artifact.md", "abc123");
        assert!(p.contains("artifact.md") && p.contains("abc123") && p.contains("write a note"));
        assert!(
            p.to_lowercase().contains("do not edit") || p.to_lowercase().contains("do not change")
        );
    }

    #[test]
    fn the_schema_requires_the_hash_and_findings() {
        let s = report_schema();
        assert_eq!(s["required"], json!(["reviewed_sha256", "findings"]));
        assert_eq!(s["additionalProperties"], json!(false));
    }
}
