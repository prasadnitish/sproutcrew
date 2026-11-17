//! Approves and sends one proposed write. Run it only after the person has approved that exact content in
//! chat or in the app. It reads the write key from the Keychain when the request is sent and prints no key.
//! Run: cargo run -p bff-core --example send_write -- <projects-dir> <project> <write-id>

use bff_core::http::HttpClient;
use bff_core::keychain::Keychain;
use bff_core::writes::{Remote, Status, Writes};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: send_write <projects-dir> <project> <write-id>");
        std::process::exit(2);
    }
    let root = std::path::Path::new(&a[1]).join(&a[2]);
    let mut writes = Writes::open(&root).expect("open writes");
    if matches!(
        writes
            .items()
            .iter()
            .find(|w| w.id == a[3])
            .map(|w| &w.status),
        Some(Status::Proposed)
    ) {
        writes.approve(&a[3], "Nitish").expect("approve");
    }
    let mut transport = HttpClient::new(&["api.notion.com", "api.github.com"]);
    let keys = Keychain::new();
    let mut remote = Remote {
        transport: &mut transport,
        creds: &keys,
    };
    match writes.send(&a[3], &mut remote) {
        Ok(url) => println!("written: {url}"),
        Err(e) => println!("not written: {e}"),
    }
}
