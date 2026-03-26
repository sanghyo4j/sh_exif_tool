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
                    ui.set_files(app.get_ui_model());
                    ui.set_selected_index(-1); // 폴더 이동 시 선택 초기화
                }
            }
        };

        // 초기 실행
        refresh_ui();

        // 2. 경로 입력 및 새로고침 핸들러
        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        let refresh = refresh_ui.clone();
        ui.on_reload(move || {
            if let Some(ui) = ui_handle.upgrade() {
                {
                    let mut app = app_handle.borrow_mut();
                    // UI의 LineEdit에 입력된 경로를 Rust app에 동기화
                    app.current_path = ui.get_current_path().to_string();
                    app.load_folder();
                }
                refresh();
                // 로드 완료 후 편집 모드 해제는 ui.slint 내부(accepted)에서 처리됨
            }
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

        // 4. 상위 폴더 이동 핸들러 (버튼 등에서 호출 시)
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

        // 5. [추가] 항목 선택 시 로직 (선택 사항)
        // 리스트에서 클릭만 했을 때 Rust 쪽에서 추가 작업(예: EXIF 미리 읽기)이 필요하다면 여기에 작성합니다.
        // 현재는 ui.slint 내부에서 selected_index를 관리하므로 비워두어도 무방합니다.

        ui.run()?;
        Ok(())
    }
}