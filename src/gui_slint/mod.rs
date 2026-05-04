pub mod app;

use slint::{language::ColorScheme, ComponentHandle};
use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use self::app::SlintApp;
use crate::exif::{read_exif_metadata, ExifMetadata};
use crate::GuiRunner;

slint::include_modules!();

pub struct SlintRunner;

impl GuiRunner for SlintRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let ui = MainWindow::new()?;
        ui.global::<Palette>().set_color_scheme(ColorScheme::Light);
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
                    ui.set_table_rows(app.get_table_model());
                    ui.set_selected_index(-1);
                    ui.set_selected_name("N/A".into());
                    ui.set_selected_created("N/A".into());
                    ui.set_selected_modified("N/A".into());
                    ui.set_selected_is_dir(false);
                    ui.set_metadata_dirty(false);
                    set_exif_metadata(&ui, ExifMetadata::default());
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
        let last_table_click: Rc<RefCell<Option<(i32, Instant)>>> = Rc::new(RefCell::new(None));
        let last_table_click_handle = last_table_click.clone();
        ui.on_table_row_clicked(move |index| {
            let now = Instant::now();
            let is_double_click = {
                let mut last = last_table_click_handle.borrow_mut();
                let is_double = last
                    .map(|(last_index, last_time)| {
                        last_index == index && now.duration_since(last_time) <= Duration::from_millis(500)
                    })
                    .unwrap_or(false);
                *last = Some((index, now));
                is_double
            };

            if !is_double_click {
                return;
            }

            let mut changed = false;
            {
                let mut app = app_handle.borrow_mut();
                let idx = index as usize;

                if idx == 0 {
                    if let Some(parent) = PathBuf::from(&app.current_path).parent() {
                        app.current_path = parent.to_string_lossy().to_string();
                        app.load_folder();
                        changed = true;
                    }
                } else if let Some(entry) = app.files.get(idx - 1) {
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

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        ui.on_table_row_selected(move |index| {
            if let Some(ui) = ui_handle.upgrade() {
                let metadata = {
                    let app = app_handle.borrow();
                    let Some(path) = app.path_for_ui_index(index) else {
                        return set_exif_metadata(&ui, ExifMetadata::default());
                    };
                    if path.is_file() {
                        read_exif_metadata(&path)
                    } else {
                        ExifMetadata::default()
                    }
                };
                set_exif_metadata(&ui, metadata);
            }
        });

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
                ui.set_metadata_dirty(false);
                set_exif_metadata(&ui, ExifMetadata::default());
            }
        });

        let ui_handle = ui.as_weak();
        ui.on_clear_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_selected_name("N/A".into());
                ui.set_selected_created("N/A".into());
                ui.set_selected_modified("N/A".into());
                ui.set_selected_is_dir(false);
                ui.set_metadata_dirty(false);
                set_exif_metadata(&ui, ExifMetadata::default());
            }
        });

        ui.on_apply_changes(move || {
            // Metadata editing is UI-only until EXIF write support is implemented.
        });

        ui.run()?;
        Ok(())
    }
}

fn set_exif_metadata(ui: &MainWindow, metadata: ExifMetadata) {
    ui.set_taken_date(metadata.taken_date.into());
    ui.set_camera_make(metadata.camera_make.into());
    ui.set_camera_model(metadata.camera_model.into());
    ui.set_lens_model(metadata.lens_model.into());
    ui.set_software(metadata.software.into());
    ui.set_artist(metadata.artist.into());
    ui.set_shutter_speed(metadata.shutter_speed.into());
    ui.set_aperture(metadata.aperture.into());
    ui.set_iso_speed(metadata.iso_speed.into());
    ui.set_focal_length(metadata.focal_length.into());
    ui.set_flash_fired(metadata.flash_fired.into());
    ui.set_metering_mode(metadata.metering_mode.into());
    ui.set_image_width(metadata.image_width.into());
    ui.set_image_height(metadata.image_height.into());
    ui.set_orientation(metadata.orientation.into());
    ui.set_color_space(metadata.color_space.into());
    ui.set_gps_latitude(metadata.gps_latitude.into());
    ui.set_gps_longitude(metadata.gps_longitude.into());
    ui.set_gps_altitude(metadata.gps_altitude.into());
}
