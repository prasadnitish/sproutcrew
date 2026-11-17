//! Groundwork for charging later. The features that might one day be paid (web research, templates, connectors)
//! ask [`check`] before they start, so making one paid later changes this file and the screen, not each feature.
//! Today every feature is allowed, for everyone.
//!
//! A licence is an offline file: who it is for, which plan, which features, when it ends, and an Ed25519
//! signature over the rest. [`read`] trusts a licence only when the signature checks against the public key built
//! into this app ([`PUBLIC_KEY`]), so editing a licence to give a better plan makes it fail. The private half of
//! the key is Nitish's alone, made and kept in his Keychain by `examples/licence_tool.rs`; it is never in this
//! repository. Until he has made the key and its public half is set below, every licence is refused, which is
//! safe because no feature needs one yet.

use serde::{Deserialize, Serialize};

/// The features a plan could one day decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    PaidModels,
    WebResearch,
    Templates,
    Connectors,
    ExcelOutput,
    ComputerUse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    /// Not allowed, with the reason in plain words.
    Refused(String),
}

/// Whether `feature` may run. Every feature is allowed in this version; the licence is not consulted.
pub fn check(_feature: Feature, _licence: Option<&Licence>) -> Decision {
    Decision::Allowed
}

pub const LICENCE_FORMAT: u32 = 1;

/// An offline licence, as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Licence {
    pub format: u32,
    /// Who it was issued to, as they gave it.
    pub holder: String,
    pub plan: String,
    pub features: Vec<Feature>,
    /// Seconds since 1970 when it stops; `None` for no end.
    pub expires: Option<u64>,
    /// Base64 of an Ed25519 signature over [`Licence::signed_bytes`].
    pub signature: String,
}

impl Licence {
    /// The bytes the signature covers: the licence without its signature, as compact JSON in field order.
    pub fn signed_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "format": self.format, "holder": self.holder, "plan": self.plan,
            "features": self.features, "expires": self.expires,
        }))
        .expect("a licence serializes")
    }
}

/// The public half of the licence signing key, as 32 bytes, made by Nitish on November 11, 2026 with
/// `cargo run -p bff-core --example licence_tool -- keygen`. Its private half is in his Keychain only.
pub const PUBLIC_KEY: Option<[u8; 32]> = Some([
    150, 14, 67, 125, 135, 220, 83, 208, 70, 165, 42, 167, 36, 21, 57, 64, 122, 193, 6, 129, 42,
    230, 217, 124, 60, 174, 107, 49, 214, 55, 32, 182,
]);

/// A licence file read, checked for shape, and trusted only if its signature checks against this app's key.
pub fn read(text: &str, now: u64) -> Result<Licence, String> {
    let key = PUBLIC_KEY.ok_or("this version of SproutCrew does not accept licences yet")?;
    read_with(text, now, &key)
}

/// [`read`] against a given public key.
pub fn read_with(text: &str, now: u64, public_key: &[u8; 32]) -> Result<Licence, String> {
    let l = read_shape(text, now)?;
    let key = ed25519_dalek::VerifyingKey::from_bytes(public_key)
        .map_err(|_| "this version's licence key is not usable")?;
    use base64::Engine;
    let sig = base64::engine::general_purpose::STANDARD
        .decode(&l.signature)
        .ok()
        .and_then(|b| ed25519_dalek::Signature::from_slice(&b).ok())
        .ok_or("this licence's signature is not readable")?;
    key.verify_strict(&l.signed_bytes(), &sig).map_err(|_| {
        "this licence was not issued by SproutCrew, or it was changed after it was issued"
    })?;
    Ok(l)
}

