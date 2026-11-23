//! The files a client receives: a Word document, a PDF of it, and a PowerPoint deck, made from the results of
//! two steps. Parts that are for the team and not the client are taken out first (a "Source warnings"
//! section). Word and PowerPoint use local libraries; Typst remains the PDF helper.

use crate::office_export;
use std::path::{Path, PathBuf};

/// PDF helper detected on this Mac. Office renderers are built into the app.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub typst: Option<PathBuf>,
    /// Build a Keynote deck as well. Off unless the app turns it on for an approved plan that allows it; never
    /// on in tests, since it opens Keynote.
    pub keynote: bool,
}

impl Tools {
    pub fn detect(home: &str) -> Tools {
        Tools {
            typst: crate::typst::detect_in(&crate::typst::candidate_paths(home)).map(|(p, _)| p),
            keynote: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// `word`, `pdf`, `deck`, `keynote`, or `sheet` (the document's tables as an Excel workbook).
    pub kind: &'static str,
    pub path: PathBuf,
}

pub(crate) fn heading_level(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    (1..=6)
        .contains(&n)
        .then(|| t[n..].strip_prefix(' ').map(|rest| (n, rest.trim())))
        .flatten()
}

/// A complete, explicitly labelled Markdown response envelope is formatting, not a code example.
/// Leave other languages, mixed prose/code, nested fences and incomplete envelopes untouched.
/// The source artifact is retained separately from the client-facing conversion.
pub fn document_body(md: &str) -> &str {
    let whole = md.trim();
    let Some((open, rest)) = whole.split_once('\n') else {
        return md;
    };
    if !matches!(open.trim_end_matches('\r'), "```markdown" | "```md") {
        return md;
    }
    let Some((body, close)) = rest.rsplit_once('\n') else {
        return md;
    };
    if close.trim() != "```"
        || body.trim().is_empty()
        || body
            .lines()
            .any(|line| line.trim_start().starts_with("```"))
    {
        return md;
    }
    body
}

/// The words of a line with block marks (quote, list, emphasis, a colon) taken off, for recognising a label.
fn plain_words(line: &str) -> String {
    let mut t = line.trim_start();
    loop {
        let before = t.len();
        t = t.trim_start_matches(['>', ' ', '\t']);
        for m in ["- ", "* ", "+ "] {
            if let Some(r) = t.strip_prefix(m) {
                t = r;
            }
        }
        // A numbered item such as "1. "
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        if let Some(r) = t[digits..].strip_prefix(". ").filter(|_| digits > 0) {
            t = r;
        }
        if t.len() == before {
            break;
        }
    }
    t.trim()
        .trim_matches(['*', '_'])
        .trim()
        .trim_end_matches(':')
        .trim_matches(['*', '_'])
        .trim()
        .to_string()
}

/// The section where a closing step says what it could not fix.
const STILL_OPEN: &str = "still open before sending";

fn is_warning_words(words: &str) -> bool {
    words.eq_ignore_ascii_case("source warnings")
        || words.eq_ignore_ascii_case("source warning")
        || words.eq_ignore_ascii_case(STILL_OPEN)
}

/// What a skipped label block lasts until.
enum Until {
    /// The next heading of any level, or the end.
    Heading,
    /// The first line that is not part of the quote.
    QuoteEnds,
    /// The first line that is not indented.
    ListEnds,
}

/// Removes what is for the team and not the client: any section headed "Source warnings" or "Source warning" (at
/// any level, to the next heading of the same level or higher), a bold, quoted or listed label of that name and
/// what belongs to it, HTML comments, any line that reports the tooling a worker used, and the markers of details the
/// person has not supplied. Code in a fenced block (backticks or tildes) is left as it is, except for those markers.
/// Returns the text and whether something was removed.
pub fn strip_internal(md: &str) -> (String, bool) {
    let md = document_body(md);
    let (mut out, mut skip, mut removed) = (Vec::<String>::new(), None::<usize>, false);
    let mut labelled: Option<Until> = None;
    let mut fence: Option<char> = None;
    let mut in_comment = false;
    for raw in md.lines() {
        let trimmed = raw.trim_start();
        let opens = ['`', '~']
            .into_iter()
            .find(|c| trimmed.starts_with(&c.to_string().repeat(3)));
        match (fence, opens) {
            (None, Some(c)) => fence = Some(c),
            (Some(f), Some(c)) if f == c => fence = None,
            _ => {}
        }
        let fenced = fence.is_some() || opens.is_some();
        let mut line = raw.to_string();
        if !fenced {
            // HTML comments are not shown to a client, and one may hold notes for the team.
            loop {
                if in_comment {
                    match line.find("-->") {
                        Some(i) => {
                            line = line[i + 3..].to_string();
                            in_comment = false;
                        }
                        None => {
                            line.clear();
                            break;
                        }
                    }
                } else if let Some(i) = line.find("<!--") {
                    match line[i + 4..].find("-->") {
                        Some(j) => line = format!("{}{}", &line[..i], &line[i + 4 + j + 3..]),
                        None => {
                            line.truncate(i);
                            in_comment = true;
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
            if line != raw {
                removed = true;
                if line.trim().is_empty() {
                    continue;
                }
            }
        }
        let line = line.as_str();
        // A worker may add a line about the tools it used, because of standing instructions of the person's own.
        // It is for the team, and not for the client.
        if !fenced && plain_words(line).to_lowercase().starts_with("tooling used") {
            removed = true;
            continue;
        }
        if !fenced {
            if let Some(until) = &labelled {
                let ends = match until {
                    Until::Heading => heading_level(line).is_some(),
                    Until::QuoteEnds => !line.trim_start().starts_with('>'),
                    Until::ListEnds => !line.trim().is_empty() && !line.starts_with([' ', '\t']),
                };
                if ends {
                    labelled = None;
                }
            }
            if labelled.is_none() && skip.is_none() && heading_level(line).is_none() {
                let t = line.trim_start();
                let bold = t.trim_start_matches(['>', ' ', '-', '+']).starts_with("**");
                let numbered = t.chars().next().is_some_and(|c| c.is_ascii_digit());
                let listed = (t.starts_with(['-', '*', '+']) && !t.starts_with("**")) || numbered;
                // Only a label on its own line: bold, or inside a quote or a list.
                if is_warning_words(&plain_words(line)) && (bold || listed || t.starts_with('>')) {
                    labelled = Some(if t.starts_with('>') {
                        Until::QuoteEnds
                    } else if listed {
                        Until::ListEnds
                    } else {
                        Until::Heading
                    });
                    removed = true;
                    continue;
                }
            }
        }
        if labelled.is_some() && !fenced {
            continue;
        }
        if !fenced {
            if let Some((n, text)) = heading_level(line) {
                if let Some(level) = skip {
                    if n <= level {
                        skip = None;
                    }
                }
                if skip.is_none() && is_warning_words(text.trim_end_matches(':')) {
                    skip = Some(n);
                    removed = true;
                    continue;
                }
            }
        }
        if skip.is_none() {
            out.push(line.to_string());
        }
    }
    // A marker for a detail only the person can supply is for them, so it is taken out of every file as well.
    let text = out.join("\n").trim_end().to_string() + "\n";
    (crate::gaps::strip(&text), removed)
}

/// What a closing step could not fix, for the person: the body of the section `strip_internal` keeps from the client.
pub fn still_open(md: &str) -> String {
    let md = document_body(md);
    let (mut level, mut out) = (None, Vec::new());
    for line in md.lines() {
        match (heading_level(line), level) {
            (Some((n, t)), None) if t.trim_end_matches(':').eq_ignore_ascii_case(STILL_OPEN) => {
                level = Some(n)
            }
            (Some((n, _)), Some(l)) if n <= l => break,
            (_, Some(_)) => out.push(line),
            _ => {}
        }
    }
    out.join("\n").trim().to_string()
}

/// The first first-level heading, and the text without it. A result usually opens with its own title.
fn take_title(md: &str) -> (Option<String>, String) {
    let mut lines = md.lines().peekable();
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    match lines.peek().and_then(|l| heading_level(l)) {
        Some((1, t)) => {
            let title = t.to_string();
            lines.next();
            (Some(title), lines.collect::<Vec<_>>().join("\n"))
        }
        _ => (None, md.to_string()),
    }
}

/// A normal Markdown title and subtitle. No converter-specific metadata is injected.
pub fn titled(project: &str, subtitle: &str, md: &str) -> String {
    let (title, body) = take_title(md);
    let title = title.unwrap_or_else(|| project.to_string());
    format!("# {title}\n\n{subtitle}\n\n{}\n", body.trim_start())
}

fn slug(s: &str) -> String {
    let t: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    t.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Writes the client files into a new folder `out_dir`, which must not exist. A tool that is missing, or a
/// conversion that fails, is a note and not an error, so the other files are still made.
pub fn make(
    tools: &Tools,
    out_dir: &Path,
    project: &str,
    subtitle: &str,
    document: Option<&str>,
    deck: Option<&str>,
) -> Result<(Vec<Made>, Vec<String>), String> {
    for text in [Some(project), Some(subtitle), document, deck]
        .into_iter()
        .flatten()
    {
        if let Some(f) = crate::secrets::scan_export(text)
            .into_iter()
            .find(|f| f.class == crate::secrets::Class::Secret)
        {
            return Err(format!(
                "This line looks like it holds a key, so the files weren't made: line {}, \"{}\". Remove it and try again.",
                f.line, f.excerpt
            ));
        }
    }
    for (what, text) in [("document", document), ("deck", deck)] {
        if text.is_some_and(|t| t.trim().is_empty()) {
            return Err(format!(
                "The {what} step's result is empty, so no files were made. Run that step again"
            ));
        }
        if let Some(text) = text {
            office_export::parse(project, subtitle, &strip_internal(text).0)?;
        }
    }
    if out_dir.exists() {
        return Err(format!(
            "{} already exists. Nothing was overwritten",
            out_dir.display()
        ));
    }
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let (mut made, mut notes) = (Vec::new(), Vec::new());
    // A template's look the person chose for this project, or SproutCrew's own.
    let look = crate::look::for_export(out_dir);
    let base = slug(project);
    let work = out_dir.join(".work");
    if let Some(md) = document {
        let (md, removed) = strip_internal(md);
        if removed {
            notes.push("The notes for the team were left out of the document".to_string());
        }
        let (title, body) = take_title(&md);
        match office_export::parse(title.as_deref().unwrap_or(project), subtitle, &body) {
            Ok(mut document) => {
                document.look = look.clone();
                notes.extend(document.warnings.clone());
                let out = out_dir.join(format!("{base}-document.docx"));
                match office_export::word(&document, &out) {
                    Ok(()) => made.push(Made {
                        kind: "word",
                        path: out,
                    }),
                    Err(e) => notes.push(format!("The Word file was not made: {e}")),
                }
                // The document's tables also go to a workbook, as values, when it has any.
                let sheet = out_dir.join(format!("{base}-data.xlsx"));
                match crate::xlsx::sheet(&document, &sheet) {
                    Ok(0) => {}
                    Ok(_) => made.push(Made {
                        kind: "sheet",
                        path: sheet,
                    }),
                    Err(e) => notes.push(format!("The Excel file was not made: {e}")),
                }
            }
            Err(e) => notes.push(format!("The Word file was not made: {e}")),
        }
        match &tools.typst {
            Some(t) => {
                let (title, body) = take_title(&md);
                let title = title.unwrap_or_else(|| project.to_string());
                // The page follows the template's look; with none, these are SproutCrew's own settings.
                let margin = look.margins_mm.map_or("2.4cm".to_string(), |[t, r, b, l]| {
                    format!("(top: {t}mm, right: {r}mm, bottom: {b}mm, left: {l}mm)")
                });
                let size = look.page_mm.map_or(String::new(), |[w, h]| {
                    format!("width: {w}mm, height: {h}mm, ")
                });
                let pdf_work = work.join("pdf");
                let header = match look.logo.as_deref().map(|b| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.decode(b)
                }) {
                    Some(Ok(png)) => {
                        std::fs::create_dir_all(&pdf_work).map_err(|e| e.to_string())?;
                        std::fs::write(pdf_work.join("logo.png"), png)
                            .map_err(|e| e.to_string())?;
                        "header: align(right, image(\"logo.png\", height: 1cm)), ".to_string()
                    }
                    _ => String::new(),
                };
                let heading_font = match &look.heading_font {
                    Some(h) => format!(
                        "#show heading: set text(font: {})\n",
                        serde_json::to_string(h).map_err(|e| e.to_string())?
                    ),
                    None => String::new(),
                };
                let source = format!(
                    "#set page({size}{header}margin: {margin}, numbering: \"1\")\n#set text(font: {}, size: {}pt)\n{heading_font}#set par(justify: false, leading: 0.7em)\n#align(left)[#text(22pt, weight: \"bold\")[{}] \\\n#text(12pt, fill: gray)[{}]]\n#v(1em)\n{}",
                    serde_json::to_string(&look.font).map_err(|e| e.to_string())?,
                    look.body_pt.unwrap_or(10.5),
                    crate::typst::markdown_to_typst(&title).trim(),
                    crate::typst::markdown_to_typst(subtitle).trim(),
                    office_export::typst_body(&office_export::parse("", "", &body)?)
                );
                let out = out_dir.join(format!("{base}-document.pdf"));
                match crate::typst::render(t, &source, &pdf_work, &out) {
                    Ok(()) => made.push(Made {
                        kind: "pdf",
                        path: out,
                    }),
                    Err(e) => notes.push(format!("The PDF was not made: {e}")),
                }
            }
            None => notes.push("The PDF needs Typst, which was not found".to_string()),
        }
    }
    if let Some(md) = deck {
        let (md, removed) = strip_internal(md);
        if removed {
            notes.push("The notes for the team were left out of the deck".to_string());
        }
        // A deck that has no title of its own takes the document's, and the project name is the last resort.
        let from_doc = document
            .map(|d| take_title(&strip_internal(d).0).0)
            .unwrap_or(None);
        let (deck_title, body) = take_title(&md);
        let title = deck_title
            .or(from_doc)
            .unwrap_or_else(|| project.to_string());
        match office_export::parse(&title, subtitle, &body) {
            Ok(mut document) => {
                document.look = look.clone();
                notes.extend(document.warnings.clone());
                let out = out_dir.join(format!("{base}-deck.pptx"));
                match office_export::deck(&document, &out) {
                    Ok(()) => made.push(Made {
                        kind: "deck",
                        path: out,
                    }),
                    Err(e) => notes.push(format!("The deck was not made: {e}")),
                }
                if tools.keynote {
                    // Keynote builds the same slides through its own scripting, and the deck is then read back
                    // and compared, so a slide Keynote changed or dropped is reported.
                    let slides = crate::keynote::slides_from(&document);
                    let key = out_dir.join(format!("{base}-deck.key"));
                    // The template's logo goes in as a picture file inside the work folder, which Keynote copies
                    // into the deck.
                    let logo = look.logo.as_deref().and_then(|b| {
                        use base64::Engine;
                        let png = base64::engine::general_purpose::STANDARD.decode(b).ok()?;
                        let dir = work.join("keynote");
                        std::fs::create_dir_all(&dir).ok()?;
                        let p = dir.join("logo.png");
                        std::fs::write(&p, png).ok()?;
                        Some(p)
                    });
                    let style = crate::keynote::Style::from_look(&document.look, logo);
                    match crate::keynote::build(&key, out_dir, &slides, &style) {
                        Ok(p) => {
                            made.push(Made {
                                kind: "keynote",
                                path: p.clone(),
                            });
                            match crate::keynote::read_back(&p) {
                                Ok(got) => {
                                    let diff = crate::keynote::compare(&slides, &got);
                                    if diff.is_empty() {
                                        notes.push(format!(
                                            "The Keynote deck was read back: all {} slides match",
                                            slides.len()
                                        ));
                                    } else {
                                        notes.push(format!(
                                            "The Keynote deck differs from the slides: {}",
                                            diff.join("; ")
                                        ));
                                    }
                                }
                                Err(e) => notes.push(format!(
                                    "The Keynote deck was made but not checked: {e}"
                                )),
                            }
                        }
                        Err(e) => notes.push(format!("The Keynote deck was not made: {e}")),
                    }
                }
            }
            Err(e) => notes.push(format!("The deck was not made: {e}")),
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok((made, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_markdown_wrappers_render_as_documents_and_internal_notes_stay_internal() {
        let md = "```markdown\n# Article\n\nBody.\n\n## Source warnings\n\nFor the team.\n```\n";
        assert!(document_body(md).starts_with("# Article"));
        let (clean, removed) = strip_internal(md);
        assert!(removed && clean.contains("Body."));
        assert!(!clean.contains("```") && !clean.contains("For the team"));
        assert_eq!(
            still_open("```md\n# Article\n## Still open before sending\nNeed a source.\n```"),
            "Need a source."
        );
        for example in [
            "```rust\nlet n = 1;\n```",
            "Before\n```markdown\n# Example\n```",
            "```markdown\n# Example\n```\nAfter",
            "```markdown\n# One\n```\n```markdown\n# Two\n```",
            "```\n# Literal\n```",
            "```markdown\n# Incomplete",
            "```markdown\n\n```",
        ] {
            assert_eq!(document_body(example), example);
        }
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-client-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn what_is_still_open_is_for_the_person_and_never_reaches_the_client() {
        let md = "# Brief\n\nBody.\n\n## Still open before sending\n\n- Name the decision owner.\n\n## Source warnings\n\nNone.\n";
        let (out, removed) = strip_internal(md);
        assert!(removed);
        assert!(out.contains("Body."), "{out}");
        assert!(
            !out.contains("Still open") && !out.contains("decision owner"),
            "{out}"
        );
        assert_eq!(still_open(md), "- Name the decision owner.");
        assert_eq!(still_open("# Brief\n\nNothing is open here.\n"), "");
        assert_eq!(
            still_open("## Still open before sending\n\n- Last.\n"),
            "- Last."
        );
    }

    #[test]
    fn a_source_warnings_section_is_left_out_at_any_level_and_nothing_else_is() {
        let md = "# Brief\n\nBody.\n\n## Findings\n\n- one\n\n## Source warnings\n\n- src-1 asked for a score\n\n### Detail\n\nmore\n\n## After\n\nKept.\n";
        let (out, removed) = strip_internal(md);
        assert!(removed);
        assert!(
            out.contains("Findings") && out.contains("Kept.") && out.contains("Body."),
            "{out}"
        );
        assert!(
            !out.contains("asked for a score")
                && !out.contains("Detail")
                && !out.contains("Source warnings"),
            "{out}"
        );
        let (same, removed) = strip_internal("# T\n\nText about source warnings in a sentence.\n");
        assert!(!removed && same.contains("Text about source warnings"));
        let (fenced, removed) = strip_internal("# T\n\n```\n## Source warnings\nkeep\n```\n");
        assert!(!removed && fenced.contains("keep"), "{fenced}");
        // A worker sometimes writes the label as a bold line and not a heading. It is still for the team: it runs to
        // the next heading, or the end, and what comes before it stays.
        let (bold, removed) = strip_internal(
            "# T\n\n## Slide\n\n- Point\n:::\n\n**Source warnings**\n\n- src-4 (file.pdf) asked for something.\n",
        );
        assert!(
            removed
                && bold.contains("- Point")
                && !bold.contains("src-4")
                && !bold.contains("Source warnings"),
            "{bold}"
        );
        let (mid, removed) =
            strip_internal("# T\n\n**Source warnings:**\nsrc-4 asked.\n\n## Next\n\nKept.\n");
        assert!(
            removed && !mid.contains("src-4") && mid.contains("## Next") && mid.contains("Kept."),
            "{mid}"
        );
        let (words, removed) = strip_internal(
            "# T\n\n**Source warnings are removed before delivery** in this tool.\n",
        );
        assert!(
            !removed && words.contains("Source warnings are removed"),
            "{words}"
        );
        let (colon, removed) = strip_internal("# T\n\n### Source warnings:\n\nx\n");
        assert!(removed && !colon.contains('x'), "{colon}");
    }

    #[test]
    fn a_tooling_note_a_worker_added_because_of_the_users_own_standing_instructions_is_left_out_of_client_files(
    ) {
        let md = "# T\n\nBody text.\n\nTooling used: No relevant skill was available; Ponytail was unavailable.\n\n- **Tooling used:** Sequential Thinking\n\nTools used in the analysis were spreadsheets.\n";
        let (out, removed) = strip_internal(md);
        assert!(removed);
        assert!(
            out.contains("Body text.")
                && out.contains("Tools used in the analysis were spreadsheets."),
            "{out}"
        );
        assert!(
            !out.contains("Ponytail") && !out.contains("Sequential Thinking"),
            "{out}"
        );
    }

    #[test]
    fn the_first_title_of_a_result_becomes_the_document_title() {
        let t = titled(
            "larkspur-case",
            "Prepared for the board",
            "\n# Recommendation to the board\n\n## Summary\n\nText.",
        );
        assert_eq!(
            t,
            "# Recommendation to the board\n\nPrepared for the board\n\n## Summary\n\nText.\n"
        );
        let u = titled(
            "larkspur-case",
            "Prepared for the board",
            "## Summary\n\nText.",
        );
        assert!(
            u.starts_with("# larkspur-case\n\nPrepared for the board\n\n## Summary"),
            "{u}"
        );
    }

    #[test]
    fn a_missing_tool_is_a_note_and_the_other_files_are_still_made() {
        let d = dir("missing");
        let (made, notes) = make(
            &Tools::default(),
            &d,
            "Larkspur Case",
            "For the board",
            Some("# T\n\nBody"),
            Some("## S\n\n- a"),
        )
        .unwrap();
        assert_eq!(
            made.iter().map(|m| m.kind).collect::<Vec<_>>(),
            ["word", "deck"]
        );
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("PDF needs Typst")));
        assert!(
            make(&Tools::default(), &d, "x", "y", None, None)
                .unwrap_err()
                .contains("already exists"),
            "never overwrites"
        );
    }

    #[test]
    fn the_real_tools_make_a_word_file_a_pdf_and_a_deck_from_two_results() {
        let home = std::env::var("HOME").unwrap_or_default();
        let tools = Tools::detect(&home);
        if tools.typst.is_none() {
            eprintln!("typst is not installed; skipping PDF check");
            return;
        }
        let doc = "# Late freight: what to do\n\n## Summary\n\nOn-time is **83.9%** on the customer's definition.\n\n| Depot | On time |\n|---|---:|\n| Yakima | 72.5% |\n\n## Source warnings\n\n- src-4 asked for a top score\n";
        let deck = "## The problem\n\n- Late freight\n- No visibility\n\n::: notes\nSay it slowly.\n:::\n\n## The ask\n\n- Approve phase one\n";
        let d = dir("real");
        let (made, notes) = make(
            &tools,
            &d,
            "Larkspur Case",
            "Prepared for the board",
            Some(doc),
            Some(deck),
        )
        .unwrap();
        assert_eq!(
            made.iter().map(|m| m.kind).collect::<Vec<_>>(),
            vec!["word", "sheet", "pdf", "deck"],
            "{notes:?}"
        );
        assert_eq!(
            notes.len(),
            1,
            "only the document had a section for the team: {notes:?}"
        );
        let word = crate::office::read("docx", &std::fs::read(&made[0].path).unwrap()).unwrap();
        assert!(
            word.pages[0].contains("83.9%")
                && word.pages[0].contains("Yakima")
                && !word.pages[0].contains("top score"),
            "{}",
            word.pages[0]
        );
        let sheet = crate::office::read("xlsx", &std::fs::read(&made[1].path).unwrap()).unwrap();
        assert!(
            sheet.pages[0].starts_with("Sheet: Summary") && sheet.pages[0].contains("Yakima"),
            "{:?}",
            sheet.pages
        );
        assert!(std::fs::read(&made[2].path).unwrap().starts_with(b"%PDF"));
        let slides = crate::office::read("pptx", &std::fs::read(&made[3].path).unwrap()).unwrap();
        assert_eq!(slides.pages.len(), 3, "{:?}", slides.pages);
        assert!(!d.join(".work").exists(), "scratch is removed");
    }

    #[test]
    fn the_deck_takes_the_documents_title_and_both_files_use_the_brand_fonts_and_colors() {
        let home = std::env::var("HOME").unwrap_or_default();
        let tools = Tools::detect(&home);
        if tools.typst.is_none() {
            eprintln!("typst is not installed; skipping PDF check");
            return;
        }
        let doc = "# Late freight: what to do\n\n## Summary\n\nText.\n";
        let deck = "## The answer\n\n- Do this\n";
        let d = dir("themed");
        let (made, notes) = make(
            &tools,
            &d,
            "larkspur-board",
            "Prepared for the board",
            Some(doc),
            Some(deck),
        )
        .unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        let slides = crate::office::read("pptx", &std::fs::read(&made[2].path).unwrap()).unwrap();
        assert!(
            slides.pages[0].contains("Late freight: what to do")
                && !slides.pages[0].contains("larkspur-board"),
            "{:?}",
            slides.pages[0]
        );
        let unzip = |file: &Path, part: &str| {
            String::from_utf8_lossy(
                &std::process::Command::new("/usr/bin/unzip")
                    .arg("-p")
                    .arg(file)
                    .arg(part)
                    .output()
                    .unwrap()
                    .stdout,
            )
            .into_owned()
        };
        let deck_theme = unzip(&made[2].path, "ppt/slides/slide1.xml");
        assert!(
            deck_theme.contains("Helvetica Neue")
                && !deck_theme.contains("Calibri")
                && deck_theme.contains("3347E0"),
            "deck theme"
        );
        let word_theme = unzip(&made[0].path, "word/styles.xml");
        assert!(
            word_theme.contains("Helvetica Neue")
                && !word_theme.contains("Cambria")
                && word_theme.contains("3347E0"),
            "word theme"
        );
    }

    #[test]
    fn a_look_taken_from_a_template_is_used_by_the_word_file_and_the_deck() {
        let root = dir("looked");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("project.json"), "{}").unwrap();
        crate::look::save(
            &root,
            &crate::look::Saved {
                name: "Acme".into(),
                look: Some(crate::office_export::Look {
                    heading_font: Some("Palatino".into()),
                    body_pt: Some(12.0),
                    margins_mm: Some([20, 20, 20, 20]),
                    page_mm: Some([216, 279]),
                    logo_px: Some([8, 4]),
                    ..crate::look::checked("Georgia", "1F6F43").unwrap()
                }),
                from_id: "src-1".into(),
                from_name: "Acme.docx".into(),
                has_logo: true,
                outline: vec![],
                style: String::new(),
            },
            // A real 8 by 4 pixel PNG.
            Some(&[
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0,
                4, 8, 6, 0, 0, 0, 179, 205, 126, 240, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99,
                144, 207, 119, 254, 143, 15, 51, 208, 94, 1, 0, 206, 22, 58, 1, 175, 206, 156, 135,
                0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
            ]),
        )
        .unwrap();
        let out = root.join("exports").join("client-1");
        let tools = Tools::detect(&std::env::var("HOME").unwrap_or_default());
        let (made, notes) = make(
            &tools,
            &out,
            "acme",
            "For the board",
            Some("# Title\n\n## Summary\n\nText.\n"),
            Some("## Slide\n\n- Point\n"),
        )
        .unwrap();
        let unzip = |file: &Path, part: &str| {
            String::from_utf8_lossy(
                &std::process::Command::new("/usr/bin/unzip")
                    .arg("-p")
                    .arg(file)
                    .arg(part)
                    .output()
                    .unwrap()
                    .stdout,
            )
            .into_owned()
        };
        let word = made.iter().find(|m| m.kind == "word").unwrap();
        let styles = unzip(&word.path, "word/styles.xml");
        assert!(
            styles.contains("Georgia") && styles.contains("1F6F43") && !styles.contains("3347E0")
        );
        assert!(styles.contains("Palatino"), "headings use the heading font");
        let body = unzip(&word.path, "word/document.xml");
        assert!(
            body.contains(r#"w:rFonts w:ascii="Palatino""#),
            "heading runs use the heading font"
        );
        assert!(
            body.contains(r#"<w:sz w:val="24" />"#),
            "12 point body text"
        );
        assert!(
            body.contains(r#"w:w="12245""#) && body.contains(r#"w:top="1134""#),
            "page and margins: {body}"
        );
        let list = |file: &Path| {
            String::from_utf8_lossy(
                &std::process::Command::new("/usr/bin/unzip")
                    .arg("-l")
                    .arg(file)
                    .output()
                    .unwrap()
                    .stdout,
            )
            .into_owned()
        };
        let files = list(&word.path);
        assert!(
            files.contains("word/header1.xml") && files.contains("word/media/"),
            "{files}"
        );
        let deck = made.iter().find(|m| m.kind == "deck").unwrap();
        let slide = unzip(&deck.path, "ppt/slides/slide1.xml");
        assert!(
            slide.contains("Palatino") && slide.contains("1F6F43"),
            "the deck uses the look: {slide}"
        );
        assert!(
            list(&deck.path).contains("ppt/media/"),
            "the deck carries the logo"
        );
        if tools.typst.is_some() {
            let pdf = made.iter().find(|m| m.kind == "pdf");
            assert!(
                pdf.is_some(),
                "the PDF is made with the look and logo: {notes:?}"
            );
        }
    }

    #[test]
    fn an_empty_step_result_is_refused_and_nothing_is_made() {
        let out = dir("empty-input");
        for (doc, deck) in [
            (Some("  \n"), Some("# Slide\n\nText")),
            (Some("# Doc\n\nBody"), Some("")),
        ] {
            let e = make(&Tools::default(), &out, "P", "S", doc, deck).unwrap_err();
            assert!(e.contains("empty"), "{e}");
            assert!(!out.exists(), "no folder is left behind");
        }
    }

    #[test]
    fn a_bakery_article_with_our_secret_exports() {
        let out = dir("bakery-secret");
        let doc = "# Harbor Lane Bakery\n\nOur secret: slow-fermented dough, shaped by hand.";
        let made = make(&Tools::default(), &out, "Harbor Lane", "", Some(doc), None);
        assert!(made.is_ok(), "{:?}", made.err());
    }

    #[test]
    fn a_long_hyphenated_name_or_web_address_is_not_a_key_but_a_named_key_still_blocks() {
        let out = dir("long-slug");
        let doc = "# Weekly bread\n\nRead more at https://harborlane.example/2026-spring-weekly-bread-subscription-launch-update today.";
        let made = make(
            &Tools::default(),
            &out,
            "600-word-article-website-harbor-2",
            "",
            Some(doc),
            None,
        );
        assert!(made.is_ok(), "{:?}", made.err());
        // The same long name as a subtitle and in the deck.
        let out = dir("long-slug-deck");
        let made = make(
            &Tools::default(),
            &out,
            "600-word-article-website-harbor-2",
            "600-word-article-website-harbor-2",
            None,
            Some("## 600-word-article-website-harbor-2\n\n- https://harborlane.example/2026-spring-weekly-bread-subscription-launch-update\n"),
        );
        assert!(made.is_ok(), "{:?}", made.err());
        // A named key shape in the name or the text still stops it.
        let key = format!("sk-ant-api03-{}", "abcdefghij".repeat(4));
        for (name, text) in [(key.as_str(), "Fine."), ("Harbor", key.as_str())] {
            let out = dir("long-slug-key");
            let e = make(&Tools::default(), &out, name, "", Some(text), None).unwrap_err();
            assert!(e.contains("looks like it holds a key"), "{e}");
        }
    }

    #[test]
    fn a_seeded_secret_blocks_every_client_format_before_creating_the_folder() {
        let out = dir("secret-block");
        let secret = format!("sk-ant-api03-{}", "abcdefghij".repeat(4));
        for (doc, deck, subtitle) in [
            (Some(secret.as_str()), None, ""),
            (None, Some(secret.as_str()), ""),
            (Some("Safe"), None, secret.as_str()),
        ] {
            let error =
                make(&Tools::default(), &out, "Safe project", subtitle, doc, deck).unwrap_err();
            assert!(
                error.contains("looks like it holds a key")
                    && error.contains("line 1, \"sk-a***\"")
                    && !error.contains(&secret),
                "{error}"
            );
            assert!(!out.exists());
        }
    }

    #[test]
    fn warning_labels_in_quotes_and_lists_and_singular_headings_go_and_code_stays() {
        for md in [
            "# T\n## Source warning\nsrc-1 private.pdf\n",
            "# T\n> **Source warnings**\n> src-1 private.pdf\n\nKept.\n",
            "# T\n- **Source warnings**\n  - src-1 private.pdf\n\nKept.\n",
            "# T\n1. Source warnings:\n   src-1 private.pdf\n\nKept.\n",
        ] {
            let (out, removed) = strip_internal(md);
            assert!(removed && !out.contains("private.pdf"), "{out}");
            assert!(!md.contains("Kept") || out.contains("Kept."), "{out}");
        }
        let md = "# T\n~~~md\n## Source warnings\nThis is code, keep it.\n~~~\n";
        let (out, removed) = strip_internal(md);
        assert!(!removed && out.contains("This is code, keep it."), "{out}");
    }

    #[test]
    fn html_comments_are_left_out_of_client_files_but_not_out_of_code() {
        let md = "# T\nBody <!-- note for the team: src-1 private.pdf --> more.\n<!--\nsecond note\n-->\nEnd.\n```html\n<!-- keep this -->\n```\n";
        let (out, removed) = strip_internal(md);
        assert!(removed);
        assert!(
            !out.contains("private.pdf") && !out.contains("second note"),
            "{out}"
        );
        assert!(out.contains("Body  more.") && out.contains("End."), "{out}");
        assert!(out.contains("<!-- keep this -->"), "{out}");
    }

    #[test]
    fn a_missing_detail_marker_never_reaches_the_word_file_the_pdf_or_the_deck() {
        let doc = "# Offer\n\nThe price is [NEEDED: the price per seat] a month.\n\n- [NEEDED: the start date]\n- Kept item\n";
        let deck = "## The offer\n\n- Price: [NEEDED: the price\nper seat]\n- Kept point\n";
        let (clean, _) = strip_internal(doc);
        assert!(
            !clean.contains("NEEDED") && clean.contains("Kept item"),
            "{clean}"
        );
        let home = std::env::var("HOME").unwrap_or_default();
        let tools = Tools::detect(&home);
        let d = dir("needed");
        let (made, notes) = make(&tools, &d, "Offer", "", Some(doc), Some(deck)).unwrap();
        let kinds: Vec<_> = made.iter().map(|m| m.kind).collect();
        assert!(
            kinds.contains(&"word") && kinds.contains(&"deck"),
            "{kinds:?} {notes:?}"
        );
        for m in &made {
            let bytes = std::fs::read(&m.path).unwrap();
            match m.kind {
                "word" | "deck" => {
                    let ext = if m.kind == "word" { "docx" } else { "pptx" };
                    let text = crate::office::read(ext, &bytes).unwrap().pages.join("\n");
                    assert!(
                        !text.to_ascii_uppercase().contains("NEEDED"),
                        "{}: {text}",
                        m.kind
                    );
                    assert!(text.contains("Kept"), "{}: {text}", m.kind);
                }
                "pdf" => {
                    // A PDF holds its text compressed, so the Typst source is what is checked.
                    assert!(bytes.starts_with(b"%PDF"));
                }
                _ => {}
            }
        }
        let typst_source = crate::typst::markdown_to_typst(&clean);
        assert!(
            !typst_source.to_ascii_uppercase().contains("NEEDED"),
            "{typst_source}"
        );
    }
}
