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

        // 1. 초기 데이터 로드
        app.borrow_mut().load_folder();

        // UI 갱신 헬퍼 함수
        let refresh_ui = {
            let ui_handle = ui.as_weak();
            let app_handle = app.clone();
            move || {
                if let Some(ui) = ui_handle.upgrade() {
                    let app = app_handle.borrow();
                    ui.set_current_path(app.current_path.as_str().into());
                    ui.set_item_count(app.file_count() as i32);
                    ui.set_files(app.get_ui_model());
                    ui.set_selected_index(-1);
                    ui.set_selected_name("N/A".into());
                    ui.set_selected_created("N/A".into());
                    ui.set_selected_modified("N/A".into());
                    ui.set_selected_is_dir(false);
                }
            }
        };

        // 초기 실행
        refresh_ui();

        // 2. 새로고침 핸들러 (버튼 등)
        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        let refresh = refresh_ui.clone();
        ui.on_reload(move || {
            if let Some(ui) = ui_handle.upgrade() {
                {
                    let mut app = app_handle.borrow_mut();
                    app.current_path = ui.get_current_path().to_string();
                    app.load_folder();
                }
                refresh();
            }
        });

        // [추가] 2.5. 경로 직접 입력 핸들러 (엔터 입력 시)
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_change_dir(move |new_path| {
            {
                let mut app = app_handle.borrow_mut();
                app.current_path = new_path.to_string();
                app.load_folder();
            }
            refresh();
        });

        // 3. 폴더 진입 핸들러 (더블클릭)
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_open_dir(move |index| {
            let mut changed = false;
            {
                let mut app = app_handle.borrow_mut();
                let idx = index as usize;
                
                if idx == 0 { // [..] 클릭
                    if let Some(parent) = PathBuf::from(&app.current_path).parent() {
                        app.current_path = parent.to_string_lossy().to_string();
                        app.load_folder();
                        changed = true;
                    }
                } else if let Some(entry) = app.files.get(idx - 1) { // 폴더 클릭
                    if entry.is_dir {
                        app.current_path = entry.path.to_string_lossy().to_string();
                        app.load_folder();
                        changed = true;
                    }
                }
            }
            if changed {
                refresh();
            }
        });

        // 4. 상위 폴더 이동 핸들러
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_go_parent(move || {
            {
                let mut app = app_handle.borrow_mut();
                if let Some(parent) = PathBuf::from(&app.current_path).parent() {
                    app.current_path = parent.to_string_lossy().to_string();
                    app.load_folder();
                }
            }
            refresh();
        });

        let ui_handle = ui.as_weak();
        ui.on_cancel_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_selected_index(-1);
                ui.set_selected_name("N/A".into());
                ui.set_selected_created("N/A".into());
                ui.set_selected_modified("N/A".into());
                ui.set_selected_is_dir(false);
            }
        });

        ui.on_apply_changes(move || {
            // Metadata editing is UI-only until EXIF write support is implemented.
        });

        ui.run()?;
        Ok(())
    }
}
