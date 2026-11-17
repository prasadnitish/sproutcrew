//! Local Office generation. Model content is parsed as data, never evaluated as JavaScript/HTML.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MAX_INPUT_BYTES: usize = 1_000_000;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Block {
    pub kind: String,
    pub level: usize,
    pub indent: usize,
    pub marker: String,
    pub runs: Vec<Span>,
    pub rows: Vec<Vec<Vec<Span>>>,
}

#[derive(Debug, Serialize)]
pub struct Document {
    pub title: String,
    pub subtitle: String,
    pub blocks: Vec<Block>,
    pub warnings: Vec<String>,
    /// The font and colour the files use. The default is SproutCrew's own.
    pub look: Look,
}

/// The look of the client files. `font` is for text and, unless `heading_font` is set, headings; `accent`, six
/// hex digits, is for headings, links and rules. The other fields are `None` for SproutCrew's own choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub font: String,
    pub accent: String,
    #[serde(default)]
    pub heading_font: Option<String>,
    /// Body text size in points.
    #[serde(default)]
    pub body_pt: Option<f32>,
    /// Page margins in millimetres: top, right, bottom, left.
    #[serde(default)]
    pub margins_mm: Option<[u32; 4]>,
    /// Page width and height in millimetres.
    #[serde(default)]
    pub page_mm: Option<[u32; 2]>,
    /// A PNG logo, as base64, with its pixel width and height. Filled in only while files are being made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_px: Option<[u32; 2]>,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            font: FONT.into(),
            accent: BLUE.into(),
            heading_font: None,
            body_pt: None,
            margins_mm: None,
            page_mm: None,
            logo: None,
            logo_px: None,
        }
    }
}

impl Look {
    fn heading(&self) -> &str {
        self.heading_font.as_deref().unwrap_or(&self.font)
    }
    fn half_points(&self) -> usize {
        self.body_pt.map_or(22, |p| (p * 2.0).round() as usize)
    }
    fn logo_png(&self) -> Option<(Vec<u8>, [u32; 2])> {
        use base64::Engine;
        let b = base64::engine::general_purpose::STANDARD
            .decode(self.logo.as_deref()?)
            .ok()?;
        Some((b, self.logo_px?))
    }
}

/// Millimetres in twentieths of a point, the unit Word's page settings use.
fn twips(mm: u32) -> u32 {
    (mm as f64 * 56.692).round() as u32
}

/// XML 1.0 cannot carry control characters even when escaped.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}') {
                c
            } else {
                '\u{fffd}'
            }
        })
        .collect()
}

fn safe_link(url: &str) -> bool {
    !url.chars().any(char::is_control)
        && (url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:"))
}

fn push_text(block: &mut Block, style: &Span, text: &str) {
    let span = Span {
        text: clean(text),
        ..style.clone()
    };
    if let Some(last) = block.runs.last_mut() {
        if (Span {
            text: String::new(),
            ..last.clone()
        }) == (Span {
            text: String::new(),
            ..span.clone()
        }) {
            last.text.push_str(&span.text);
            return;
        }
    }
    block.runs.push(span);
}

/// Mask explicit speaker-note containers while preserving byte offsets for Markdown parsing.
/// An unclosed container remains visible text, so malformed input cannot silently swallow content.
fn speaker_notes(markdown: &str) -> (String, Vec<(usize, String)>) {
    let lines: Vec<_> = markdown.split_inclusive('\n').collect();
    let mut bytes = markdown.as_bytes().to_vec();
    let mut notes = Vec::new();
    let (mut offset, mut index, mut fence) = (0, 0, None::<char>);
    while index < lines.len() {
        let line = lines[index].trim();
        let opens = ['`', '~']
            .into_iter()
            .find(|c| line.starts_with(&c.to_string().repeat(3)));
        if let Some(c) = opens {
            if fence == Some(c) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(c);
            }
        }
        if fence.is_none() && line == "::: notes" {
            if let Some(end) = (index + 1..lines.len()).find(|&i| lines[i].trim() == ":::") {
                let content = lines[index + 1..end].concat();
                let size: usize = lines[index..=end].iter().map(|l| l.len()).sum();
                notes.push((offset, clean(content.trim())));
                for b in &mut bytes[offset..offset + size] {
                    if *b != b'\n' && *b != b'\r' {
                        *b = b' ';
                    }
                }
                offset += size;
                index = end + 1;
                continue;
            }
        }
        offset += lines[index].len();
        index += 1;
    }
    (
        String::from_utf8(bytes).expect("masked ASCII keeps valid UTF-8"),
        notes,
    )
}

