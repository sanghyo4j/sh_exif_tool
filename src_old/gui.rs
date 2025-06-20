use fltk::{app, browser::Browser, frame::Frame, prelude::*, window::Window};
use crate::utils::list_image_files;
use std::env;

pub fn run_gui() {
    let app = app::App::default();
    let mut win = Window::new(100, 100, 500, 350, "Image List");

    let current_dir = env::current_dir().unwrap();
    let path_str = current_dir.to_string_lossy().to_string();

    // 상단 경로 표시용 프레임
    let mut path_label = Frame::new(20, 10, 460, 30, "");
    path_label.set_label(&path_str);
    path_label.set_label_size(12);

    // 이미지 파일 리스트
    let mut browser = Browser::new(20, 50, 460, 270, "");

    for file in list_image_files(&current_dir) {
        if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
            browser.add(name);
        }
    }

    win.end();
    win.show();
    app.run().unwrap();
}
