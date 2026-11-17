//! What a humaniser step may not change. Its guide says to change wording only, but a guide is a request. This
//! checks the result against the text it was given, and a result that changed numbers, structure, or length too
//! much is not accepted as the step's work.

/// The distinct numbers in a text, as written: every run of digits with the commas, points, and percent signs
/// inside it.
fn numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let inside = c.is_ascii_digit()
            || (!cur.is_empty()
                && matches!(c, ',' | '.' | '%')
                && chars
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == '%'));
        if inside {
            cur.push(*c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.sort();
    out.dedup();
    out
}

fn headings(text: &str, level: usize) -> Vec<String> {
    let mark = "#".repeat(level);
    let mut fenced = false;
    text.lines()
        .filter(|l| {
            if l.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            !fenced
        })
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix(&mark)
                .filter(|r| r.starts_with(' '))
                .map(|r| r.trim().to_string())
        })
        .collect()
}

fn notes_blocks(text: &str) -> usize {
    text.lines()
        .filter(|l| l.trim().starts_with("::: notes"))
        .count()
}

/// What is wrong with `polished` as a rewrite of `draft`. Empty means it kept what must stay. A deck is
/// recognised by its notes blocks: its slide headings are messages and may be reworded, but their count, the
/// notes, and the last heading stay. A document keeps every heading.
pub fn problems(draft: &str, polished: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (a, b) = (numbers(draft), numbers(polished));
    if a != b {
        let missing: Vec<&String> = a.iter().filter(|x| !b.contains(x)).take(5).collect();
        let added: Vec<&String> = b.iter().filter(|x| !a.contains(x)).take(5).collect();
        out.push(format!(
            "the numbers changed (missing: {}; new: {})",
            if missing.is_empty() {
                "none".into()
            } else {
                missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            if added.is_empty() {
                "none".into()
            } else {
                added
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ));
    }
    let deck = notes_blocks(draft) > 0;
    if deck {
        if headings(draft, 2).len() != headings(polished, 2).len() {
            out.push(format!(
                "the slide count changed from {} to {}",
                headings(draft, 2).len(),
                headings(polished, 2).len()
            ));
        }
        if notes_blocks(draft) != notes_blocks(polished) {
            out.push(format!(
                "the notes blocks changed from {} to {}",
                notes_blocks(draft),
                notes_blocks(polished)
            ));
        }
        // The closing slide is named by the format, so it keeps its name.
        let questions = |t: &str| {
            headings(t, 2)
                .last()
                .is_some_and(|h| h.eq_ignore_ascii_case("questions to expect"))
        };
        if questions(draft) && !questions(polished) {
            out.push("the last slide is no longer headed Questions to expect".to_string());
        }
    } else {
        for level in 1..=3 {
            let (a, b) = (headings(draft, level), headings(polished, level));
            if a != b {
                let gone: Vec<&String> = a.iter().filter(|h| !b.contains(h)).collect();
                let new: Vec<&String> = b.iter().filter(|h| !a.contains(h)).collect();
                let list = |v: Vec<&String>| {
                    v.iter()
                        .map(|h| format!("\"{h}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                out.push(match (gone.is_empty(), new.is_empty()) {
                    (false, false) => format!(
                        "level {level} headings changed ({} became {})",
                        list(gone),
                        list(new)
                    ),
                    (false, true) => {
                        format!("level {level} headings were dropped ({})", list(gone))
                    }
                    (true, false) => format!("level {level} headings were added ({})", list(new)),
                    (true, true) => format!("the level {level} headings are in a different order"),
                });
            }
        }
    }
    let (n, m) = (
        draft.chars().count() as f64,
        polished.chars().count() as f64,
    );
    if n > 0.0 && (m < n * 0.7 || m > n * 1.3) {
        out.push(format!("the length changed from {n} to {m} characters"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saying_a_figure_fewer_times_is_not_changing_it() {
        let draft = "Cost 42 now. Cost 42 later. Date 14 January.";
        assert!(problems(draft, "Cost 42 now and later. Date 14 January.").is_empty());
        assert!(
            !problems(draft, "Cost 42 now and later.").is_empty(),
            "a figure that is gone is caught"
        );
    }

    const DOC: &str = "# Title\n\n## Summary\n\nCosts rose 42% to $4,200 in 2026. The plan serves as a guide for 3 teams.\n\n## Risks\n\nTwo risks remain.\n";

    #[test]
    fn a_rewrite_that_keeps_numbers_and_headings_is_accepted() {
        let polished = DOC.replace("serves as a guide for", "guides");
        assert!(
            problems(DOC, &polished).is_empty(),
            "{:?}",
            problems(DOC, &polished)
        );
    }

    #[test]
    fn a_changed_or_dropped_number_is_caught() {
        let p = problems(DOC, &DOC.replace("42%", "40%"));
        assert!(
            p.iter().any(|x| x.contains("numbers") && x.contains("42")),
            "{p:?}"
        );
        let p = problems(DOC, &DOC.replace(" in 2026", ""));
        assert!(p.iter().any(|x| x.contains("2026")), "{p:?}");
    }

    #[test]
    fn a_document_must_keep_every_heading() {
        let p = problems(DOC, &DOC.replace("## Risks", "## What could go wrong"));
        assert!(
            p.iter()
                .any(|x| x.contains("level 2 headings") && x.contains("Risks")),
            "{p:?}"
        );
    }

    #[test]
    fn a_deck_may_reword_a_slide_heading_but_not_lose_slides_or_notes_or_its_last_heading() {
        let deck = "## The reported cost is high\n\n- Cost 42%\n\n::: notes\nSay it.\n:::\n\n## Questions to expect\n\n- Why?\n\n::: notes\nBecause.\n:::\n";
        let reworded = deck.replace("The reported cost is high", "Reported cost is high");
        assert!(
            problems(deck, &reworded).is_empty(),
            "{:?}",
            problems(deck, &reworded)
        );
        let fewer = "## The reported cost is high\n\n- Cost 42%\n\n::: notes\nSay it.\n:::\n";
        let p = problems(deck, fewer);
        assert!(
            p.iter().any(|x| x.contains("slide count"))
                && p.iter().any(|x| x.contains("notes blocks")),
            "{p:?}"
        );
        let last = deck.replace("## Questions to expect", "## Any questions");
        assert!(problems(deck, &last)
            .iter()
            .any(|x| x.contains("Questions to expect")));
    }

    #[test]
    fn a_rewrite_far_from_the_length_of_the_draft_is_caught() {
        assert!(problems(DOC, "# Title\n")
            .iter()
            .any(|x| x.contains("length")));
    }
}
