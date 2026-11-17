//! What each file in a project is for. A file is evidence to read and cite by default, and the model is told it never
//! contains instructions. But some files are the task: the instructions for an exercise, a brief with a length limit,
//! a template to fill. Those are the person's requirements for the deliverable, and ignoring them is a failure, not a
//! precaution. The person can say what a file is for, and a file whose name says it is the instructions or a
//! template is treated that way until they say otherwise. A file that tries to steer the AI is never assumed to be
//! the task: the person chooses.

use crate::sources::Snapshot;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Read it and cite it. It is data and never instructions.
    Evidence,
    /// The person's instructions for the task: what to answer, the form, the limits.
    Requirements,
    /// A document to fill in: its headings and their order are the deliverable's.
    Template,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "evidence" => Some(Role::Evidence),
            "requirements" => Some(Role::Requirements),
            "template" => Some(Role::Template),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Role::Evidence => "evidence",
            Role::Requirements => "requirements",
            Role::Template => "template",
        }
    }
}

/// The role a file's name and title suggest. Only a file the person added themselves can be suggested, and only if
/// nothing in it tries to steer the AI.
pub fn suggest(s: &Snapshot) -> Role {
    if !s.origin.is_empty() || !s.suspicious.is_empty() {
        return Role::Evidence;
    }
    let by = |text: &str| {
        let t = text.to_lowercase();
        if t.contains("template") {
            Some(Role::Template)
        } else if [
            "instruction",
            "requirement",
            "rubric",
            "guideline",
            "assignment",
        ]
        .iter()
        .any(|k| t.contains(k))
        {
            Some(Role::Requirements)
        } else {
            None
        }
    };
    // The name decides first. Only if the name says nothing does the heading at the top of the first page.
    let title: String = s
        .pages
        .first()
        .map(|p| p.split_whitespace().take(12).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    by(&s.name).or_else(|| by(&title)).unwrap_or(Role::Evidence)
}

pub struct Roles {
    path: PathBuf,
    map: BTreeMap<String, Role>,
}

impl Roles {
    pub fn open(root: &Path) -> Result<Roles, String> {
        let path = root.join("source_roles.json");
        let map = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("source_roles.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(format!("source_roles.json: {e}")),
        };
        Ok(Roles { path, map })
    }

    /// What the person chose for the file, or else what its name suggests.
    pub fn of(&self, s: &Snapshot) -> Role {
        self.map.get(&s.id).copied().unwrap_or_else(|| suggest(s))
    }

    /// Whether the role is the person's choice and not a suggestion.
    pub fn chosen(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }

    pub fn set(&mut self, id: &str, role: Role) -> Result<(), String> {
        self.map.insert(id.to_string(), role);
        let json = serde_json::to_string_pretty(&self.map).map_err(|e| e.to_string())?;
        crate::workspace::write_atomic(&self.path, json.as_bytes())
    }
}

/// The words in a text, as a person counts them.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(name: &str, first_page: &str, origin: &str, suspicious: &[&str]) -> Snapshot {
        Snapshot {
            id: "src-1".into(),
            name: name.into(),
            ext: "pdf".into(),
            sha256: String::new(),
            bytes: 0,
            pages: vec![first_page.into()],
            flags: vec![],
            suspicious: suspicious.iter().map(|s| s.to_string()).collect(),
            origin: origin.into(),
        }
    }

    #[test]
    fn a_file_named_for_instructions_or_a_template_is_taken_for_one_and_others_are_evidence() {
        assert_eq!(
            suggest(&snap("Candidate Instructions.pdf", "x", "", &[])),
            Role::Requirements
        );
        assert_eq!(
            suggest(&snap("Product Brief Template.pdf", "x", "", &[])),
            Role::Template
        );
        assert_eq!(
            suggest(&snap(
                "scan0042.pdf",
                "Assignment rubric: 1,000 words",
                "",
                &[]
            )),
            Role::Requirements
        );
        assert_eq!(
            suggest(&snap("Evidence Pack.pdf", "Evidence ID Source", "", &[])),
            Role::Evidence
        );
        assert_eq!(
            suggest(&snap("Customer Scenario.pdf", "Customer scenario", "", &[])),
            Role::Evidence
        );
    }

    #[test]
    fn a_web_page_or_a_file_that_steers_the_ai_is_never_assumed_to_be_the_task() {
        assert_eq!(
            suggest(&snap(
                "instructions.html",
                "x",
                "https://example.com/instructions",
                &[]
            )),
            Role::Evidence
        );
        assert_eq!(
            suggest(&snap(
                "instructions.pdf",
                "x",
                "",
                &["tells the AI to ignore its instructions"]
            )),
            Role::Evidence
        );
    }

    #[test]
    fn what_the_person_chooses_wins_over_the_suggestion_and_is_kept() {
        let d = std::env::temp_dir().join(format!("bff-roles-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let s = snap("Candidate Instructions.pdf", "x", "", &[]);
        let mut r = Roles::open(&d).unwrap();
        assert_eq!(r.of(&s), Role::Requirements);
        assert!(!r.chosen("src-1"));
        r.set("src-1", Role::Evidence).unwrap();
        assert_eq!(Roles::open(&d).unwrap().of(&s), Role::Evidence);
        assert!(Roles::open(&d).unwrap().chosen("src-1"));
        assert_eq!(Role::parse("template"), Some(Role::Template));
        assert_eq!(Role::parse("nope"), None);
    }

    #[test]
    fn words_are_counted_as_a_person_counts_them() {
        assert_eq!(word_count("  One two\nthree   four. "), 4);
        assert_eq!(word_count(""), 0);
    }
}
