//! An Excel workbook of the tables in a finished document: one sheet per table, named after the nearest heading
//! before it, with the header row in bold. Values only: a cell is written as a number only when its whole text is
//! a plain number, and every other cell, including one that starts with `=`, is text, so nothing in a workbook is
//! ever a formula. The file is a zip with stored (uncompressed) parts, written here, so no zip library is needed.

use crate::office_export::{Block, Document};
use std::path::Path;

/// A table's sheet name and its cells, row by row.
pub type Table = (String, Vec<Vec<String>>);

fn text_of(spans: &[crate::office_export::Span]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Excel's rules for a sheet name: at most 31 characters, none of `[]:*?/\`, not empty, and unique in the book.
fn sheet_name(want: &str, taken: &[String]) -> String {
    let clean: String = want
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') && !c.is_control())
        .collect();
    let clean = clean.trim().trim_matches('\'').to_string();
    let base = if clean.is_empty() {
        "Table".to_string()
    } else {
        clean
    };
    let fit = |s: &str, room: usize| s.chars().take(room).collect::<String>();
    let mut name = fit(&base, 31);
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        let suffix = format!(" ({n})");
        name = format!("{}{suffix}", fit(&base, 31 - suffix.chars().count()));
        n += 1;
    }
    name
}

/// The document's tables, each named after the heading above it.
pub fn tables(document: &Document) -> Vec<Table> {
    let mut out: Vec<Table> = Vec::new();
    let mut heading = String::new();
    for b in &document.blocks {
        let Block {
            kind, runs, rows, ..
        } = b;
        match kind.as_str() {
            "heading" => heading = text_of(runs),
            "table" if !rows.is_empty() => {
                let taken: Vec<String> = out.iter().map(|t| t.0.clone()).collect();
                let name = sheet_name(&heading, &taken);
                let cells = rows
                    .iter()
                    .map(|r| r.iter().map(|c| text_of(c)).collect())
                    .collect();
                out.push((name, cells));
            }
            _ => {}
        }
    }
    out
}

/// A cell's text as a number, when the whole text is one: an optional minus, digits, and an optional decimal part.
/// Thousands separators, units, percent and currency signs keep a cell as text, so its meaning is never changed.
fn plain_number(s: &str) -> Option<&str> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let (whole, frac) = match digits.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (digits, None),
    };
    let ok = !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
        && !(whole.len() > 1 && whole.starts_with('0'))
        && digits.len() <= 15;
    ok.then_some(s)
}

