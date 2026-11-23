//! What only the person knows. A first step of the approved plan reads the brief and the files and lists up to five
//! facts the piece needs that neither gives, such as a price or a date. If it lists any, the run waits on a short form,
//! the answers are kept as notes the later steps read, and the run goes on. The step runs after the person approves
//! the plan, so nothing is sent to a model before then.

use serde::Serialize;

/// The step, and the role that does it: the writer, on the writing model.
pub const FACTS_STEP: &str = "facts";
pub const FACTS_ROLE: &str = "writer";

/// What the step is told. It ends on a fixed list that [`parse_questions`] reads.
pub const FACTS_GUIDE: &str = "Read the brief and the files. List up to five facts that the piece needs and only the person can know, which the brief and files don't give. For each, write one line: `Q: <question> | From: <file name or nothing> | Suggested: <answer found in that file or nothing>`. If nothing is missing, write exactly `Nothing missing`.";

/// Why the steps after the check do not start while the person has questions to answer.
pub const WAITING_REASON: &str = "Waiting for your answers";

/// What a refused answer says. An answer is a note, so it keeps the strict secret rule, and there is no override.
pub const REFUSED: &str =
    "This answer looks like it holds a password, so it wasn't saved. Please reword it.";

/// The first words of the note the answers become.
pub const FACTS_NOTE: &str = "Facts from the person, given before the draft was written. Use them where the piece needs them, and do not mark them [NEEDED]:";

/// The most questions asked, and the longest question and suggestion kept.
pub const MOST: usize = 5;
const LONGEST_ASK: usize = 200;
const LONGEST_SUGGESTED: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Question {
    pub ask: String,
    /// The file the suggestion came from.
    pub from: Option<String>,
    pub suggested: Option<String>,
}

/// Whether a step's guide is the one that asks for these questions.
pub fn asks_for_facts(guide: &str) -> bool {
    guide.contains(FACTS_GUIDE)
}

/// What a field says, or `None` when it says nothing: empty, or "nothing", "none" or "n/a".
fn said(raw: &str, longest: usize) -> Option<String> {
    let t = raw
        .trim()
        .trim_matches('`')
        .trim()
        .trim_end_matches('.')
        .trim();
    let t = t.trim_matches(['*', '_']).trim();
    if t.is_empty() || ["nothing", "none", "n/a", "-"].contains(&t.to_lowercase().as_str()) {
        return None;
    }
    Some(t.chars().take(longest).collect())
}

/// The questions in a result: each line that starts with `Q:`, read as `Q: ask | From: file | Suggested: answer`,
/// up to [`MOST`]. A list marker or backticks round the line are allowed. Any other line, such as `Nothing missing`,
/// is not a question.
pub fn parse_questions(md: &str) -> Vec<Question> {
    md.lines()
        .filter_map(|line| {
            let line = line
                .trim()
                .trim_start_matches(['-', '*', '>'])
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches(['.', ')'])
                .trim()
                .trim_matches('`')
                .trim();
            let rest = line
                .strip_prefix("Q:")
                .or_else(|| line.strip_prefix("q:"))?;
            let (mut ask, mut from, mut suggested) = (String::new(), String::new(), String::new());
            let mut at = 0;
            for (i, part) in rest.split('|').enumerate() {
                let t = part.trim_start();
                let lower = t.to_lowercase();
                if i > 0 && lower.starts_with("from:") {
                    at = 1;
                    from.push_str(&t["from:".len()..]);
                } else if i > 0 && lower.starts_with("suggested:") {
                    at = 2;
                    suggested.push_str(&t["suggested:".len()..]);
                } else {
                    let into = [&mut ask, &mut from, &mut suggested];
                    let into = into.into_iter().nth(at).expect("three fields");
                    if i > 0 {
                        into.push('|');
                    }
                    into.push_str(part);
                }
            }
            Some(Question {
                ask: said(&ask, LONGEST_ASK)
                    .filter(|a| !a.eq_ignore_ascii_case("nothing missing"))?,
                from: said(&from, 120),
                suggested: said(&suggested, LONGEST_SUGGESTED),
            })
        })
        .take(MOST)
        .collect()
}

