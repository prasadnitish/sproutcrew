//! Building a Keynote deck through Keynote's own scripting commands, for the computer-use step. The
//! accessibility route can read Keynote but not edit its slides (docs/issues.md item 100), so this is the
//! Keynote-only path the plan named as its fallback.
//!
//! The script is one fixed text in this file. The person's words never become part of it: the file path and
//! every title, body and note are passed to it as separate arguments (`argv`), so no text can change what the
//! script does. It makes a new document, fills its slides, saves it at a new path, and closes it. It never opens
//! or changes an existing file: a path that already exists is refused before Keynote is asked.
//!
//! macOS asks the person once whether SproutCrew may control Keynote (Automation permission), the first time a
//! deck is built. Nothing here asks for it earlier.

use std::path::{Path, PathBuf};

pub const MAX_SLIDES: usize = 50;
const MAX_TEXT: usize = 4_000;

/// One slide: its title, the body (lines become bullets on a bullet layout), and the presenter notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slide {
    pub title: String,
    pub body: String,
    pub notes: String,
}

/// How the deck looks: fonts, the title colour and an optional logo. It comes from the project's template (the
/// same look the Word, PDF and PowerPoint files take); an empty field keeps Keynote's own. Text sizes stay the
/// theme's, as in the PowerPoint deck: a document's body size is too small for a slide.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    pub font: String,
    pub heading_font: String,
    /// The title colour as Keynote's 16-bit red, green and blue.
    pub accent: Option<[u32; 3]>,
    /// A PNG logo inside the project's folder, with its size in pixels.
    pub logo: Option<(PathBuf, [u32; 2])>,
}

/// The slide width; slides are 1920 by 1080 points, the 16:9 shape of the PowerPoint deck.
const W: f64 = 1920.0;

impl Style {
    /// The Keynote style for a template look. `logo` is where the look's PNG logo was written, if it has one.
    pub fn from_look(look: &crate::office_export::Look, logo: Option<PathBuf>) -> Style {
        let hex = |i: usize| u32::from_str_radix(look.accent.get(i..i + 2)?, 16).ok();
        let accent = match (look.accent.len(), hex(0), hex(2), hex(4)) {
            (6, Some(r), Some(g), Some(b)) => Some([r * 257, g * 257, b * 257]),
            _ => None,
        };
        Style {
            font: look.font.clone(),
            heading_font: look
                .heading_font
                .clone()
                .unwrap_or_else(|| look.font.clone()),
            accent,
            logo: logo.zip(look.logo_px),
        }
    }

    /// The logo's place, as the PowerPoint deck has it: about 0.4 inch high in the top right corner, at most
    /// 2.5 inches wide. Width, height, left, top, in points.
    fn logo_box(&self) -> Option<[u32; 4]> {
        let (_, [pw, ph]) = self.logo.as_ref()?;
        if *pw == 0 || *ph == 0 {
            return None;
        }
        let inch = W / 13.333;
        let h = 0.4 * inch;
        let w = (h * *pw as f64 / *ph as f64).min(2.5 * inch);
        Some([w, h, W - 0.35 * inch - w, 0.15 * inch].map(|v| v.round() as u32))
    }
}

/// The fixed script. Arguments: the path to save at, the number of slides, nine style values (text font, title
/// font, title red, green and blue, logo path, logo width, height, left and top; empty keeps Keynote's own), then title, body and notes for each slide.
pub const SCRIPT: &str = r#"on run argv
	set savePath to item 1 of argv
	set slideCount to (item 2 of argv) as integer
	set textFont to item 3 of argv
	set titleFont to item 4 of argv
	set logoPath to item 8 of argv
	tell application "Keynote"
		try
			set d to make new document with properties {document theme:theme "Basic White", width:1920, height:1080}
		on error
			set d to make new document with properties {width:1920, height:1080}
		end try
		tell d
			repeat with i from 1 to slideCount
				set base to 12 + (i - 1) * 3
				if i is 1 then
					set s to slide 1
				else
					set s to make new slide
				end if
				try
					set object text of default title item of s to item (base + 1) of argv
				end try
				try
					set object text of default body item of s to item (base + 2) of argv
				end try
				set presenter notes of s to item (base + 3) of argv
				if titleFont is not "" then
					try
						set font of object text of default title item of s to titleFont
					end try
				end if
				if item 5 of argv is not "" then
					try
						set color of object text of default title item of s to {(item 5 of argv) as integer, (item 6 of argv) as integer, (item 7 of argv) as integer}
					end try
				end if
				if textFont is not "" then
					try
						set font of object text of default body item of s to textFont
					end try
				end if
				if logoPath is not "" then
					tell s
						set img to make new image with properties {file:(POSIX file logoPath)}
						set width of img to (item 9 of argv) as integer
						set height of img to (item 10 of argv) as integer
						set position of img to {(item 11 of argv) as integer, (item 12 of argv) as integer}
					end tell
				end if
			end repeat
		end tell
		save d in POSIX file savePath
		close d saving no
	end tell
	return "saved " & slideCount
