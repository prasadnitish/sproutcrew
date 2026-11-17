//! Text from the Office formats before 2007: `doc`, `xls` and `ppt`.
//!
//! * `doc` is converted by macOS's `textutil`, which is a system program, run as a child with a time limit.
//! * `xls` is read by the `calamine` crate. It gives one page per visible sheet, written as CSV like `xlsx`.
//! * `ppt` is a compound file whose "PowerPoint Document" stream is a list of records. The text of each slide
//!   is read from them in the order of the deck, using the file's own table of where each slide starts. When
//!   that table is damaged the slides are taken in the order they are stored, and the notes say so.
//!
//! Like the newer formats, speaker notes and comments are not read, and the notes say so.

use crate::office::Extracted;
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;

const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// A sheet's text is cut here, like a sheet in a newer workbook.
const MAX_SHEET_CHARS: usize = 400_000;

pub fn read(ext: &str, bytes: &[u8]) -> Result<Extracted, String> {
    match ext {
        "doc" => read_doc(bytes),
        "xls" => {
            check_ole(bytes, "Excel 97-2003 workbook")?;
            read_xls(bytes)
        }
        "ppt" => {
            check_ole(bytes, "PowerPoint 97-2003 presentation")?;
            read_ppt(bytes)
        }
        other => Err(format!(".{other} is not an old Office type")),
    }
}

fn check_ole(bytes: &[u8], what: &str) -> Result<(), String> {
    if bytes.starts_with(&OLE_MAGIC) {
        Ok(())
    } else {
        Err(format!(
            "this is not an {what}. If it opens in Excel or PowerPoint, save it again as .xlsx or .pptx and add that file"
        ))
    }
}

// ---- doc ------------------------------------------------------------------------------------------------

fn read_doc(bytes: &[u8]) -> Result<Extracted, String> {
    // A `.doc` is usually a Word 97-2003 file, but some programs save rich text or a renamed `.docx` under
    // that name, and macOS reads those too. Anything else would only be turned into garbled text, which
    // is what macOS does with a damaged compound file, so that one must name a Word document.
    let word = if bytes.starts_with(&OLE_MAGIC) {
        // The names of streams are stored as UTF-16. Some writers, macOS's among them, make files that a
        // strict compound-file reader refuses, so the directory is searched for the name instead.
        let name: Vec<u8> = "WordDocument"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.windows(name.len()).any(|w| w == name.as_slice())
    } else {
        bytes.starts_with(b"{\\rtf") || bytes.starts_with(b"PK\x03\x04")
    };
    if !word {
        return Err("this is not a Word file. If it opens in Word, save it again as .docx and add that file".into());
    }
    let textutil = Path::new("/usr/bin/textutil");
    if !textutil.exists() {
        return Err("old .doc files are read with macOS's textutil, which is missing here. Save the file as .docx and add that".into());
    }
    let file = crate::helper::TempFile::create("doc", bytes)?;
    let out = crate::helper::run(
        textutil,
        &[
            "-convert".as_ref(),
            "txt".as_ref(),
            "-encoding".as_ref(),
            "UTF-8".as_ref(),
            "-stdout".as_ref(),
            file.path().as_os_str(),
        ],
        Duration::from_secs(60),
        32 * 1024 * 1024,
    )?;
    if out.timed_out {
        return Err("converting the .doc file took too long and was stopped".into());
    }
    if !out.success {
        return Err("macOS could not read this as a Word file. Open it in Word and save it as .docx, then add that file".into());
    }
    let text = String::from_utf8_lossy(&out.stdout).replace(['\u{c}', '\r'], "\n");
    let mut notes = vec![
        "This old Word file was converted by macOS. Tables and layout are flattened, and comments, headers and footnotes may be missing".to_string(),
    ];
    if out.too_long {
        notes.push("The text was cut because the file is very long".into());
    }
    Ok(Extracted {
        pages: vec![text],
        notes,
    })
}

// ---- xls ------------------------------------------------------------------------------------------------

