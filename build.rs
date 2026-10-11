#[cfg(feature = "gui-slint")]
fn main() {
    std::thread::Builder::new()
        .name("slint-compiler".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| slint_build::compile("src/gui_slint/ui.slint"))
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
}

#[cfg(not(feature = "gui-slint"))]
fn main() {
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("app.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
