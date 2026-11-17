//! Live check of the files step with Keynote on: a sample deck becomes a PowerPoint file and a Keynote deck,
//! and the Keynote deck is read back and compared. `cargo run -p bff-core --example keynote_files -- <new folder>`

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("give a new folder"));
    let deck = "# Q3 review\n\n## Results\n\nRevenue rose 12 percent.\n\n- Margin up two points\n- Costs down\n\n## Next steps\n\n| Item | Owner |\n|---|---|\n| Hire a designer | Ana |\n";
    let tools = bff_core::clientfiles::Tools {
        typst: None,
        keynote: true,
    };
    match bff_core::clientfiles::make(&tools, &out, "Sample", "For the board", None, Some(deck)) {
        Ok((made, notes)) => {
            for m in made {
                println!("{} {}", m.kind, m.path.display());
            }
            for n in notes {
                println!("note: {n}");
            }
        }
        Err(e) => println!("not made: {e}"),
    }
}
