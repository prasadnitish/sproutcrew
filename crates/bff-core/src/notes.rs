//! A person's own instructions to a step, such as "use 14 January as the board date". A note belongs to one
//! step, or to the whole project when its step is empty. Notes are kept in `notes.json` beside the plan, not in
//! it, so adding one does not change the plan or its approval. A step's next run is told its notes in the part
//! of the prompt that holds the step's own instructions, apart from the material, which stays untrusted.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The longest note, in characters, and the most notes a project keeps.
pub const MAX_NOTE: usize = 2000;
pub const MAX_NOTES: usize = 30;
/// The file in the app's data folder that keeps the notes for every project.
pub const EVERYWHERE_FILE: &str = "memory.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    /// The step the note is for. Empty means every step.
    pub stage: String,
    pub text: String,
    pub at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    next: u64,
    notes: Vec<Note>,
}

pub struct Notes {
    path: PathBuf,
    file: File,
}

impl Notes {
    /// A project with no `notes.json` has no notes. A file that cannot be read is an error, so a damaged file
    /// is never taken for an empty list and then overwritten.
    pub fn open(root: &Path) -> Result<Notes, String> {
        let path = root.join("notes.json");
        let file = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("notes.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(format!("notes.json: {e}")),
        };
        Ok(Notes { path, file })
    }

    /// The notes kept for every project, in `memory.json` in the app's data folder, with the same limits and
    /// checks as a project's notes. Every one of them is for every step.
    pub fn open_everywhere(data: &Path) -> Result<Notes, String> {
        let path = data.join(EVERYWHERE_FILE);
        let file = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{EVERYWHERE_FILE}: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(format!("{EVERYWHERE_FILE}: {e}")),
        };
        Ok(Notes { path, file })
    }

    /// The every-project notes for the project at `root`, which lives in `<data>/projects/<name>`. A folder that
    /// is not laid out that way, such as a test's, has none.
    pub fn everywhere_for(root: &Path) -> Result<Vec<Note>, String> {
        let Some(projects) = root.parent() else {
            return Ok(vec![]);
        };
        if projects.file_name().and_then(|n| n.to_str()) != Some("projects") {
            return Ok(vec![]);
        }
        match projects.parent() {
            Some(data) => Ok(Notes::open_everywhere(data)?.file.notes),
            None => Ok(vec![]),
        }
    }

    /// Every-project notes as the text a worker is given, or empty when there are none. They come before the
    /// project's own notes, which win where the two differ.
    pub fn everywhere_instructions(notes: &[Note]) -> String {
        if notes.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = notes
            .iter()
            .map(|n| format!("- {}", n.text.replace('\n', " ")))
            .collect();
        format!(
            "Things the person who owns this project asked to be remembered in all their projects. Follow them unless this project's own instructions say otherwise:\n{}",
            lines.join("\n")
        )
    }

    pub fn list(&self) -> &[Note] {
        &self.file.notes
    }

    fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.file).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| format!("notes.json: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("notes.json: {e}"))
    }

    /// A note's text as it would be kept, or why it cannot be a note. Nothing is stored.
    pub fn check(text: &str) -> Result<String, String> {
        let (clean, _) = crate::guard::strip_hidden(text);
        let clean = clean.trim().to_string();
        if clean.is_empty() {
            return Err("a note cannot be empty".into());
        }
        if clean.chars().count() > MAX_NOTE {
            return Err(format!("a note can be at most {MAX_NOTE} characters"));
        }
        if let Some(f) = crate::secrets::scan(&clean)
            .iter()
            .find(|f| f.class == crate::secrets::Class::Secret)
        {
            return Err(format!(
                "the note contains what looks like a secret (line {}). A note goes to a model, so remove it first",
                f.line
            ));
        }
        Ok(clean)
    }

    pub fn add(&mut self, stage: &str, text: &str, now: u64) -> Result<Note, String> {
        let clean = Self::check(text)?;
        if self.file.notes.len() >= MAX_NOTES {
            return Err(format!(
                "a project keeps at most {MAX_NOTES} notes. Remove one first"
            ));
        }
        self.file.next += 1;
        let note = Note {
            id: format!("n{}", self.file.next),
            stage: stage.to_string(),
            text: clean,
            at: now,
        };
        self.file.notes.push(note.clone());
        self.save()?;
        Ok(note)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.file.notes.len();
        self.file.notes.retain(|n| n.id != id);
        if self.file.notes.len() == before {
            return Err(format!("there is no note {id:?}"));
        }
        self.save()
    }

    /// Edit a person's memory without changing its identity or scope. Validate before touching the file.
    pub fn replace(&mut self, id: &str, text: &str, now: u64) -> Result<Note, String> {
        let clean = Self::check(text)?;
        let note = self
            .file
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("there is no note {id:?}"))?;
        note.text = clean;
        note.at = now;
        let kept = note.clone();
        self.save()?;
        Ok(kept)
    }

    /// The notes for `stage` as the text a worker is given: the project-wide notes, then the step's own, each
    /// oldest first. Empty when there are none.
    pub fn instructions(&self, stage: &str) -> String {
        let mine = |n: &&Note| n.stage.is_empty() || (!stage.is_empty() && n.stage == stage);
        let lines: Vec<String> = self
            .file
            .notes
            .iter()
            .filter(mine)
            .map(|n| format!("- {}", n.text.replace('\n', " ")))
            .collect();
        if lines.is_empty() {
            return String::new();
        }
        format!(
            "Instructions from the person who owns this project. Follow them. Where they differ from the step description above, they win:\n{}",
            lines.join("\n")
        )
    }
}

