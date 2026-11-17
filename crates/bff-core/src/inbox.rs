//! The global playbook inbox. A project lesson that is validated can be proposed for the global playbooks,
//! and it changes nothing there until a person accepts it. Applied project lessons are listed so they can
//! be removed. A candidate lesson cannot be proposed.

use crate::lessons::{Lesson, LessonClass, Status as LessonStatus};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub project: String,
    pub lesson_id: String,
    pub text: String,
    pub class: LessonClass,
    pub decision: Decision,
    pub decided_by: Option<String>,
}

pub struct Inbox {
    path: PathBuf,
    items: Vec<Item>,
}

impl Inbox {
    pub fn open(root: &Path) -> Result<Inbox, String> {
        let path = root.join("playbook-inbox.json");
        let items = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("playbook-inbox.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Inbox { path, items })
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

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Proposes a validated lesson. A candidate is refused, and so is one already proposed.
    pub fn propose(&mut self, project: &str, lesson: &Lesson) -> Result<String, String> {
        if lesson.status != LessonStatus::Validated {
            return Err("only a validated lesson can be proposed for the global playbooks".into());
        }
        if self.items.iter().any(|i| {
            i.project == project && i.lesson_id == lesson.id && i.decision != Decision::Rejected
        }) {
            return Err("this lesson is already in the inbox".into());
        }
        let id = format!("G{}", self.items.len() + 1);
        self.items.push(Item {
            id: id.clone(),
            project: project.to_string(),
            lesson_id: lesson.id.clone(),
            text: lesson.text.clone(),
            class: lesson.class,
            decision: Decision::Pending,
            decided_by: None,
        });
        self.save()?;
        Ok(id)
    }

    pub fn decide(&mut self, id: &str, accept: bool, by: &str) -> Result<(), String> {
        if by.trim().is_empty() {
            return Err("say who decides".into());
        }
        let item = self
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| format!("no inbox item {id}"))?;
        if item.decision != Decision::Pending {
            return Err("this item was already decided".into());
        }
        item.decision = if accept {
            Decision::Accepted
        } else {
            Decision::Rejected
        };
        item.decided_by = Some(by.trim().to_string());
        self.save()
    }

    /// The only lessons that change the global playbooks.
    pub fn accepted(&self) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| i.decision == Decision::Accepted)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(id: &str, status: LessonStatus) -> Lesson {
        Lesson {
            id: id.into(),
            text: "State the unit".into(),
            class: LessonClass::Process,
            status,
            claim: None,
            notes: vec![],
            confirmed_by: None,
        }
    }

    fn inbox(tag: &str) -> Inbox {
        let d = std::env::temp_dir().join(format!("bff-inbox-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Inbox::open(&d).unwrap()
    }

    #[test]
    fn only_a_validated_lesson_can_be_proposed_and_only_once() {
        let mut i = inbox("propose");
        assert!(i
            .propose("case", &lesson("L1", LessonStatus::Candidate))
            .is_err());
        let id = i
            .propose("case", &lesson("L1", LessonStatus::Validated))
            .unwrap();
        assert_eq!(i.items()[0].decision, Decision::Pending);
        assert!(
            i.propose("case", &lesson("L1", LessonStatus::Validated))
                .is_err(),
            "already in the inbox"
        );
        assert!(
            i.propose("other", &lesson("L1", LessonStatus::Validated))
                .is_ok(),
            "a different project's L1 is a different lesson"
        );
        assert_eq!(id, "G1");
    }

    #[test]
    fn nothing_reaches_the_global_playbooks_until_a_person_accepts_it() {
        let mut i = inbox("decide");
        let a = i
            .propose("case", &lesson("L1", LessonStatus::Validated))
            .unwrap();
        let b = i
            .propose("case", &lesson("L2", LessonStatus::Validated))
            .unwrap();
        assert!(i.accepted().is_empty(), "pending changes nothing");
        assert!(i.decide(&a, true, "  ").is_err());
        i.decide(&a, true, "Nitish").unwrap();
        i.decide(&b, false, "Nitish").unwrap();
        assert_eq!(i.accepted().len(), 1);
        assert_eq!(i.accepted()[0].lesson_id, "L1");
        assert!(i.decide(&a, false, "Nitish").is_err(), "decided once");
        assert!(i.decide("G9", true, "Nitish").is_err());
        // A rejected lesson may be proposed again.
        assert!(i
            .propose("case", &lesson("L2", LessonStatus::Validated))
            .is_ok());
        let again = Inbox::open(i.path.parent().unwrap()).unwrap();
        assert_eq!(again.items(), i.items());
    }
}