pub fn parse(title: &str, subtitle: &str, markdown: &str) -> Result<Document, String> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    if title.len() + subtitle.len() + markdown.len() > MAX_INPUT_BYTES {
        return Err("Office export exceeds the 1 MB text limit; split the document first".into());
    }
    let mut document = Document {
        title: clean(title),
        subtitle: clean(subtitle),
        blocks: Vec::new(),
        warnings: Vec::new(),
        look: Look::default(),
    };
    let mut block = Block::default();
    let mut style = Span::default();
    let mut styles = Vec::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut marker = String::new();
    let mut quote = 0usize;
    let mut table: Option<Block> = None;
    let mut row = Vec::new();
    let mut image_depth = 0usize;
    let (markdown, note_blocks) = speaker_notes(markdown);
    let mut note_blocks = note_blocks.into_iter().peekable();
    let flush = |block: &mut Block, document: &mut Document| {
        if !block.runs.is_empty() {
            document.blocks.push(std::mem::take(block));
        }
    };
    for (event, range) in Parser::new_ext(
        &markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .into_offset_iter()
    {
        while note_blocks.peek().is_some_and(|(at, _)| *at <= range.start) {
            flush(&mut block, &mut document);
            let (_, text) = note_blocks.next().expect("peeked note");
            document.blocks.push(Block {
                kind: "notes".into(),
                runs: vec![Span {
                    text,
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        match event {
            Event::Start(Tag::Paragraph | Tag::HtmlBlock) => {
                flush(&mut block, &mut document);
                block.kind = "paragraph".into();
                block.indent = lists.len() + quote;
                if block.marker.is_empty() {
                    block.marker = std::mem::take(&mut marker);
                }
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::HtmlBlock | TagEnd::Heading(_) | TagEnd::CodeBlock,
            ) => flush(&mut block, &mut document),
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut block, &mut document);
                block.kind = "heading".into();
                block.level = level as usize;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut block, &mut document);
                block.kind = "code".into();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut block, &mut document);
                quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut block, &mut document);
                quote = quote.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => lists.push(start),
            Event::End(TagEnd::List(_)) => {
                flush(&mut block, &mut document);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut block, &mut document);
                marker = match lists.last_mut() {
                    Some(Some(n)) => {
                        let text = format!("{n}.");
                        *n += 1;
                        text
                    }
                    _ => "•".into(),
                };
                block.kind = "paragraph".into();
                block.indent = lists.len() + quote;
                block.marker = std::mem::take(&mut marker);
            }
            Event::End(TagEnd::Item) => flush(&mut block, &mut document),
            Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. }) => {
                styles.push(style.clone());
                match event {
                    Event::Start(Tag::Strong) => style.bold = true,
                    Event::Start(Tag::Emphasis) => style.italic = true,
                    Event::Start(Tag::Strikethrough) => style.strike = true,
                    Event::Start(Tag::Link { dest_url, .. }) => {
                        if safe_link(&dest_url) {
                            style.link = Some(dest_url.to_string());
                        } else {
                            document
                                .warnings
                                .push("An unsafe or relative link was left as plain text".into());
                        }
                    }
                    _ => {}
                }
            }
            Event::End(
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link,
            ) => style = styles.pop().unwrap_or_default(),
            Event::Start(Tag::Image { dest_url, .. }) => {
                image_depth += 1;
                push_text(&mut block, &style, &format!("[Image: {dest_url} — "));
                document.warnings.push("Images remain text references; Office export does not read files or fetch media".into());
            }
            Event::End(TagEnd::Image) => {
                image_depth = image_depth.saturating_sub(1);
                push_text(&mut block, &style, "]");
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut block, &mut document);
                table = Some(Block {
                    kind: "table".into(),
                    ..Default::default()
                });
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => row.clear(),
            Event::Start(Tag::TableCell) => block = Block::default(),
            Event::End(TagEnd::TableCell) => {
                row.push(std::mem::take(&mut block.runs));
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if row.len() > 32 {
                    return Err("Office tables support at most 32 columns".into());
                }
                if let Some(t) = table.as_mut() {
                    t.rows.push(std::mem::take(&mut row));
                    if t.rows.len() > 2000 {
                        return Err("Office table has too many rows".into());
                    }
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    document.blocks.push(t);
                }
                block = Block::default();
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if block.kind.is_empty() {
                    block.kind = "paragraph".into();
                }
                let mut s = style.clone();
                s.code |= block.kind == "code";
                push_text(&mut block, &s, &text);
            }
            Event::Code(text) => {
                let s = Span {
                    code: true,
                    ..style.clone()
                };
                push_text(&mut block, &s, &text);
            }
            Event::SoftBreak => push_text(&mut block, &style, " "),
            Event::HardBreak => push_text(&mut block, &style, "\n"),
            Event::TaskListMarker(done) => {
                push_text(&mut block, &style, if done { "[x] " } else { "[ ] " })
            }
            Event::Rule => {
                flush(&mut block, &mut document);
                document.blocks.push(Block {
                    kind: "rule".into(),
                    ..Default::default()
                });
            }
            _ => {}
        }
        if document.blocks.len() > 2000 || lists.len() + quote > 16 {
            return Err("Office document is too complex; split it into smaller documents".into());
        }
    }
    flush(&mut block, &mut document);
    for (_, text) in note_blocks {
        document.blocks.push(Block {
            kind: "notes".into(),
            runs: vec![Span {
                text,
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    if document.blocks.len() > 2000 {
        return Err("Office document has too many blocks".into());
    }
    document.warnings.sort();
    document.warnings.dedup();
    Ok(document)
}

const FONT: &str = "Helvetica Neue";
const BLUE: &str = "3347E0";

/// Only fixed template expressions carry Typst authority; every content string is quoted data.
pub fn typst_body(document: &Document) -> String {
    fn quoted(text: &str) -> String {
        serde_json::to_string(text).expect("text serialization")
    }
    fn inline(runs: &[Span]) -> String {
        runs.iter()
            .map(|span| {
                let mut text = if span.code {
                    format!("#raw({})", quoted(&span.text))
                } else {
                    format!("#text({})", quoted(&span.text))
                };
                if span.bold {
                    text = format!("#strong[{text}]");
                }
                if span.italic {
                    text = format!("#emph[{text}]");
                }
                if span.strike {
                    text = format!("#strike[{text}]");
                }
                if let Some(url) = &span.link {
                    text = format!("#link({})[{text}]", quoted(url));
                }
                text
            })
            .collect()
    }
    let mut source = String::new();
    for block in &document.blocks {
        let text = inline(&block.runs);
        match block.kind.as_str() {
            "heading" => source.push_str(&format!("#heading(level: {})[{text}]\n\n", block.level)),
            "code" => source.push_str(&format!(
                "#raw({}, block: true)\n\n",
                quoted(
                    &block
                        .runs
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<String>()
                )
            )),
            "rule" => source.push_str("#line(length: 100%, stroke: 0.5pt + gray)\n\n"),
            "table" => {
                let columns = block.rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                source.push_str(&format!(
                    "#table(columns: ({}), inset: 6pt, stroke: 0.5pt + rgb(\"c8ced8\"),\n",
                    "1fr,".repeat(columns)
                ));
                for (i, row) in block.rows.iter().enumerate() {
                    if i == 0 {
                        source.push_str("table.header(");
                    }
                    for col in 0..columns {
                        let cell = inline(row.get(col).map(Vec::as_slice).unwrap_or(&[]));
                        source.push_str(&if i == 0 {
                            format!("[#strong[{cell}]],")
                        } else {
                            format!("[{cell}],")
                        });
                    }
                    if i == 0 {
                        source.push_str("),");
                    }
                    source.push('\n');
                }
                source.push_str(")\n\n");
            }
            "notes" => source.push_str(&format!(
                "#block(inset: 6pt, fill: rgb(\"f7f3ec\"))[#strong[Notes: ]{text}]\n\n"
            )),
            _ => {
                let marker = if block.marker.is_empty() {
                    String::new()
                } else {
                    format!("#text({})", quoted(&format!("{}  ", block.marker)))
                };
                source.push_str(&format!(
                    "#block(inset: (left: {}pt))[{marker}{text}]\n\n",
                    block.indent * 12
                ));
            }
        }
    }
    source
}

fn paragraph(runs: &[Span], size: usize, look: &Look) -> docx_rs::Paragraph {
    use docx_rs::*;
    let mut paragraph = Paragraph::new().line_spacing(LineSpacing::new().line(290).after(120));
    for span in runs {
        let mut run = Run::new().size(size).fonts(
            RunFonts::new()
                .ascii(if span.code { "Menlo" } else { &look.font })
                .hi_ansi(if span.code { "Menlo" } else { &look.font }),
        );
        if span.bold {
            run = run.bold();
        }
        if span.italic {
            run = run.italic();
        }
        if span.strike {
            run = run.strike();
        }
        let lines: Vec<_> = span.text.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                run = run.add_break(BreakType::TextWrapping);
            }
            run = run.add_text(*line);
        }
        paragraph = if let Some(url) = &span.link {
            paragraph.add_hyperlink(
                Hyperlink::new(url, HyperlinkType::External)
                    .add_run(run.color(&look.accent).underline("single")),
            )
        } else {
            paragraph.add_run(run)
        };
    }
    paragraph
}

fn literal(text: &str, size: usize, look: &Look) -> docx_rs::Paragraph {
    paragraph(
        &[Span {
            text: text.into(),
            ..Default::default()
        }],
        size,
        look,
    )
}

/// Render completely in memory, then create a new owner-only file; never replace an existing artifact.
fn save(bytes: &[u8], out: &Path) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Office output exceeds 32 MB".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(out)
        .map_err(|_| "Cannot create Office file; the destination may already exist".to_string())?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        let _ = std::fs::remove_file(out);
        return Err("Could not finish writing Office file".into());
    }
    Ok(())
}

