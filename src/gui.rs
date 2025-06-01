use fltk::{app, browser::Browser, prelude::*, window::Window};
use crate::utils::list_image_files;
use std::env;

pub fn run_gui() {
    let app = app::App::default();
    let mut win = Window::new(100, 100, 400, 300, "Image List");

    let mut browser = Browser::new(20, 20, 360, 260, "");

    let current_dir = env::current_dir().unwrap();
    let files = list_image_files(&current_dir);

    for file in files {
        if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
            browser.add(name);
        }
    }

    win.end();
    win.show();
    app.run().unwrap();
}
