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
