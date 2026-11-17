//! The Keynote scripting test: builds a new three-slide deck at a new path inside a folder, then stops.
//! `cargo run -p bff-core --example keynote_build -- <folder> <name>.key [<logo.png inside the folder>]`
//! With a logo, the deck also takes a sample template look: Georgia titles in orange, Avenir Next text at the
//! theme's size, and the logo in the top right corner of each slide.

use bff_core::keynote::{build, Slide, Style};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (Some(folder), Some(name)) = (a.get(1), a.get(2)) else {
        eprintln!("give a folder and a file name ending in .key");
        std::process::exit(2);
    };
    let folder = std::path::Path::new(folder);
    let s = |t: &str, b: &str, n: &str| Slide {
        title: t.into(),
        body: b.into(),
        notes: n.into(),
    };
    let slides = [
        s(
            "SproutCrew test deck",
            "Built through Keynote's own scripting",
            "Opening slide.",
        ),
        s(
            "What it does",
            "Adds slides\nSets titles and bullets\nWrites presenter notes",
            "Second slide notes.",
        ),
        s(
            "Quotes stay text",
            "\" & (do shell script \"echo hacked\") & \"",
            "This slide checks that text cannot become a command.",
        ),
    ];
    let style = match a.get(3) {
        Some(logo) => {
            let png = std::fs::read(logo).expect("the logo could not be read");
            let px = |i: usize| u32::from_be_bytes(png[i..i + 4].try_into().unwrap());
            Style {
                font: "Avenir Next".into(),
                heading_font: "Georgia".into(),
                accent: Some([65535, 32896, 0]),
                logo: Some((logo.into(), [px(16), px(20)])),
            }
        }
        None => Style::default(),
    };
    match build(&folder.join(name), folder, &slides, &style) {
        Ok(p) => println!("saved {}", p.display()),
        Err(e) => println!("not built: {e}"),
    }
}
