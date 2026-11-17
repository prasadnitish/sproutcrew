//! One unauthenticated GET to GitHub's public `/zen` endpoint, to show the HTTPS client works end to end.
//! It sends no credential. Run: cargo run -p bff-core --example http_smoke

use bff_core::connector::{Request, Transport};
use bff_core::http::HttpClient;

fn main() {
    let mut c = HttpClient::new(&["api.github.com"]);
    match c.get(&Request {
        url: "https://api.github.com/zen".into(),
        headers: vec![],
    }) {
        Ok(r) => println!(
            "status {} body {:?}",
            r.status,
            r.body.chars().take(80).collect::<String>()
        ),
        Err(e) => println!("failed: {e:?}"),
    }
    // A host that is not on the list is refused before any connection.
    println!(
        "{:?}",
        c.get(&Request {
            url: "https://example.com/".into(),
            headers: vec![]
        })
    );
}
