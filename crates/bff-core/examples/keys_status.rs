//! Which BFF keychain items exist and which scopes they declare. Reads attributes only, never a password.
//! Run: cargo run -p bff-core --example keys_status

use bff_core::keychain::Keychain;

fn main() {
    let k = Keychain::new();
    for account in [
        "openai-api",
        "deepseek-api",
        "kimi-api",
        "openrouter-api",
        "anthropic-api",
        "xai-api",
        "gemini-api",
        "github",
        "notion",
        "github-write",
        "notion-write",
        "drive",
    ] {
        match k.declared_scopes(account) {
            Some(s) if s.is_empty() => println!("{account:14} present, no scopes written"),
            Some(s) => println!("{account:14} present, scopes: {}", s.join(", ")),
            None => println!("{account:14} not set"),
        }
    }
}
