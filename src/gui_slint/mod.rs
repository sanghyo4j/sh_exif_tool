pub mod app;

use slint::{language::ColorScheme, ComponentHandle};
use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use self::app::SlintApp;
use crate::exif::{extract_datetime_from_filename, read_exif_metadata, ExifMetadata};
use crate::fs::{rename_entry, save_file_copy};
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
                    ui.set_original_selected_name("N/A".into());
                    ui.set_original_selected_created("N/A".into());
                    ui.set_original_selected_modified("N/A".into());
                    ui.set_selected_is_dir(false);
                    ui.set_metadata_dirty(false);
                    reset_metadata_dirty_flags(&ui);
                    set_loaded_exif_metadata(&ui, ExifMetadata::default());
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
                *last_table_click_handle.borrow_mut() = None;
                refresh();
            }
        });

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        ui.on_table_row_selected(move |index| {
            if let Some(ui) = ui_handle.upgrade() {
                let metadata = {
                    let app = app_handle.borrow();
                    if let Some((name, created, modified, is_dir)) = app.ui_details_for_index(index) {
                        ui.set_selected_name(name.clone().into());
                        ui.set_selected_created(created.clone().into());
                        ui.set_selected_modified(modified.clone().into());
                        ui.set_original_selected_name(name.into());
                        ui.set_original_selected_created(created.into());
                        ui.set_original_selected_modified(modified.into());
                        ui.set_selected_is_dir(is_dir);
                    }

                    let Some(path) = app.path_for_ui_index(index) else {
                        return set_loaded_exif_metadata(&ui, ExifMetadata::default());
                    };
                    if path.is_file() {
                        read_exif_metadata(&path)
                    } else {
                        ExifMetadata::default()
                    }
                };
                set_loaded_exif_metadata(&ui, metadata);
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
        ui.on_remove_exif_tags(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_index() >= 0 && !ui.get_selected_is_dir() {
                    ui.set_metadata_dirty(true);
                    set_exif_metadata(&ui, ExifMetadata::default());
                    update_metadata_dirty_state(&ui);
                }
            }
        });

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        ui.on_fill_taken_date_from_filename(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_index() < 0 || ui.get_selected_is_dir() {
                    show_message(&ui, "Unable to Fill Taken Date", "Select a file before filling Taken Date.");
                    return;
                }

                let path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };

                let Some(path) = path else {
                    show_message(&ui, "Unable to Fill Taken Date", "Selected file could not be resolved.");
                    return;
                };

                let Some(datetime) = extract_datetime_from_filename(&path) else {
                    show_message(&ui, "Unable to Fill Taken Date", "No supported date pattern was found in the filename.");
                    return;
                };

                ui.set_taken_date(datetime.into());
                update_metadata_dirty_state(&ui);
            }
        });

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        ui.on_revert_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let metadata = {
                    let app = app_handle.borrow();
                    let index = ui.get_selected_index();

                    if let Some((name, created, modified, is_dir)) = app.ui_details_for_index(index) {
                        ui.set_selected_name(name.into());
                        ui.set_selected_created(created.into());
                        ui.set_selected_modified(modified.into());
                        ui.set_selected_is_dir(is_dir);
                    }

                    let Some(path) = app.path_for_ui_index(index) else {
                        return set_loaded_exif_metadata(&ui, ExifMetadata::default());
                    };
                    if path.is_file() {
                        read_exif_metadata(&path)
                    } else {
                        ExifMetadata::default()
                    }
                };
                set_loaded_exif_metadata(&ui, metadata);
                ui.set_metadata_dirty(false);
                reset_metadata_dirty_flags(&ui);
            }
        });

        let ui_handle = ui.as_weak();
        ui.on_metadata_value_edited(move || {
            if let Some(ui) = ui_handle.upgrade() {
                update_metadata_dirty_state(&ui);
            }
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_save_copy(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_index() < 0 || ui.get_selected_is_dir() {
                    show_message(&ui, "Save Copy Failed", "Select a file before saving a copy.");
                    return;
                }

                let result = {
                    let mut app = app_handle.borrow_mut();
                    let Some(path) = app.path_for_ui_index(ui.get_selected_index()) else {
                        show_message(&ui, "Save Copy Failed", "Selected file could not be resolved.");
                        return;
                    };

                    save_file_copy(&path).map(|target_path| {
                        app.load_folder();
                        target_path
                    })
                };

                match result {
                    Ok(_) => {
                        refresh();
                    }
                    Err(err) => show_message(&ui, "Save Copy Failed", &err),
                }
            }
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_apply_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_name_dirty() {
                    let new_name = ui.get_selected_name().trim().to_string();

                    let result = {
                        let mut app = app_handle.borrow_mut();
                        let Some(current_path) = app.path_for_ui_index(ui.get_selected_index()) else {
                            show_message(&ui, "Rename Failed", "Selected file could not be resolved.");
                            return;
                        };

                        rename_entry(&current_path, &new_name).map(|_| {
                            app.load_folder();
                        })
                    };

                    if let Err(err) = result {
                        show_message(&ui, "Rename Failed", &format!("{err}"));
                        return;
                    }

                    refresh();
                    return;
                }

                // Metadata editing is UI-only until EXIF write support is implemented.
                store_current_as_original(&ui);
                update_metadata_dirty_state(&ui);
                {
                    let mut app = app_handle.borrow_mut();
                    app.load_folder();
                }
                refresh();
            }
        });

        ui.run()?;
        Ok(())
    }
}

