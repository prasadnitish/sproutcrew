//! Makes outbox drafts from the posts in a step's newest result and prints what was made and what was left out.
//! Drafts only: nothing is approved or sent.
//! Run: cargo run -p bff-core --example queue_drafts -- <projects-dir> <project> <step> <first-post-unix-time> <seconds-apart>

use bff_core::app::App;
use std::path::PathBuf;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("usage: queue_drafts <projects-dir> <project> <step> <first-post-unix-time> <seconds-apart>");
        std::process::exit(2);
    }
    let mut app = App::new(PathBuf::from(&a[1]));
    app.open_project(&a[2]).expect("open the project");
    let start: u64 = a[4].parse().expect("a unix time");
    let gap: u64 = a[5].parse().expect("seconds");
    match app.queue_to_drafts(&a[3], start, gap) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => println!("not made: {e}"),
    }
}
