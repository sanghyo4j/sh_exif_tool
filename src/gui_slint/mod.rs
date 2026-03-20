pub mod app;

use slint::ComponentHandle;
use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;
use self::app::SlintApp;
use crate::GuiRunner;

slint::include_modules!();

pub struct SlintRunner;

impl GuiRunner for SlintRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let ui = MainWindow::new()?;
        let app = Rc::new(RefCell::new(SlintApp::new()));

        let refresh_ui = {
            let ui_handle = ui.as_weak();
            let app_handle = app.clone();
            move || {
                let ui = ui_handle.unwrap();
                let app = app_handle.borrow();
                ui.set_current_path(app.current_path.as_str().into());
                ui.set_files(app.get_ui_model());
                ui.set_selected_index(-1);
            }
        };

        refresh_ui();

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_reload(move || {
            app_handle.borrow_mut().load_folder();
            refresh();
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_open_dir(move |index| {
            let mut app = app_handle.borrow_mut();
            if let Some(entry) = app.files.get(index as usize) {
                if entry.is_dir {
                    app.current_path = entry.path.to_string_lossy().to_string();
                    app.load_folder();
                    refresh();
                }
            }
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_go_parent(move || {
            let mut app = app_handle.borrow_mut();
            if let Some(parent) = PathBuf::from(&app.current_path).parent() {
                app.current_path = parent.to_string_lossy().to_string();
                app.load_folder();
                refresh();
            }
        });

        ui.run()?;
        Ok(())
    }
}