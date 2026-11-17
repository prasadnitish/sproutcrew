//! Nitish's licence tool. It is never shipped in the app.
//!
//! `cargo run -p bff-core --example licence_tool -- keygen` makes a new signing key from this Mac's random source,
//! saves its private half in the Keychain (service `sproutcrew`, item `licence-signing`, as URL-safe base64),
//! and prints only the
//! public half, to paste into `PUBLIC_KEY` in `src/entitlement.rs`. It refuses to replace a key already saved,
//! because licences signed with the old one would stop working.
//!
//! `cargo run -p bff-core --example licence_tool -- sign "<holder>" <plan> <feature,feature> [<ends, seconds since 1970>]`
//! prints a signed licence file. Features: paid_models, web_research, templates, connectors, excel_output,
//! computer_use.

use base64::Engine;
use bff_core::connector::Credentials;
use bff_core::entitlement::{Feature, Licence, LICENCE_FORMAT};
use ed25519_dalek::{Signer, SigningKey};
use std::io::Read;

const ITEM: &str = "licence-signing";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
/// The Keychain takes letters, digits, dot, dash and underscore only.
const STORE: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

fn fail(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let keys = bff_core::keychain::Keychain::new();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            if keys.get(ITEM).is_some() {
                fail("A licence signing key is already saved. Licences signed with it would stop working, so it was kept.");
            }
            let mut seed = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut seed))
                .unwrap_or_else(|_| fail("This Mac's random source could not be read."));
            let k = SigningKey::from_bytes(&seed);
            keys.save(ITEM, &STORE.encode(k.to_bytes()))
                .unwrap_or_else(|e| fail(&e));
            let public = k.verifying_key().to_bytes();
            println!("Saved the private key in the Keychain. Paste this into PUBLIC_KEY:");
            println!("Some({public:?})");
        }
        Some("sign") => {
            let (Some(holder), Some(plan), Some(features)) =
                (args.get(1), args.get(2), args.get(3))
            else {
                fail("sign needs a holder, a plan and a comma-separated list of features");
            };
            let features: Vec<Feature> = features
                .split(',')
                .map(|f| {
                    serde_json::from_value(serde_json::json!(f.trim()))
                        .unwrap_or_else(|_| fail(&format!("{f:?} is not a feature")))
                })
                .collect();
            let expires = args.get(4).map(|e| {
                e.parse::<u64>()
                    .unwrap_or_else(|_| fail("the end must be seconds since 1970"))
            });
            let secret = keys
                .get(ITEM)
                .unwrap_or_else(|| fail("There is no signing key. Run keygen first."));
            let bytes: [u8; 32] = STORE
                .decode(secret.token())
                .ok()
                .and_then(|b| b.try_into().ok())
                .unwrap_or_else(|| fail("The saved signing key is not readable."));
            let k = SigningKey::from_bytes(&bytes);
            let mut l = Licence {
                format: LICENCE_FORMAT,
                holder: holder.clone(),
                plan: plan.clone(),
                features,
                expires,
                signature: String::new(),
            };
            l.signature = B64.encode(k.sign(&l.signed_bytes()).to_bytes());
            bff_core::entitlement::read_with(
                &serde_json::to_string(&l).unwrap(),
                0,
                &k.verifying_key().to_bytes(),
            )
            .unwrap_or_else(|e| fail(&format!("the licence did not check: {e}")));
            println!("{}", serde_json::to_string_pretty(&l).unwrap());
        }
        _ => fail("Use keygen, or sign \"<holder>\" <plan> <features> [<ends>]."),
    }
}
