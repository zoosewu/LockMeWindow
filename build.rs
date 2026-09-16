fn main() {
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new()
            .with_bundled_translations("lang")
            .with_default_translation_context(slint_build::DefaultTranslationContext::None),
    )
    .unwrap();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/window-warden.ico")
            .set("ProductName", "WindowWarden")
            .set("FileDescription", "WindowWarden")
            .set("CompanyName", "zoosewu")
            .set(
                "LegalCopyright",
                "Copyright (c) 2026 zoosewu. Based on AutoCursorLock, Copyright (c) 2020 James La Novara-Gsell. MIT License.",
            )
            .set("OriginalFilename", "window-warden.exe")
            .set("InternalName", "window-warden")
            .compile()
            .unwrap();
    }
    let manifest =
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed=assets/window-warden.ico");
    println!("cargo:rustc-link-arg-bin=window-warden=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=window-warden=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!(
        "cargo:rustc-link-arg-bin=window-warden=/MANIFESTUAC:level='asInvoker' uiAccess='false'"
    );
}
