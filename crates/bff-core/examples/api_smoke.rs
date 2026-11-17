//! One tiny request on each API route that is set up and has its key, to check the route works and that
//! tokens are counted. It uses the real app settings, key items, and token cap, so what it uses is recorded
//! there. Run: cargo run -p bff-core --example api_smoke -- [route ...]

use bff_core::api_engine::ApiEngine;
use bff_core::engine::{Engine, Request};
use bff_core::http::HttpClient;
use bff_core::keychain::Keychain;
use serde_json::Value;
use std::sync::{Arc, Mutex};

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let data =
        std::path::PathBuf::from(&home).join("Library/Application Support/app.sproutcrew.mac");
    let cfg = std::fs::read_to_string(data.join("api-routes.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| bff_core::apiconfig::parse(&v).ok())
        .unwrap_or_default();
    let want: Vec<String> = std::env::args().skip(1).collect();
    let tokens = Arc::new(Mutex::new(
        bff_core::spend::SpendBook::open_unit(
            &data.join("tokens.json"),
            bff_core::spend::Unit::Tokens,
        )
        .expect("tokens book"),
    ));
    let keys = Keychain::new();
    let models = [
        ("R5", "gpt-6-luna"),
        ("R10", "deepseek-v4-flash"),
        ("R12", "deepseek/deepseek-v4-flash"),
    ];
    for (route, model) in models {
        if !want.is_empty() && !want.iter().any(|w| w == route) {
            continue;
        }
        let Some(c) = cfg.get(route) else {
            println!("{route}: not set up in api-routes.json");
            continue;
        };
        let provider = bff_core::apiroute::Provider::for_route(route).expect("route");
        if !keys.present(provider.credential()) {
            println!("{route}: no key item {}", provider.credential());
            continue;
        }
        let mut engine = ApiEngine::new(
            provider,
            Box::new(HttpClient::new(&[provider.host()])),
            Box::new(Keychain::new()),
            c.bounds,
            c.prices,
            c.allowance_micro,
        );
        if c.token_mode {
            engine = engine.token_mode();
        }
        let mut engine = engine.with_book(tokens.clone());
        let dir = std::env::temp_dir().join(format!("bff-smoke-{}-{route}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let started = std::time::Instant::now();
        let r = engine.run_worker(&Request {
            web: false,
            route,
            step: "smoke",
            task_dir: &dir,
            prompt: "Reply with the single word OK in a file's worth of text. Nothing else.",
            model,
            effort: "low",
        });
        let used: Vec<String> = r
            .usage
            .iter()
            .map(|e| format!("{:?}={:?}", e.meter, e.value))
            .collect();
        let text = std::fs::read_to_string(dir.join("artifact.md")).unwrap_or_default();
        println!(
            "{route} {model}: completed={} in {}s, usage [{}], answer {:?}{}",
            r.completed,
            started.elapsed().as_secs(),
            used.join(", "),
            text.chars().take(60).collect::<String>(),
            if r.completed {
                String::new()
            } else {
                format!(", note: {}", r.note)
            }
        );
    }
    let s = tokens.lock().unwrap().status();
    println!("token cap: {} used of {}", s.spent, s.cap);
}