pub fn word(document: &Document, out: &Path) -> Result<(), String> {
    use docx_rs::*;
    let look = &document.look;
    // Headings and the title in the heading font; everything else in the body font.
    let head = Look {
        font: look.heading().to_string(),
        ..look.clone()
    };

    let [pw, ph] = look
        .page_mm
        .map_or([11906, 16838], |[w, h]| [twips(w), twips(h)]);
    let [mt, mr, mb, ml] = look
        .margins_mm
        .map_or([1134; 4], |m| m.map(|x| twips(x) as i32).map(|x| x as u32))
        .map(|x| x as i32);
    let mut doc = Docx::new()
        .page_size(pw, ph)
        .page_margin(PageMargin::new().top(mt).bottom(mb).left(ml).right(mr))
        .default_fonts(RunFonts::new().ascii(&look.font).hi_ansi(&look.font))
        .default_size(look.half_points())
        .add_paragraph(literal(&document.title, 44, &head).style("Title"))
        .add_paragraph(
            literal(&document.subtitle, 24, look)
                .color("666666")
                .line_spacing(LineSpacing::new().after(240)),
        )
        .add_style(
            Style::new("Title", StyleType::Paragraph)
                .name("Title")
                .size(44)
                .bold()
                .color("14161C"),
        );
    if let Some((png, [px_w, px_h])) = look.logo_png() {
        // A logo about a centimetre high at the top of every page.
        let h_emu: u32 = 360_000;
        let w_emu = (h_emu as f64 * px_w as f64 / px_h.max(1) as f64).round() as u32;
        let pic = Pic::new_with_dimensions(png, px_w, px_h).size(w_emu, h_emu);
        doc = doc.header(
            Header::new().add_paragraph(
                Paragraph::new()
                    .align(AlignmentType::Right)
                    .add_run(Run::new().add_image(pic)),
            ),
        );
    }
    for level in 1..=6 {
        doc = doc.add_style(
            Style::new(format!("Heading{level}"), StyleType::Paragraph)
                .name(format!("heading {level}"))
                .fonts(
                    RunFonts::new()
                        .ascii(look.heading())
                        .hi_ansi(look.heading()),
                )
                .size(36usize.saturating_sub(level * 2))
                .color(&look.accent)
                .bold()
                .outline_lvl(level - 1),
        );
    }
    for block in &document.blocks {
        if block.kind == "table" {
            let columns = block
                .rows
                .iter()
                .map(|row| row.len())
                .max()
                .unwrap_or(1)
                .max(1);
            let rows = block
                .rows
                .iter()
                .enumerate()
                .map(|(i, cells)| {
                    TableRow::new(
                        cells
                            .iter()
                            .map(|runs| {
                                let mut runs = runs.clone();
                                if i == 0 {
                                    for r in &mut runs {
                                        r.bold = true;
                                    }
                                }
                                TableCell::new()
                                    .width(9500 / columns, WidthType::Dxa)
                                    .add_paragraph(paragraph(&runs, look.half_points() - 2, look))
                            })
                            .collect(),
                    )
                })
                .collect();
            doc = doc.add_table(
                Table::new(rows)
                    .width(9500, WidthType::Dxa)
                    .set_grid(vec![9500 / columns; columns])
                    .margins(TableCellMargins::new().margin(90, 100, 90, 100)),
            );
        } else if block.kind == "rule" {
            doc = doc.add_paragraph(literal("────────", 20, look).color(&look.accent));
        } else {
            let mut p = paragraph(
                &block.runs,
                if block.kind == "code" {
                    18
                } else if block.kind == "heading" {
                    36usize.saturating_sub(block.level * 2)
                } else {
                    look.half_points()
                },
                if block.kind == "heading" { &head } else { look },
            );
            if !block.marker.is_empty() {
                let mut runs = vec![Span {
                    text: format!("{}  ", block.marker),
                    ..Default::default()
                }];
                runs.extend(block.runs.clone());
                p = paragraph(&runs, look.half_points(), look);
            }
            if block.indent > 0 {
                p = p.indent(Some((block.indent * 360) as i32), None, None, None);
            }
            if block.kind == "heading" {
                p = p
                    .style(&format!("Heading{}", block.level))
                    .keep_next(true)
                    .line_spacing(LineSpacing::new().before(220).after(120));
            }
            doc = doc.add_paragraph(p);
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    doc.pack(&mut bytes)
        .map_err(|_| "Word document generation failed".to_string())?;
    save(bytes.get_ref(), out)
}

const RUNTIME_SETUP: &str = "globalThis.window=globalThis; globalThis.console={log(){},warn(){},error(){}}; globalThis.setImmediate=(fn,...args)=>Promise.resolve().then(()=>fn(...args)); globalThis.setTimeout=(fn,delay,...args)=>setImmediate(fn,...args); globalThis.clearTimeout=()=>{};";

fn bounded_runtime(timeout: std::time::Duration) -> Result<rquickjs::Runtime, String> {
    let runtime = rquickjs::Runtime::new().map_err(|_| "Cannot start PowerPoint renderer")?;
    runtime.set_memory_limit(128 * 1024 * 1024);
    runtime.set_max_stack_size(2 * 1024 * 1024);
    let deadline = std::time::Instant::now() + timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || std::time::Instant::now() > deadline)));
    Ok(runtime)
}

