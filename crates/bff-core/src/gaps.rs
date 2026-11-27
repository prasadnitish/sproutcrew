//! Details only the person can supply. A writing step that lacks a fact marks its place with `[NEEDED: what is
//! missing]` and never guesses it. This counts those markers and the items a closing step lists under "Still open
//! before sending", and takes the markers out of the text that goes into files.

use serde::Serialize;
use std::collections::HashSet;

/// What opens a marker. A guide tells a writing step to write this, then what is missing, then a closing bracket.
pub const MARK: &str = "[NEEDED: ";

/// The first words of the note "Finish with what I have" saves: the step writes around the missing details.
pub const WRITE_AROUND: &str = "Write around these missing details; do not leave placeholders: ";

/// The first line of the note "Finish with my answers" saves, followed by one `what: answer` line for each answer.
pub const DETAILS_NOTE: &str = "Details from the person:";

/// What a marker says when it says nothing.
const UNNAMED: &str = "a missing detail";

/// A marker that is not closed within this many bytes, or before the end of its paragraph, is taken to end with its
/// line, so one stray bracket cannot swallow the page.
const LONGEST: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gap {
    pub what: String,
}

/// Every marker in the text: where it starts and ends, and what it says with line breaks and runs of spaces made one
/// space. A `]` inside the marker closes a `[` inside it first, so `[NEEDED: price [per week]]` is one marker.
fn markers(md: &str) -> Vec<(usize, usize, String)> {
    let lower = md.to_ascii_lowercase();
    let open = MARK.trim_end().to_ascii_lowercase();
    let (mut out, mut from) = (Vec::new(), 0);
    while let Some(i) = lower[from..].find(&open).map(|i| i + from) {
        let inner = i + open.len();
        let window_end = {
            let mut e = (inner + LONGEST).min(md.len());
            while !md.is_char_boundary(e) {
                e -= 1;
            }
            md[inner..e].find("\n\n").map_or(e, |p| inner + p)
        };
        let mut depth = 1;
        let mut close = None;
        for (j, c) in md[inner..window_end].char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(inner + j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let (what_end, end) = match close {
            Some(c) => (c, c + 1),
            None => {
                let line_end = md[inner..].find('\n').map_or(md.len(), |p| inner + p);
                (line_end, line_end)
            }
        };
        let what = md[inner..what_end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        out.push((i, end, what));
        from = end.max(i + 1);
    }
    out
}

/// The items under "Still open before sending": each bullet, or each paragraph when the section has no bullets. A
/// bullet that holds a marker is left to the marker, and "none" is not an item.
fn open_items(md: &str) -> Vec<String> {
    let body = crate::clientfiles::still_open(md);
    let (mut items, mut continues) = (Vec::<String>::new(), false);
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() {
            continues = false;
            continue;
        }
        let bullet = t
            .strip_prefix(['-', '*', '+'])
            .filter(|r| r.starts_with(' '))
            .or_else(|| {
                let digits = t.bytes().take_while(u8::is_ascii_digit).count();
                t[digits..]
                    .strip_prefix(['.', ')'])
                    .filter(|r| digits > 0 && r.starts_with(' '))
            });
        match (bullet, continues) {
            (Some(rest), _) => items.push(rest.trim().to_string()),
            (None, true) => {
                if let Some(last) = items.last_mut() {
                    last.push(' ');
                    last.push_str(t);
                }
            }
            (None, false) => items.push(t.to_string()),
        }
        continues = true;
    }
    items
        .into_iter()
        .map(|i| i.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|i| !i.is_empty() && !i.to_ascii_lowercase().contains("[needed:"))
        .filter(|i| {
            let w = i.trim_end_matches(['.', '!']).to_ascii_lowercase();
            !matches!(
                w.as_str(),
                "none" | "nothing" | "n/a" | "nothing open" | "nothing is open"
            )
        })
        .collect()
}

/// The details missing from a result: each marker, then each open item, once each however many times it is said.
/// A marker inside a code fence is code, not a missing detail: fenced text is left alone here and by `fill` and
/// `drop_unfilled`.
pub fn find(md: &str) -> Vec<Gap> {
    let mut seen = HashSet::new();
    loose_markers(md)
        .into_iter()
        .map(|(_, _, what)| {
            if what.is_empty() {
                UNNAMED.to_string()
            } else {
                what
            }
        })
        .chain(open_items(md))
        .filter(|what| seen.insert(what.to_lowercase()))
        .map(|what| Gap { what })
        .collect()
}

/// What each marker left in the text says, once each: the details a piece was finished without.
pub fn unfilled(md: &str) -> Vec<Gap> {
    let mut seen = HashSet::new();
    loose_markers(md)
        .into_iter()
        .map(|(_, _, what)| {
            if what.is_empty() {
                UNNAMED.to_string()
            } else {
                what
            }
        })
        .filter(|what| seen.insert(what.to_lowercase()))
        .map(|what| Gap { what })
        .collect()
}

/// "1 detail missing", "3 details missing".
pub fn missing(n: usize) -> String {
    format!("{n} {} missing", if n == 1 { "detail" } else { "details" })
}

/// The text without its markers, for files. A space the marker leaves before a stop, or doubled, is closed up, and a
/// list item that held only a marker is dropped. Text with no marker comes back as it was.
pub fn strip(md: &str) -> String {
    let spans = markers(md);
    if spans.is_empty() {
        return md.to_string();
    }
    let mut out = String::with_capacity(md.len());
    let mut at = 0;
    for (start, end, _) in spans {
        out.push_str(&md[at..start]);
        at = end;
        let next = md[at..].chars().next();
        if out.ends_with(' ') && next.is_none_or(|c| c.is_whitespace() || ".,;:!?)".contains(c)) {
            out.pop();
        }
    }
    out.push_str(&md[at..]);
    let bare = |l: &str| {
        let t = l.trim();
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        matches!(t, "-" | "*" | "+") || (digits > 0 && matches!(&t[digits..], "." | ")"))
    };
    let kept: Vec<&str> = out.lines().filter(|l| !bare(l)).collect();
    let mut text = kept.join("\n");
    if md.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// The markers outside code fences, which are what the person's answers and "write around it" act on.
fn loose_markers(md: &str) -> Vec<(usize, usize, String)> {
    let fences = fenced(md);
    markers(md)
        .into_iter()
        .filter(|(s, _, _)| !fences.iter().any(|(a, b)| a <= s && s < b))
        .collect()
}

/// The byte ranges of fenced code blocks. A fence that never closes runs to the end.
fn fenced(md: &str) -> Vec<(usize, usize)> {
    let (mut out, mut at, mut open) = (Vec::new(), 0, None);
    for line in md.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            match open.take() {
                Some(from) => out.push((from, at + line.len())),
                None => open = Some(at),
            }
        }
        at += line.len();
    }
    if let Some(from) = open {
        out.push((from, md.len()));
    }
    out
}