impl Notes {
    /// What a checking step is told about the notes given to the other steps: the owner's own decisions, which
    /// the files do not contain, so a fact that comes only from them is not a fault in the files. Its own notes
    /// are left out, because they reach it as instructions. Empty when there are none.
    pub fn decisions_for_checker(&self, stage: &str) -> String {
        let lines: Vec<String> = self
            .file
            .notes
            .iter()
            .filter(|n| !n.stage.is_empty() && n.stage != stage)
            .map(|n| format!("- for the step {}: {}", n.stage, n.text.replace('\n', " ")))
            .collect();
        if lines.is_empty() {
            return String::new();
        }
        format!(
            "The owner of this project gave these instructions to the steps that did the work. They are the owner's own decisions, not claims from the files, so a fact that comes only from them is not a fault in the files and is not unsupported. Check that the work followed them:\n{}",
            lines.join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bff-notes-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_note_is_kept_across_reopening() {
        let d = dir("keep");
        let mut n = Notes::open(&d).unwrap();
        let note = n
            .add("document", "Use 14 January as the board date.", 5)
            .unwrap();
        assert_eq!(note.stage, "document");
        assert_eq!(note.at, 5);
        let again = Notes::open(&d).unwrap();
        assert_eq!(again.list().len(), 1);
        assert_eq!(again.list()[0].text, "Use 14 January as the board date.");
    }

    #[test]
    fn a_step_gets_its_own_notes_and_the_project_wide_ones_in_order() {
        let d = dir("scope");
        let mut n = Notes::open(&d).unwrap();
        n.add("", "Write in British English.", 1).unwrap();
        n.add("deck", "Keep slide titles under ten words.", 2)
            .unwrap();
        n.add("document", "Cut option 3.", 3).unwrap();
        let text = n.instructions("document");
        assert!(text.contains("British English") && text.contains("Cut option 3."));
        assert!(!text.contains("slide titles"));
        assert!(text.find("British").unwrap() < text.find("Cut option").unwrap());
        assert_eq!(n.instructions("analyse").matches("- ").count(), 1);
    }

    #[test]
    fn no_notes_means_no_text() {
        let d = dir("none");
        assert_eq!(Notes::open(&d).unwrap().instructions("document"), "");
    }

    #[test]
    fn shared_memory_edits_survive_reopen_and_rejected_edits_preserve_the_original() {
        let d = dir("edit-memory");
        let mut n = Notes::open(&d).unwrap();
        let note = n.add("", "Write for founders.", 1).unwrap();
        assert!(n.replace(&note.id, " ", 2).is_err());
        assert_eq!(
            Notes::open(&d).unwrap().list()[0].text,
            "Write for founders."
        );
        n.replace(&note.id, "Write for product managers.", 3)
            .unwrap();
        let reopened = Notes::open(&d).unwrap();
        assert_eq!(reopened.list()[0].id, note.id);
        assert_eq!(reopened.list()[0].at, 3);
        for step in ["draft", "review", "finish"] {
            assert!(reopened.instructions(step).contains("product managers"));
        }
        n.remove(&note.id).unwrap();
        assert!(Notes::open(&d).unwrap().instructions("draft").is_empty());
    }

    #[test]
    fn an_empty_note_is_refused() {
        let d = dir("empty");
        let mut n = Notes::open(&d).unwrap();
        assert!(n.add("document", "  \n ", 1).is_err());
        assert!(n.list().is_empty());
    }

    #[test]
    fn a_note_with_a_secret_is_refused_and_not_stored() {
        let d = dir("secret");
        let mut n = Notes::open(&d).unwrap();
        let e = n
            .add(
                "document",
                "use sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH",
                1,
            )
            .unwrap_err();
        assert!(e.contains("secret"), "{e}");
        assert!(n.list().is_empty());
        assert!(!std::fs::read_to_string(d.join("notes.json"))
            .unwrap_or_default()
            .contains("sk-ant"));
    }

    #[test]
    fn a_long_note_and_too_many_notes_are_refused() {
        let d = dir("limits");
        let mut n = Notes::open(&d).unwrap();
        assert!(n.add("document", &"a".repeat(MAX_NOTE + 1), 1).is_err());
        for i in 0..MAX_NOTES {
            n.add("document", &format!("note {i}"), i as u64).unwrap();
        }
        assert!(n.add("document", "one more", 99).is_err());
    }

    #[test]
    fn hidden_characters_are_removed_from_a_note() {
        let d = dir("hidden");
        let mut n = Notes::open(&d).unwrap();
        let note = n.add("document", "Cut\u{200b} option 3.", 1).unwrap();
        assert_eq!(note.text, "Cut option 3.");
    }

    #[test]
    fn a_note_can_be_removed_and_ids_are_not_reused() {
        let d = dir("remove");
        let mut n = Notes::open(&d).unwrap();
        let a = n.add("document", "first", 1).unwrap();
        n.remove(&a.id).unwrap();
        let b = n.add("document", "second", 2).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(Notes::open(&d).unwrap().list().len(), 1);
        assert!(n.remove("n99").is_err());
    }

    #[test]
    fn a_damaged_file_is_an_error_not_an_empty_list() {
        let d = dir("bad");
        std::fs::write(d.join("notes.json"), "{not json").unwrap();
        assert!(Notes::open(&d).is_err());
    }
}