pub fn deck(document: &Document, out: &Path) -> Result<(), String> {
    use base64::Engine;
    use rquickjs::Context;
    let runtime = bounded_runtime(std::time::Duration::from_secs(30))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let context = Context::full(&runtime).map_err(|_| "Cannot create PowerPoint context")?;
    let data = serde_json::to_string(document).map_err(|_| "Cannot prepare PowerPoint content")?;
    let result: Result<String, String> = context.with(|ctx| {
        let fail = |_| "PowerPoint rendering failed or exceeded its resource limit".to_string();
        ctx.eval::<(), _>(RUNTIME_SETUP).map_err(fail)?;
        ctx.eval::<(), _>(include_str!("../export-js/vendor/pptxgen.bundle.js")).map_err(fail)?;
        ctx.eval::<(), _>(include_str!("../export-js/render.js")).map_err(fail)?;
        // Set a data property, never interpolate model text into executable source.
        ctx.globals().set("inputJson", data).map_err(fail)?;
        ctx.eval::<(), _>("renderDeck(JSON.parse(inputJson)).then(x=>globalThis.outputBase64=x,()=>globalThis.renderFailed=true);").map_err(fail)?;
        for _ in 0..200_000 {
            if std::time::Instant::now() > deadline { return Err("PowerPoint rendering exceeded 30 seconds".into()); }
            if ctx.globals().get::<_, Option<bool>>("renderFailed").map_err(fail)?.unwrap_or(false) { return Err("PowerPoint layout or generation failed".into()); }
            if let Some(value) = ctx.globals().get::<_, Option<String>>("outputBase64").map_err(fail)? { return Ok(value); }
            if !ctx.execute_pending_job() { break; }
        }
        Err("PowerPoint renderer did not finish".into())
    });
    let encoded = result?;
    if encoded.len() > 44 * 1024 * 1024 {
        return Err("PowerPoint output exceeds its limit".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid PowerPoint output")?;
    if !bytes.starts_with(b"PK") {
        return Err("PowerPoint output is not an Office archive".into());
    }
    save(&bytes, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dir(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("sprout-office-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn embedded_runtime_has_no_host_io_and_enforces_memory_and_cpu_limits() {
        let runtime = bounded_runtime(std::time::Duration::from_millis(100)).unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| {
            assert!(ctx.eval::<bool, _>("typeof process === 'undefined' && typeof require === 'undefined' && typeof fetch === 'undefined' && typeof XMLHttpRequest === 'undefined'").unwrap());
            assert!(ctx.eval::<(), _>("new Uint8Array(256 * 1024 * 1024)").is_err());
            let _ = ctx.catch();
            assert!(ctx.eval::<(), _>("while (true) {}").is_err());
            let _ = ctx.catch();
        });
    }

    #[test]
    fn document_pdf_uses_structured_tables_and_literal_text_without_typst_authority() {
        let d = parse("T", "", "| Metric | Value |\n|---|---|\n| Revenue | 42 |\n\n3. Third item\n\n[Source](https://example.com)\n\n#read(\"/etc/passwd\")\n").unwrap();
        let source = typst_body(&d);
        assert!(source.contains("#table(") && source.contains("3.  ") && source.contains("#link("));
        assert!(source.contains("#text(\"#read(\\\"/etc/passwd\\\")\")"));
        if let Some((tool, _)) = crate::typst::detect_in(&crate::typst::candidate_paths("")) {
            let p = dir("pdf-structured");
            crate::typst::render(&tool, &source, &p.join("work"), &p.join("document.pdf")).unwrap();
            let pdf = lopdf::Document::load(p.join("document.pdf")).unwrap();
            let text = pdf
                .extract_text(&pdf.get_pages().keys().copied().collect::<Vec<_>>())
                .unwrap();
            assert!(
                text.contains("Revenue") && text.contains("Third item") && text.contains("#read")
            );
            assert!(!text.contains("|---") && !text.contains("root:x:"));
        }
    }

    #[test]
    fn markdown_preserves_structure_formatting_unicode_and_safe_links() {
        let d = parse("Research — 日本語", "Board brief", "## Findings\n\nA **bold** and *italic* [source](https://example.com).\n\n1. First\n2. Second\n   - Nested\n\n| Metric | Value |\n|---|---|\n| Δ | 42 |\n\n```rust\nlet x = 1;\n```\n").unwrap();
        assert!(d.blocks.iter().any(|b| b.kind == "heading" && b.level == 2));
        assert!(d
            .blocks
            .iter()
            .any(|b| b.kind == "table" && b.rows.len() == 2));
        assert!(d.blocks.iter().any(|b| b.marker == "2."));
        assert!(d.blocks.iter().any(|b| b.indent == 2));
        let runs: Vec<_> = d.blocks.iter().flat_map(|b| &b.runs).collect();
        assert!(runs.iter().any(|r| r.bold && r.text == "bold"));
        assert!(runs.iter().any(|r| r.italic && r.text == "italic"));
        assert!(runs
            .iter()
            .any(|r| r.link.as_deref() == Some("https://example.com")));
        assert!(runs.iter().any(|r| r.code && r.text.contains("let x")));
        let wrapped = parse("T", "", "A wrapped\nparagraph.  \nA deliberate break.\n").unwrap();
        assert_eq!(
            wrapped.blocks[0].runs[0].text,
            "A wrapped paragraph.\nA deliberate break."
        );
    }

    #[test]
    fn hostile_markup_is_data_and_external_media_is_not_loaded() {
        let d = parse("Title", "", "<script>globalThis.result='owned'</script>\n\n[bad](javascript:alert%281%29) ![host file](file:///etc/passwd)\n\n```js\nrequire('fs').readFileSync('/etc/passwd')\n```\n").unwrap();
        assert!(d
            .blocks
            .iter()
            .flat_map(|b| &b.runs)
            .all(|r| r.link.is_none()));
        assert!(!d.warnings.is_empty());
        let p = dir("hostile");
        word(&d, &p.join("document.docx")).unwrap();
        deck(&d, &p.join("deck.pptx")).unwrap();
        for (kind, file) in [("docx", "document.docx"), ("pptx", "deck.pptx")] {
            let parsed = crate::office::read(kind, &std::fs::read(p.join(file)).unwrap()).unwrap();
            let text = parsed.pages.join("\n");
            assert!(text.contains("globalThis.result") && text.contains("readFileSync"));
            assert!(!text.contains("root:x:"));
        }
    }

    #[test]
    fn actual_office_files_roundtrip_without_pandoc_and_never_overwrite() {
        let d = parse("Research — 日本語", "Prepared for the board", "## Findings\n\n- **Revenue** grew 12%.\n- Check [the source](https://example.com).\n\n| Region | Value |\n|---|---|\n| West | 42 |\n\n## Decision\n\nApprove the pilot.\n").unwrap();
        let p = dir("roundtrip");
        for (kind, file) in [("docx", "document.docx"), ("pptx", "deck.pptx")] {
            let out = p.join(file);
            if kind == "docx" {
                word(&d, &out).unwrap();
            } else {
                deck(&d, &out).unwrap();
            }
            let bytes = std::fs::read(&out).unwrap();
            let parsed = crate::office::read(kind, &bytes).unwrap();
            let text = parsed.pages.join("\n");
            assert!(
                text.contains("日本語")
                    && text.contains("Revenue")
                    && text.contains("42")
                    && text.contains("Approve"),
                "{text}"
            );
            if kind == "docx" {
                assert!(word(&d, &out).is_err());
            } else {
                assert!(deck(&d, &out).is_err());
            }
            assert_eq!(std::fs::read(&out).unwrap(), bytes);
        }
    }

    #[test]
    fn input_limits_fail_before_output_and_long_decks_continue_without_dropping_text() {
        assert!(parse("T", "", &"x".repeat(MAX_INPUT_BYTES + 1)).is_err());
        let body = (0..100)
            .map(|i| format!("- Item {i}: {}\n", "evidence ".repeat(20)))
            .collect::<String>();
        let d = parse("Long", "", &format!("## Evidence\n\n{body}")).unwrap();
        let p = dir("long");
        deck(&d, &p.join("long.pptx")).unwrap();
        let slides =
            crate::office::read("pptx", &std::fs::read(p.join("long.pptx")).unwrap()).unwrap();
        assert!(slides.pages.len() > 3);
        let text = slides.pages.join("\n");
        for i in 0..100 {
            assert!(text.contains(&format!("Item {i}:")), "missing item {i}");
        }
    }

    #[test]
    fn speaker_notes_stay_out_of_visible_slides_and_tables_remain_editable_tables() {
        let d = parse("T", "", "## Evidence\n\n| Metric | Value |\n|---|---|\n| Revenue | 42 |\n\n::: notes\nPrivate speaker cue.\n:::\n\n## Next\n\nPublic text.\n").unwrap();
        assert!(d.blocks.iter().any(|b| b.kind == "notes"));
        let p = dir("notes");
        deck(&d, &p.join("deck.pptx")).unwrap();
        let bytes = std::fs::read(p.join("deck.pptx")).unwrap();
        let extracted = crate::office::read("pptx", &bytes).unwrap();
        assert!(!extracted.pages.join("\n").contains("Private speaker cue"));
        let xml = std::process::Command::new("/usr/bin/unzip")
            .args(["-p"])
            .arg(p.join("deck.pptx"))
            .arg("ppt/slides/slide2.xml")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&xml.stdout).contains("<a:tbl>"));
        assert!(String::from_utf8_lossy(&xml.stdout).contains("firstRow=\"1\""));
        assert!(String::from_utf8_lossy(&xml.stdout).contains("type=\"title\""));
        let notes = std::process::Command::new("/usr/bin/unzip")
            .args(["-p"])
            .arg(p.join("deck.pptx"))
            .arg("ppt/notesSlides/notesSlide2.xml")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&notes.stdout).contains("Private speaker cue"));
    }
}
