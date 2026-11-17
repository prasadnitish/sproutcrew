//! Research answers from the Responses API. The answer and the addresses it cites are stored as a source
//! snapshot so claims can cite them. The cited pages themselves were not fetched: the snapshot holds what
//! the model said, and the note at the top of it says so. An E2 check against it shows the model's words
//! are there, which is weaker than a check against the page.

use crate::apiroute::Parsed;
use crate::sources::{ImportError, Snapshot, SourceStore};

pub const NOTE: &str = "Model-reported research. The cited pages were not fetched, so this records what the model said, not what the pages say.";

/// Stores the answer and its citations as one source. Returns the snapshot.
pub fn snapshot_answer(
    store: &mut SourceStore,
    question: &str,
    parsed: &Parsed,
) -> Result<Snapshot, ImportError> {
    let mut text = format!(
        "{NOTE}\n\nQuestion: {}\n\n{}\n",
        question.trim(),
        parsed.text.trim()
    );
    if !parsed.citations.is_empty() {
        text.push_str("\nCited addresses:\n");
        for c in &parsed.citations {
            text.push_str(&format!(
                "- {} ({})\n",
                c.url,
                if c.title.is_empty() {
                    "no title"
                } else {
                    &c.title
                }
            ));
        }
    }
    let n = store.list().len() + 1;
    store.import(&format!("research-{n}.md"), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apiroute::Citation;

    fn store(tag: &str) -> SourceStore {
        let d = std::env::temp_dir().join(format!("bff-research-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        SourceStore::open(&d).unwrap()
    }

    fn parsed() -> Parsed {
        Parsed {
            text: "Typical team pilots last two weeks.".into(),
            input_tokens: Some(1),
            output_tokens: Some(1),
            reported_micro: None,
            tool_calls: 0,
            citations: vec![
                Citation {
                    url: "https://example.test/pilots".into(),
                    title: "Pilots".into(),
                },
                Citation {
                    url: "https://example.test/x".into(),
                    title: String::new(),
                },
            ],
        }
    }

    #[test]
    fn the_answer_and_its_citations_become_a_source_that_says_the_pages_were_not_fetched() {
        let mut s = store("a");
        let snap = snapshot_answer(&mut s, "How long are pilots?", &parsed()).unwrap();
        assert_eq!(snap.id, "src-1");
        let text = &snap.pages[0];
        assert!(text.starts_with(NOTE));
        assert!(
            text.contains("https://example.test/pilots (Pilots)") && text.contains("(no title)")
        );
        assert_eq!(
            s.find_span("src-1#p1", "pilots last two weeks"),
            Ok(()),
            "the model's words can be cited"
        );
        assert!(s.find_span("src-1", "pilots last six weeks").is_err());
    }

    #[test]
    fn the_same_answer_twice_is_one_source_and_a_new_one_is_the_next() {
        let mut s = store("b");
        let a = snapshot_answer(&mut s, "Q", &parsed()).unwrap();
        let b = snapshot_answer(&mut s, "Q", &parsed()).unwrap();
        assert_eq!(a.id, b.id);
        let mut other = parsed();
        other.text = "Different.".into();
        assert_eq!(snapshot_answer(&mut s, "Q", &other).unwrap().id, "src-2");
    }
}