/// The licence checked for shape only, before its signature.
fn read_shape(text: &str, now: u64) -> Result<Licence, String> {
    let l: Licence =
        serde_json::from_str(text).map_err(|e| format!("this is not a SproutCrew licence: {e}"))?;
    if l.format != LICENCE_FORMAT {
        return Err(format!(
            "this licence is format {}, and this version reads format {LICENCE_FORMAT}",
            l.format
        ));
    }
    if l.holder.trim().is_empty() || l.plan.trim().is_empty() {
        return Err("this licence names no holder or plan".into());
    }
    if l.expires.is_some_and(|e| e <= now) {
        return Err("this licence has ended".into());
    }
    use base64::Engine;
    let sig = base64::engine::general_purpose::STANDARD
        .decode(&l.signature)
        .map_err(|_| "this licence's signature is not readable")?;
    if sig.len() != 64 {
        return Err("this licence's signature is the wrong length".into());
    }
    Ok(l)
}

/// [`check`] as a result: `Ok` when allowed, the reason otherwise.
pub fn require(feature: Feature) -> Result<(), String> {
    match check(feature, None) {
        Decision::Allowed => Ok(()),
        Decision::Refused(why) => Err(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(expires: &str, sig: &str) -> String {
        format!(
            r#"{{"format":1,"holder":"A tester","plan":"pro","features":["paid_models","templates"],"expires":{expires},"signature":"{sig}"}}"#
        )
    }

    #[test]
    fn every_feature_is_allowed_today_with_or_without_a_licence() {
        for f in [
            Feature::PaidModels,
            Feature::WebResearch,
            Feature::Templates,
            Feature::Connectors,
            Feature::ExcelOutput,
            Feature::ComputerUse,
        ] {
            assert_eq!(check(f, None), Decision::Allowed);
        }
    }

    fn signed(holder: &str, plan: &str) -> (String, [u8; 32]) {
        use base64::Engine;
        use ed25519_dalek::Signer;
        // A key made only for this test, from fixed bytes.
        let k = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let mut l = Licence {
            format: 1,
            holder: holder.into(),
            plan: plan.into(),
            features: vec![Feature::Templates],
            expires: None,
            signature: String::new(),
        };
        l.signature =
            base64::engine::general_purpose::STANDARD.encode(k.sign(&l.signed_bytes()).to_bytes());
        (
            serde_json::to_string(&l).unwrap(),
            k.verifying_key().to_bytes(),
        )
    }

    #[test]
    fn only_a_licence_signed_with_the_apps_key_and_unchanged_since_is_trusted() {
        let (text, key) = signed("A tester", "pro");
        assert_eq!(read_with(&text, 1, &key).unwrap().plan, "pro");
        // Editing the plan after it was signed breaks it.
        let edited = text.replace("\"pro\"", "\"team\"");
        assert!(read_with(&edited, 1, &key)
            .unwrap_err()
            .contains("changed after"));
        // A licence signed with another key is refused.
        let other = ed25519_dalek::SigningKey::from_bytes(&[9; 32])
            .verifying_key()
            .to_bytes();
        assert!(read_with(&text, 1, &other).is_err());
        // The app's own key is a usable key, and a licence signed with the test key is refused by it.
        let built_in = PUBLIC_KEY.expect("the app has a licence key");
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&built_in).is_ok());
        assert!(read(&text, 1).is_err());
    }

    #[test]
    fn a_licence_file_is_read_for_shape_and_an_ended_or_malformed_one_is_refused() {
        let sig = "A".repeat(86) + "==";
        let l = read_shape(&text("null", &sig), 100).unwrap();
        assert_eq!(l.features, [Feature::PaidModels, Feature::Templates]);
        let signed = String::from_utf8(l.signed_bytes()).unwrap();
        assert!(
            signed.contains("\"plan\":\"pro\"") && !signed.contains("signature"),
            "{signed}"
        );
        assert!(read_shape(&text("50", &sig), 100)
            .unwrap_err()
            .contains("ended"));
        assert!(read_shape(&text("null", "short"), 100).is_err());
        assert!(read_shape(
            r#"{"format":2,"holder":"a","plan":"b","features":[],"expires":null,"signature":""}"#,
            1
        )
        .unwrap_err()
        .contains("format 2"));
        assert!(read_shape(r#"{"format":1,"holder":"a","plan":"b","features":[],"expires":null,"signature":"","admin":true}"#, 1).is_err(), "unknown fields are refused");
    }
}