/// What a marker says, as the key an answer is matched by: lower case, spaces made one, `UNNAMED` when empty.
fn key(what: &str) -> String {
    let flat = what.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        UNNAMED.to_ascii_lowercase()
    } else {
        flat.to_ascii_lowercase()
    }
}

/// The answers the person gave to the markers in the text: for each marker, the newest note line that starts with what
/// it says and a colon. `note_lines` are the `what: answer` lines of the notes, oldest first. An empty answer is none.
pub fn answers_for(md: &str, note_lines: &[String]) -> Vec<(String, String)> {
    let flat: Vec<String> = note_lines
        .iter()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (_, _, what) in loose_markers(md) {
        let what = if what.is_empty() {
            UNNAMED.to_string()
        } else {
            what
        };
        let want = format!("{}:", key(&what));
        if !seen.insert(want.clone()) {
            continue;
        }
        let answer = flat.iter().rev().find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower
                .starts_with(&want)
                .then(|| l[want.len()..].trim().to_string())
        });
        if let Some(a) = answer.filter(|a| !a.is_empty()) {
            out.push((what, a));
        }
    }
    out
}

/// Whether the person has answered what a detail asked for: a note line that starts with it and a colon, and then has
/// something to say.
pub fn answered(what: &str, note_lines: &[String]) -> bool {
    let want = format!("{}:", key(what));
    note_lines.iter().any(|l| {
        let flat = l.split_whitespace().collect::<Vec<_>>().join(" ");
        flat.to_ascii_lowercase()
            .strip_prefix(&want)
            .is_some_and(|rest| !rest.trim().is_empty())
    })
}