fn set_exif_metadata(ui: &MainWindow, metadata: ExifMetadata) {
    ui.set_exif_available(metadata.has_exif);
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

fn set_loaded_exif_metadata(ui: &MainWindow, metadata: ExifMetadata) {
    set_exif_metadata(ui, metadata.clone());
    ui.set_original_taken_date(metadata.taken_date.into());
    ui.set_original_camera_make(metadata.camera_make.into());
    ui.set_original_camera_model(metadata.camera_model.into());
    ui.set_original_lens_model(metadata.lens_model.into());
    ui.set_original_software(metadata.software.into());
    ui.set_original_artist(metadata.artist.into());
    ui.set_original_shutter_speed(metadata.shutter_speed.into());
    ui.set_original_aperture(metadata.aperture.into());
    ui.set_original_iso_speed(metadata.iso_speed.into());
    ui.set_original_focal_length(metadata.focal_length.into());
    ui.set_original_flash_fired(metadata.flash_fired.into());
    ui.set_original_metering_mode(metadata.metering_mode.into());
    ui.set_original_image_width(metadata.image_width.into());
    ui.set_original_image_height(metadata.image_height.into());
    ui.set_original_orientation(metadata.orientation.into());
    ui.set_original_color_space(metadata.color_space.into());
    ui.set_original_gps_latitude(metadata.gps_latitude.into());
    ui.set_original_gps_longitude(metadata.gps_longitude.into());
    ui.set_original_gps_altitude(metadata.gps_altitude.into());
    ui.set_metadata_dirty(false);
    reset_metadata_dirty_flags(ui);
}

fn show_message(ui: &MainWindow, title: &str, message: &str) {
    ui.set_message_title(title.into());
    ui.set_message_text(message.into());
    ui.set_message_visible(true);
}

fn reset_metadata_dirty_flags(ui: &MainWindow) {
    ui.set_selected_name_dirty(false);
    ui.set_selected_created_dirty(false);
    ui.set_selected_modified_dirty(false);
    ui.set_taken_date_dirty(false);
    ui.set_camera_make_dirty(false);
    ui.set_camera_model_dirty(false);
    ui.set_lens_model_dirty(false);
    ui.set_software_dirty(false);
    ui.set_artist_dirty(false);
    ui.set_shutter_speed_dirty(false);
    ui.set_aperture_dirty(false);
    ui.set_iso_speed_dirty(false);
    ui.set_focal_length_dirty(false);
    ui.set_flash_fired_dirty(false);
    ui.set_metering_mode_dirty(false);
    ui.set_image_width_dirty(false);
    ui.set_image_height_dirty(false);
    ui.set_orientation_dirty(false);
    ui.set_color_space_dirty(false);
    ui.set_gps_latitude_dirty(false);
    ui.set_gps_longitude_dirty(false);
    ui.set_gps_altitude_dirty(false);
}

fn store_current_as_original(ui: &MainWindow) {
    ui.set_original_selected_name(ui.get_selected_name());
    ui.set_original_selected_created(ui.get_selected_created());
    ui.set_original_selected_modified(ui.get_selected_modified());
    ui.set_original_taken_date(ui.get_taken_date());
    ui.set_original_camera_make(ui.get_camera_make());
    ui.set_original_camera_model(ui.get_camera_model());
    ui.set_original_lens_model(ui.get_lens_model());
    ui.set_original_software(ui.get_software());
    ui.set_original_artist(ui.get_artist());
    ui.set_original_shutter_speed(ui.get_shutter_speed());
    ui.set_original_aperture(ui.get_aperture());
    ui.set_original_iso_speed(ui.get_iso_speed());
    ui.set_original_focal_length(ui.get_focal_length());
    ui.set_original_flash_fired(ui.get_flash_fired());
    ui.set_original_metering_mode(ui.get_metering_mode());
    ui.set_original_image_width(ui.get_image_width());
    ui.set_original_image_height(ui.get_image_height());
    ui.set_original_orientation(ui.get_orientation());
    ui.set_original_color_space(ui.get_color_space());
    ui.set_original_gps_latitude(ui.get_gps_latitude());
    ui.set_original_gps_longitude(ui.get_gps_longitude());
    ui.set_original_gps_altitude(ui.get_gps_altitude());
}

fn update_metadata_dirty_state(ui: &MainWindow) {
    let selected_name_dirty = ui.get_selected_name() != ui.get_original_selected_name();
    let selected_created_dirty = ui.get_selected_created() != ui.get_original_selected_created();
    let selected_modified_dirty = ui.get_selected_modified() != ui.get_original_selected_modified();
    let taken_date_dirty = ui.get_taken_date() != ui.get_original_taken_date();
    let camera_make_dirty = ui.get_camera_make() != ui.get_original_camera_make();
    let camera_model_dirty = ui.get_camera_model() != ui.get_original_camera_model();
    let lens_model_dirty = ui.get_lens_model() != ui.get_original_lens_model();
    let software_dirty = ui.get_software() != ui.get_original_software();
    let artist_dirty = ui.get_artist() != ui.get_original_artist();
    let shutter_speed_dirty = ui.get_shutter_speed() != ui.get_original_shutter_speed();
    let aperture_dirty = ui.get_aperture() != ui.get_original_aperture();
    let iso_speed_dirty = ui.get_iso_speed() != ui.get_original_iso_speed();
    let focal_length_dirty = ui.get_focal_length() != ui.get_original_focal_length();
    let flash_fired_dirty = ui.get_flash_fired() != ui.get_original_flash_fired();
    let metering_mode_dirty = ui.get_metering_mode() != ui.get_original_metering_mode();
    let image_width_dirty = ui.get_image_width() != ui.get_original_image_width();
    let image_height_dirty = ui.get_image_height() != ui.get_original_image_height();
    let orientation_dirty = ui.get_orientation() != ui.get_original_orientation();
    let color_space_dirty = ui.get_color_space() != ui.get_original_color_space();
    let gps_latitude_dirty = ui.get_gps_latitude() != ui.get_original_gps_latitude();
    let gps_longitude_dirty = ui.get_gps_longitude() != ui.get_original_gps_longitude();
    let gps_altitude_dirty = ui.get_gps_altitude() != ui.get_original_gps_altitude();

    ui.set_selected_name_dirty(selected_name_dirty);
    ui.set_selected_created_dirty(selected_created_dirty);
    ui.set_selected_modified_dirty(selected_modified_dirty);
    ui.set_taken_date_dirty(taken_date_dirty);
    ui.set_camera_make_dirty(camera_make_dirty);
    ui.set_camera_model_dirty(camera_model_dirty);
    ui.set_lens_model_dirty(lens_model_dirty);
    ui.set_software_dirty(software_dirty);
    ui.set_artist_dirty(artist_dirty);
    ui.set_shutter_speed_dirty(shutter_speed_dirty);
    ui.set_aperture_dirty(aperture_dirty);
    ui.set_iso_speed_dirty(iso_speed_dirty);
    ui.set_focal_length_dirty(focal_length_dirty);
    ui.set_flash_fired_dirty(flash_fired_dirty);
    ui.set_metering_mode_dirty(metering_mode_dirty);
    ui.set_image_width_dirty(image_width_dirty);
    ui.set_image_height_dirty(image_height_dirty);
    ui.set_orientation_dirty(orientation_dirty);
    ui.set_color_space_dirty(color_space_dirty);
    ui.set_gps_latitude_dirty(gps_latitude_dirty);
    ui.set_gps_longitude_dirty(gps_longitude_dirty);
    ui.set_gps_altitude_dirty(gps_altitude_dirty);

    ui.set_metadata_dirty(
        selected_name_dirty
            || selected_created_dirty
            || selected_modified_dirty
            || taken_date_dirty
            || camera_make_dirty
            || camera_model_dirty
            || lens_model_dirty
            || software_dirty
            || artist_dirty
            || shutter_speed_dirty
            || aperture_dirty
            || iso_speed_dirty
            || focal_length_dirty
            || flash_fired_dirty
            || metering_mode_dirty
            || image_width_dirty
            || image_height_dirty
            || orientation_dirty
            || color_space_dirty
            || gps_latitude_dirty
            || gps_longitude_dirty
            || gps_altitude_dirty,
    );
}
