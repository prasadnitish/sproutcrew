#[path = "../examples/export_smoke.rs"]
mod smoke;

#[test]
fn every_export_uses_the_real_office_renderers_and_keeps_package_hashes() {
    // Typst itself is optional in development; Office unit fixtures always execute without it.
    if bff_core::clientfiles::Tools::detect("").typst.is_none() {
        return;
    }
    let base = std::env::temp_dir().join(format!("sprout-all-exports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    smoke::run(&base).unwrap();
}
