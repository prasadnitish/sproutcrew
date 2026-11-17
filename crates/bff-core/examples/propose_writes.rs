//! Proposes writes for a project, to be approved in the app. It writes no key and sends nothing.
//! Run: cargo run -p bff-core --example propose_writes -- <projects-dir> <project> <notion-page-address> <owner/repo> [text-file-for-notion]

use bff_core::app::App;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: propose_writes <projects-dir> <project> <notion-page-address> <owner/repo> [text-file]");
        std::process::exit(2);
    }
    let mut app = App::new(std::path::PathBuf::from(&a[1]));
    app.open_project(&a[2]).expect("open the project");
    let root = app.project_root().expect("root");
    let text_file = a
        .get(5)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("code/README.md"));
    let body = std::fs::read_to_string(&text_file).expect("read the text for Notion");
    let title = format!(
        "{}: {}",
        a[2],
        text_file.file_name().unwrap_or_default().to_string_lossy()
    );
    match app.propose_notion(&a[3], &title, &body, false) {
        Ok(id) => println!(
            "Notion proposal {id}: {title:?}, {} characters",
            body.chars().count()
        ),
        Err(e) => println!("Notion proposal refused: {e}"),
    }
    match app.propose_github(&a[4], "", "", bff_core::clock::now()) {
        Ok(id) => println!("GitHub proposal {id}"),
        Err(e) => println!("GitHub proposal refused: {e}"),
    }
    let state = app.state("replay").expect("state");
    for w in state["writes"].as_array().cloned().unwrap_or_default() {
        println!(
            "{}",
            serde_json::to_string_pretty(&w)
                .unwrap_or_default()
                .chars()
                .take(900)
                .collect::<String>()
        );
    }
}
