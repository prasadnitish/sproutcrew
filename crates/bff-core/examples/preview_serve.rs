//! Scaffolds the prototype kit into a folder and serves it for a while, so a browser can check it under the
//! preview CSP. Run: cargo run -p bff-core --example preview_serve -- <empty-folder> <seconds>

use bff_core::{kit, preview::Preview};
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("folder"));
    let secs: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    kit::scaffold(
        &dir,
        "Pilot planner",
        "Teams adopt a tool faster with a checklist",
    )
    .expect("scaffold");
    let problems = kit::smoke(&dir);
    println!("smoke problems: {problems:?}");
    let p = Preview::start(&dir).expect("start");
    println!("URL {}", p.url());
    std::thread::sleep(std::time::Duration::from_secs(secs));
    p.stop();
}