end run"#;

fn plain(t: &str) -> bool {
    t.chars().count() <= 100 && !t.chars().any(char::is_control)
}

/// Checks a deck before Keynote is asked: a new `.key` path inside `folder`, slides within the limits, and a
/// style whose font names are plain and whose logo is a PNG file inside the same folder.
pub fn check(path: &Path, folder: &Path, slides: &[Slide], style: &Style) -> Result<(), String> {
    if path.extension().and_then(|e| e.to_str()) != Some("key") {
        return Err("a Keynote deck is saved as a .key file".into());
    }
    let parent = path.parent().ok_or("the deck needs a folder")?;
    let (Ok(parent), Ok(folder)) = (parent.canonicalize(), folder.canonicalize()) else {
        return Err("the deck's folder does not exist".into());
    };
    if !parent.starts_with(&folder) {
        return Err("the deck must be saved inside the project's folder".into());
    }
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(
            "a file of that name is already there; a deck is never saved over a file".into(),
        );
    }
    if slides.is_empty() || slides.len() > MAX_SLIDES {
        return Err(format!("a deck has 1 to {MAX_SLIDES} slides"));
    }
    for s in slides {
        for t in [&s.title, &s.body, &s.notes] {
            if t.chars().count() > MAX_TEXT
                || t.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err("a slide's text is too long or has control characters".into());
            }
        }
    }
    if !plain(&style.font) || !plain(&style.heading_font) {
        return Err("a font name is too long or has control characters".into());
    }
    if let Some((logo, _)) = &style.logo {
        let inside = logo
            .canonicalize()
            .is_ok_and(|l| l.starts_with(&folder) && l.is_file());
        if !inside || logo.extension().and_then(|e| e.to_str()) != Some("png") {
            return Err("the logo must be a PNG file inside the project's folder".into());
        }
    }
    Ok(())
}

/// The arguments for `/usr/bin/osascript`: the fixed script, then the path, the style and each slide's text as
/// values.
pub fn args(path: &Path, slides: &[Slide], style: &Style) -> Vec<String> {
    let opt = |v: Option<u32>| v.map(|n| n.to_string()).unwrap_or_default();
    let rgb = style.accent;
    let logo = style.logo_box();
    let mut v = vec![
        "-e".to_string(),
        SCRIPT.to_string(),
        path.to_string_lossy().into_owned(),
        slides.len().to_string(),
        style.font.clone(),
        style.heading_font.clone(),
        opt(rgb.map(|c| c[0])),
        opt(rgb.map(|c| c[1])),
        opt(rgb.map(|c| c[2])),
        match (&style.logo, logo) {
            (Some((p, _)), Some(_)) => p.to_string_lossy().into_owned(),
            _ => String::new(),
        },
    ];
    for i in 0..4 {
        v.push(opt(logo.map(|b| b[i])));
    }
    for s in slides {
        // Keynote reads a carriage return as a new paragraph in a text box.
        v.extend([s.title.clone(), s.body.replace('\n', "\r"), s.notes.clone()]);
    }
    v
}

/// Builds the deck with Keynote and returns where it was saved. Runs `osascript` from an empty environment, with
/// the arguments as an array and never through a shell.
pub fn build(
    path: &Path,
    folder: &Path,
    slides: &[Slide],
    style: &Style,
) -> Result<PathBuf, String> {
    check(path, folder, slides, style)?;
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(args(path, slides, style))
        .env_clear()
        .env("HOME", std::env::var_os("HOME").unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Keynote could not be asked: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.contains("-1743") || err.contains("Not authorized") {
            "SproutCrew is not allowed to control Keynote. Allow it in System Settings, Privacy & Security, Automation".into()
        } else {
            format!(
                "Keynote could not build the deck: {}",
                err.trim().chars().take(300).collect::<String>()
            )
        });
    }
    if !path.exists() {
        return Err("Keynote finished but the deck was not saved".into());
    }
    Ok(path.to_path_buf())
}

