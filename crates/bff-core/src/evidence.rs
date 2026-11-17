//! Evidence classes for claims. E1: a check ran against this claim and this artifact version (a
//! calculation that reproduces). E2: the cited span exists in the stored source, which shows the words are
//! there and not that they support the claim. E3: a person confirmed the decision or assumption. M: the
//! claim rests on a model's say-so. A claim that cites a span that is not there fails, and a check that
//! ran on an older version of the artifact counts for nothing.

use crate::calc::{Record, Verdict};
use crate::sources::{SourceStore, SpanError};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fact,
    Assumption,
    Calculation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimIn {
    pub id: String,
    pub text: String,
    pub kind: Kind,
    /// `src-7#p12`.
    pub source: Option<String>,
    /// The words the claim cites from that source.
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    E1,
    E2,
    E3,
    M,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimEvidence {
    pub id: String,
    pub class: Class,
    /// Whether the claim has the evidence its kind needs. `M` with `passed == false` is a failure.
    pub passed: bool,
    pub notes: Vec<String>,
}

/// A calculation check and the version of the artifact it ran against.
#[derive(Debug, Clone, PartialEq)]
pub struct CalcResult {
    pub record: Record,
    pub artifact_sha256: String,
}

pub struct Context<'a> {
    pub sources: &'a SourceStore,
    pub calcs: &'a [CalcResult],
    /// The artifact version being checked.
    pub artifact_sha256: &'a str,
    /// Claim ids a person confirmed.
    pub confirmed: &'a BTreeSet<String>,
}

