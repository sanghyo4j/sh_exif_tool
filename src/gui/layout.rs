use fltk::{
    app,
    button::Button,
    frame::Frame,
    input::Input,
    prelude::*,
    window::Window,
};

pub struct UiWidgets {
    pub input_path: Input,
    pub btn_browse: Button,
    pub btn_process: Button,
    pub output: Frame,
}

pub fn build_ui() -> (Window, UiWidgets) {
    let mut wind = Window::new(100, 100, 400, 200, "EXIF Tool");

    let input_path = Input::new(20, 20, 260, 30, "Directory:");
    let btn_browse = Button::new(290, 20, 90, 30, "Browse");
    let btn_process = Button::new(20, 70, 360, 40, "Start Processing");
    let output = Frame::new(20, 130, 360, 30, "");

    wind.end();

    let widgets = UiWidgets {
        input_path,
        btn_browse,
        btn_process,
        output,
    };

    (wind, widgets)
}
