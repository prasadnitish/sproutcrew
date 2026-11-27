fn main() {
    build_ocr_program();
    google_client();
}

/// Compiles `ocr/main.swift` into `$OUT_DIR/bff-ocr`, which `ocr.rs` carries inside the crate. Where there
/// is no macOS or no Swift compiler the file is left empty and BFF says it cannot read pictures.
fn build_ocr_program() {
    println!("cargo:rerun-if-changed=ocr/main.swift");
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("bff-ocr");
    let _ = std::fs::remove_file(&out);
    let mac = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos");
    let built = mac
        && std::process::Command::new("xcrun")
            .args(["swiftc", "-O", "ocr/main.swift", "-o"])
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success());
    if !built {
        println!("cargo:warning=text recognition for pictures was not built (needs macOS and Xcode's command line tools)");
        std::fs::write(&out, []).unwrap();
    }
}

/// Nitish's Google OAuth client for reading Google Docs, from a local file that is not committed.
fn google_client() {
    let path = "google-client.json";
    println!("cargo:rerun-if-changed={path}");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let v: serde_json::Value = serde_json::from_str(&text).expect("google-client.json is not JSON");
    for (key, env) in [
        ("client_id", "SPROUTCREW_GOOGLE_CLIENT_ID"),
        ("client_secret", "SPROUTCREW_GOOGLE_CLIENT_SECRET"),
    ] {
        if let Some(x) = v[key].as_str() {
            println!("cargo:rustc-env={env}={x}");
        }
    }
}