/// Claims from the structured block of an artifact: `{"claims": [{"text", "kind", "source", "quote", "id"}]}`.
pub fn parse_claims(v: &Value) -> Result<Vec<ClaimIn>, String> {
    let list = v
        .get("claims")
        .and_then(Value::as_array)
        .ok_or("there is no claims list")?;
    let opt = |c: &Value, k: &str| {
        c.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    list.iter()
        .enumerate()
        .map(|(i, c)| {
            let kind = match opt(c, "kind").as_deref() {
                Some("fact") => Kind::Fact,
                Some("assumption") => Kind::Assumption,
                Some("calculation") => Kind::Calculation,
                other => return Err(format!("claim {} has an unknown kind {other:?}", i + 1)),
            };
            Ok(ClaimIn {
                id: opt(c, "id").unwrap_or_else(|| format!("c{}", i + 1)),
                text: opt(c, "text").ok_or_else(|| format!("claim {} has no text", i + 1))?,
                kind,
                source: opt(c, "source"),
                quote: opt(c, "quote"),
            })
        })
        .collect()
}

pub fn classify(claims: &[ClaimIn], ctx: &Context) -> Result<Vec<ClaimEvidence>, String> {
    let mut seen = BTreeSet::new();
    for c in claims {
        if !seen.insert(c.id.as_str()) {
            return Err(format!("the claim id {:?} is used twice", c.id));
        }
    }
    Ok(claims.iter().map(|c| one(c, ctx)).collect())
}

fn one(c: &ClaimIn, ctx: &Context) -> ClaimEvidence {
    let fail = |note: String| ClaimEvidence {
        id: c.id.clone(),
        class: Class::M,
        passed: false,
        notes: vec![note],
    };
    let pass = |class, note: &str| ClaimEvidence {
        id: c.id.clone(),
        class,
        passed: true,
        notes: vec![note.to_string()],
    };
    match c.kind {
        Kind::Calculation => match ctx.calcs.iter().rev().find(|r| r.record.claim.id == c.id) {
            None => fail("no calculation check has run for this claim".into()),
            Some(r) if r.artifact_sha256 != ctx.artifact_sha256 => {
                fail("the check ran on an older version of the artifact".into())
            }
            Some(r) if r.record.verdict != Verdict::Pass => fail(format!(
                "the calculation does not reproduce: {:?}",
                r.record.verdict
            )),
            Some(_) => pass(Class::E1, "the calculation reproduces on this version"),
        },
        Kind::Assumption => {
            if ctx.confirmed.contains(&c.id) {
                pass(Class::E3, "a person confirmed this assumption")
            } else {
                fail("the assumption is not confirmed by a person".into())
            }
        }
        Kind::Fact => {
            let Some(source) = &c.source else {
                return fail("no source cited".into());
            };
            let quote = c.quote.as_deref().unwrap_or("");
            if quote.trim().is_empty() {
                return fail(format!("{source} is cited with no span"));
            }
            match ctx.sources.find_span(source, quote) {
                Ok(()) => pass(
                    Class::E2,
                    "the cited span exists in the source. That shows the words are there, not that it supports the claim",
                ),
                Err(SpanError::NoSource(id)) => fail(format!("no source {id} is stored")),
                Err(SpanError::NoPage { source, page }) => fail(format!("{source} has no page {page}")),
                Err(SpanError::NoSpan) => fail(format!("the cited span was not found in {source}")),
                Err(other) => fail(format!("the citation could not be checked: {other:?}")),
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub e1: usize,
    pub e2: usize,
    pub e3: usize,
    pub m: usize,
    pub failed: usize,
}

pub fn summarize(items: &[ClaimEvidence]) -> Summary {
    let mut s = Summary::default();
    for i in items {
        match i.class {
            Class::E1 => s.e1 += 1,
            Class::E2 => s.e2 += 1,
            Class::E3 => s.e3 += 1,
            Class::M => s.m += 1,
        }
        if !i.passed {
            s.failed += 1;
        }
    }
    s
}

#[allow(dead_code)]
fn keep(_: Verdict) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::{reproduce, Claim, Input};
    use serde_json::json;

    fn store() -> SourceStore {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("bff-ev-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut s = SourceStore::open(&d).unwrap();
        let _ = s.import(
            "notes.txt",
            b"The pilot costs $5 per license per month. It starts in March.",
        );
        s
    }

    fn inputs() -> Vec<Input> {
        vec![
            Input {
                name: "licenses".into(),
                value: 4.0,
                unit: "".into(),
            },
            Input {
                name: "price".into(),
                value: 5.0,
                unit: "usd/month".into(),
            },
        ]
    }

    fn calc(id: &str, stated: f64, unit: &str, sha: &str) -> CalcResult {
        let claim = Claim {
            id: id.into(),
            formula: "licenses * price".into(),
            stated_value: stated,
            stated_unit: unit.into(),
            tolerance: 0.001,
        };
        CalcResult {
            record: reproduce(&inputs(), &claim),
            artifact_sha256: sha.into(),
        }
    }

    fn claim(id: &str, kind: Kind, source: Option<&str>, quote: Option<&str>) -> ClaimIn {
        ClaimIn {
            id: id.into(),
            text: format!("claim {id}"),
            kind,
            source: source.map(Into::into),
            quote: quote.map(Into::into),
        }
    }

    fn run(claims: &[ClaimIn], calcs: &[CalcResult], confirmed: &[&str]) -> Vec<ClaimEvidence> {
        let s = store();
        let c: BTreeSet<String> = confirmed.iter().map(|x| x.to_string()).collect();
        classify(
            claims,
            &Context {
                sources: &s,
                calcs,
                artifact_sha256: "v2",
                confirmed: &c,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_fact_whose_cited_span_exists_is_e2_and_a_missing_span_fails() {
        let out = run(
            &[
                claim("a", Kind::Fact, Some("src-1#p1"), Some("starts in March")),
                claim("b", Kind::Fact, Some("src-1#p1"), Some("starts in April")),
                claim("c", Kind::Fact, Some("src-1#p1"), None),
                claim("d", Kind::Fact, Some("src-9#p1"), Some("anything")),
                claim("e", Kind::Fact, None, None),
            ],
            &[],
            &[],
        );
        assert_eq!((out[0].class, out[0].passed), (Class::E2, true));
        assert!(
            out[0]
                .notes
                .iter()
                .any(|n| n.contains("not that it supports")),
            "{:?}",
            out[0].notes
        );
        for (i, why) in [
            (1, "not found"),
            (2, "no span"),
            (3, "no source"),
            (4, "no source"),
        ] {
            assert_eq!(
                (out[i].class, out[i].passed),
                (Class::M, false),
                "claim {i}"
            );
            assert!(
                out[i].notes.iter().any(|n| n.to_lowercase().contains(why)),
                "claim {i}: {:?}",
                out[i].notes
            );
        }
    }

    #[test]
    fn a_calculation_is_e1_only_when_it_reproduces_on_this_version() {
        let ok = calc("k1", 20.0, "usd/month", "v2");
        let wrong_number = calc("k2", 25.0, "usd/month", "v2");
        let wrong_unit = calc("k3", 20.0, "usd/year", "v2");
        let old_version = calc("k4", 20.0, "usd/month", "v1");
        let out = run(
            &[
                claim("k1", Kind::Calculation, None, None),
                claim("k2", Kind::Calculation, None, None),
                claim("k3", Kind::Calculation, None, None),
                claim("k4", Kind::Calculation, None, None),
                claim("k5", Kind::Calculation, None, None),
            ],
            &[ok, wrong_number, wrong_unit, old_version],
            &[],
        );
        assert_eq!((out[0].class, out[0].passed), (Class::E1, true));
        assert_eq!((out[1].class, out[1].passed), (Class::M, false));
        assert!(
            out[1].notes.iter().any(|n| n.contains("ValueMismatch")),
            "{:?}",
            out[1].notes
        );
        assert!(
            out[2].notes.iter().any(|n| n.contains("UnitMismatch")),
            "{:?}",
            out[2].notes
        );
        assert!(
            out[3].notes.iter().any(|n| n.contains("older version")),
            "{:?}",
            out[3].notes
        );
        assert!(!out[3].passed);
        assert!(
            out[4]
                .notes
                .iter()
                .any(|n| n.contains("no calculation check")),
            "{:?}",
            out[4].notes
        );
    }

    #[test]
    fn an_assumption_is_e3_only_after_a_person_confirms_it() {
        let c = [
            claim("s1", Kind::Assumption, None, None),
            claim("s2", Kind::Assumption, None, None),
        ];
        let out = run(&c, &[], &["s1"]);
        assert_eq!((out[0].class, out[0].passed), (Class::E3, true));
        assert_eq!((out[1].class, out[1].passed), (Class::M, false));
        assert!(out[1].notes.iter().any(|n| n.contains("not confirmed")));
    }

    #[test]
    fn a_check_for_one_claim_never_validates_another() {
        // A reproducing calculation under another id does not lift a fact.
        let ok = calc("k1", 20.0, "usd/month", "v2");
        let out = run(&[claim("f1", Kind::Fact, None, None)], &[ok], &["k1"]);
        assert_eq!((out[0].class, out[0].passed), (Class::M, false));
    }

    #[test]
    fn duplicate_claim_ids_are_refused_and_the_summary_counts_each_class() {
        let s = store();
        let c = BTreeSet::new();
        let ctx = Context {
            sources: &s,
            calcs: &[],
            artifact_sha256: "v2",
            confirmed: &c,
        };
        assert!(classify(
            &[
                claim("a", Kind::Fact, None, None),
                claim("a", Kind::Fact, None, None)
            ],
            &ctx
        )
        .is_err());
        let out = run(
            &[
                claim("a", Kind::Fact, Some("src-1#p1"), Some("pilot costs")),
                claim("b", Kind::Fact, Some("src-1#p1"), Some("nowhere")),
                claim("c", Kind::Assumption, None, None),
            ],
            &[],
            &["c"],
        );
        assert_eq!(
            summarize(&out),
            Summary {
                e1: 0,
                e2: 1,
                e3: 1,
                m: 1,
                failed: 1
            }
        );
    }

    #[test]
    fn claims_are_read_from_the_structured_block_with_ids_filled_in() {
        let v = json!({"claims": [
            {"text": "Starts in March", "kind": "fact", "source": "src-1#p1", "quote": "starts in March"},
            {"id": "calc-total", "text": "Total is $20", "kind": "calculation"},
            {"text": "Users want it", "kind": "assumption"}]});
        let claims = parse_claims(&v).unwrap();
        assert_eq!(claims.len(), 3);
        assert_eq!(claims[0].id, "c1");
        assert_eq!(claims[1].id, "calc-total");
        assert_eq!(claims[2].kind, Kind::Assumption);
        assert_eq!(claims[0].quote.as_deref(), Some("starts in March"));
        assert!(parse_claims(&json!({})).is_err(), "no claims list");
        assert!(
            parse_claims(&json!({"claims": [{"text": "x", "kind": "rumor"}]})).is_err(),
            "unknown kind"
        );
        assert!(
            parse_claims(&json!({"claims": [{"kind": "fact"}]})).is_err(),
            "no text"
        );
    }
}
