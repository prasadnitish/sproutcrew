//! Lessons collected during a session. A factual lesson is validated only by a passing E1 check on the claim
//! tied to it. A preference or process lesson is validated only by a person's confirmation (E3) and stays
//! labelled as a decision. E2 supports a factual lesson without validating it, and model agreement (M)
//! validates nothing, however many models agree. In M3 they are collected and shown at the end of a session.

use crate::evidence::{ClaimEvidence, Class};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LessonClass {
    Factual,
    Preference,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Candidate,
    Validated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub text: String,
    pub class: LessonClass,
    pub status: Status,
    /// For a factual lesson, the claim whose check validates it.
    pub claim: Option<String>,
    pub notes: Vec<String>,
    pub confirmed_by: Option<String>,
}

pub struct Lessons {
    path: PathBuf,
    items: Vec<Lesson>,
}

impl Lessons {
    pub fn open(project_root: &Path) -> Result<Lessons, String> {
        let path = project_root.join("lessons.json");
        let items = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("lessons.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Lessons { path, items })
    }

    fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.items).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> &[Lesson] {
        &self.items
    }

    /// A new candidate. A factual lesson names the claim that will validate it.
    pub fn add(
        &mut self,
        text: &str,
        class: LessonClass,
        claim: Option<&str>,
    ) -> Result<String, String> {
        if text.trim().is_empty() {
            return Err("a lesson needs text".into());
        }
        if class == LessonClass::Factual && claim.is_none_or(|c| c.trim().is_empty()) {
            return Err("a factual lesson must name the claim that validates it".into());
        }
        let next = self
            .items
            .iter()
            .filter_map(|l| l.id.trim_start_matches('L').parse::<usize>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let id = format!("L{next}");
        self.items.push(Lesson {
            id: id.clone(),
            text: text.trim().to_string(),
            class,
            status: Status::Candidate,
            claim: claim.map(str::to_string),
            notes: vec![],
            confirmed_by: None,
        });
        self.save()?;
        Ok(id)
    }

    /// Evidence for the lesson's claim. Only a passing E1 validates a factual lesson. Evidence about any other
    /// claim is refused.
    pub fn attach_evidence(&mut self, id: &str, ev: &ClaimEvidence) -> Result<Status, String> {
        let l = self
            .items
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| format!("no lesson {id}"))?;
        if l.class != LessonClass::Factual {
            return Err("only a factual lesson takes claim evidence. A decision lesson needs a person to confirm it".into());
        }
        if l.claim.as_deref() != Some(ev.id.as_str()) {
            return Err(format!(
                "this evidence is for claim {}, not the lesson's claim",
                ev.id
            ));
        }
        match (ev.class, ev.passed) {
            (Class::E1, true) => l.status = Status::Validated,
            (Class::E2, true) => l
                .notes
                .push("supported by a citation that exists; that does not validate it".into()),
            (Class::E3, _) => l
                .notes
                .push("a person's confirmation records a decision, not a verified fact".into()),
            _ => l
                .notes
                .push("the evidence did not pass; the lesson stays a candidate".into()),
        }
        let s = l.status;
        self.save()?;
        Ok(s)
    }