/// The slides of a deck, split as the PowerPoint export splits them: a title slide, then a new slide at each
/// heading of level 1 or 2. Paragraphs, list items and table rows become body lines; a notes block becomes the
/// presenter notes.
pub fn slides_from(doc: &crate::office_export::Document) -> Vec<Slide> {
    let text = |b: &crate::office_export::Block| {
        b.runs.iter().map(|r| r.text.as_str()).collect::<String>()
    };
    let mut out = vec![Slide {
        title: doc.title.clone(),
        body: doc.subtitle.clone(),
        notes: String::new(),
    }];
    let mut current: Option<Slide> = None;
    for b in &doc.blocks {
        if b.kind == "heading" && b.level <= 2 {
            out.extend(current.take());
            current = Some(Slide {
                title: text(b),
                body: String::new(),
                notes: String::new(),
            });
            continue;
        }
        let s = current.get_or_insert_with(|| Slide {
            title: doc.title.clone(),
            body: String::new(),
            notes: String::new(),
        });
        let line = match b.kind.as_str() {
            "notes" => {
                if !s.notes.is_empty() {
                    s.notes.push('\n');
                }
                s.notes.push_str(&text(b));
                continue;
            }
            "rule" => continue,
            "table" => b
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| c.iter().map(|x| x.text.as_str()).collect::<String>())
                        .collect::<Vec<_>>()
                        .join(" · ")
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => text(b),
        };
        if line.trim().is_empty() {
            continue;
        }
        if !s.body.is_empty() {
            s.body.push('\n');
        }
        s.body.push_str(&line);
    }
    out.extend(current);
    out.truncate(MAX_SLIDES);
    out
}

/// The fixed script that reads a deck back: for each slide its title, body and notes, separated by fixed marks.
const READ_SCRIPT: &str = r#"on run argv
	set sep to (ASCII character 31)
	set rowSep to (ASCII character 30)
	tell application "Keynote"
		set d to open POSIX file (item 1 of argv)
		set out to ""
		repeat with s in slides of d
			set t to ""
			set b to ""
			try
				set t to object text of default title item of s
			end try
			try
				set b to object text of default body item of s
			end try
			set out to out & t & sep & b & sep & (presenter notes of s) & rowSep
		end repeat
		close d saving no
	end tell
	return out
end run"#;

