//! Makes the Word file, the PDF, and the deck for a project from the newest results of two steps.
//! Run: cargo run -p bff-core --example make_client_files -- <projects-dir> <project> <document-step> <deck-step> <subtitle>

use bff_core::app::App;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("usage: make_client_files <projects-dir> <project> <document-step> <deck-step> <subtitle>");
        std::process::exit(2);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut app = App::new(std::path::PathBuf::from(&a[1]));
    app.open_project(&a[2]).expect("open the project");
    let tools = bff_core::clientfiles::Tools::detect(&home);
    match app.export_client_files(
        Some(&a[3]),
        Some(&a[4]),
        &a[5],
        &tools,
        bff_core::clock::now(),
    ) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => println!("not made: {e}"),
    }
}