fn csv_cell(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

fn number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

fn read_xls(bytes: &[u8]) -> Result<Extracted, String> {
    use calamine::{Data, Reader, SheetVisible, Xls};
    let mut book = Xls::new(Cursor::new(bytes))
        .map_err(|e| format!("this is not an Excel 97-2003 workbook that BFF can read ({e})"))?;
    let (mut pages, mut hidden_sheets, mut cut) = (Vec::new(), 0usize, Vec::new());
    for sheet in book.sheets_metadata().to_vec() {
        if sheet.visible != SheetVisible::Visible {
            hidden_sheets += 1;
            continue;
        }
        let range = book
            .worksheet_range(&sheet.name)
            .map_err(|e| format!("the sheet {} could not be read ({e})", sheet.name))?;
        let first_column = range.start().map(|(_, c)| c as usize).unwrap_or(0);
        let mut csv = String::new();
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .map(|cell| match cell {
                    Data::Empty => String::new(),
                    Data::Int(i) => i.to_string(),
                    Data::Float(f) => number(*f),
                    Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => csv_cell(s),
                    Data::Bool(b) => b.to_string(),
                    Data::DateTime(d) => number(d.as_f64()),
                    Data::Error(e) => e.to_string(),
                })
                .collect();
            csv.push_str(&",".repeat(first_column));
            csv.push_str(&cells.join(","));
            csv.push('\n');
            if csv.len() > MAX_SHEET_CHARS {
                cut.push(sheet.name.clone());
                break;
            }
        }
        pages.push(format!("Sheet: {}\n{csv}", sheet.name));
    }
    if pages.is_empty() && hidden_sheets == 0 {
        return Err("this workbook has no sheets".into());
    }
    let mut notes = Vec::new();
    if hidden_sheets > 0 {
        notes.push(if hidden_sheets == 1 {
            "1 hidden sheet was left out".to_string()
        } else {
            format!("{hidden_sheets} hidden sheets were left out")
        });
    }
    for name in cut {
        notes.push(format!(
            "The sheet {name} is very long and was cut after {MAX_SHEET_CHARS} characters"
        ));
    }
    notes.push(
        "Hidden rows and columns in an old .xls file cannot be told apart, so they are included"
            .into(),
    );
    notes.push("Cells show their stored values: formulas show their last saved result and a date shows as a serial number".into());
    Ok(Extracted { pages, notes })
}

// ---- ppt ------------------------------------------------------------------------------------------------