/// Reads a built deck back through Keynote, without changing it.
pub fn read_back(path: &Path) -> Result<Vec<Slide>, String> {
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", READ_SCRIPT, &path.to_string_lossy()])
        .env_clear()
        .env("HOME", std::env::var_os("HOME").unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Keynote could not be asked: {e}"))?;
    if !out.status.success() {
        return Err("Keynote could not read the deck back".into());
    }
    Ok(parse_read(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_read(text: &str) -> Vec<Slide> {
    text.trim_end_matches('\n')
        .split('\u{1e}')
        .filter(|r| !r.is_empty())
        .map(|r| {
            let mut p = r.split('\u{1f}').map(|x| x.replace('\r', "\n"));
            Slide {
                title: p.next().unwrap_or_default(),
                body: p.next().unwrap_or_default(),
                notes: p.next().unwrap_or_default(),
            }
        })
        .collect()
}

/// What differs between the slides sent and the slides read back, in plain sentences. Empty when they match.
pub fn compare(sent: &[Slide], got: &[Slide]) -> Vec<String> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = vec![];
    if sent.len() != got.len() {
        out.push(format!(
            "The deck has {} of the {} slides sent",
            got.len(),
            sent.len()
        ));
    }
    for (i, (a, b)) in sent.iter().zip(got).enumerate() {
        for (what, x, y) in [
            ("title", &a.title, &b.title),
            ("text", &a.body, &b.body),
            ("presenter notes", &a.notes, &b.notes),
        ] {
            if norm(x) != norm(y) {
                out.push(format!("Slide {}: the {what} is not what was sent", i + 1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(t: &str) -> Slide {
        Slide {
            title: t.into(),
            body: "One\nTwo".into(),
            notes: "Say this".into(),
        }
    }

    #[test]
    fn a_deck_splits_into_slides_at_its_headings_with_notes_kept_apart() {
        let doc = crate::office_export::parse(
            "Q3 review",
            "For the board",
            "## Results\n\nRevenue rose.\n\n- Margin up\n- Costs down\n\n## Next\n\n| Item | Owner |\n|---|---|\n| Hire | Ana |\n",
        )
        .unwrap();
        let s = slides_from(&doc);
        assert_eq!(
            s[0],
            Slide {
                title: "Q3 review".into(),
                body: "For the board".into(),
                notes: String::new()
            }
        );
        assert_eq!(s[1].title, "Results");
        assert_eq!(s[1].body, "Revenue rose.\nMargin up\nCosts down");
        assert_eq!(s[2].title, "Next");
        assert!(s[2].body.contains("Hire · Ana"), "{:?}", s[2]);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn a_deck_read_back_is_compared_slide_by_slide() {
        let sent = vec![slide("A"), slide("B")];
        let back =
            parse_read("A\u{1f}One\rTwo\u{1f}Say this\u{1e}B\u{1f}One\rTwo\u{1f}Say this\u{1e}\n");
        assert_eq!(back, sent);
        assert!(compare(&sent, &back).is_empty());
        let mut wrong = back.clone();
        wrong[1].title = "C".into();
        assert_eq!(
            compare(&sent, &wrong),
            ["Slide 2: the title is not what was sent"]
        );
        assert_eq!(
            compare(&sent, &back[..1]),
            ["The deck has 1 of the 2 slides sent"]
        );
    }

    #[test]
    fn the_script_is_fixed_and_the_text_goes_in_as_separate_values() {
        let tricky = slide("\" & (do shell script \"rm -rf ~\") & \"");
        let a = args(
            Path::new("/p/Deck.key"),
            &[tricky.clone(), slide("Two")],
            &Style::default(),
        );
        assert_eq!(a[0], "-e");
        assert_eq!(
            a[1], SCRIPT,
            "the person's text is never part of the script"
        );
        assert!(!SCRIPT.contains("do shell script"));
        assert_eq!(&a[2..4], ["/p/Deck.key", "2"]);
        assert!(
            a[4..14].iter().all(String::is_empty),
            "no style keeps Keynote's own"
        );
        assert_eq!(a[14], tricky.title);
        assert_eq!(a[15], "One\rTwo");
        assert_eq!(a.len(), 14 + 2 * 3);
    }

    #[test]
    fn a_template_look_becomes_fonts_a_title_colour_and_a_logo_in_the_corner_with_theme_sizes() {
        let look = crate::office_export::Look {
            font: "Avenir Next".into(),
            accent: "FF8000".into(),
            heading_font: Some("Georgia".into()),
            body_pt: Some(20.4),
            logo_px: Some([200, 100]),
            ..Default::default()
        };
        let st = Style::from_look(&look, Some("/p/logo.png".into()));
        assert_eq!(st.accent, Some([65535, 32896, 0]));
        assert_eq!(st.heading_font, "Georgia");
        let a = args(Path::new("/p/D.key"), &[slide("A")], &st);
        assert_eq!(
            &a[4..14],
            [
                "Avenir Next",
                "Georgia",
                "65535",
                "32896",
                "0",
                "/p/logo.png",
                "115",
                "58",
                "1754",
                "22"
            ]
        );
        // No logo file, or a bad colour, leaves those values empty.
        let plain = Style::from_look(
            &crate::office_export::Look {
                accent: "nope".into(),
                ..look.clone()
            },
            None,
        );
        let a = args(Path::new("/p/D.key"), &[slide("A")], &plain);
        assert!(a[6..14].iter().all(String::is_empty), "{:?}", &a[6..14]);
        assert_eq!(
            a[5], "Georgia",
            "the title font falls back to the text font only when unset"
        );
    }

    #[test]
    fn a_deck_is_new_inside_the_project_folder_and_within_the_limits() {
        let root = std::env::temp_dir().join(format!("bff-keynote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("exports")).unwrap();
        let none = Style::default();
        let ok = root.join("exports/Deck.key");
        assert!(check(&ok, &root, &[slide("A")], &none).is_ok());
        std::fs::write(&ok, "x").unwrap();
        assert!(check(&ok, &root, &[slide("A")], &none)
            .unwrap_err()
            .contains("never saved over"));
        assert!(check(&root.join("exports/Deck.pptx"), &root, &[slide("A")], &none).is_err());
        assert!(check(
            &std::env::temp_dir().join("Elsewhere.key"),
            &root,
            &[slide("A")],
            &none
        )
        .unwrap_err()
        .contains("inside the project"));
        assert!(check(&root.join("exports/New.key"), &root, &[], &none).is_err());
        let many = vec![slide("A"); MAX_SLIDES + 1];
        assert!(check(&root.join("exports/New.key"), &root, &many, &none).is_err());
        let bad = Slide {
            title: "a\u{7}".into(),
            body: String::new(),
            notes: String::new(),
        };
        assert!(check(&root.join("exports/New.key"), &root, &[bad], &none).is_err());
        let new = root.join("exports/New.key");
        let outside = std::env::temp_dir().join(format!("bff-logo-{}.png", std::process::id()));
        std::fs::write(&outside, "png").unwrap();
        let with = |p: PathBuf| Style {
            logo: Some((p, [100, 50])),
            ..Style::default()
        };
        assert!(check(&new, &root, &[slide("A")], &with(outside.clone()))
            .unwrap_err()
            .contains("logo"));
        let inside = root.join("exports/logo.png");
        std::fs::write(&inside, "png").unwrap();
        assert!(check(&new, &root, &[slide("A")], &with(inside)).is_ok());
        let odd = Style {
            font: "Avenir\u{0}".into(),
            ..Style::default()
        };
        assert!(check(&new, &root, &[slide("A")], &odd).is_err());
        std::fs::remove_file(&outside).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
