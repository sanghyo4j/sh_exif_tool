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

        // UI 갱신 로직: 내부에서 borrow()를 수행함
        let refresh_ui = {
            let ui_handle = ui.as_weak();
            let app_handle = app.clone();
            move || {
                if let Some(ui) = ui_handle.upgrade() {
                    let app = app_handle.borrow();
                    ui.set_current_path(app.current_path.as_str().into());
                    ui.set_files(app.get_ui_model());
                    ui.set_selected_index(-1);
                }
            }
        };

        refresh_ui();

        // 1. 새로고침 핸들러
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_reload(move || {
            app_handle.borrow_mut().load_folder();
            // borrow_mut가 여기서 끝나야 하므로 refresh를 바로 부르지 않고 
            // scope를 분리하거나 아래처럼 호출 순서를 조정합니다.
            drop(app_handle.borrow_mut()); // 명시적으로 borrow 해제는 불가능하므로 구조적 분리 필요
        });
        
        // 실제로는 아래와 같이 scope 블록을 사용하여 해결합니다.

        // 2. 폴더 진입 핸들러
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_open_dir(move |index| {
            let mut changed = false;
            {
                let mut app = app_handle.borrow_mut();
                // [..] 아이템이 인덱스 0번에 추가되었으므로 실제 데이터는 index - 1
                let idx = index as usize;
                
                if idx == 0 {
                    // [..] 클릭 시 상위 폴더로
                    if let Some(parent) = PathBuf::from(&app.current_path).parent() {
                        app.current_path = parent.to_string_lossy().to_string();
                        app.load_folder();
                        changed = true;
                    }
                } else if let Some(entry) = app.files.get(idx - 1) {
                    // 실제 폴더 클릭 시
                    if entry.is_dir {
                        app.current_path = entry.path.to_string_lossy().to_string();
                        app.load_folder();
                        changed = true;
                    }
                }
            } // 여기서 borrow_mut가 해제됨 (중요)
            
            if changed {
                refresh();
            }
        });

        // 3. 상위 폴더 이동 핸들러
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_go_parent(move || {
            {
                let mut app = app_handle.borrow_mut();
                if let Some(parent) = PathBuf::from(&app.current_path).parent() {
                    app.current_path = parent.to_string_lossy().to_string();
                    app.load_folder();
                }
            } // 여기서 borrow_mut 해제
            refresh();
        });

        ui.run()?;
        Ok(())
    }
}