fn xml(s: &str) -> String {
    s.chars()
        .filter(|c| matches!(c, '\t' | '\n' | '\r') || *c >= ' ')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn column(mut n: usize) -> String {
    let mut s = String::new();
    n += 1;
    while n > 0 {
        let r = (n - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn sheet_xml(rows: &[Vec<String>]) -> String {
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for (r, row) in rows.iter().enumerate() {
        x.push_str(&format!("<row r=\"{}\">", r + 1));
        let style = if r == 0 { " s=\"1\"" } else { "" };
        for (c, cell) in row.iter().enumerate() {
            let at = format!("{}{}", column(c), r + 1);
            match plain_number(cell).filter(|_| r > 0) {
                Some(n) => x.push_str(&format!("<c r=\"{at}\"{style}><v>{n}</v></c>")),
                None => x.push_str(&format!(
                    "<c r=\"{at}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                    xml(cell)
                )),
            }
        }
        x.push_str("</row>");
    }
    x.push_str("</sheetData></worksheet>");
    x
}

/// A zip of `files`, stored without compression.
fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let crc = crc.sum();
        let offset = out.len() as u32;
        let size = data.len() as u32;
        let name_len = name.len() as u16;
        // Local file header: version 20, no flags, stored, no time, then the sizes and the name.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&[0; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let at = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// The workbook's bytes for `tables`. `None` when there are no tables.
pub fn workbook(tables: &[Table]) -> Option<Vec<u8>> {
    if tables.is_empty() {
        return None;
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let sheets: String = (1..=tables.len())
        .map(|i| format!(r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#))
        .collect();
    files.push(("[Content_Types].xml".into(), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>{sheets}</Types>"#).into_bytes()));
    files.push(("_rels/.rels".into(), br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec()));
    let book_sheets: String = tables
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            format!(
                r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#,
                xml(name),
                i + 1,
                i + 1
            )
        })
        .collect();
    files.push(("xl/workbook.xml".into(), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{book_sheets}</sheets></workbook>"#).into_bytes()));
    let rels: String = (1..=tables.len())
        .map(|i| format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#))
        .collect();
    let styles_id = tables.len() + 1;
    files.push(("xl/_rels/workbook.xml.rels".into(), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}<Relationship Id="rId{styles_id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#).into_bytes()));
    files.push(("xl/styles.xml".into(), br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="2"><xf fontId="0"/><xf fontId="1" applyFont="1"/></cellXfs></styleSheet>"#.to_vec()));
    for (i, (_, rows)) in tables.iter().enumerate() {
        files.push((
            format!("xl/worksheets/sheet{}.xml", i + 1),
            sheet_xml(rows).into_bytes(),
        ));
    }
    Some(zip(&files))
}

/// Writes the document's tables as a workbook at `out`. Returns how many sheets it has; with no tables, writes
/// nothing and returns 0.
pub fn sheet(document: &Document, out: &Path) -> Result<usize, String> {
    let t = tables(document);
    match workbook(&t) {
        Some(bytes) => {
            crate::workspace::write_atomic(out, &bytes)?;
            Ok(t.len())
        }
        None => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "## Costs by region\n\n| Region | Cost | Note |\n|---|---|---|\n| North | 1200 | =SUM(A1) |\n| South | -3.5 | 4,200 |\n| East | 007 | 12% |\n\n## Costs by region\n\n| A | B |\n|---|---|\n| x | 1 |\n";

    #[test]
    fn tables_round_trip_as_values_with_no_formula_and_numbers_only_when_plain() {
        let doc = crate::office_export::parse("Report", "", MD).unwrap();
        let t = tables(&doc);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].0, "Costs by region");
        assert_eq!(t[1].0, "Costs by region (2)", "sheet names stay unique");
        let bytes = workbook(&t).unwrap();
        let back = crate::office::read("xlsx", &bytes)
            .expect("Excel's format, read back by the app's own reader");
        let first = &back.pages[0];
        assert!(first.starts_with("Sheet: Costs by region"), "{first}");
        for v in ["North", "1200", "=SUM(A1)", "-3.5", "4,200", "007", "12%"] {
            assert!(first.contains(v), "{v} in {first}");
        }
        let x = sheet_xml(&t[0].1);
        assert!(x.contains("<v>1200</v>") && x.contains("<v>-3.5</v>"));
        assert!(!x.contains("<f>"), "no formulas");
        assert!(x.contains("=SUM(A1)</t>"), "a leading = is text");
        assert!(x.contains("4,200</t>") && x.contains("007</t>") && x.contains("12%</t>"));
        assert!(
            x.contains(r#"<c r="A1" s="1" t="inlineStr">"#),
            "the header row is bold"
        );
    }

    #[test]
    fn a_document_with_no_table_makes_no_workbook_and_sheet_names_follow_excels_rules() {
        let doc = crate::office_export::parse("Report", "", "## Plain\n\nNo table here.").unwrap();
        assert!(workbook(&tables(&doc)).is_none());
        assert_eq!(sheet_name("A/B: [x]?", &[]), "AB x");
        assert_eq!(sheet_name("", &[]), "Table");
        assert_eq!(sheet_name(&"y".repeat(40), &[]).chars().count(), 31);
        assert_eq!(column(0), "A");
        assert_eq!(column(26), "AA");
    }
}