/// The details a "Finish with what I have" note lists, one for each `; `. The page keeps a `;` out of a detail, so
/// a comma or colon inside one never splits it. A last item the note's length cut short is dropped.
pub fn write_around_items(note: &str) -> Vec<String> {
    let Some(list) = note.strip_prefix(WRITE_AROUND.trim_end()) else {
        return vec![];
    };
    let mut items: Vec<String> = list
        .trim_start_matches(':')
        .split(';')
        .map(|i| i.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|i| !i.is_empty())
        .collect();
    if items.last().is_some_and(|i| i.ends_with('\u{2026}')) {
        items.pop();
    }
    items
}

/// The text with each marker the person answered replaced by the answer, where the marker stood. An answer that has no
/// closing stop gets one when the marker stood alone as a paragraph, and one that has the stop the sentence already
/// had is not doubled. An answer that holds a marker is not used, so the text never gains one. Code is left as it is.
pub fn fill(md: &str, answers: &[(String, String)]) -> String {
    let mut out = String::with_capacity(md.len());
    let mut at = 0;
    for (start, end, what) in loose_markers(md) {
        let wanted = key(&what);
        let Some(answer) = answers
            .iter()
            .rev()
            .find(|(w, _)| key(w) == wanted)
            .map(|(_, a)| a.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|a| !a.is_empty() && !a.to_ascii_lowercase().contains("[needed"))
        else {
            continue;
        };
        out.push_str(&md[at..start]);
        at = end;
        let line_start = md[..start].rfind('\n').map_or(0, |p| p + 1);
        let line_end = md[end..].find('\n').map_or(md.len(), |p| end + p);
        let (before, after) = (&md[line_start..start], &md[end..line_end]);
        let mut text = answer;
        if before.trim().is_empty() && after.trim().is_empty() {
            if !text.ends_with(['.', '!', '?', ':', ')', '"', '\u{201d}']) {
                text.push('.');
            }
        } else if after.starts_with('.') && text.ends_with('.') {
            text.pop();
        }
        out.push_str(&text);
    }
    out.push_str(&md[at..]);
    out
}

/// A line of Markdown as `drop_unfilled` takes it: what it is, and so what removing a marker from it means.
enum Unit {
    Fence(String),
    Blank,
    Heading(usize, String),
    /// A list item: its bullet, then its text with the lines that continue it.
    Item(String, String),
    Row(String),
    Paragraph(String),
}

/// The level of a heading line, if it is one.
fn heading_level(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let n = t.bytes().take_while(|b| *b == b'#').count();
    ((1..=6).contains(&n) && t[n..].starts_with([' ', '\t'])).then_some(n)
}

/// The length of a list item's bullet and the space after it, if the line starts one.
fn bullet(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let lead = line.len() - t.len();
    let after = t
        .strip_prefix(['-', '*', '+'])
        .filter(|r| r.starts_with(' '))
        .map(|_| 1)
        .or_else(|| {
            let digits = t.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0 && t[digits..].starts_with(['.', ')']) && t[digits + 1..].starts_with(' '))
                .then_some(digits + 1)
        })?;
    Some(lead + after + 1)
}

fn units(md: &str) -> Vec<Unit> {
    let (mut out, mut fence) = (Vec::<Unit>::new(), None::<String>);
    for line in md.lines() {
        let t = line.trim_start();
        let fence_line = t.starts_with("```") || t.starts_with("~~~");
        if let Some(f) = fence.as_mut() {
            f.push('\n');
            f.push_str(line);
            if fence_line {
                out.push(Unit::Fence(fence.take().unwrap_or_default()));
            }
        } else if fence_line {
            fence = Some(line.to_string());
        } else if t.is_empty() {
            out.push(Unit::Blank);
        } else if let Some(n) = heading_level(line) {
            out.push(Unit::Heading(n, line.to_string()));
        } else if let Some(n) = bullet(line) {
            out.push(Unit::Item(line[..n].to_string(), line[n..].to_string()));
        } else if t.starts_with('|') {
            out.push(Unit::Row(line.to_string()));
        } else {
            match out.last_mut() {
                Some(Unit::Paragraph(p) | Unit::Item(_, p)) => {
                    p.push('\n');
                    p.push_str(line);
                }
                _ => out.push(Unit::Paragraph(line.to_string())),
            }
        }
    }
    if let Some(f) = fence {
        out.push(Unit::Fence(f));
    }
    out
}