/// The note the answers become: one line for each question the person answered. `None` when none was answered.
pub fn answers_note(questions: &[Question], answers: &[Option<String>]) -> Option<String> {
    let lines: Vec<String> = questions
        .iter()
        .zip(answers)
        .filter_map(|(q, a)| {
            let a = a.as_deref().map(str::trim).filter(|a| !a.is_empty())?;
            Some(format!("- Question: {} Answer: {a}", q.ask))
        })
        .collect();
    (!lines.is_empty()).then(|| format!("{FACTS_NOTE}\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_questions_are_read_with_the_file_and_the_suggestion() {
        let q = parse_questions(
            "Q: What does the dough class cost? | From: pricing-notes.txt | Suggested: $45 a person\n\
             Q: When does the next class start? | From: nothing | Suggested: nothing\n\
             Q: Who should people write to? | From: brief.md | Suggested: nothing",
        );
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].ask, "What does the dough class cost?");
        assert_eq!(q[0].from.as_deref(), Some("pricing-notes.txt"));
        assert_eq!(q[0].suggested.as_deref(), Some("$45 a person"));
        assert_eq!((q[1].from.clone(), q[1].suggested.clone()), (None, None));
        assert_eq!(q[2].from.as_deref(), Some("brief.md"));
        assert_eq!(q[2].suggested, None);
    }

    #[test]
    fn nothing_missing_is_no_questions() {
        assert!(parse_questions("Nothing missing").is_empty());
        assert!(parse_questions("`Nothing missing`\n").is_empty());
        assert!(parse_questions("").is_empty());
    }

    #[test]
    fn more_than_five_questions_keep_the_first_five() {
        let text: String = (1..=8)
            .map(|i| format!("Q: Question {i}? | From: nothing | Suggested: nothing\n"))
            .collect();
        let q = parse_questions(&text);
        assert_eq!(q.len(), 5);
        assert_eq!(q[4].ask, "Question 5?");
    }

    #[test]
    fn a_line_without_q_is_ignored() {
        let q = parse_questions(
            "Here is what is missing:\nWhat is the price? | From: nothing | Suggested: nothing\nQ: Who is the audience? | From: nothing | Suggested: nothing\nSee the brief for Q: details",
        );
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].ask, "Who is the audience?");
    }

    #[test]
    fn list_marks_backticks_and_a_pipe_inside_a_suggestion_do_not_break_a_line() {
        let q = parse_questions(
            "- `Q: What is the price? | From: price.txt | Suggested: $10 | $12 with tax`\n2. Q: A date? | From: None | Suggested: n/a.",
        );
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].suggested.as_deref(), Some("$10 | $12 with tax"));
        assert_eq!((q[1].from.clone(), q[1].suggested.clone()), (None, None));
    }

    #[test]
    fn a_question_with_no_text_is_not_a_question() {
        assert!(parse_questions("Q: | From: a.txt | Suggested: nothing").is_empty());
    }

    #[test]
    fn the_answers_note_has_one_line_for_each_answered_question_and_nothing_for_skipped_ones() {
        let q = parse_questions(
            "Q: Price? | From: nothing | Suggested: nothing\nQ: Date? | From: nothing | Suggested: nothing",
        );
        let note = answers_note(&q, &[Some(" $45 ".into()), None]).unwrap();
        assert!(note.starts_with(FACTS_NOTE), "{note}");
        assert!(note.contains("- Question: Price? Answer: $45"), "{note}");
        assert!(!note.contains("Date?"), "{note}");
        assert_eq!(answers_note(&q, &[None, Some("  ".into())]), None);
    }

    #[test]
    fn the_guide_is_recognised_and_asks_for_the_exact_line() {
        assert!(asks_for_facts(FACTS_GUIDE));
        assert!(!asks_for_facts("Write the first draft."));
        assert!(FACTS_GUIDE.contains("Q: <question> | From: <file name or nothing> | Suggested:"));
    }
}