    /// A person confirms a preference or process lesson. A factual one cannot be confirmed this way.
    pub fn confirm(&mut self, id: &str, by: &str) -> Result<(), String> {
        let l = self
            .items
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| format!("no lesson {id}"))?;
        if l.class == LessonClass::Factual {
            return Err("a factual lesson is validated by a check, not by confirmation".into());
        }
        if by.trim().is_empty() {
            return Err("say who confirms it".into());
        }
        l.status = Status::Validated;
        l.confirmed_by = Some(by.trim().to_string());
        self.save()
    }

    /// Several models agreeing is recorded and changes nothing.
    pub fn note_model_agreement(&mut self, id: &str, models: usize) -> Result<(), String> {
        let l = self
            .items
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| format!("no lesson {id}"))?;
        l.notes.push(format!(
            "{models} model(s) agree. Model agreement does not validate a lesson"
        ));
        self.save()
    }

    /// Validated lessons, which apply to this project's later work without asking again.
    pub fn applied(&self) -> Vec<&Lesson> {
        self.items
            .iter()
            .filter(|l| l.status == Status::Validated)
            .collect()
    }

    /// Text for a worker's prompt, or nothing if no lesson is applied. Each line names the lesson so it can be removed.
    pub fn applied_block(&self) -> String {
        let applied = self.applied();
        if applied.is_empty() {
            return String::new();
        }
        let mut s = String::from(
            "Lessons already validated for this project (the person can remove any of them):\n",
        );
        for l in applied {
            s.push_str(&format!("- [{}] {}\n", l.id, l.text));
        }
        s.push_str("To remove a lesson, the person uses remove with its id.\n");
        s
    }

    /// A person takes a lesson out.
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.items.len();
        self.items.retain(|l| l.id != id);
        if self.items.len() == before {
            return Err(format!("no lesson {id}"));
        }
        self.save()
    }

    /// The end-of-session list.
    pub fn session_end_markdown(&self) -> String {
        let mut s = String::from("## Lessons from this session\n\n");
        if self.items.is_empty() {
            return s + "None collected.\n";
        }
        for (title, status) in [
            ("Validated", Status::Validated),
            ("Candidates", Status::Candidate),
        ] {
            let rows: Vec<&Lesson> = self.items.iter().filter(|l| l.status == status).collect();
            if rows.is_empty() {
                continue;
            }
            s.push_str(&format!("### {title}\n\n"));
            for l in rows {
                let basis = match (l.status, l.class) {
                    (Status::Validated, LessonClass::Factual) => " (checked: E1)".to_string(),
                    (Status::Validated, _) => format!(
                        " (decision, confirmed by {})",
                        l.confirmed_by.as_deref().unwrap_or("a person")
                    ),
                    (Status::Candidate, _) => String::new(),
                };
                s.push_str(&format!("- {}: {}{basis}\n", l.id, l.text));
            }
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lessons(tag: &str) -> Lessons {
        let d = std::env::temp_dir().join(format!("bff-lessons-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Lessons::open(&d).unwrap()
    }

    fn ev(id: &str, class: Class, passed: bool) -> ClaimEvidence {
        ClaimEvidence {
            id: id.into(),
            class,
            passed,
            notes: vec![],
        }
    }

    #[test]
    fn a_factual_lesson_is_validated_only_by_a_passing_e1_on_its_own_claim() {
        let mut l = lessons("fact");
        let id = l
            .add(
                "Monthly total is licenses times price",
                LessonClass::Factual,
                Some("k1"),
            )
            .unwrap();
        assert_eq!(
            l.attach_evidence(&id, &ev("k1", Class::E2, true)).unwrap(),
            Status::Candidate,
            "a citation supports, it does not validate"
        );
        assert_eq!(
            l.attach_evidence(&id, &ev("k1", Class::M, false)).unwrap(),
            Status::Candidate
        );
        assert_eq!(
            l.attach_evidence(&id, &ev("k1", Class::E1, false)).unwrap(),
            Status::Candidate,
            "a failed check"
        );
        assert!(
            l.attach_evidence(&id, &ev("other", Class::E1, true))
                .is_err(),
            "a check on another claim counts for nothing"
        );
        assert_eq!(l.list()[0].status, Status::Candidate);
        assert_eq!(
            l.attach_evidence(&id, &ev("k1", Class::E1, true)).unwrap(),
            Status::Validated
        );
    }

    #[test]
    fn a_decision_lesson_needs_a_person_and_no_amount_of_model_agreement_validates_anything() {
        let mut l = lessons("pref");
        let p = l
            .add("Keep notes under 80 words", LessonClass::Preference, None)
            .unwrap();
        let q = l
            .add("Review before sending", LessonClass::Process, None)
            .unwrap();
        l.note_model_agreement(&p, 5).unwrap();
        assert_eq!(l.list()[0].status, Status::Candidate);
        assert!(
            l.attach_evidence(&p, &ev("x", Class::E1, true)).is_err(),
            "claim evidence does not apply"
        );
        assert!(l.confirm(&p, "  ").is_err());
        l.confirm(&p, "Nitish").unwrap();
        l.confirm(&q, "Nitish").unwrap();
        assert!(l.list().iter().all(|x| x.status == Status::Validated));
        assert_eq!(l.list()[0].confirmed_by.as_deref(), Some("Nitish"));
        let f = l.add("A fact", LessonClass::Factual, Some("c")).unwrap();
        assert!(
            l.confirm(&f, "Nitish").is_err(),
            "a fact is checked, not confirmed"
        );
    }

    #[test]
    fn a_factual_lesson_must_name_its_claim_and_text_is_required() {
        let mut l = lessons("names");
        assert!(l.add("x", LessonClass::Factual, None).is_err());
        assert!(l.add("x", LessonClass::Factual, Some(" ")).is_err());
        assert!(l.add("  ", LessonClass::Preference, None).is_err());
        assert!(l.attach_evidence("L9", &ev("c", Class::E1, true)).is_err());
        assert!(l.list().is_empty());
    }

    #[test]
    fn lessons_persist_and_the_session_end_list_separates_validated_from_candidates() {
        let mut l = lessons("persist");
        assert!(l.session_end_markdown().contains("None collected"));
        let a = l
            .add(
                "Total is licenses times price",
                LessonClass::Factual,
                Some("k1"),
            )
            .unwrap();
        l.attach_evidence(&a, &ev("k1", Class::E1, true)).unwrap();
        let b = l
            .add("Prefer short notes", LessonClass::Preference, None)
            .unwrap();
        l.add("Unproven idea", LessonClass::Process, None).unwrap();
        l.confirm(&b, "Nitish").unwrap();
        let reopened = Lessons::open(l.path.parent().unwrap()).unwrap();
        assert_eq!(reopened.list(), l.list());
        let md = reopened.session_end_markdown();
        let validated = md.find("### Validated").unwrap();
        let candidates = md.find("### Candidates").unwrap();
        assert!(validated < candidates);
        assert!(md.contains("(checked: E1)") && md.contains("(decision, confirmed by Nitish)"));
        assert!(md.contains("L3: Unproven idea\n"));
    }

    #[test]
    fn validated_lessons_apply_automatically_are_listed_for_the_plan_and_can_be_removed() {
        let mut l = lessons("applied");
        assert_eq!(
            l.applied_block(),
            "",
            "nothing applied, nothing added to a prompt"
        );
        let a = l
            .add(
                "State the unit next to every figure",
                LessonClass::Process,
                None,
            )
            .unwrap();
        let b = l.add("Unproven idea", LessonClass::Process, None).unwrap();
        l.confirm(&a, "Nitish").unwrap();
        let block = l.applied_block();
        assert!(
            block.contains("State the unit") && !block.contains("Unproven"),
            "{block}"
        );
        assert!(
            block.contains(&a) && block.contains("remove"),
            "each applied lesson is named so it can be removed: {block}"
        );
        assert_eq!(l.applied().len(), 1);
        l.remove(&a).unwrap();
        assert!(l.applied().is_empty() && l.applied_block().is_empty());
        assert!(l.remove(&a).is_err(), "already gone");
        let c = l
            .add("Added after a removal", LessonClass::Process, None)
            .unwrap();
        assert_ne!(c, b, "an id is never reused while another lesson holds it");
        assert!(
            l.list().iter().any(|x| x.id == b),
            "other lessons are untouched"
        );
        let again = Lessons::open(l.path.parent().unwrap()).unwrap();
        assert_eq!(again.list(), l.list());
    }
}