const REC_DOCUMENT: u16 = 0x03E8;
const REC_SLIDE: u16 = 0x03EE;
const REC_SLIDE_PERSIST: u16 = 0x03F3;
const REC_SLIDE_LIST: u16 = 0x0FF0;
const REC_TEXT_CHARS: u16 = 0x0FA0;
const REC_TEXT_BYTES: u16 = 0x0FA8;
const REC_USER_EDIT: u16 = 0x0FF5;
const REC_PERSIST_DIR: u16 = 0x1772;
/// How deep containers may nest, how many edits are followed, and how much text one file may give.
const MAX_DEPTH: usize = 16;
const MAX_EDITS: usize = 1000;
const MAX_TEXT: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy)]
struct Rec {
    container: bool,
    instance: u16,
    kind: u16,
    start: usize,
    end: usize,
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn rec_at(d: &[u8], at: usize) -> Option<Rec> {
    let head = d.get(at..at.checked_add(8)?)?;
    let word = u16::from_le_bytes([head[0], head[1]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let start = at + 8;
    let end = start.checked_add(len)?;
    (end <= d.len()).then_some(Rec {
        container: word & 0xF == 0xF,
        instance: word >> 4,
        kind: u16::from_le_bytes([head[2], head[3]]),
        start,
        end,
    })
}

/// The records one after another between `from` and `to`. A broken record ends the list.
fn children(d: &[u8], from: usize, to: usize) -> Vec<Rec> {
    let (mut out, mut at) = (Vec::new(), from);
    while at + 8 <= to {
        let Some(r) = rec_at(d, at) else { break };
        if r.end > to {
            break;
        }
        at = r.end;
        out.push(r);
    }
    out
}

/// The text of a text record: paragraph breaks become new lines and other control characters go.
fn record_text(d: &[u8], r: Rec) -> Option<String> {
    let raw = &d[r.start..r.end];
    let text: String = match r.kind {
        REC_TEXT_CHARS => char::decode_utf16(
            raw.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])),
        )
        .map(|c| c.unwrap_or('\u{fffd}'))
        .collect(),
        REC_TEXT_BYTES => raw.iter().map(|&b| b as char).collect(),
        _ => return None,
    };
    Some(
        text.chars()
            .filter_map(|c| match c {
                '\r' | '\u{b}' => Some('\n'),
                '\n' | '\t' => Some(c),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect(),
    )
}

/// Every piece of text under a record, in the order stored.
fn texts_in(d: &[u8], r: Rec, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH {
        return;
    }
    for child in children(d, r.start, r.end) {
        if child.container {
            texts_in(d, child, depth + 1, out);
        } else if let Some(t) = record_text(d, child) {
            out.push(t);
        }
    }
}

/// Where each persisted object starts, from the newest edit back to the first. A newer edit wins.
fn persist_offsets(
    doc: &[u8],
    current_user: Option<&[u8]>,
) -> Option<(std::collections::HashMap<u32, usize>, u32)> {
    let mut at = u32_at(current_user?, 16)? as usize;
    let (mut map, mut doc_id) = (std::collections::HashMap::new(), None);
    for _ in 0..MAX_EDITS {
        let edit = rec_at(doc, at).filter(|r| r.kind == REC_USER_EDIT)?;
        let last_edit = u32_at(doc, edit.start + 8)? as usize;
        let directory = u32_at(doc, edit.start + 12)? as usize;
        doc_id.get_or_insert(u32_at(doc, edit.start + 16)?);
        let dir = rec_at(doc, directory).filter(|r| r.kind == REC_PERSIST_DIR)?;
        let mut p = dir.start;
        while p + 4 <= dir.end {
            let head = u32_at(doc, p)?;
            let (first, count) = (head & 0xFFFFF, (head >> 20) as usize);
            p += 4;
            for i in 0..count {
                let off = u32_at(doc, p + 4 * i)? as usize;
                map.entry(first + i as u32).or_insert(off);
            }
            p += 4 * count;
        }
        if last_edit == 0 {
            return Some((map, doc_id?));
        }
        at = last_edit;
    }
    None
}

/// The slides in deck order, each as the text of its placeholders followed by the text in its drawing.
fn slides_in_deck_order(doc: &[u8], current_user: Option<&[u8]>) -> Option<Vec<Vec<String>>> {
    let (offsets, doc_id) = persist_offsets(doc, current_user)?;
    let document = rec_at(doc, *offsets.get(&doc_id)?).filter(|r| r.kind == REC_DOCUMENT)?;
    let list = children(doc, document.start, document.end)
        .into_iter()
        .find(|r| r.kind == REC_SLIDE_LIST && r.instance == 0)?;
    let mut slides: Vec<(u32, Vec<String>)> = Vec::new();
    for r in children(doc, list.start, list.end) {
        if r.kind == REC_SLIDE_PERSIST {
            slides.push((u32_at(doc, r.start)?, Vec::new()));
        } else if let Some(t) = record_text(doc, r) {
            slides.last_mut()?.1.push(t);
        }
    }
    let mut out = Vec::new();
    for (id, mut texts) in slides {
        let slide = rec_at(doc, *offsets.get(&id)?).filter(|r| r.kind == REC_SLIDE)?;
        texts_in(doc, slide, 0, &mut texts);
        out.push(texts);
    }
    (!out.is_empty()).then_some(out)
}

/// Slides in the order they are stored, for a file whose table of slides cannot be followed.
fn slides_in_file_order(doc: &[u8]) -> Vec<Vec<String>> {
    children(doc, 0, doc.len())
        .into_iter()
        .filter(|r| r.container && r.kind == REC_SLIDE)
        .map(|slide| {
            let mut texts = Vec::new();
            texts_in(doc, slide, 0, &mut texts);
            texts
        })
        .collect()
}

fn stream(file: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    file.open_stream(name)
        .ok()?
        .take(64 * 1024 * 1024)
        .read_to_end(&mut data)
        .ok()?;
    Some(data)
}

fn read_ppt(bytes: &[u8]) -> Result<Extracted, String> {
    let mut file = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|_| {
        "this is not a PowerPoint 97-2003 presentation that BFF can read".to_string()
    })?;
    let doc = stream(&mut file, "/PowerPoint Document")
        .ok_or("this is not a PowerPoint presentation (it has no PowerPoint Document)")?;
    let current_user = stream(&mut file, "/Current User");
    let mut notes = Vec::new();
    let slides = match slides_in_deck_order(&doc, current_user.as_deref()) {
        Some(s) => s,
        None => {
            notes.push("The table that gives the order of the slides could not be read, so the slides are in the order they are stored in the file".into());
            slides_in_file_order(&doc)
        }
    };
    if slides.is_empty() {
        return Err("this presentation has no slides".into());
    }
    let mut budget = MAX_TEXT;
    let pages: Vec<String> = slides
        .into_iter()
        .map(|texts| {
            let mut page = texts.join("\n");
            page.truncate(page.floor_char_boundary(budget));
            budget -= page.len();
            page
        })
        .collect();
    notes.push("Speaker notes and comments were not read, and slides that are hidden in the presentation were kept".into());
    Ok(Extracted { pages, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ---- doc

    fn doc_from_text(text: &str) -> Vec<u8> {
        let dir = crate::helper::private_dir().unwrap();
        let src = dir.join(format!("legacy-test-{}.txt", std::process::id()));
        let out = dir.join(format!("legacy-test-{}.doc", std::process::id()));
        std::fs::write(&src, text).unwrap();
        let status = std::process::Command::new("/usr/bin/textutil")
            .args(["-convert", "doc", "-output"])
            .arg(&out)
            .arg(&src)
            .status()
            .unwrap();
        assert!(status.success());
        let bytes = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
        bytes
    }

    #[test]
    fn an_old_word_file_gives_its_text() {
        let bytes = doc_from_text("Candidate instructions\n\nWrite at most 1,000 words. Café.");
        assert!(bytes.starts_with(&OLE_MAGIC), "the fixture is a real .doc");
        let x = read("doc", &bytes).unwrap();
        assert_eq!(x.pages.len(), 1);
        assert!(
            x.pages[0].contains("Candidate instructions"),
            "{:?}",
            x.pages
        );
        assert!(
            x.pages[0].contains("at most 1,000 words. Café."),
            "{:?}",
            x.pages
        );
        assert!(x.notes.iter().any(|n| n.contains("converted by macOS")));
    }

    #[test]
    fn a_file_that_is_not_a_word_file_is_refused_in_words() {
        let e = read("doc", b"\x00\x01 not a document at all \xff\xfe").unwrap_err();
        assert!(e.contains("not a Word file"), "{e}");
        // Looks like a Word file but is damaged: macOS would turn the bytes into garbled text.
        let mut damaged = OLE_MAGIC.to_vec();
        damaged.extend([7u8; 600]);
        let e = read("doc", &damaged).unwrap_err();
        assert!(e.contains("not a Word file"), "{e}");
        let e = read("doc", &compound(&[("Workbook", vec![1, 2, 3])])).unwrap_err();
        assert!(e.contains("not a Word file"), "a workbook named .doc: {e}");
        // Rich text saved as .doc is read.
        let rtf = read("doc", b"{\\rtf1\\ansi Hello from rich text}").unwrap();
        assert!(
            rtf.pages[0].contains("Hello from rich text"),
            "{:?}",
            rtf.pages
        );
    }

    // ---- xls

    fn record(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
        out.extend(kind.to_le_bytes());
        out.extend((data.len() as u16).to_le_bytes());
        out.extend(data);
    }

    fn bof(out: &mut Vec<u8>, kind: u16) {
        let mut d = Vec::new();
        d.extend(0x0600u16.to_le_bytes());
        d.extend(kind.to_le_bytes());
        d.extend(0x0DBBu16.to_le_bytes());
        d.extend(0x07CCu16.to_le_bytes());
        d.extend(0u32.to_le_bytes());
        d.extend(6u32.to_le_bytes());
        record(out, 0x0809, &d);
    }

    fn label(out: &mut Vec<u8>, row: u16, col: u16, text: &str) {
        let mut d = Vec::new();
        d.extend(row.to_le_bytes());
        d.extend(col.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend((text.len() as u16).to_le_bytes());
        d.push(0);
        d.extend(text.as_bytes());
        record(out, 0x0204, &d);
    }

    fn num(out: &mut Vec<u8>, row: u16, col: u16, v: f64) {
        let mut d = Vec::new();
        d.extend(row.to_le_bytes());
        d.extend(col.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend(v.to_le_bytes());
        record(out, 0x0203, &d);
    }

    fn sheet_stream(cells: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
        let mut s = Vec::new();
        bof(&mut s, 0x0010);
        let mut dim = Vec::new();
        dim.extend(0u32.to_le_bytes());
        dim.extend(10u32.to_le_bytes());
        dim.extend(0u16.to_le_bytes());
        dim.extend(10u16.to_le_bytes());
        dim.extend(0u16.to_le_bytes());
        record(&mut s, 0x0200, &dim);
        cells(&mut s);
        record(&mut s, 0x000A, &[]);
        s
    }

    /// A workbook with these sheets: name, hidden, and the cells of each.
    type Cells = Box<dyn Fn(&mut Vec<u8>)>;

    fn workbook(sheets: &[(&str, bool, Cells)]) -> Vec<u8> {
        let bodies: Vec<Vec<u8>> = sheets.iter().map(|(_, _, c)| sheet_stream(c)).collect();
        let globals = |offsets: &[u32]| {
            let mut g = Vec::new();
            bof(&mut g, 0x0005);
            record(&mut g, 0x0042, &1200u16.to_le_bytes());
            for ((name, hidden, _), off) in sheets.iter().zip(offsets) {
                let mut d = Vec::new();
                d.extend(off.to_le_bytes());
                d.push(u8::from(*hidden));
                d.push(0);
                d.push(name.len() as u8);
                d.push(0);
                d.extend(name.as_bytes());
                record(&mut g, 0x0085, &d);
            }
            record(&mut g, 0x000A, &[]);
            g
        };
        let size = globals(&vec![0; sheets.len()]).len() as u32;
        let mut offsets = Vec::new();
        let mut at = size;
        for b in &bodies {
            offsets.push(at);
            at += b.len() as u32;
        }
        let mut stream = globals(&offsets);
        for b in bodies {
            stream.extend(b);
        }
        compound(&[("Workbook", stream)])
    }

    fn compound(streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut f = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        for (name, data) in streams {
            f.create_stream(format!("/{name}"))
                .unwrap()
                .write_all(data)
                .unwrap();
        }
        f.flush().unwrap();
        f.into_inner().into_inner()
    }

    #[test]
    fn an_old_workbook_gives_a_page_per_visible_sheet_and_leaves_hidden_ones_out() {
        let bytes = workbook(&[
            (
                "Budget",
                false,
                Box::new(|s| {
                    label(s, 0, 0, "Item");
                    label(s, 0, 1, "Cost, in USD");
                    label(s, 1, 0, "Licences");
                    num(s, 1, 1, 90.0);
                    num(s, 2, 1, 12.5);
                }),
            ),
            ("Secret", true, Box::new(|s| label(s, 0, 0, "do not show"))),
        ]);
        let x = read("xls", &bytes).unwrap();
        assert_eq!(x.pages.len(), 1, "{:?}", x.pages);
        assert_eq!(
            x.pages[0],
            "Sheet: Budget\nItem,\"Cost, in USD\"\nLicences,90\n,12.5\n"
        );
        assert!(x.notes.contains(&"1 hidden sheet was left out".to_string()));
        assert!(!x.pages.iter().any(|p| p.contains("do not show")));
    }

    #[test]
    fn something_that_is_not_an_old_office_file_is_refused() {
        for ext in ["xls", "ppt"] {
            let e = read(ext, b"PK\x03\x04 a new format file").unwrap_err();
            assert!(e.contains("save it again as"), "{e}");
        }
        let e = read("xls", &compound(&[("Other", vec![1, 2, 3])])).unwrap_err();
        assert!(e.contains("Excel 97-2003"), "{e}");
    }

    // ---- ppt

    fn rec(container: bool, instance: u16, kind: u16, payload: &[u8]) -> Vec<u8> {
        let word = (instance << 4) | if container { 0xF } else { 0 };
        let mut out = Vec::new();
        out.extend(word.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend(payload);
        out
    }

    fn chars(text: &str) -> Vec<u8> {
        let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        rec(false, 0, REC_TEXT_CHARS, &utf16)
    }

    fn latin(text: &str) -> Vec<u8> {
        rec(false, 0, REC_TEXT_BYTES, text.as_bytes())
    }

    fn persist_atom(id: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend(id.to_le_bytes());
        p.extend([0u8; 16]);
        rec(false, 0, REC_SLIDE_PERSIST, &p)
    }

    /// A slide whose drawing holds a text box.
    fn slide(text: &str) -> Vec<u8> {
        let textbox = rec(true, 0, 0x0F00, &chars(text));
        let drawing = rec(true, 0, 0x040C, &textbox);
        let mut body = rec(false, 2, 0x03EF, &[0u8; 24]);
        body.extend(drawing);
        rec(true, 0, REC_SLIDE, &body)
    }

    /// A deck of two slides stored A then B, listed in deck order B then A. Persist ids: document 1, A 2, B 3.
    fn deck(order_is_readable: bool) -> Vec<u8> {
        let (a, b) = (
            slide("Drawn text on slide A"),
            slide("Drawn text on slide B"),
        );
        let mut list = persist_atom(3);
        list.extend(rec(false, 0, 0x0F9F, &[0, 0, 0, 0]));
        list.extend(chars("Title of B\rBullet of B"));
        list.extend(persist_atom(2));
        list.extend(latin("Title of A"));
        let document = rec(
            true,
            0,
            REC_DOCUMENT,
            &[
                rec(true, 0, REC_SLIDE_LIST, &list),
                rec(false, 0, 0x03E9, &[0u8; 8]),
            ]
            .concat(),
        );
        let mut doc = Vec::new();
        let off_a = 0u32;
        doc.extend(&a);
        let off_b = doc.len() as u32;
        doc.extend(&b);
        let off_doc = doc.len() as u32;
        doc.extend(&document);
        let off_dir = doc.len() as u32;
        let mut dir = Vec::new();
        dir.extend((1u32 | (3 << 20)).to_le_bytes());
        for off in [off_doc, off_a, off_b] {
            dir.extend(off.to_le_bytes());
        }
        doc.extend(rec(false, 0, REC_PERSIST_DIR, &dir));
        let off_edit = doc.len() as u32;
        let mut edit = Vec::new();
        edit.extend(0u32.to_le_bytes());
        edit.extend([0u8; 4]);
        edit.extend(0u32.to_le_bytes());
        edit.extend(
            (if order_is_readable {
                off_dir
            } else {
                0xFFFF_0000
            })
            .to_le_bytes(),
        );
        edit.extend(1u32.to_le_bytes());
        doc.extend(rec(false, 0, REC_USER_EDIT, &edit));
        let mut user = vec![0u8; 16];
        user.extend(off_edit.to_le_bytes());
        compound(&[("PowerPoint Document", doc), ("Current User", user)])
    }

    #[test]
    fn an_old_deck_gives_its_slides_in_deck_order_with_placeholder_and_drawn_text() {
        let x = read("ppt", &deck(true)).unwrap();
        assert_eq!(
            x.pages,
            vec![
                "Title of B\nBullet of B\nDrawn text on slide B",
                "Title of A\nDrawn text on slide A"
            ]
        );
        assert!(x.notes.iter().any(|n| n.contains("Speaker notes")));
        assert!(!x.notes.iter().any(|n| n.contains("order of the slides")));
    }

    #[test]
    fn a_deck_whose_slide_table_is_damaged_is_read_in_stored_order_and_says_so() {
        let x = read("ppt", &deck(false)).unwrap();
        assert_eq!(
            x.pages,
            vec!["Drawn text on slide A", "Drawn text on slide B"]
        );
        assert!(x.notes.iter().any(|n| n.contains("order of the slides")));
    }

    #[test]
    fn a_deck_with_cut_off_records_does_not_panic() {
        let full = deck(true);
        let mut f = cfb::CompoundFile::open(Cursor::new(&full[..])).unwrap();
        let doc = stream(&mut f, "/PowerPoint Document").unwrap();
        let user = stream(&mut f, "/Current User").unwrap();
        for cut in (0..doc.len()).step_by(7) {
            let bytes = compound(&[
                ("PowerPoint Document", doc[..cut].to_vec()),
                ("Current User", user.clone()),
            ]);
            let _ = read("ppt", &bytes);
        }
    }

    #[test]
    fn a_presentation_without_slides_is_an_error() {
        let bytes = compound(&[("PowerPoint Document", vec![])]);
        assert!(read("ppt", &bytes).is_err());
    }
}
