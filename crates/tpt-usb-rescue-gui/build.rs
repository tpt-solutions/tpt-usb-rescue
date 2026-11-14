//! Embeds the Common Controls v6 manifest into the GUI executable.
//!
//! Done through the MSVC linker (`/MANIFESTINPUT`), so no resource-compiler crate is needed.

fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
        let manifest = std::path::Path::new(&dir).join("app.manifest");
        println!("cargo:rustc-link-arg-bin=tpt-usb-rescue-gui=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=tpt-usb-rescue-gui=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
