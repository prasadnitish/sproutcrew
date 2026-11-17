//! Text from pictures: an image file, or the pages of a scanned PDF that have no text layer. The reading is
//! done by Apple's Vision framework, in a small program (`ocr/main.swift`) that `build.rs` compiles and this
//! crate carries inside itself. The program runs as a child with an empty environment, a time limit and a cap
//! on its output, so a damaged file can only stop that child. Without macOS and Xcode's command line tools
//! at build time the program is left out, and every call here says so in words.

use crate::helper;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock};
use std::time::Duration;

static PROGRAM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bff-ocr"));

/// Most PDF pages read in one file. A longer scan keeps its first pages and says where it stopped.
pub const MAX_PAGES: usize = 60;
const MAX_OUT: usize = 16 * 1024 * 1024;

/// Pages that were read, by page number, and whether the time limit stopped the rest.
#[derive(Debug, PartialEq, Eq)]
pub struct Scan {
    pub pages: Vec<(usize, String)>,
    pub stopped_early: bool,
}

fn program() -> Result<&'static Path, String> {
    static INSTALLED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    INSTALLED
        .get_or_init(install)
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(Clone::clone)
}

/// The first time a new copy of the program reads anything, macOS spends about half a minute getting its text
/// model ready. This does that once per run of BFF. Every read waits for it, so none is cut short by it.
fn warm() {
    static WARMED: Once = Once::new();
    WARMED.call_once(|| {
        if let Ok(path) = program() {
            let _ = helper::run(path, &["warm"], Duration::from_secs(300), 4096);
        }
    });
}

/// Starts getting the reader ready in the background, so the first picture or scan is not slow.
pub fn warm_up() {
    if !PROGRAM.is_empty() {
        std::thread::spawn(warm);
    }
}

/// Puts the carried program in the private folder, under a name that holds its hash.
fn install() -> Result<PathBuf, String> {
    if PROGRAM.is_empty() {
        return Err("this build of BFF cannot read text from pictures. It needs macOS and Xcode's command line tools when BFF is built".into());
    }
    let dir = helper::private_dir()?;
    let hash = format!("{:x}", Sha256::digest(PROGRAM));
    let path = dir.join(format!("bff-ocr-{}", &hash[..16]));
    if std::fs::read(&path).is_ok_and(|b| b == PROGRAM) {
        return Ok(path);
    }
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let staging = dir.join(format!("bff-ocr-new-{}", std::process::id()));
    let _ = std::fs::remove_file(&staging);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&staging)
        .map_err(|e| e.to_string())?;
    file.write_all(PROGRAM).map_err(|e| e.to_string())?;
    drop(file);
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&staging, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Pages and the one error line a run printed.
fn parse(stdout: &[u8]) -> (Vec<(usize, String)>, Option<String>) {
    let (mut pages, mut error) = (Vec::new(), None);
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(e) = v["error"].as_str() {
            error = Some(e.to_string());
        } else if let (Some(n), Some(t)) = (v["page"].as_u64(), v["text"].as_str()) {
            pages.push((n as usize, t.to_string()));
        }
    }
    (pages, error)
}

/// The text in one picture. `ext` is the file's type, for example `png`.
pub fn image(bytes: &[u8], ext: &str) -> Result<String, String> {
    let program = program()?;
    warm();
    let file = helper::TempFile::create(ext, bytes)?;
    let out = helper::run(
        program,
        &["image".as_ref(), file.path().as_os_str()],
        Duration::from_secs(90),
        MAX_OUT,
    )?;
    let (pages, error) = parse(&out.stdout);
    if out.timed_out {
        return Err("reading the picture took too long and was stopped".into());
    }
    if let Some(e) = error {
        return Err(e);
    }
    pages
        .into_iter()
        .next()
        .map(|(_, text)| text)
        .ok_or_else(|| "the text reader gave no answer".into())
}

/// The text on these pages (numbered from 1) of a PDF, read as pictures. Pages that were not read before the
/// time limit are missing from the answer, and `stopped_early` is set.
pub fn pdf(bytes: &[u8], pages: &[usize]) -> Result<Scan, String> {
    if pages.is_empty() {
        return Ok(Scan {
            pages: vec![],
            stopped_early: false,
        });
    }
    let program = program()?;
    warm();
    let file = helper::TempFile::create("pdf", bytes)?;
    let list = pages
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let limit = Duration::from_secs((30 + 6 * pages.len() as u64).min(420));
    let out = helper::run(
        program,
        &["pdf".as_ref(), file.path().as_os_str(), list.as_ref()],
        limit,
        MAX_OUT,
    )?;
    let (read, error) = parse(&out.stdout);
    if let Some(e) = error {
        return Err(e);
    }
    Ok(Scan {
        stopped_early: out.timed_out || out.too_long || read.len() < pages.len(),
        pages: read,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICTURE: &[u8] = include_bytes!("../tests/fixtures/ocr/sample.png");
    const SCAN: &[u8] = include_bytes!("../tests/fixtures/ocr/scanned.pdf");

    fn squash(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_picture_of_text_is_read() {
        let text = image(PICTURE, "png").unwrap();
        assert!(
            squash(&text).contains("Invoice 4821")
                && squash(&text).contains("Total due 90 dollars by June 30"),
            "{text:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_a_picture_is_an_error_not_empty_text() {
        let e = image(b"this is not a picture", "png").unwrap_err();
        assert!(e.contains("could not be read"), "{e}");
    }

    #[test]
    fn the_pages_of_a_scanned_pdf_are_read_by_number() {
        let scan = pdf(SCAN, &[2, 1]).unwrap();
        assert!(!scan.stopped_early);
        let page = |n: usize| {
            scan.pages
                .iter()
                .find(|(p, _)| *p == n)
                .map(|(_, t)| squash(t))
                .unwrap_or_default()
        };
        assert!(
            page(1).contains("Candidate instructions"),
            "{:?}",
            scan.pages
        );
        assert!(
            page(1).contains("Write 1000 words at most"),
            "{:?}",
            scan.pages
        );
        assert!(page(2).contains("Submit by Friday"), "{:?}", scan.pages);
    }

    #[test]
    fn a_page_number_past_the_end_is_reported_as_not_read() {
        let scan = pdf(SCAN, &[1, 9]).unwrap();
        assert_eq!(scan.pages.len(), 1);
        assert!(scan.stopped_early, "a missing page must not look complete");
    }

    #[test]
    fn a_pdf_that_cannot_be_opened_is_an_error() {
        let e = pdf(b"%PDF-1.4 nothing here", &[1]).unwrap_err();
        assert!(e.contains("could not be opened"), "{e}");
    }

    #[test]
    fn no_pages_means_no_work_and_no_helper() {
        assert_eq!(
            pdf(b"", &[]).unwrap(),
            Scan {
                pages: vec![],
                stopped_early: false
            }
        );
    }

    #[test]
    fn the_installed_program_is_private_and_matches_what_is_carried() {
        let path = program().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), PROGRAM);
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(path).unwrap().mode() & 0o777, 0o700);
    }
}