/// Words that end in a full stop without ending a sentence.
const ABBREVIATIONS: [&str; 10] = [
    "e.g", "i.e", "vs", "etc", "dr", "mr", "mrs", "ms", "no", "inc",
];

/// Whether a line starts something of its own: a list item, a bold label, or a short "Label:".
fn opens_a_line(line: &str) -> bool {
    if line.starts_with("**") || line.starts_with("__") || bullet(line).is_some() {
        return true;
    }
    let label: String = line.chars().take_while(|c| *c != ':').collect();
    let ends_there = line[label.len()..]
        .strip_prefix(':')
        .is_some_and(|r| r.is_empty() || r.starts_with(' '));
    ends_there
        && !label.is_empty()
        && label.chars().count() <= 30
        && label
            .chars()
            .all(|c| c.is_alphanumeric() || " /&'()-".contains(c))
}

/// The text without the sentences that hold a marker, the rest joined as it was. Empty when every sentence holds one.
/// A sentence ends at `.`, `!` or `?` (and any closing quote) followed by a space, unless the next word is lower case
/// or the word before is an abbreviation, and at a line break in the cases `opens_a_line` and the paragraph below
/// describe. The inside of a marker never ends one.
fn without_marked_sentences(text: &str) -> String {
    let spans = markers(text);
    if spans.is_empty() {
        return text.to_string();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let (mut ends, mut k) = (Vec::new(), 0);
    while k < chars.len() {
        let (i, c) = chars[k];
        if let Some(&(_, close, _)) = spans.iter().find(|s| s.0 <= i && i < s.1) {
            while k < chars.len() && chars[k].0 < close {
                k += 1;
            }
            continue;
        }
        k += 1;
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let mut j = k;
        while j < chars.len() && "\"')*_\u{201d}\u{2019}".contains(chars[j].1) {
            j += 1;
        }
        if j < chars.len() && !chars[j].1.is_whitespace() {
            continue;
        }
        let word: String = text[..chars[k - 1].0]
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let lower_next = chars[j..]
            .iter()
            .find(|(_, c)| !c.is_whitespace())
            .is_some_and(|(_, c)| c.is_lowercase());
        if ABBREVIATIONS.contains(&word.as_str()) || lower_next {
            continue;
        }
        ends.push(chars.get(j).map_or(text.len(), |(b, _)| *b));
        k = j;
    }
    // A line also ends a unit where the text has no sentence stops at all (a signature, an address, a block of key
    // facts) or where the next line opens a list item, a bold label or a "Label:". Only the marked line then goes, and
    // the facts on the lines around it stay.
    let punctuated = !ends.is_empty();
    for (b, _) in text.match_indices('\n') {
        if spans.iter().any(|s| s.0 <= b && b < s.1) {
            continue;
        }
        if !punctuated || opens_a_line(text[b + 1..].trim_start_matches([' ', '\t'])) {
            ends.push(b);
        }
    }
    ends.sort_unstable();
    ends.dedup();
    if ends.last() != Some(&text.len()) {
        ends.push(text.len());
    }
    let (mut out, mut from) = (String::new(), 0);
    for end in ends {
        let sentence = &text[from..end];
        let holds = spans.iter().any(|s| s.0 < end && s.1 > from);
        from = end;
        if holds {
            continue;
        }
        let body = sentence.trim_start();
        if !out.is_empty() {
            out.push_str(&sentence[..sentence.len() - body.len()]);
        }
        out.push_str(body);
    }
    out.trim_end().to_string()
}

/// The text with every marker left in it taken out, and what only that marker held up. A paragraph or list item that
/// is only a marker goes. A marker inside a sentence takes the whole sentence with it, so no half sentence is left
/// (`at .`) and no space is doubled. A heading with a marker in it goes. A table cell is left empty. A heading whose
/// section lost something to this and has no text left goes too, down to the next heading as high or higher; a
/// heading the author left empty stays. Code is left as it is. Text with no marker comes back as it was.
pub fn drop_unfilled(md: &str) -> String {
    if loose_markers(md).is_empty() {
        return md.to_string();
    }
    // What each unit becomes: its text (None when it is gone), the level if it is a heading, whether something was
    // taken from it, and whether what is left counts as text under a heading.
    struct Out {
        text: Option<String>,
        level: Option<usize>,
        cut: bool,
        body: bool,
    }
    let mut out: Vec<Out> = units(md)
        .into_iter()
        .map(|u| {
            let (text, level, cut, body) = match u {
                Unit::Fence(t) => (Some(t), None, false, true),
                Unit::Blank => (Some(String::new()), None, false, false),
                Unit::Heading(n, t) if markers(&t).is_empty() => (Some(t), Some(n), false, false),
                Unit::Heading(n, _) => (None, Some(n), true, false),
                Unit::Row(t) if markers(&t).is_empty() => (Some(t), None, false, true),
                Unit::Row(t) => {
                    let mut cut = String::new();
                    let mut at = 0;
                    for (s, e, _) in markers(&t) {
                        cut.push_str(&t[at..s]);
                        at = e;
                    }
                    cut.push_str(&t[at..]);
                    (Some(cut), None, true, true)
                }
                Unit::Item(bullet, t) if markers(&t).is_empty() => {
                    (Some(format!("{bullet}{t}")), None, false, true)
                }
                Unit::Paragraph(t) if markers(&t).is_empty() => (Some(t), None, false, true),
                Unit::Item(bullet, t) => {
                    let kept = without_marked_sentences(&t);
                    let text = (!kept.is_empty()).then(|| format!("{bullet}{kept}"));
                    let body = text.is_some();
                    (text, None, true, body)
                }
                Unit::Paragraph(t) => {
                    let kept = without_marked_sentences(&t);
                    let text = (!kept.is_empty()).then_some(kept);
                    let body = text.is_some();
                    (text, None, true, body)
                }
            };
            Out {
                text,
                level,
                cut,
                body,
            }
        })
        .collect();
    // A heading is gone when its section lost something and has nothing left. Judged on the headings that survived
    // the first pass, all at once, so a parent over only empty children goes with them.
    let alive: Vec<usize> = (0..out.len())
        .filter(|&i| out[i].level.is_some() && out[i].text.is_some())
        .collect();
    let mut gone = Vec::new();
    for (n, &i) in alive.iter().enumerate() {
        let level = out[i].level.unwrap_or(1);
        let to = alive[n + 1..]
            .iter()
            .find(|&&j| out[j].level.unwrap_or(1) <= level)
            .copied()
            .unwrap_or(out.len());
        let section = &out[i + 1..to];
        if section.iter().any(|u| u.cut) && !section.iter().any(|u| u.body && u.text.is_some()) {
            gone.push(i);
        }
    }
    for i in gone {
        out[i].text = None;
    }
    let mut lines: Vec<String> = Vec::new();
    for u in out {
        match u.text {
            Some(t) if t.is_empty() => {
                if lines.last().is_some_and(|l| !l.is_empty()) {
                    lines.push(t);
                }
            }
            Some(t) => lines.push(t),
            None => {}
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut text = lines.join("\n");
    if !text.is_empty() && md.ends_with('\n') {
        text.push('\n');
    }
    // A marker split across what this took for two units is closed up the way `strip` does, so none is ever left.
    if loose_markers(&text).is_empty() {
        text
    } else {
        strip(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whats(md: &str) -> Vec<String> {
        find(md).into_iter().map(|g| g.what).collect()
    }

    #[test]
    fn two_markers_and_a_still_open_bullet_are_three_details() {
        let md = "# Offer\n\nThe price is [NEEDED: the price per seat] and it starts on [NEEDED: the start date].\n\n## Still open before sending\n\n- Name the decision owner.\n\n## Source warnings\n\nNone.\n";
        assert_eq!(
            whats(md),
            [
                "the price per seat",
                "the start date",
                "Name the decision owner."
            ]
        );
    }

    #[test]
    fn a_marker_split_by_a_line_break_is_one_detail_and_a_nested_bracket_stays() {
        assert_eq!(
            whats("Cost [NEEDED: the\ncost per   seat] now."),
            ["the cost per seat"]
        );
        assert_eq!(
            whats("Cost [NEEDED: price [per week]] now."),
            ["price [per week]"]
        );
    }

    #[test]
    fn a_detail_said_twice_counts_once() {
        let md = "A [NEEDED: the date] B [needed: The  date] C.\n\n## Still open before sending\n\n- the date\n- Other.\n";
        assert_eq!(whats(md), ["the date", "Other."]);
    }

    #[test]
    fn a_result_from_before_markers_still_counts_what_is_open_and_none_is_not_a_detail() {
        assert_eq!(
            whats("# T\n\nBody.\n\n## Still open before sending\n\nNeed a source for the claim.\n"),
            ["Need a source for the claim."]
        );
        assert_eq!(
            whats("# T\n\n## Still open before sending\n\n1. One.\n2) Two\n   goes on.\n"),
            ["One.", "Two goes on."]
        );
        assert!(whats("# T\n\n## Still open before sending\n\nNone.\n").is_empty());
        assert!(whats("# T\n\nA finished piece.\n").is_empty());
        assert!(
            whats("# T\n\n## Still open before sending\n\n- [NEEDED: x]\n").len() == 1,
            "a marker in the section is counted once, as a marker"
        );
    }

    #[test]
    fn a_marker_that_never_closes_ends_with_its_line_and_a_blank_marker_is_named() {
        assert_eq!(
            whats("A [NEEDED: the date\n\nNext paragraph with a ] later."),
            ["the date"]
        );
        assert_eq!(whats("A [NEEDED: ] B"), [UNNAMED]);
    }

    #[test]
    fn strip_leaves_no_marker_and_closes_up_the_gap() {
        let md = "Start [NEEDED: the date] end.\nPrice [NEEDED: price [per week]], then more.\n- [NEEDED: x]\n- Kept\nA [NEEDED: a\nb] z\n";
        let out = strip(md);
        assert!(!out.to_ascii_uppercase().contains("[NEEDED"), "{out}");
        assert_eq!(out, "Start end.\nPrice, then more.\n- Kept\nA z\n");
        let plain = "Nothing marked.\n- a\n-\n";
        assert_eq!(strip(plain), plain, "text without a marker is untouched");
    }

    /// The shape of the text a live run finished with: a heading over a marker alone, a marker closing a sentence, two
    /// markers on separate lines of one paragraph, a list of nothing but markers, and a marker inline in a sentence.
    const LIVE: &str = "# Your weekly box is changing

We are moving to a fortnightly box from [NEEDED: the date the change starts]. Your delivery day stays the same.

## Why we are making the change

[NEEDED: the reason for the change]

## What stays the same

Your recipes and your price per box stay the same.

## Skipping a week

Tell us by Friday if you want to skip. [NEEDED: whether subscribers can skip a week and how]

## Dates

[NEEDED: the first fortnightly delivery date]
[NEEDED: the last weekly delivery date]

## Prices

- [NEEDED: price of the small box]
- [NEEDED: price of the large box]

## Contact

Write to us any time.
You can also reach us at [NEEDED: the support email address].
Call [NEEDED: the phone number]
";

    fn lines(l: &[&str]) -> Vec<String> {
        l.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn drop_unfilled_takes_out_every_marker_and_what_only_it_held_up() {
        let out = drop_unfilled(LIVE);
        assert_eq!(
            out,
            "# Your weekly box is changing

Your delivery day stays the same.

## What stays the same

Your recipes and your price per box stay the same.

## Skipping a week

Tell us by Friday if you want to skip.

## Contact

Write to us any time.
"
        );
        assert!(markers(&out).is_empty());
    }

    #[test]
    fn drop_unfilled_leaves_no_orphan_words_doubled_spaces_or_empty_bullets() {
        let out = drop_unfilled(
            "Start.\n\nYou can also reach us at [NEEDED: the email]. Thanks.\n\n- Kept\n- [NEEDED: x]\n- Also kept [NEEDED: y] with more.\n- Last one.\n\nA [NEEDED: a] B. C [NEEDED: c], d.\n",
        );
        assert_eq!(out, "Start.\n\nThanks.\n\n- Kept\n- Last one.\n");
        for bad in ["  ", " .", "at.", "- \n", "-\n"] {
            assert!(!out.contains(bad), "{bad:?} in {out:?}");
        }
    }

    #[test]
    fn drop_unfilled_removes_a_heading_only_when_its_section_lost_everything_and_keeps_an_authors_empty_heading(
    ) {
        let md = "# Title\n\n## Part\n\n### Inner\n\n[NEEDED: a]\n\n## Next\n\nText.\n\n## Author left this empty\n\n## Last\n\n- [NEEDED: z]\n";
        assert_eq!(
            drop_unfilled(md),
            "# Title\n\n## Next\n\nText.\n\n## Author left this empty\n"
        );
        // A heading whose section keeps text from a deeper heading stays.
        assert_eq!(
            drop_unfilled("## A\n\n### B\n\nKept.\n\n### C\n\n[NEEDED: c]\n"),
            "## A\n\n### B\n\nKept.\n"
        );
    }

    #[test]
    fn drop_unfilled_on_a_document_of_only_markers_is_empty_and_text_without_markers_is_untouched()
    {
        assert_eq!(drop_unfilled("# Title\n\n[NEEDED: a]\n\n[NEEDED: b]\n"), "");
        let plain = "# T\n\n\n\nBody.\n\n\n- a\n";
        assert_eq!(drop_unfilled(plain), plain);
    }

    #[test]
    fn drop_unfilled_keeps_code_fences_and_abbreviations_and_table_rows_whole() {
        let md = "Run it.\n\n```\nprice = \"[NEEDED: price]\"\n```\n\nShip it, e.g. on [NEEDED: the day] or later. Done.\n\n| Plan | Price |\n| --- | --- |\n| Small | [NEEDED: price] |\n";
        let out = drop_unfilled(md);
        assert!(out.contains("price = \"[NEEDED: price]\""), "{out}");
        assert!(out.contains("Done.") && !out.contains("e.g."), "{out}");
        assert!(out.contains("| Small |  |"), "{out}");
        assert_eq!(markers(&out).len(), 1, "only the one in the fence is left");
    }

    #[test]
    fn answers_are_found_by_what_each_marker_says_whatever_colons_it_holds() {
        let notes = lines(&[
            "the support email address: help@box.example",
            "Name the owner.: old",
            "Name the owner.: Priya Shah",
            "whether subscribers can skip a week and how:   Yes,  tell us by Friday ",
            "the phone number:",
            "something unrelated: x",
        ]);
        let md = "A [NEEDED: Name the owner.] B [needed: The  support email   address] C [NEEDED: whether subscribers can skip a week and how] D [NEEDED: the phone number] E [NEEDED: nobody answered].";
        assert_eq!(
            answers_for(md, &notes),
            [
                ("Name the owner.".to_string(), "Priya Shah".to_string()),
                (
                    "The support email address".to_string(),
                    "help@box.example".to_string()
                ),
                (
                    "whether subscribers can skip a week and how".to_string(),
                    "Yes, tell us by Friday".to_string()
                ),
            ]
        );
    }

    #[test]
    fn fill_replaces_an_answered_marker_in_place_and_adds_a_stop_only_to_a_marker_that_stood_alone()
    {
        let answers = vec![
            (
                "the reason for the change".to_string(),
                "We are cutting waste".to_string(),
            ),
            (
                "whether subscribers can skip a week and how".to_string(),
                "Yes, tell us by Friday.".to_string(),
            ),
            ("price of the small box".to_string(), "$12".to_string()),
            (
                "the date the change starts".to_string(),
                "1 March.".to_string(),
            ),
        ];
        let out = fill(LIVE, &answers);
        assert!(
            out.contains("fortnightly box from 1 March. Your delivery day"),
            "a stop the answer has is not doubled: {out}"
        );
        assert!(
            out.contains("## Why we are making the change\n\nWe are cutting waste.\n"),
            "{out}"
        );
        assert!(out.contains("skip. Yes, tell us by Friday.\n"), "{out}");
        assert!(out.contains("- $12\n"), "a list item gets no stop: {out}");
        assert_eq!(
            markers(&out).len(),
            LIVE.matches("[NEEDED").count() - 4,
            "only the answered four were replaced: {out}"
        );
        assert!(out.contains("[NEEDED: price of the large box]"));
        // An answer that holds a marker is not used, so the text never gains one.
        let tricky = vec![("a".to_string(), "see [NEEDED: b]".to_string())];
        assert_eq!(fill("x [NEEDED: a] y", &tricky), "x [NEEDED: a] y");
        // Nothing answered, nothing changed.
        assert_eq!(fill(LIVE, &[]), LIVE);
    }

    #[test]
    fn a_marker_in_a_block_of_lines_without_stops_takes_only_its_own_line() {
        // Key facts: the person's own facts either side of the marker stay.
        assert_eq!(
            drop_unfilled("**Owner:** Priya\n**Start:** [NEEDED: date]\n**Budget:** $5\n"),
            "**Owner:** Priya\n**Budget:** $5\n"
        );
        // A signature.
        assert_eq!(
            drop_unfilled("Thanks for reading.\n\nBest,\nJane\n[NEEDED: title]\n"),
            "Thanks for reading.\n\nBest,\nJane\n"
        );
        // An address.
        assert_eq!(
            drop_unfilled("Acme Bakery\n[NEEDED: street address]\nSpringfield\n"),
            "Acme Bakery\nSpringfield\n"
        );
        // A block of labels with a sentence above it: the labels still end each line.
        assert_eq!(
            drop_unfilled("The details follow.\nOwner: Priya\nStart: [NEEDED: date]\nBudget: $5\n"),
            "The details follow.\nOwner: Priya\nBudget: $5\n"
        );
        // A list item with a marker on a continued line and labelled lines.
        assert_eq!(
            drop_unfilled("- Owner: Priya\n  Start: [NEEDED: date]\n  Budget: $5\n"),
            "- Owner: Priya\n  Budget: $5\n"
        );
        // Lines that are all markers still go whole.
        assert_eq!(
            drop_unfilled("Keep.\n\n[NEEDED: a]\n[NEEDED: b]\n"),
            "Keep.\n"
        );
    }

    #[test]
    fn a_marker_inside_a_code_fence_is_not_a_missing_detail() {
        let md = "Run:\n\n```\nprice = \"[NEEDED: price]\"\n```\n";
        assert!(find(md).is_empty());
        assert_eq!(
            whats("A [NEEDED: real].\n\n```\n[NEEDED: fenced]\n```\n"),
            ["real"]
        );
        // Fenced code is left alone by fill and drop, so a fenced marker is neither counted nor changed.
        assert_eq!(drop_unfilled(md), md);
    }

    #[test]
    fn a_write_around_note_gives_back_its_items_and_a_comma_or_colon_does_not_split_one() {
        let note = format!(
            "{WRITE_AROUND}the price, per seat; whether to skip: how;  Name   the owner.; "
        );
        assert_eq!(
            write_around_items(&note),
            [
                "the price, per seat",
                "whether to skip: how",
                "Name the owner."
            ]
        );
        // A list the note limit cut short loses its cut last item; a note of another kind has none.
        assert_eq!(
            write_around_items(&format!("{WRITE_AROUND}a; b; c long ite\u{2026}")),
            ["a", "b"]
        );
        assert!(write_around_items("Details from the person:\nx: y").is_empty());
        assert!(answered("the price", &lines(&["The  Price: $5"])));
        assert!(!answered(
            "the price",
            &lines(&["the price:  ", "the price per seat: 5"])
        ));
    }
}
