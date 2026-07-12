pub mod app;

use chrono::{Local, NaiveDateTime, TimeZone};
use slint::{language::ColorScheme, ComponentHandle};
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use self::app::SlintApp;
use crate::exif::{
    extract_datetime_from_filename,
    is_generated_new_exif_path,
    read_exif_metadata,
    remove_gps_information,
    rewrite_basic_exif_metadata,
    rewrite_generated_basic_exif_metadata,
    write_aperture,
    write_artist,
    write_camera_make,
    write_camera_model,
    write_color_space,
    write_flash_fired,
    write_focal_length,
    write_iso_speed,
    write_lens_model,
    write_metering_mode,
    write_orientation,
    write_shutter_speed,
    write_software,
    write_taken_date,
    ExifMetadata,
};
use crate::fs::{copy_file_times, copy_file_to_folder, move_file_to_recycle_bin, open_in_file_manager, rename_entry, save_file_copy, set_file_times};
use crate::GuiRunner;

slint::include_modules!();

pub struct SlintRunner;

impl GuiRunner for SlintRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let ui = MainWindow::new()?;
        ui.global::<Palette>().set_color_scheme(ColorScheme::Light);
        let app = Rc::new(RefCell::new(SlintApp::new()));
        let copied_files = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
        let pending_filename_taken_dates = Rc::new(RefCell::new(HashMap::<PathBuf, String>::new()));
        let pending_modified_dates = Rc::new(RefCell::new(HashMap::<PathBuf, SystemTime>::new()));

        // 1. 초기 데이터 로드
        app.borrow_mut().load_folder();

        // UI 갱신 헬퍼 함수
        let refresh_ui = {
            let ui_handle = ui.as_weak();
            let app_handle = app.clone();
            move |selected_path: Option<PathBuf>| {
                if let Some(ui) = ui_handle.upgrade() {
                    let mut app = app_handle.borrow_mut();
                    let selected_index = selected_path
                        .as_deref()
                        .and_then(|path| app.ui_index_for_path(path))
                        .unwrap_or(-1);
                    if selected_index >= 0 {
                        app.select_ui_index(selected_index, false, false);
                    }
                    ui.set_current_path(app.current_path.as_str().into());
                    ui.set_item_count(app.file_count() as i32);
                    ui.set_files(app.get_ui_model());
                    ui.set_table_rows(app.get_table_model());
                    set_selected_files(&ui, &app);
                }
            }
        };

        // 초기 실행
        refresh_ui(None);

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_set_show_only_supported_images(move |enabled| {
            {
                let mut app = app_handle.borrow_mut();
                app.show_only_supported_images = enabled;
                app.selected_indices.clear();
            }
            refresh(None);
        });

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        let refresh = refresh_ui.clone();
        ui.on_set_show_extension_in_file_name(move |_| {
            if let Some(ui) = ui_handle.upgrade() {
                let selected_path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };
                refresh(selected_path);
            }
        });

        // 2. 새로고침 핸들러 (버튼 등)
        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        let refresh = refresh_ui.clone();
        ui.on_reload(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let selected_path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };
                {
                    let mut app = app_handle.borrow_mut();
                    app.current_path = ui.get_current_path().to_string();
                    app.load_folder();
                }
                refresh(selected_path);
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
            refresh(None);
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
                refresh(None);
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
                refresh(None);
            }
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_table_row_selected(move |index, ctrl, shift| {
            if let Some(ui) = ui_handle.upgrade() {
                pending_taken_dates.borrow_mut().clear();
                pending_modified_dates_handle.borrow_mut().clear();
                let mut app = app_handle.borrow_mut();
                app.select_ui_index(index, ctrl, shift);
                ui.set_selected_index(index);
                ui.set_files(app.get_ui_model());
                set_selected_files(&ui, &app);
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
            refresh(None);
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
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_create_exif_structure(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_index() < 0 || ui.get_selected_is_dir() {
                    show_message(&ui, "Create EXIF Failed", "Select a file before creating EXIF.");
                    return;
                }

                let path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };

                let Some(path) = path else {
                    show_message(&ui, "Create EXIF Failed", "Selected file could not be resolved.");
                    return;
                };

                if read_exif_metadata(&path).has_exif {
                    show_message(&ui, "Create EXIF", "EXIF is already available.");
                    return;
                }

                let metadata = collect_current_exif_metadata(&ui);
                let new_path = match rewrite_basic_exif_metadata(&path, &metadata) {
                    Ok(path) => path,
                    Err(err) => {
                        show_message(&ui, "Create EXIF Failed", &err);
                        return;
                    }
                };
                if let Err(err) = copy_file_times(&path, &new_path) {
                    show_message(&ui, "Create EXIF Failed", &err);
                    return;
                }

                {
                    let mut app = app_handle.borrow_mut();
                    app.load_folder();
                }
                refresh(Some(new_path.clone()));
                show_message(
                    &ui,
                    "EXIF File Created",
                    &format!("Created {}", new_path.display()),
                );
            }
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_fill_taken_date_from_filename(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_file_count() == 0 {
                    show_message(&ui, "Unable to Set Taken Date", "Select a file before setting Taken Date.");
                    return;
                }

                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };

                if selected_paths.is_empty() {
                    show_message(&ui, "Unable to Set Taken Date", "Selected file could not be resolved.");
                    return;
                }

                if selected_paths.len() == 1 {
                    let path = &selected_paths[0];
                    let Some(datetime) = extract_datetime_from_filename(path) else {
                        show_message(&ui, "Unable to Set Taken Date", "No supported date pattern was found in the filename.");
                        return;
                    };
                    if !should_apply_taken_date_candidate(ui.get_taken_date().as_str(), &datetime) {
                        show_message(&ui, "Taken Date Ignored", "Filename timestamp is later than the existing Taken Date.");
                        return;
                    }

                    pending_taken_dates.borrow_mut().clear();
                    ui.set_taken_date(datetime.into());
                    update_metadata_dirty_state(&ui);
                    return;
                }

                let mut pending = HashMap::new();
                for path in &selected_paths {
                    if let Some(datetime) = extract_datetime_from_filename(path) {
                        let existing_taken_date = read_exif_metadata(path).taken_date;
                        if should_apply_taken_date_candidate(&existing_taken_date, &datetime) {
                            pending.insert(path.clone(), datetime);
                        }
                    }
                }

                if pending.is_empty() {
                    show_message(&ui, "Unable to Set Taken Date", "No supported date pattern was found in the selected filenames.");
                    return;
                }

                let parsed_count = pending.len();
                let skipped_count = selected_paths.len().saturating_sub(parsed_count);
                *pending_taken_dates.borrow_mut() = pending;

                ui.set_taken_date("".into());
                ui.set_taken_date_status("Mixed".into());
                ui.set_taken_date_dirty(true);
                ui.set_metadata_dirty(true);

                if skipped_count == 0 {
                    show_message(&ui, "Taken Date Staged", &format!("Taken Date was parsed for {parsed_count} files. Save to apply."));
                } else {
                    show_message(&ui, "Taken Date Staged", &format!("Taken Date was parsed for {parsed_count} files. {skipped_count} files could not be parsed. Save to apply."));
                }
            }
        });

        let ui_handle = ui.as_weak();
        ui.on_fill_taken_date_from_created_date(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_index() < 0 || ui.get_selected_is_dir() {
                    show_message(&ui, "Unable to Set Taken Date", "Select a file before setting Taken Date.");
                    return;
                }

                let created = ui.get_selected_created().to_string();
                let modified = ui.get_selected_modified().to_string();

                let created_time = match parse_timestamp(&created) {
                    Ok(value) => value,
                    Err(err) => {
                        show_message(&ui, "Unable to Set Taken Date", &err);
                        return;
                    }
                };

                let modified_time = match parse_timestamp(&modified) {
                    Ok(value) => value,
                    Err(err) => {
                        show_message(&ui, "Unable to Set Taken Date", &err);
                        return;
                    }
                };

                let timestamp = if modified_time < created_time {
                    modified
                } else {
                    created
                };
                if !should_apply_taken_date_candidate(ui.get_taken_date().as_str(), &timestamp) {
                    show_message(&ui, "Taken Date Ignored", "File timestamp is later than the existing Taken Date.");
                    return;
                }
                ui.set_taken_date(timestamp.into());
                update_metadata_dirty_state(&ui);
            }
        });

        let app_handle = app.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_set_modified_date_from_created_date(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_file_count() == 0 {
                    show_message(&ui, "Unable to Set Modified Date", "Select files before setting Modified Date.");
                    return;
                }
                if ui.get_selected_recyclable_count() != ui.get_selected_file_count() {
                    show_message(&ui, "Unable to Set Modified Date", "Please select files only.");
                    return;
                }

                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };
                if selected_paths.is_empty() {
                    show_message(&ui, "Unable to Set Modified Date", "Selected files could not be resolved.");
                    return;
                }

                if selected_paths.len() == 1 {
                    let created = ui.get_selected_created().to_string();
                    if parse_timestamp(&created).is_err() {
                        show_message(&ui, "Unable to Set Modified Date", "Selected file created date could not be resolved.");
                        return;
                    }
                    pending_modified_dates_handle.borrow_mut().clear();
                    ui.set_selected_modified(created.into());
                    update_metadata_dirty_state(&ui);
                    return;
                }

                let mut pending = HashMap::new();
                for path in &selected_paths {
                    if let Ok(metadata) = path.metadata() {
                        if let Ok(created) = metadata.created() {
                            pending.insert(path.clone(), created);
                        }
                    }
                }

                if pending.is_empty() {
                    show_message(&ui, "Unable to Set Modified Date", "Selected file created dates could not be resolved.");
                    return;
                }

                let staged_count = pending.len();
                let skipped_count = selected_paths.len().saturating_sub(staged_count);
                *pending_modified_dates_handle.borrow_mut() = pending;
                ui.set_selected_modified("".into());
                ui.set_selected_modified_status("Mixed".into());
                ui.set_selected_modified_dirty(true);
                ui.set_metadata_dirty(true);

                if skipped_count == 0 {
                    show_message(&ui, "Modified Date Staged", &format!("Modified Date was staged for {staged_count} files. Save to apply."));
                } else {
                    show_message(&ui, "Modified Date Staged", &format!("Modified Date was staged for {staged_count} files. {skipped_count} files could not be staged. Save to apply."));
                }
            }
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_revert_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                pending_taken_dates.borrow_mut().clear();
                pending_modified_dates_handle.borrow_mut().clear();
                let metadata = {
                    let app = app_handle.borrow();
                    let index = ui.get_selected_index();

                    if let Some((name, created, modified, is_dir)) = app.ui_details_for_index(index) {
                        let display_name = app
                            .path_for_ui_index(index)
                            .map(|path| display_file_name(&path, &name, is_dir, ui.get_show_extension_in_file_name()))
                            .unwrap_or(name);
                        ui.set_selected_name(display_name.into());
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

        let ui_handle = ui.as_weak();
        ui.on_exit_app(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let _ = ui.hide();
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

                let selected_path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };

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
                        refresh(selected_path);
                    }
                    Err(err) => show_message(&ui, "Save Copy Failed", &err),
                }
            }
        });

        let app_handle = app.clone();
        let copied_files_handle = copied_files.clone();
        let ui_handle = ui.as_weak();
        ui.on_copy_files(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_recyclable_count() == 0 {
                    show_message(&ui, "Copy Failed", "Select files before copying.");
                    return;
                }
                if ui.get_selected_recyclable_count() != ui.get_selected_file_count() {
                    show_message(&ui, "Copy Failed", "Please select files only.");
                    return;
                }

                let paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };
                if paths.is_empty() {
                    show_message(&ui, "Copy Failed", "Selected files could not be resolved.");
                    return;
                }

                *copied_files_handle.borrow_mut() = paths;
                ui.set_copied_file_count(copied_files_handle.borrow().len() as i32);
            }
        });

        let app_handle = app.clone();
        let copied_files_handle = copied_files.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_paste_files(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let copied_paths = copied_files_handle.borrow().clone();
                if copied_paths.is_empty() {
                    show_message(&ui, "Paste Failed", "Copy files before pasting.");
                    return;
                }

                let result = (|| {
                    let mut app = app_handle.borrow_mut();
                    let target_dir = PathBuf::from(&app.current_path);

                    let mut last_target_path = None;
                    for path in copied_paths {
                        last_target_path = Some(copy_file_to_folder(&path, &target_dir)?);
                    }

                    app.load_folder();
                    Ok::<Option<PathBuf>, String>(last_target_path)
                })();

                match result {
                    Ok(target_path) => refresh(target_path),
                    Err(err) => show_message(&ui, "Paste Failed", &err),
                }
            }
        });

        let ui_handle = ui.as_weak();
        ui.on_confirm_delete_file(move || {
            let ui_handle = ui_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle.upgrade() {
                    ui.invoke_delete_file();
                }
            });
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_delete_file(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_recyclable_count() == 0 {
                    show_message(&ui, "Delete Failed", "Select a file or folder before deleting.");
                    return;
                }

                let result = (|| {
                    let mut app = app_handle.borrow_mut();
                    let paths = selected_recyclable_paths(&app);
                    if paths.is_empty() {
                        show_message(&ui, "Delete Failed", "Selected file could not be resolved.");
                        return Ok(());
                    }

                    for path in paths {
                        move_file_to_recycle_bin(&path)?;
                    }

                    app.load_folder();
                    Ok::<(), String>(())
                })();

                match result {
                    Ok(_) => refresh(None),
                    Err(err) => show_message(&ui, "Delete Failed", &err),
                }
            }
        });

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        ui.on_open_in_explorer(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let path = {
                    let app = app_handle.borrow();
                    PathBuf::from(&app.current_path)
                };

                if let Err(err) = open_in_file_manager(&path) {
                    show_message(&ui, "Open in Explorer Failed", &err);
                }
            }
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_apply_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_name_dirty() {
                    let requested_name = ui.get_selected_name().trim().to_string();

                    let result = {
                        let mut app = app_handle.borrow_mut();
                        let Some(current_path) = app.path_for_ui_index(ui.get_selected_index()) else {
                            show_message(&ui, "Rename Failed", "Selected file could not be resolved.");
                            return;
                        };
                        let new_name = rename_name_preserving_extension(&current_path, &requested_name);

                        rename_entry(&current_path, &new_name).map(|new_path| {
                            app.load_folder();
                            new_path
                        })
                    };

                    match result {
                        Ok(new_path) => refresh(Some(new_path)),
                        Err(err) => show_message(&ui, "Rename Failed", &format!("{err}")),
                    }
                    return;
                }

                let selected_path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };

                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };

                if selected_paths.len() > 1 {
                    let mut refresh_path = selected_path.clone();
                    let pending_taken_dates_snapshot = pending_taken_dates.borrow().clone();
                    let pending_modified_dates_snapshot = pending_modified_dates_handle.borrow().clone();
                    for path in &selected_paths {
                        match apply_metadata_changes_to_path(&ui, path, Some(&pending_taken_dates_snapshot), Some(&pending_modified_dates_snapshot)) {
                            Ok(Some(new_path)) => {
                                refresh_path = Some(new_path);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        }
                    }

                    store_current_as_original(&ui);
                    pending_taken_dates.borrow_mut().clear();
                    pending_modified_dates_handle.borrow_mut().clear();
                    update_metadata_dirty_state(&ui);
                    {
                        let mut app = app_handle.borrow_mut();
                        app.load_folder();
                    }
                    refresh(refresh_path);
                    return;
                }

                let apply_path = selected_path.clone();
                if has_exif_metadata_changes(&ui) {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };

                    let current_metadata = read_exif_metadata(path);
                    if !current_metadata.has_exif {
                        let metadata = collect_current_exif_metadata(&ui);
                        let new_path = match rewrite_basic_exif_metadata(path, &metadata) {
                            Ok(path) => path,
                            Err(err) => {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        };
                        if let Err(err) = copy_file_times(path, &new_path) {
                            show_message(&ui, "Apply Failed", &err);
                            return;
                        }

                        if ui.get_selected_created_dirty() || ui.get_selected_modified_dirty() {
                            let created_time = if ui.get_selected_created_dirty() {
                                match parse_timestamp(ui.get_selected_created().as_str()) {
                                    Ok(value) => Some(value),
                                    Err(err) => {
                                        show_message(&ui, "Apply Failed", &err);
                                        return;
                                    }
                                }
                            } else {
                                None
                            };

                            let modified_time = if ui.get_selected_modified_dirty() {
                                match parse_timestamp(ui.get_selected_modified().as_str()) {
                                    Ok(value) => Some(value),
                                    Err(err) => {
                                        show_message(&ui, "Apply Failed", &err);
                                        return;
                                    }
                                }
                            } else {
                                None
                            };

                            if let Err(err) = set_file_times(&new_path, created_time, modified_time) {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        }

                        store_current_as_original(&ui);
                        update_metadata_dirty_state(&ui);
                        {
                            let mut app = app_handle.borrow_mut();
                            app.load_folder();
                        }
                        refresh(Some(new_path.clone()));
                        show_message(
                            &ui,
                            "EXIF File Created",
                            &format!("Created {}", new_path.display()),
                        );
                        return;
                    }

                    if is_generated_new_exif_path(path) {
                        let metadata = collect_current_exif_metadata(&ui);
                        if let Err(err) = rewrite_generated_basic_exif_metadata(path, &metadata) {
                            show_message(&ui, "Apply Failed", &err);
                            return;
                        }

                        if ui.get_selected_created_dirty() || ui.get_selected_modified_dirty() {
                            let created_time = if ui.get_selected_created_dirty() {
                                match parse_timestamp(ui.get_selected_created().as_str()) {
                                    Ok(value) => Some(value),
                                    Err(err) => {
                                        show_message(&ui, "Apply Failed", &err);
                                        return;
                                    }
                                }
                            } else {
                                None
                            };

                            let modified_time = if ui.get_selected_modified_dirty() {
                                match parse_timestamp(ui.get_selected_modified().as_str()) {
                                    Ok(value) => Some(value),
                                    Err(err) => {
                                        show_message(&ui, "Apply Failed", &err);
                                        return;
                                    }
                                }
                            } else {
                                None
                            };

                            if let Err(err) = set_file_times(path, created_time, modified_time) {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        }

                        store_current_as_original(&ui);
                        update_metadata_dirty_state(&ui);
                        {
                            let mut app = app_handle.borrow_mut();
                            app.load_folder();
                        }
                        refresh(selected_path);
                        return;
                    }
                }

                if ui.get_taken_date_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_taken_date(path, ui.get_taken_date().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_camera_make_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_camera_make(path, ui.get_camera_make().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_camera_model_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_camera_model(path, ui.get_camera_model().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_lens_model_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_lens_model(path, ui.get_lens_model().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_software_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_software(path, ui.get_software().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_artist_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_artist(path, ui.get_artist().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_shutter_speed_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_shutter_speed(path, ui.get_shutter_speed().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_aperture_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_aperture(path, ui.get_aperture().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_iso_speed_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_iso_speed(path, ui.get_iso_speed().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_focal_length_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_focal_length(path, ui.get_focal_length().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_flash_fired_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_flash_fired(path, ui.get_flash_fired().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_metering_mode_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_metering_mode(path, ui.get_metering_mode().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_orientation_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_orientation(path, ui.get_orientation().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_color_space_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_color_space(path, ui.get_color_space().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if has_gps_metadata_changes(&ui) {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if let Err(err) = write_dirty_gps_tags(&ui, path) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                if ui.get_selected_created_dirty() || ui.get_selected_modified_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };

                    let created_time = if ui.get_selected_created_dirty() {
                        match parse_timestamp(ui.get_selected_created().as_str()) {
                            Ok(value) => Some(value),
                            Err(err) => {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        }
                    } else {
                        None
                    };

                    let modified_time = if ui.get_selected_modified_dirty() {
                        match parse_timestamp(ui.get_selected_modified().as_str()) {
                            Ok(value) => Some(value),
                            Err(err) => {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        }
                    } else {
                        None
                    };

                    if let Err(err) = set_file_times(path, created_time, modified_time) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                // Other metadata fields are UI-only until their EXIF writers are implemented.
                store_current_as_original(&ui);
                update_metadata_dirty_state(&ui);
                {
                    let mut app = app_handle.borrow_mut();
                    app.load_folder();
                }
                refresh(selected_path);
            }
        });

        ui.run()?;
        Ok(())
    }
}

fn set_selected_file(ui: &MainWindow, app: &SlintApp, index: i32) {
    let Some((name, created, modified, is_dir)) = app.ui_details_for_index(index) else {
        clear_selected_file(ui);
        return;
    };
    let display_name = app
        .path_for_ui_index(index)
        .map(|path| display_file_name(&path, &name, is_dir, ui.get_show_extension_in_file_name()))
        .unwrap_or(name);

    ui.set_selected_index(index);
    ui.set_selected_name(display_name.clone().into());
    ui.set_selected_created(created.clone().into());
    ui.set_selected_modified(modified.clone().into());
    ui.set_original_selected_name(display_name.into());
    ui.set_original_selected_created(created.into());
    ui.set_original_selected_modified(modified.into());
    ui.set_selected_is_dir(is_dir);
    ui.set_selected_file_count(if is_dir { 0 } else { 1 });
    ui.set_selected_recyclable_count(if index > 0 { 1 } else { 0 });
    ui.set_selected_delete_message(delete_confirmation_message(ui.get_selected_recyclable_count()).into());
    ui.set_selected_name_status(status_for_value(ui.get_selected_name().as_str()).into());
    ui.set_selected_created_status(status_for_value(ui.get_selected_created().as_str()).into());
    ui.set_selected_modified_status(status_for_value(ui.get_selected_modified().as_str()).into());

    let metadata = app
        .path_for_ui_index(index)
        .filter(|path| path.is_file())
        .map(|path| read_exif_metadata(&path))
        .unwrap_or_default();
    set_loaded_exif_metadata(ui, metadata);
}

fn set_selected_files(ui: &MainWindow, app: &SlintApp) {
    let indices = app.selected_indices();
    if indices.is_empty() {
        clear_selected_file(ui);
        return;
    }

    if indices.len() == 1 {
        set_selected_file(ui, app, indices[0]);
        return;
    }

    let mut names = Vec::new();
    let mut created_values = Vec::new();
    let mut modified_values = Vec::new();
    let mut has_dir = false;
    let mut metadata_values = Vec::new();

    for index in indices {
        if let Some((name, created, modified, is_dir)) = app.ui_details_for_index(*index) {
            let display_name = app
                .path_for_ui_index(*index)
                .map(|path| display_file_name(&path, &name, is_dir, ui.get_show_extension_in_file_name()))
                .unwrap_or(name);
            names.push(display_name);
            created_values.push(created);
            modified_values.push(modified);
            has_dir |= is_dir;
        }

        let metadata = app
            .path_for_ui_index(*index)
            .filter(|path| path.is_file())
            .map(|path| read_exif_metadata(&path))
            .unwrap_or_default();
        metadata_values.push(metadata);
    }

    ui.set_selected_index(*indices.last().unwrap_or(&-1));
    let name_display = selection_display(names);
    let created_display = selection_display(created_values);
    let modified_display = selection_display(modified_values);
    ui.set_selected_name(name_display.value.into());
    ui.set_selected_created(created_display.value.into());
    ui.set_selected_modified(modified_display.value.into());
    ui.set_selected_name_status(name_display.status.into());
    ui.set_selected_created_status(created_display.status.into());
    ui.set_selected_modified_status(modified_display.status.into());
    ui.set_original_selected_name(ui.get_selected_name());
    ui.set_original_selected_created(ui.get_selected_created());
    ui.set_original_selected_modified(ui.get_selected_modified());
    ui.set_selected_is_dir(has_dir);
    ui.set_selected_file_count(selected_file_paths(app).len() as i32);
    ui.set_selected_recyclable_count(selected_recyclable_paths(app).len() as i32);
    ui.set_selected_delete_message(delete_confirmation_message(ui.get_selected_recyclable_count()).into());

    set_loaded_exif_metadata(ui, join_metadata(&metadata_values));
    set_joined_metadata_statuses(ui, &metadata_values);
}

fn clear_selected_file(ui: &MainWindow) {
    ui.set_selected_index(-1);
    ui.set_selected_name("N/A".into());
    ui.set_selected_created("N/A".into());
    ui.set_selected_modified("N/A".into());
    ui.set_original_selected_name("N/A".into());
    ui.set_original_selected_created("N/A".into());
    ui.set_original_selected_modified("N/A".into());
    ui.set_selected_name_status("".into());
    ui.set_selected_created_status("".into());
    ui.set_selected_modified_status("".into());
    ui.set_selected_is_dir(false);
    ui.set_selected_file_count(0);
    ui.set_selected_recyclable_count(0);
    ui.set_selected_delete_message(String::new().into());
    ui.set_metadata_dirty(false);
    reset_metadata_dirty_flags(ui);
    set_loaded_exif_metadata(ui, ExifMetadata::default());
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
    set_metadata_statuses(ui, &metadata);
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

fn set_metadata_statuses(ui: &MainWindow, metadata: &ExifMetadata) {
    ui.set_taken_date_status(status_for_value(&metadata.taken_date).into());
    ui.set_camera_make_status(status_for_value(&metadata.camera_make).into());
    ui.set_camera_model_status(status_for_value(&metadata.camera_model).into());
    ui.set_lens_model_status(status_for_value(&metadata.lens_model).into());
    ui.set_software_status(status_for_value(&metadata.software).into());
    ui.set_artist_status(status_for_value(&metadata.artist).into());
    ui.set_shutter_speed_status(status_for_value(&metadata.shutter_speed).into());
    ui.set_aperture_status(status_for_value(&metadata.aperture).into());
    ui.set_iso_speed_status(status_for_value(&metadata.iso_speed).into());
    ui.set_focal_length_status(status_for_value(&metadata.focal_length).into());
    ui.set_flash_fired_status(status_for_value(&metadata.flash_fired).into());
    ui.set_metering_mode_status(status_for_value(&metadata.metering_mode).into());
    ui.set_image_width_status(status_for_value(&metadata.image_width).into());
    ui.set_image_height_status(status_for_value(&metadata.image_height).into());
    ui.set_orientation_status(status_for_value(&metadata.orientation).into());
    ui.set_color_space_status(status_for_value(&metadata.color_space).into());
    ui.set_gps_latitude_status(status_for_value(&metadata.gps_latitude).into());
    ui.set_gps_longitude_status(status_for_value(&metadata.gps_longitude).into());
    ui.set_gps_altitude_status(status_for_value(&metadata.gps_altitude).into());
}

fn set_joined_metadata_statuses(ui: &MainWindow, values: &[ExifMetadata]) {
    ui.set_taken_date_status(selection_display(values.iter().map(|metadata| metadata.taken_date.clone()).collect()).status.into());
    ui.set_camera_make_status(selection_display(values.iter().map(|metadata| metadata.camera_make.clone()).collect()).status.into());
    ui.set_camera_model_status(selection_display(values.iter().map(|metadata| metadata.camera_model.clone()).collect()).status.into());
    ui.set_lens_model_status(selection_display(values.iter().map(|metadata| metadata.lens_model.clone()).collect()).status.into());
    ui.set_software_status(selection_display(values.iter().map(|metadata| metadata.software.clone()).collect()).status.into());
    ui.set_artist_status(selection_display(values.iter().map(|metadata| metadata.artist.clone()).collect()).status.into());
    ui.set_shutter_speed_status(selection_display(values.iter().map(|metadata| metadata.shutter_speed.clone()).collect()).status.into());
    ui.set_aperture_status(selection_display(values.iter().map(|metadata| metadata.aperture.clone()).collect()).status.into());
    ui.set_iso_speed_status(selection_display(values.iter().map(|metadata| metadata.iso_speed.clone()).collect()).status.into());
    ui.set_focal_length_status(selection_display(values.iter().map(|metadata| metadata.focal_length.clone()).collect()).status.into());
    ui.set_flash_fired_status(selection_display(values.iter().map(|metadata| metadata.flash_fired.clone()).collect()).status.into());
    ui.set_metering_mode_status(selection_display(values.iter().map(|metadata| metadata.metering_mode.clone()).collect()).status.into());
    ui.set_image_width_status(selection_display(values.iter().map(|metadata| metadata.image_width.clone()).collect()).status.into());
    ui.set_image_height_status(selection_display(values.iter().map(|metadata| metadata.image_height.clone()).collect()).status.into());
    ui.set_orientation_status(selection_display(values.iter().map(|metadata| metadata.orientation.clone()).collect()).status.into());
    ui.set_color_space_status(selection_display(values.iter().map(|metadata| metadata.color_space.clone()).collect()).status.into());
    ui.set_gps_latitude_status(selection_display(values.iter().map(|metadata| metadata.gps_latitude.clone()).collect()).status.into());
    ui.set_gps_longitude_status(selection_display(values.iter().map(|metadata| metadata.gps_longitude.clone()).collect()).status.into());
    ui.set_gps_altitude_status(selection_display(values.iter().map(|metadata| metadata.gps_altitude.clone()).collect()).status.into());
}

fn joined_selection_value(values: Vec<String>) -> String {
    selection_display(values).value
}

struct SelectionDisplay {
    value: String,
    status: String,
}

fn selection_display(values: Vec<String>) -> SelectionDisplay {
    let mut iter = values.into_iter();
    let Some(first) = iter.next() else {
        return SelectionDisplay {
            value: String::new(),
            status: String::new(),
        };
    };

    let mut mixed = false;
    for value in iter {
        if value != first {
            mixed = true;
            break;
        }
    }

    if mixed {
        SelectionDisplay {
            value: String::new(),
            status: "Mixed".to_string(),
        }
    } else {
        let status = status_for_value(&first);
        SelectionDisplay { value: first, status }
    }
}

fn status_for_value(value: &str) -> String {
    let _ = value;
    String::new()
}

fn join_metadata(values: &[ExifMetadata]) -> ExifMetadata {
    ExifMetadata {
        has_exif: values.iter().any(|metadata| metadata.has_exif),
        taken_date: joined_selection_value(values.iter().map(|metadata| metadata.taken_date.clone()).collect()),
        camera_make: joined_selection_value(values.iter().map(|metadata| metadata.camera_make.clone()).collect()),
        camera_model: joined_selection_value(values.iter().map(|metadata| metadata.camera_model.clone()).collect()),
        lens_model: joined_selection_value(values.iter().map(|metadata| metadata.lens_model.clone()).collect()),
        software: joined_selection_value(values.iter().map(|metadata| metadata.software.clone()).collect()),
        artist: joined_selection_value(values.iter().map(|metadata| metadata.artist.clone()).collect()),
        shutter_speed: joined_selection_value(values.iter().map(|metadata| metadata.shutter_speed.clone()).collect()),
        aperture: joined_selection_value(values.iter().map(|metadata| metadata.aperture.clone()).collect()),
        iso_speed: joined_selection_value(values.iter().map(|metadata| metadata.iso_speed.clone()).collect()),
        focal_length: joined_selection_value(values.iter().map(|metadata| metadata.focal_length.clone()).collect()),
        flash_fired: joined_selection_value(values.iter().map(|metadata| metadata.flash_fired.clone()).collect()),
        metering_mode: joined_selection_value(values.iter().map(|metadata| metadata.metering_mode.clone()).collect()),
        image_width: joined_selection_value(values.iter().map(|metadata| metadata.image_width.clone()).collect()),
        image_height: joined_selection_value(values.iter().map(|metadata| metadata.image_height.clone()).collect()),
        orientation: joined_selection_value(values.iter().map(|metadata| metadata.orientation.clone()).collect()),
        color_space: joined_selection_value(values.iter().map(|metadata| metadata.color_space.clone()).collect()),
        gps_latitude: joined_selection_value(values.iter().map(|metadata| metadata.gps_latitude.clone()).collect()),
        gps_longitude: joined_selection_value(values.iter().map(|metadata| metadata.gps_longitude.clone()).collect()),
        gps_altitude: joined_selection_value(values.iter().map(|metadata| metadata.gps_altitude.clone()).collect()),
    }
}

fn show_message(ui: &MainWindow, title: &str, message: &str) {
    ui.set_message_title(title.into());
    ui.set_message_text(message.into());
    ui.set_message_visible(true);
}

fn display_file_name(path: &std::path::Path, fallback: &str, is_dir: bool, show_extension: bool) -> String {
    if is_dir || show_extension {
        return fallback.to_string();
    }

    path.file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn rename_name_preserving_extension(current_path: &std::path::Path, requested_name: &str) -> String {
    if !current_path.is_file() {
        return requested_name.to_string();
    }

    let Some(extension) = current_path.extension().and_then(|value| value.to_str()) else {
        return requested_name.to_string();
    };
    if extension.is_empty() {
        return requested_name.to_string();
    }

    let requested_path = std::path::Path::new(requested_name);
    let requested_extension = requested_path.extension().and_then(|value| value.to_str());
    let stem = if requested_extension.is_some_and(|value| value.eq_ignore_ascii_case(extension)) {
        requested_path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| requested_name.to_string())
    } else {
        requested_name.to_string()
    };

    format!("{stem}.{extension}")
}

fn selected_file_paths(app: &SlintApp) -> Vec<PathBuf> {
    app.selected_indices()
        .iter()
        .filter_map(|index| app.path_for_ui_index(*index))
        .filter(|path| path.is_file())
        .collect()
}

fn selected_recyclable_paths(app: &SlintApp) -> Vec<PathBuf> {
    app.selected_indices()
        .iter()
        .filter(|index| **index > 0)
        .filter_map(|index| app.path_for_ui_index(*index))
        .filter(|path| path.exists())
        .collect()
}

fn delete_confirmation_message(count: i32) -> String {
    match count {
        1 => "1 file/folder is selected. Send it to the Recycle Bin?".to_string(),
        count if count > 1 => format!("{count} files/folders are selected. Send them to the Recycle Bin?"),
        _ => String::new(),
    }
}

fn apply_metadata_changes_to_path(
    ui: &MainWindow,
    path: &std::path::Path,
    pending_taken_dates: Option<&HashMap<PathBuf, String>>,
    pending_modified_dates: Option<&HashMap<PathBuf, SystemTime>>,
) -> Result<Option<PathBuf>, String> {
    let taken_date_override = pending_taken_dates
        .and_then(|values| values.get(path))
        .map(String::as_str);
    let has_pending_taken_date = taken_date_override.is_some();
    let has_exif_changes = if pending_taken_dates.is_some() && !has_pending_taken_date {
        has_exif_metadata_changes_without_taken_date(ui)
    } else {
        has_exif_metadata_changes(ui)
    };

    let target_path = if has_exif_changes || has_pending_taken_date {
        let current_metadata = read_exif_metadata(path);
        if !current_metadata.has_exif {
            let mut metadata = collect_current_exif_metadata(ui);
            if let Some(taken_date) = taken_date_override {
                metadata.taken_date = taken_date.to_string();
            }
            let new_path = rewrite_basic_exif_metadata(path, &metadata)?;
            copy_file_times(path, &new_path)?;
            Some(new_path)
        } else if is_generated_new_exif_path(path) {
            let mut metadata = collect_current_exif_metadata(ui);
            if let Some(taken_date) = taken_date_override {
                metadata.taken_date = taken_date.to_string();
            }
            rewrite_generated_basic_exif_metadata(path, &metadata)?;
            None
        } else {
            write_dirty_exif_tags(ui, path, taken_date_override, pending_taken_dates.is_some())?;
            None
        }
    } else {
        None
    };

    let file_time_path = target_path.as_deref().unwrap_or(path);
    write_dirty_file_times(ui, file_time_path, pending_modified_dates)?;
    Ok(target_path)
}

fn write_dirty_exif_tags(
    ui: &MainWindow,
    path: &std::path::Path,
    taken_date_override: Option<&str>,
    using_pending_taken_dates: bool,
) -> Result<(), String> {
    if let Some(taken_date) = taken_date_override {
        write_taken_date(path, taken_date)?;
    } else if ui.get_taken_date_dirty() && !using_pending_taken_dates {
        write_taken_date(path, ui.get_taken_date().as_str())?;
    }
    if ui.get_camera_make_dirty() {
        write_camera_make(path, ui.get_camera_make().as_str())?;
    }
    if ui.get_camera_model_dirty() {
        write_camera_model(path, ui.get_camera_model().as_str())?;
    }
    if ui.get_lens_model_dirty() {
        write_lens_model(path, ui.get_lens_model().as_str())?;
    }
    if ui.get_software_dirty() {
        write_software(path, ui.get_software().as_str())?;
    }
    if ui.get_artist_dirty() {
        write_artist(path, ui.get_artist().as_str())?;
    }
    if ui.get_shutter_speed_dirty() {
        write_shutter_speed(path, ui.get_shutter_speed().as_str())?;
    }
    if ui.get_aperture_dirty() {
        write_aperture(path, ui.get_aperture().as_str())?;
    }
    if ui.get_iso_speed_dirty() {
        write_iso_speed(path, ui.get_iso_speed().as_str())?;
    }
    if ui.get_focal_length_dirty() {
        write_focal_length(path, ui.get_focal_length().as_str())?;
    }
    if ui.get_flash_fired_dirty() {
        write_flash_fired(path, ui.get_flash_fired().as_str())?;
    }
    if ui.get_metering_mode_dirty() {
        write_metering_mode(path, ui.get_metering_mode().as_str())?;
    }
    if ui.get_orientation_dirty() {
        write_orientation(path, ui.get_orientation().as_str())?;
    }
    if ui.get_color_space_dirty() {
        write_color_space(path, ui.get_color_space().as_str())?;
    }
    if has_gps_metadata_changes(ui) {
        write_dirty_gps_tags(ui, path)?;
    }
    Ok(())
}

fn has_gps_metadata_changes(ui: &MainWindow) -> bool {
    ui.get_gps_latitude_dirty()
        || ui.get_gps_longitude_dirty()
        || ui.get_gps_altitude_dirty()
}

fn write_dirty_gps_tags(ui: &MainWindow, path: &std::path::Path) -> Result<(), String> {
    if ui.get_gps_latitude().is_empty()
        && ui.get_gps_longitude().is_empty()
        && ui.get_gps_altitude().is_empty()
    {
        remove_gps_information(path)
    } else {
        Err("Editing GPS values is not supported yet. Use Remove GPS to clear location metadata.".to_string())
    }
}

fn write_dirty_file_times(
    ui: &MainWindow,
    path: &std::path::Path,
    pending_modified_dates: Option<&HashMap<PathBuf, SystemTime>>,
) -> Result<(), String> {
    let pending_modified_time = pending_modified_dates.and_then(|values| values.get(path).copied());
    let has_pending_modified_map = pending_modified_dates.is_some();

    if has_pending_modified_map && pending_modified_time.is_none() && !ui.get_selected_created_dirty() {
        return Ok(());
    }

    if !ui.get_selected_created_dirty() && !ui.get_selected_modified_dirty() && pending_modified_time.is_none() {
        return Ok(());
    }

    let created_time = if ui.get_selected_created_dirty() {
        Some(parse_timestamp(ui.get_selected_created().as_str())?)
    } else {
        None
    };
    let modified_time = if let Some(modified_time) = pending_modified_time {
        Some(modified_time)
    } else if ui.get_selected_modified_dirty() && !has_pending_modified_map {
        Some(parse_timestamp(ui.get_selected_modified().as_str())?)
    } else {
        None
    };

    set_file_times(path, created_time, modified_time)
}

fn has_exif_metadata_changes(ui: &MainWindow) -> bool {
    ui.get_taken_date_dirty()
        || has_exif_metadata_changes_without_taken_date(ui)
}

fn has_exif_metadata_changes_without_taken_date(ui: &MainWindow) -> bool {
    ui.get_camera_make_dirty()
        || ui.get_camera_model_dirty()
        || ui.get_lens_model_dirty()
        || ui.get_software_dirty()
        || ui.get_artist_dirty()
        || ui.get_shutter_speed_dirty()
        || ui.get_aperture_dirty()
        || ui.get_iso_speed_dirty()
        || ui.get_focal_length_dirty()
        || ui.get_flash_fired_dirty()
        || ui.get_metering_mode_dirty()
        || ui.get_orientation_dirty()
        || ui.get_color_space_dirty()
        || has_gps_metadata_changes(ui)
}

fn collect_current_exif_metadata(ui: &MainWindow) -> ExifMetadata {
    ExifMetadata {
        has_exif: true,
        taken_date: ui.get_taken_date().to_string(),
        camera_make: ui.get_camera_make().to_string(),
        camera_model: ui.get_camera_model().to_string(),
        lens_model: ui.get_lens_model().to_string(),
        software: ui.get_software().to_string(),
        artist: ui.get_artist().to_string(),
        shutter_speed: ui.get_shutter_speed().to_string(),
        aperture: ui.get_aperture().to_string(),
        iso_speed: ui.get_iso_speed().to_string(),
        focal_length: ui.get_focal_length().to_string(),
        flash_fired: ui.get_flash_fired().to_string(),
        metering_mode: ui.get_metering_mode().to_string(),
        image_width: ui.get_image_width().to_string(),
        image_height: ui.get_image_height().to_string(),
        orientation: ui.get_orientation().to_string(),
        color_space: ui.get_color_space().to_string(),
        gps_latitude: ui.get_gps_latitude().to_string(),
        gps_longitude: ui.get_gps_longitude().to_string(),
        gps_altitude: ui.get_gps_altitude().to_string(),
    }
}

fn parse_timestamp(value: &str) -> Result<SystemTime, String> {
    if value.trim().is_empty() || value == "N/A" || value == "-" {
        return Err("Invalid timestamp format.".to_string());
    }

    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .map_err(|_| "Expected datetime format: YYYY-MM-DD HH:MM:SS".to_string())?;

    Local
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| "Invalid or ambiguous local datetime.".to_string())
        .map(|dt| dt.into())
}

fn should_apply_taken_date_candidate(existing: &str, candidate: &str) -> bool {
    let existing = existing.trim();
    if existing.is_empty() || existing == "N/A" || existing == "-" {
        return true;
    }

    let Ok(existing_datetime) = NaiveDateTime::parse_from_str(existing, "%Y-%m-%d %H:%M:%S") else {
        return true;
    };
    let Ok(candidate_datetime) = NaiveDateTime::parse_from_str(candidate, "%Y-%m-%d %H:%M:%S") else {
        return false;
    };

    candidate_datetime <= existing_datetime
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
