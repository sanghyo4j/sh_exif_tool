use fltk::{
    prelude::*,
    input::Input,
    tree::Tree,
    browser::HoldBrowser,
    group::{Flex, FlexType},
    window::Window,
};

#[derive(Clone)]
pub struct UiWidgets {
    pub path_display: Input,
    pub file_tree: Tree,
    pub file_list: HoldBrowser,
}

pub fn build_ui() -> (Window, UiWidgets) {
    let mut wind = Window::new(100, 100, 800, 600, "");
    let mut root = Flex::default_fill().column();

    let path_display = Input::default().with_size(0, 30);
    let mut bottom = Flex::default().row();

    let file_tree = Tree::default().with_size(0, 0);
    let file_list = HoldBrowser::default().with_size(0, 0);
    let file_info = HoldBrowser::default().with_size(0, 0);

    bottom.set_size(&file_tree, 160);
    bottom.set_size(&file_list, 320);
    bottom.end();

    root.set_size(&path_display, 30);
    root.end();

    wind.end();
    wind.resizable(&root);

    let widgets = UiWidgets {
        path_display,
        file_tree,
        file_list,
    };

    (wind, widgets)
}
