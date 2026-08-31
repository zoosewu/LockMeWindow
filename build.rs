fn main() {
    let manifest =
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bin=lock-me-window=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=lock-me-window=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!(
        "cargo:rustc-link-arg-bin=lock-me-window=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'"
    );
}
