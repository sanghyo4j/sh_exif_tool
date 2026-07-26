pub mod app;

use chrono::{Duration as ChronoDuration, Local, NaiveDate, NaiveDateTime, TimeZone};
use slint::{language::ColorScheme, ComponentHandle};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};
use self::app::{CachedMediaScan, SlintApp};
use crate::exif::{
    extract_datetime_from_filename,
    exif_backup_path,
    is_generated_new_exif_path,
    read_exif_metadata,
    remove_exif_metadata,
    remove_exif_tag,
    remove_gps_information,
    rewrite_basic_exif_metadata,
    rewrite_generated_basic_exif_metadata,
    rewrite_repairable_exif_metadata,
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
use crate::fs::{
    copy_file_times,
    copy_file_to_folder,
    move_file_to_recycle_bin,
    move_trailing_numbers_to_front,
    open_in_file_manager,
    rename_entry,
    save_file_copy,
    set_file_times,
    trailing_number_rename_candidate_count,
};
use crate::media::{
    read_png_date_sources,
    remove_png_date_source,
    remove_png_date_metadata,
    scan_media_file,
    write_png_date_sources,
    write_mp4_media_date,
    write_png_media_date,
    MediaScanJob,
    PngDateSources,
};
use crate::GuiRunner;

slint::include_modules!();

pub struct SlintRunner;

#[derive(Clone)]
enum ExifClipboard {
    Tag {
        value: String,
    },
    Section {
        section: String,
        values: Vec<(String, String)>,
    },
}

#[derive(Clone)]
enum ExifContextTarget {
    Tag {
        key: String,
        value: String,
    },
    Section {
        section: String,
        values: Vec<(String, String)>,
    },
}

#[derive(Clone)]
struct PendingExifPaste {
    values: Vec<(String, String)>,
}

struct MediaScanBatch {
    epoch: u64,
    jobs: Vec<MediaScanJob>,
}

fn start_media_scan_worker(
    batches: std::sync::mpsc::Receiver<MediaScanBatch>,
    current_epoch: Arc<std::sync::atomic::AtomicU64>,
    results: Arc<std::sync::Mutex<HashMap<PathBuf, CachedMediaScan>>>,
    results_dirty: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while let Ok(mut batch) = batches.recv() {
            let mut jobs = VecDeque::from(batch.jobs);
            loop {
                while let Ok(newer_batch) = batches.try_recv() {
                    batch = newer_batch;
                    jobs = VecDeque::from(batch.jobs);
                }
                let Some(job) = jobs.pop_front() else {
                    break;
                };
                if current_epoch.load(Ordering::Acquire) != batch.epoch {
                    break;
                }
                let result = scan_media_file(&job.path);
                if current_epoch.load(Ordering::Acquire) != batch.epoch {
                    continue;
                }
                let cached = CachedMediaScan {
                    size: job.size,
                    modified: job.modified,
                    result,
                };
                results
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(job.path, cached);
                results_dirty.store(true, Ordering::Release);
            }
        }
    });
}

impl GuiRunner for SlintRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let ui = MainWindow::new()?;
        ui.global::<Palette>().set_color_scheme(ColorScheme::Light);
        let app = Rc::new(RefCell::new(SlintApp::new()));
        let copied_files = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
        let pending_filename_renames =
            Rc::new(RefCell::new(HashMap::<PathBuf, String>::new()));
        let pending_filename_taken_dates = Rc::new(RefCell::new(HashMap::<PathBuf, String>::new()));
        let pending_created_dates = Rc::new(RefCell::new(HashMap::<PathBuf, SystemTime>::new()));
        let pending_modified_dates = Rc::new(RefCell::new(HashMap::<PathBuf, SystemTime>::new()));
        let pending_exif_removals = Rc::new(RefCell::new(HashSet::<PathBuf>::new()));
        let pending_exif_tag_removals =
            Rc::new(RefCell::new(HashMap::<PathBuf, HashSet<String>>::new()));
        let exif_clipboard = Rc::new(RefCell::new(None::<ExifClipboard>));
        let exif_context_target = Rc::new(RefCell::new(None::<ExifContextTarget>));
        let pending_exif_paste = Rc::new(RefCell::new(None::<PendingExifPaste>));
        let previous_taken_date_input = Rc::new(RefCell::new(String::new()));
        let previous_png_creation_time_input = Rc::new(RefCell::new(String::new()));
        let previous_png_exif_original_input = Rc::new(RefCell::new(String::new()));
        let date_overwrite_confirmed = Rc::new(Cell::new(false));
        let scan_results_dirty = Arc::new(AtomicBool::new(false));
        let (scan_batch_sender, scan_batch_receiver) = std::sync::mpsc::channel();

        {
            let app = app.borrow();
            start_media_scan_worker(
                scan_batch_receiver,
                app.scan_epoch.clone(),
                app.scan_results.clone(),
                scan_results_dirty.clone(),
            );
        }

        // 1. 초기 데이터 로드
        app.borrow_mut().load_folder();

        let schedule_media_scan = {
            let app_handle = app.clone();
            let scan_batch_sender = scan_batch_sender.clone();
            move || {
                let app = app_handle.borrow();
                app.restart_scan();
                let (epoch, jobs) = app.prepare_scan_jobs();
                drop(app);
                let _ = scan_batch_sender.send(MediaScanBatch { epoch, jobs });
            }
        };

        // UI 갱신 헬퍼 함수
        let refresh_ui = {
            let ui_handle = ui.as_weak();
            let app_handle = app.clone();
            let schedule_scan = schedule_media_scan.clone();
            let pending_filename_renames = pending_filename_renames.clone();
            move |selected_path: Option<PathBuf>| {
                if let Some(ui) = ui_handle.upgrade() {
                    pending_filename_renames.borrow_mut().clear();
                    let mut app = app_handle.borrow_mut();
                    let selected_index = selected_path
                        .as_deref()
                        .and_then(|path| app.ui_index_for_path(path))
                        .unwrap_or(-1);
                    if selected_index >= 0 {
                        app.select_ui_index(selected_index, false, false);
                    }
                    ui.set_current_path(app.current_path.as_str().into());
                    ui.set_activity_text("Ready".into());
                    ui.set_item_count(app.file_count() as i32);
                    ui.set_files(app.get_ui_model());
                    set_selected_files(&ui, &app);
                    drop(app);
                    schedule_scan();
                }
            }
        };

        let scan_refresh_timer = slint::Timer::default();
        {
            let ui_handle = ui.as_weak();
            let app_handle = app.clone();
            let scan_results_dirty = scan_results_dirty.clone();
            scan_refresh_timer.start(slint::TimerMode::Repeated, Duration::from_millis(33), move || {
                if !scan_results_dirty.swap(false, Ordering::AcqRel) {
                    return;
                }
                if let Some(ui) = ui_handle.upgrade() {
                    let mut app = app_handle.borrow_mut();
                    if app.show_only_missing_media_date {
                        // Rows can disappear as their Media Date is discovered. Numeric
                        // selections would otherwise point at a different file.
                        app.select_ui_index(-1, false, false);
                    }
                    ui.set_item_count(app.file_count() as i32);
                    ui.set_files(app.get_ui_model());
                    set_selected_files(&ui, &app);
                }
            });
        }

        // 초기 실행
        refresh_ui(None);

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_set_file_filter(move |filter| {
            {
                let mut app = app_handle.borrow_mut();
                app.file_filter = filter;
                app.selected_indices.clear();
            }
            refresh(None);
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        ui.on_set_show_only_missing_media_date(move |enabled| {
            {
                let mut app = app_handle.borrow_mut();
                app.show_only_missing_media_date = enabled;
                app.selected_indices.clear();
            }
            refresh(None);
        });

        let app_handle = app.clone();
        let clipboard = exif_clipboard.clone();
        let context_target = exif_context_target.clone();
        let ui_handle = ui.as_weak();
        ui.on_exif_tag_right_clicked(move |key, value| {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let Some(_path) = app_handle.borrow().path_for_ui_index(ui.get_selected_index()) else { return; };
            let key = key.to_string();
            let value = value.to_string();
            let paste_enabled = is_writable_metadata_key(&ui, &key)
                && matches!(*clipboard.borrow(), Some(ExifClipboard::Tag { .. }));
            *context_target.borrow_mut() = Some(ExifContextTarget::Tag {
                key: key.clone(),
                value: value.clone(),
            });
            ui.set_exif_context_paste_enabled(paste_enabled);
            ui.set_exif_context_remove_enabled(
                ui.get_selected_file_count() == 1
                    && !value.trim().is_empty()
                    && value.trim() != "-"
                    && ((ui.get_selected_media_kind().as_str() == "jpeg"
                        && is_removable_exif_key(&key))
                        || (ui.get_selected_media_kind().as_str() == "png"
                            && matches!(key.as_str(), "png_creation_time" | "date_time_original"))),
            );
            ui.set_exif_context_menu_visible(true);
        });

        let app_handle = app.clone();
        let clipboard = exif_clipboard.clone();
        let context_target = exif_context_target.clone();
        let ui_handle = ui.as_weak();
        ui.on_exif_section_right_clicked(move |section| {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let Some(_path) = app_handle.borrow().path_for_ui_index(ui.get_selected_index()) else { return; };
            let section = section.to_string();
            let current_values = collect_exif_section(&ui, &section);
            if current_values.is_empty() {
                show_toast(&ui, "This EXIF section is read-only.");
                return;
            }
            let paste_enabled = matches!(
                &*clipboard.borrow(),
                Some(ExifClipboard::Section { section: source_section, .. }) if source_section == &section
            ) && is_writable_exif_section(&section);
            *context_target.borrow_mut() = Some(ExifContextTarget::Section { section, values: current_values });
            ui.set_exif_context_paste_enabled(paste_enabled);
            ui.set_exif_context_remove_enabled(false);
            ui.set_exif_context_menu_visible(true);
        });

        let clipboard = exif_clipboard.clone();
        let context_target = exif_context_target.clone();
        let ui_handle = ui.as_weak();
        ui.on_exif_context_copy(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let Some(target) = context_target.borrow().clone() else { return; };
            match target {
                ExifContextTarget::Tag { key, value } => {
                    *clipboard.borrow_mut() = Some(ExifClipboard::Tag {
                        value: value.clone(),
                    });
                    let display_value = if value.is_empty() { "(empty)" } else { value.as_str() };
                    show_toast(&ui, &format!("Copied: '{}: {}'", exif_key_label(&key), display_value));
                }
                ExifContextTarget::Section { section, values } => {
                    let count = values.len();
                    *clipboard.borrow_mut() = Some(ExifClipboard::Section {
                        section: section.clone(),
                        values,
                    });
                    show_toast(&ui, &format!("Copied: '{} section ({} tags)'", exif_section_label(&section), count));
                }
            }
        });

        let clipboard = exif_clipboard.clone();
        let context_target = exif_context_target.clone();
        let pending_paste = pending_exif_paste.clone();
        let ui_handle = ui.as_weak();
        ui.on_exif_context_paste(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let Some(target) = context_target.borrow().clone() else { return; };
            let values = match (target, clipboard.borrow().clone()) {
                (ExifContextTarget::Tag { key, .. }, Some(ExifClipboard::Tag { value, .. }))
                    if is_writable_metadata_key(&ui, &key) => Some(vec![(key, value)]),
                (
                    ExifContextTarget::Section { section, .. },
                    Some(ExifClipboard::Section { section: source_section, values, .. }),
                ) if section == source_section && is_writable_exif_section(&section) => Some(values),
                _ => None,
            };
            if let Some(values) = values {
                stage_or_confirm_exif_paste(&ui, values, &pending_paste);
            }
        });

        let app_handle = app.clone();
        let context_target = exif_context_target.clone();
        let pending_tag_removals = pending_exif_tag_removals.clone();
        let ui_handle = ui.as_weak();
        ui.on_exif_context_remove(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let Some(ExifContextTarget::Tag { key, .. }) = context_target.borrow().clone() else {
                return;
            };
            let removable = (ui.get_selected_media_kind().as_str() == "jpeg"
                && is_removable_exif_key(&key))
                || (ui.get_selected_media_kind().as_str() == "png"
                    && matches!(key.as_str(), "png_creation_time" | "date_time_original"));
            if !removable {
                return;
            }
            let Some(path) = app_handle.borrow().path_for_ui_index(ui.get_selected_index()) else {
                return;
            };
            pending_tag_removals
                .borrow_mut()
                .entry(path)
                .or_default()
                .insert(key.clone());
            set_exif_value(&ui, &key, "");
            set_exif_key_dirty(&ui, &key, false);
            ui.set_metadata_dirty(true);
            show_toast(
                &ui,
                &format!("Remove staged: '{}'. Ctrl+S to save.", exif_key_label(&key)),
            );
        });

        let pending_paste = pending_exif_paste.clone();
        let ui_handle = ui.as_weak();
        ui.on_confirm_metadata_paste(move |confirmed| {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let pending = pending_paste.borrow_mut().take();
            if confirmed {
                if let Some(pending) = pending {
                    apply_exif_paste(&ui, &pending.values);
                    show_toast(&ui, "Pasted. Save to apply.");
                }
            }
            ui.set_message_title("Operation Failed".into());
            ui.set_message_text("".into());
        });

        let app_handle = app.clone();
        let ui_handle = ui.as_weak();
        ui.on_select_files_without_exif(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let mut app = app_handle.borrow_mut();
                app.select_files_without_exif();
                ui.set_files(app.get_ui_model());
                set_selected_files(&ui, &app);
            }
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
                    app.load_folder();
                }
                refresh(selected_path);
            }
        });

        // [추가] 2.5. 경로 직접 입력 핸들러 (엔터 입력 시)
        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_change_dir(move |new_path| {
            if let Some(ui) = ui_handle.upgrade() {
                let requested_path = PathBuf::from(new_path.trim());
                if !requested_path.is_dir() {
                    let current_path = {
                        let app = app_handle.borrow();
                        app.current_path.clone()
                    };
                    ui.set_current_path(current_path.into());
                    show_message(&ui, "Invalid Path", "The entered path is not a valid folder.");
                    return;
                }

                {
                    let mut app = app_handle.borrow_mut();
                    app.current_path = requested_path.to_string_lossy().to_string();
                    app.load_folder();
                }
                refresh(None);
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
                } else if let Some(path) = app.path_for_ui_index(index) { // 폴더 클릭
                    if path.is_dir() {
                        app.current_path = path.to_string_lossy().to_string();
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
                } else if let Some(path) = app.path_for_ui_index(index) {
                    if path.is_dir() {
                        app.current_path = path.to_string_lossy().to_string();
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
        let pending_renames = pending_filename_renames.clone();
        let ui_handle = ui.as_weak();
        ui.on_set_filename_from_media_date(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };
                if selected_paths.is_empty() {
                    show_message(
                        &ui,
                        "Unable to Set Filename",
                        "Select one or more files before setting filenames from Media Date.",
                    );
                    return;
                }

                if selected_paths.len() == 1 {
                    pending_renames.borrow_mut().clear();
                    let path = &selected_paths[0];
                    let media_date = scan_media_file(path).media_date;
                    let new_name = match filename_from_media_date(
                        path,
                        &media_date,
                        ui.get_show_extension_in_file_name(),
                    ) {
                        Ok(value) => value,
                        Err(err) => {
                            show_message(&ui, "Unable to Set Filename", &err);
                            return;
                        }
                    };

                    ui.set_selected_name(new_name.into());
                    update_metadata_dirty_state(&ui);
                    return;
                }

                let mut reserved_names = HashSet::new();
                let mut staged = HashMap::new();
                let mut skipped_count = 0usize;
                for path in &selected_paths {
                    let media_date = scan_media_file(path).media_date;
                    let new_name = match filename_from_media_date_with_reserved(
                        path,
                        &media_date,
                        true,
                        &mut reserved_names,
                    ) {
                        Ok(value) => value,
                        Err(_) => {
                            skipped_count += 1;
                            continue;
                        }
                    };
                    if path.file_name().is_some_and(|value| value == new_name.as_str()) {
                        skipped_count += 1;
                        continue;
                    }
                    staged.insert(path.clone(), new_name);
                }

                if staged.is_empty() {
                    show_message(
                        &ui,
                        "Unable to Set Filename",
                        "No selected file has a usable Media Date or requires renaming.",
                    );
                    return;
                }

                let staged_count = staged.len();
                *pending_renames.borrow_mut() = staged;
                ui.set_selected_name("".into());
                ui.set_selected_name_status("Mixed".into());
                ui.set_selected_name_dirty(true);
                ui.set_metadata_dirty(true);
                if skipped_count == 0 {
                    show_toast(
                        &ui,
                        &format!("Staged {staged_count} filename(s). Ctrl+S to save."),
                    );
                } else {
                    show_toast(
                        &ui,
                        &format!(
                            "Staged {staged_count}; skipped {skipped_count}. Ctrl+S to save."
                        ),
                    );
                }
            }
        });

        let app_handle = app.clone();
        let pending_renames = pending_filename_renames.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_created_dates_handle = pending_created_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let pending_exif_removals_handle = pending_exif_removals.clone();
        let pending_exif_tag_removals_handle = pending_exif_tag_removals.clone();
        let ui_handle = ui.as_weak();
        let schedule_scan = schedule_media_scan.clone();
        ui.on_table_row_selected(move |index, ctrl, shift| {
            if let Some(ui) = ui_handle.upgrade() {
                pending_renames.borrow_mut().clear();
                pending_taken_dates.borrow_mut().clear();
                pending_created_dates_handle.borrow_mut().clear();
                pending_modified_dates_handle.borrow_mut().clear();
                pending_exif_removals_handle.borrow_mut().clear();
                pending_exif_tag_removals_handle.borrow_mut().clear();
                ui.set_activity_text("Ready".into());
                let mut app = app_handle.borrow_mut();
                app.select_ui_index(index, ctrl, shift);
                ui.set_selected_index(index);
                ui.set_files(app.get_ui_model());
                set_selected_files(&ui, &app);
                drop(app);
                schedule_scan();
            }
        });

        let app_handle = app.clone();
        let pending_renames = pending_filename_renames.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_created_dates_handle = pending_created_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let pending_exif_removals_handle = pending_exif_removals.clone();
        let pending_exif_tag_removals_handle = pending_exif_tag_removals.clone();
        let ui_handle = ui.as_weak();
        let schedule_scan = schedule_media_scan.clone();
        ui.on_select_all_table_rows(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            pending_renames.borrow_mut().clear();
            pending_taken_dates.borrow_mut().clear();
            pending_created_dates_handle.borrow_mut().clear();
            pending_modified_dates_handle.borrow_mut().clear();
            pending_exif_removals_handle.borrow_mut().clear();
            pending_exif_tag_removals_handle.borrow_mut().clear();
            ui.set_activity_text("Ready".into());
            let mut app = app_handle.borrow_mut();
            app.select_all_visible_entries();
            ui.set_files(app.get_ui_model());
            set_selected_files(&ui, &app);
            drop(app);
            schedule_scan();
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

        let app_handle = app.clone();
        let pending_exif_removals_handle = pending_exif_removals.clone();
        let ui_handle = ui.as_weak();
        ui.on_remove_exif_tags(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let selected_paths = {
                let app = app_handle.borrow();
                selected_file_paths(&app)
            };
            if selected_paths.is_empty() {
                show_message(
                    &ui,
                    "Remove Metadata Failed",
                    "Select JPEG or PNG files first.",
                );
                return;
            }
            if selected_paths
                .iter()
                .any(|path| !is_jpeg_path(path) && !is_png_path(path))
            {
                show_message(
                    &ui,
                    "Remove Metadata Failed",
                    "Metadata removal is supported for JPEG and PNG files only.",
                );
                return;
            }

            let removals: HashSet<_> = selected_paths
                .into_iter()
                .filter(|path| {
                    if is_png_path(path) {
                        read_png_date_sources(path).has_existing_date()
                    } else {
                        read_exif_metadata(path).has_exif
                    }
                })
                .collect();
            if removals.is_empty() {
                show_message(
                    &ui,
                    "Remove Metadata",
                    "The selected files do not contain removable metadata.",
                );
                return;
            }

            let staged_count = removals.len();
            *pending_exif_removals_handle.borrow_mut() = removals;
            set_exif_metadata(&ui, ExifMetadata::default());
            ui.set_exif_available(false);
            ui.set_metadata_dirty(true);
            show_toast(
                &ui,
                &format!("Staged metadata removal for {staged_count} file(s). Ctrl+S to save."),
            );
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_create_exif_structure(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_selected_file_count() == 0 {
                    show_message(&ui, "Create EXIF Failed", "Select a file before creating EXIF.");
                    return;
                }

                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };

                if selected_paths.is_empty() {
                    show_message(&ui, "Create EXIF Failed", "Selected file could not be resolved.");
                    return;
                }
                if selected_paths.iter().any(|path| !is_jpeg_path(path)) {
                    show_message(&ui, "Create EXIF Failed", "Creating an EXIF structure is supported for JPEG files only.");
                    return;
                }

                let mut created_paths = Vec::new();
                let mut skipped_count = 0usize;
                let metadata = collect_current_exif_metadata(&ui);
                let backup_before_changes = ui.get_backup_before_changes();

                for path in selected_paths {
                    if read_exif_metadata(&path).has_exif {
                        skipped_count += 1;
                        continue;
                    }

                    let new_path = match rewrite_basic_exif_metadata(&path, &metadata, backup_before_changes) {
                        Ok(path) => path,
                        Err(err) => {
                            show_message(&ui, "Create EXIF Failed", &err);
                            return;
                        }
                    };
                    if backup_before_changes {
                        let backup_path = exif_backup_path(&new_path);
                        if let Err(err) = copy_file_times(&backup_path, &new_path) {
                            show_message(&ui, "Create EXIF Failed", &err);
                            return;
                        }
                    }
                    created_paths.push(new_path);
                }

                if created_paths.is_empty() {
                    show_message(&ui, "Create EXIF", "EXIF is already available for the selected files.");
                    return;
                }

                let refresh_path = created_paths.last().cloned();
                {
                    let mut app = app_handle.borrow_mut();
                    app.load_folder();
                }
                refresh(refresh_path);

                if created_paths.len() == 1 && skipped_count == 0 {
                    show_message(&ui, "EXIF File Created", &format!("Created {}", created_paths[0].display()));
                } else if skipped_count == 0 {
                    show_message(&ui, "EXIF Files Created", &format!("Created EXIF files for {} selected files.", created_paths.len()));
                } else {
                    show_message(&ui, "EXIF Files Created", &format!("Created EXIF files for {} selected files. Skipped {} files that already had EXIF.", created_paths.len(), skipped_count));
                }
            }
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_request_shift_media_date(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };
                if selected_paths.is_empty() {
                    show_message(&ui, "Unable to Shift Media Date", "Select one or more media files first.");
                    return;
                }
                ui.set_shift_media_date_subtract(false);
                ui.set_shift_media_date_days("0".into());
                ui.set_shift_media_date_hours("0".into());
                ui.set_shift_media_date_minutes("0".into());
                ui.set_shift_media_date_seconds("0".into());
                let preview = build_shift_media_date_preview(
                    &selected_paths,
                    &pending_taken_dates.borrow(),
                    "0", "0", "0", "0", false,
                );
                ui.set_shift_media_date_preview(preview.into());
                ui.set_shift_media_date_visible(true);
            }
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        ui.on_update_shift_media_date_preview(move |days, hours, minutes, seconds, subtract| {
            let selected_paths = {
                let app = app_handle.borrow();
                selected_file_paths(&app)
            };
            build_shift_media_date_preview(
                &selected_paths,
                &pending_taken_dates.borrow(),
                days.as_str(), hours.as_str(), minutes.as_str(), seconds.as_str(), subtract,
            ).into()
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_apply_shift_media_date(move |days, hours, minutes, seconds, subtract| {
            let Some(ui) = ui_handle.upgrade() else { return false; };
            let duration = match parse_media_date_shift(days.as_str(), hours.as_str(), minutes.as_str(), seconds.as_str()) {
                Ok(duration) if duration != ChronoDuration::zero() => duration,
                Ok(_) => {
                    ui.set_shift_media_date_preview("Enter a non-zero time shift.".into());
                    return false;
                }
                Err(err) => {
                    ui.set_shift_media_date_preview(err.into());
                    return false;
                }
            };
            let selected_paths = {
                let app = app_handle.borrow();
                selected_file_paths(&app)
            };
            let existing_pending = pending_taken_dates.borrow().clone();
            let mut staged = existing_pending.clone();
            let mut shifted = Vec::new();
            let mut skipped_count = 0usize;
            for path in &selected_paths {
                if !is_jpeg_path(path) && !is_png_path(path) && !is_mp4_path(path) {
                    skipped_count += 1;
                    continue;
                }
                let current = media_date_for_shift(path, &existing_pending);
                let Some(new_date) = shift_display_datetime(&current, duration, subtract) else {
                    skipped_count += 1;
                    continue;
                };
                staged.insert(path.clone(), new_date.clone());
                shifted.push((path.clone(), new_date));
            }
            if shifted.is_empty() {
                ui.set_shift_media_date_preview("No selected file has a usable Media Date.".into());
                return false;
            }

            *pending_taken_dates.borrow_mut() = staged;
            if shifted.len() == 1 {
                stage_taken_date_in_ui(&ui, &shifted[0].1);
                ui.set_taken_date_status("Modified".into());
            } else {
                ui.set_taken_date("".into());
                ui.set_taken_date_status("Mixed".into());
            }
            ui.set_taken_date_dirty(true);
            ui.set_metadata_dirty(true);

            let message = if skipped_count == 0 {
                format!("Shifted {} date(s). Ctrl+S to save.", shifted.len())
            } else {
                format!("Shifted {} date(s); skipped {}. Ctrl+S to save.", shifted.len(), skipped_count)
            };
            show_toast(&ui, &message);
            true
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_fill_taken_date_from_filename(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let date_label = if matches!(ui.get_selected_media_kind().as_str(), "mp4" | "png") {
                    "Media Date"
                } else {
                    "Taken Date"
                };
                let unable_title = format!("Unable to Set {date_label}");
                if ui.get_selected_file_count() == 0 {
                    show_message(
                        &ui,
                        &unable_title,
                        &format!("Select a file before setting {date_label}."),
                    );
                    return;
                }

                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };

                if selected_paths.is_empty() {
                    show_message(&ui, &unable_title, "Selected file could not be resolved.");
                    return;
                }

                if selected_paths.len() == 1 {
                    let path = &selected_paths[0];
                    let Some(datetime) = extract_datetime_from_filename(path) else {
                        show_message(&ui, &unable_title, "No supported date pattern was found in the filename.");
                        return;
                    };
                    if !should_apply_taken_date_candidate(ui.get_taken_date().as_str(), &datetime) {
                        show_message(
                            &ui,
                            &format!("{date_label} Ignored"),
                            &format!("Filename timestamp is later than the existing {date_label}."),
                        );
                        return;
                    }

                    pending_taken_dates.borrow_mut().clear();
                    stage_taken_date_in_ui(&ui, &datetime);
                    if ui.get_selected_media_kind().as_str() == "jpeg" && !ui.get_exif_available() {
                        // Stage a new EXIF structure in the UI. The file is still only
                        // changed when Apply/Ctrl+S is invoked.
                        ui.set_exif_available(true);
                    }
                    update_metadata_dirty_state(&ui);
                    return;
                }

                let mut pending = HashMap::new();
                for path in &selected_paths {
                    if let Some(datetime) = extract_datetime_from_filename(path) {
                        let existing_taken_date = if is_mp4_path(path) || is_png_path(path) {
                            scan_media_file(path).media_date
                        } else {
                            read_exif_metadata(path).taken_date
                        };
                        if should_apply_taken_date_candidate(&existing_taken_date, &datetime) {
                            pending.insert(path.clone(), datetime);
                        }
                    }
                }

                if pending.is_empty() {
                    show_message(&ui, &unable_title, "No supported date pattern was found in the selected filenames.");
                    return;
                }

                let parsed_count = pending.len();
                let skipped_count = selected_paths.len().saturating_sub(parsed_count);
                *pending_taken_dates.borrow_mut() = pending;

                ui.set_taken_date("".into());
                ui.set_taken_date_status("Mixed".into());
                ui.set_taken_date_dirty(true);
                ui.set_metadata_dirty(true);
                if ui.get_selected_media_kind().as_str() == "jpeg" && !ui.get_exif_available() {
                    // Each EXIF-less JPEG will receive its structure during Apply.
                    ui.set_exif_available(true);
                }

                if skipped_count == 0 {
                    show_toast(&ui, &format!("Parsed {parsed_count} date(s). Ctrl+S to save."));
                } else {
                    show_toast(&ui, &format!("Parsed {parsed_count}; skipped {skipped_count}. Ctrl+S to save."));
                }
            }
        });

        let app_handle = app.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_fill_taken_date_from_created_date(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };
                if selected_paths.is_empty() {
                    show_message(
                        &ui,
                        "Unable to Set Date",
                        "Select one or more media files before setting the date.",
                    );
                    return;
                }

                let mut pending = HashMap::new();
                let mut unavailable_count = 0usize;
                let mut later_count = 0usize;
                for path in &selected_paths {
                    if !is_jpeg_path(path) && !is_png_path(path) && !is_mp4_path(path) {
                        unavailable_count += 1;
                        continue;
                    }
                    let Some(timestamp) = earliest_file_timestamp(path) else {
                        unavailable_count += 1;
                        continue;
                    };
                    let existing_taken_date = if is_mp4_path(path) || is_png_path(path) {
                        scan_media_file(path).media_date
                    } else {
                        read_exif_metadata(path).taken_date
                    };
                    if should_apply_taken_date_candidate(&existing_taken_date, &timestamp) {
                        pending.insert(path.clone(), timestamp);
                    } else {
                        later_count += 1;
                    }
                }

                if pending.is_empty() {
                    let message = if later_count > 0 {
                        "The earlier file timestamp is later than the existing Taken Date."
                    } else {
                        "Created and Modified timestamps could not be read from the selected media files."
                    };
                    show_message(&ui, "Date Ignored", message);
                    return;
                }

                if selected_paths.len() == 1 {
                    let timestamp = pending.values().next().cloned().unwrap_or_default();
                    pending_taken_dates.borrow_mut().clear();
                    stage_taken_date_in_ui(&ui, &timestamp);
                    if is_jpeg_path(&selected_paths[0]) && !ui.get_exif_available() {
                        ui.set_exif_available(true);
                    }
                    update_metadata_dirty_state(&ui);
                    show_toast(&ui, "Date staged. Ctrl+S to save.");
                    return;
                }

                let staged_count = pending.len();
                *pending_taken_dates.borrow_mut() = pending;
                ui.set_taken_date("".into());
                ui.set_taken_date_status("Mixed".into());
                ui.set_taken_date_dirty(true);
                ui.set_metadata_dirty(true);
                if pending_taken_dates
                    .borrow()
                    .keys()
                    .any(|path| is_jpeg_path(path))
                    && !ui.get_exif_available()
                {
                    ui.set_exif_available(true);
                }

                let skipped_count = unavailable_count + later_count;
                let message = if skipped_count == 0 {
                    format!(
                        "The earlier file timestamp was staged for {staged_count} files. Save to apply."
                    )
                } else {
                    format!(
                        "The earlier file timestamp was staged for {staged_count} files. {skipped_count} files were skipped. Save to apply."
                    )
                };
                show_toast(&ui, &message);
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
                    show_toast(&ui, &format!("Staged {staged_count} modified date(s). Ctrl+S to save."));
                } else {
                    show_toast(&ui, &format!("Staged {staged_count}; skipped {skipped_count}. Ctrl+S to save."));
                }
            }
        });

        let app_handle = app.clone();
        let pending_created_dates_handle = pending_created_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let ui_handle = ui.as_weak();
        ui.on_set_file_dates_from_media_date(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            if ui.get_selected_file_count() == 0 {
                show_message(
                    &ui,
                    "Unable to Set File Dates",
                    "Select files before setting file dates.",
                );
                return;
            }
            if ui.get_selected_recyclable_count() != ui.get_selected_file_count() {
                show_message(
                    &ui,
                    "Unable to Set File Dates",
                    "Please select files only.",
                );
                return;
            }

            let selected_paths = {
                let app = app_handle.borrow();
                selected_file_paths(&app)
            };
            if selected_paths.is_empty() {
                show_message(
                    &ui,
                    "Unable to Set File Dates",
                    "Selected files could not be resolved.",
                );
                return;
            }

            let mut pending_created = HashMap::new();
            let mut pending_modified = HashMap::new();
            let mut display_dates = Vec::new();
            for path in &selected_paths {
                let media_date = scan_media_file(path).media_date;
                if let Ok(timestamp) = parse_timestamp(&media_date) {
                    pending_created.insert(path.clone(), timestamp);
                    pending_modified.insert(path.clone(), timestamp);
                    display_dates.push(media_date);
                }
            }

            if pending_created.is_empty() {
                show_message(
                    &ui,
                    "Unable to Set File Dates",
                    "No selected file has a usable Media Date.",
                );
                return;
            }

            let staged_count = pending_created.len();
            let skipped_count = selected_paths.len().saturating_sub(staged_count);
            *pending_created_dates_handle.borrow_mut() = pending_created;
            *pending_modified_dates_handle.borrow_mut() = pending_modified;

            if selected_paths.len() == 1 {
                let display_date = display_dates.pop().unwrap_or_default();
                ui.set_selected_created(display_date.clone().into());
                ui.set_selected_modified(display_date.into());
                ui.set_selected_created_status("".into());
                ui.set_selected_modified_status("".into());
            } else {
                ui.set_selected_created("".into());
                ui.set_selected_modified("".into());
                ui.set_selected_created_status("Mixed".into());
                ui.set_selected_modified_status("Mixed".into());
            }
            ui.set_selected_created_dirty(true);
            ui.set_selected_modified_dirty(true);
            ui.set_metadata_dirty(true);

            if skipped_count == 0 {
                show_toast(
                    &ui,
                    &format!(
                        "Staged Created and Modified dates for {staged_count} file(s). Ctrl+S to save."
                    ),
                );
            } else {
                show_toast(
                    &ui,
                    &format!(
                        "Staged {staged_count}; skipped {skipped_count} without Media Date. Ctrl+S to save."
                    ),
                );
            }
        });

        let app_handle = app.clone();
        let pending_renames = pending_filename_renames.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_created_dates_handle = pending_created_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let pending_exif_removals_handle = pending_exif_removals.clone();
        let ui_handle = ui.as_weak();
        ui.on_revert_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                pending_renames.borrow_mut().clear();
                pending_taken_dates.borrow_mut().clear();
                pending_created_dates_handle.borrow_mut().clear();
                pending_modified_dates_handle.borrow_mut().clear();
                pending_exif_removals_handle.borrow_mut().clear();
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
        ui.on_request_move_trailing_numbers(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let paths = selected_file_paths(&app_handle.borrow());
            if paths.is_empty() {
                show_message(&ui, "No Files Selected", "Select one or more files first.");
                return;
            }
            match trailing_number_rename_candidate_count(&paths) {
                Ok(0) => show_message(
                    &ui,
                    "No Matching Files",
                    "None of the selected files end with a delimited 2- or 3-digit number.",
                ),
                Ok(count) => {
                    ui.set_message_title("Confirm Filename Change".into());
                    ui.set_message_text(
                        format!(
                            "Move the trailing 2- or 3-digit number to the front for {count} selected file(s)? (Y/N)"
                        )
                        .into(),
                    );
                    ui.set_confirm_yes_selected(false);
                    ui.set_confirm_trailing_rename_visible(true);
                }
                Err(err) => show_message(&ui, "Filename Change Failed", &err),
            }
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let ui_handle = ui.as_weak();
        ui.on_confirm_move_trailing_numbers(move || {
            let Some(ui) = ui_handle.upgrade() else { return; };
            let result = {
                let mut app = app_handle.borrow_mut();
                let paths = selected_file_paths(&app);
                move_trailing_numbers_to_front(&paths).map(|count| {
                    app.load_folder();
                    count
                })
            };

            match result {
                Ok(count) => {
                    refresh(None);
                    show_message(
                        &ui,
                        "Filename Change Complete",
                        &format!("Renamed {count} file(s)."),
                    );
                }
                Err(err) => show_message(&ui, "Filename Change Failed", &err),
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

        let overwrite_confirmed = date_overwrite_confirmed.clone();
        let ui_handle = ui.as_weak();
        ui.on_confirm_date_overwrite(move |confirmed| {
            let Some(ui) = ui_handle.upgrade() else { return; };
            ui.set_message_title("Operation Failed".into());
            ui.set_message_text("".into());
            if confirmed {
                overwrite_confirmed.set(true);
                ui.invoke_apply_changes();
            }
        });

        let app_handle = app.clone();
        let refresh = refresh_ui.clone();
        let pending_renames = pending_filename_renames.clone();
        let pending_taken_dates = pending_filename_taken_dates.clone();
        let pending_created_dates_handle = pending_created_dates.clone();
        let pending_modified_dates_handle = pending_modified_dates.clone();
        let pending_exif_removals_handle = pending_exif_removals.clone();
        let pending_exif_tag_removals_handle = pending_exif_tag_removals.clone();
        let overwrite_confirmed = date_overwrite_confirmed.clone();
        let ui_handle = ui.as_weak();
        ui.on_apply_changes(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if !pending_renames.borrow().is_empty() {
                    let plan = pending_renames.borrow().clone();
                    let renamed_paths = match apply_filename_rename_plan(&plan) {
                        Ok(paths) => paths,
                        Err(err) => {
                            show_message(&ui, "Rename Failed", &err);
                            return;
                        }
                    };
                    let renamed_count = renamed_paths.len();
                    let refresh_path = renamed_paths.last().cloned();
                    pending_renames.borrow_mut().clear();
                    {
                        let mut app = app_handle.borrow_mut();
                        app.load_folder();
                    }
                    refresh(refresh_path);
                    show_toast(&ui, &format!("Renamed {renamed_count} file(s)."));
                    return;
                }

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

                trim_taken_date_before_save(&ui);
                trim_png_date_sources_before_save(&ui);

                let selected_path = {
                    let app = app_handle.borrow();
                    app.path_for_ui_index(ui.get_selected_index())
                };

                let selected_paths = {
                    let app = app_handle.borrow();
                    selected_file_paths(&app)
                };

                let has_staged_png_overwrite = selected_paths.iter().any(|path| {
                    if !is_png_path(path) {
                        return false;
                    }
                    let sources = read_png_date_sources(path);
                    let synchronized_change = pending_taken_dates.borrow().contains_key(path)
                        || (selected_paths.len() == 1 && ui.get_taken_date_dirty());
                    (synchronized_change && sources.has_existing_date())
                        || (selected_paths.len() == 1
                            && ((ui.get_png_creation_time_dirty()
                                && !sources.creation_time.is_empty())
                                || (ui.get_png_exif_date_time_original_dirty()
                                    && !sources.date_time_original.is_empty())))
                });
                if has_staged_png_overwrite && !overwrite_confirmed.replace(false) {
                    ui.set_message_title("Confirm Date Overwrite".into());
                    ui.set_message_text(
                        "Existing date metadata may be overwritten. Continue? (Y/N)".into(),
                    );
                    ui.set_confirm_yes_selected(false);
                    ui.set_confirm_date_overwrite_visible(true);
                    return;
                }
                overwrite_confirmed.set(false);

                if selected_paths.len() > 1 {
                    let mut refresh_path = selected_path.clone();
                    let pending_exif_removals_snapshot =
                        pending_exif_removals_handle.borrow().clone();
                    let pending_taken_dates_snapshot = pending_taken_dates.borrow().clone();
                    let pending_created_dates_snapshot =
                        pending_created_dates_handle.borrow().clone();
                    let pending_modified_dates_snapshot = pending_modified_dates_handle.borrow().clone();
                    let pending_taken_dates_arg = if pending_taken_dates_snapshot.is_empty() {
                        None
                    } else {
                        Some(&pending_taken_dates_snapshot)
                    };
                    let pending_modified_dates_arg = if pending_modified_dates_snapshot.is_empty() {
                        None
                    } else {
                        Some(&pending_modified_dates_snapshot)
                    };
                    let pending_created_dates_arg = if pending_created_dates_snapshot.is_empty() {
                        None
                    } else {
                        Some(&pending_created_dates_snapshot)
                    };
                    for path in &selected_paths {
                        if pending_exif_removals_snapshot.contains(path) {
                            if let Err(err) = remove_supported_metadata(
                                path,
                                ui.get_backup_before_changes(),
                            ) {
                                show_message(&ui, "Remove Metadata Failed", &err);
                                return;
                            }
                            continue;
                        }
                        match apply_metadata_changes_to_path(
                            &ui,
                            path,
                            pending_taken_dates_arg,
                            pending_created_dates_arg,
                            pending_modified_dates_arg,
                        ) {
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
                    pending_created_dates_handle.borrow_mut().clear();
                    pending_modified_dates_handle.borrow_mut().clear();
                    pending_exif_removals_handle.borrow_mut().clear();
                    pending_exif_tag_removals_handle.borrow_mut().clear();
                    update_metadata_dirty_state(&ui);
                    {
                        let mut app = app_handle.borrow_mut();
                        app.load_folder();
                    }
                    refresh(refresh_path);
                    return;
                }

                let apply_path = selected_path.clone();
                if let Some(path) = apply_path.as_ref() {
                    let staged_tag_removals = pending_exif_tag_removals_handle
                        .borrow()
                        .get(path)
                        .cloned()
                        .unwrap_or_default();
                    if !staged_tag_removals.is_empty() {
                        for key in &staged_tag_removals {
                            let result = if is_png_path(path) {
                                remove_png_date_source(path, key, ui.get_backup_before_changes())
                            } else {
                                remove_exif_tag(path, key, ui.get_backup_before_changes())
                            };
                            if let Err(err) = result {
                                show_message(&ui, "Remove Tag Failed", &err);
                                return;
                            }
                        }
                        pending_exif_tag_removals_handle.borrow_mut().remove(path);
                        if !has_exif_metadata_changes(&ui)
                            && !ui.get_selected_created_dirty()
                            && !ui.get_selected_modified_dirty()
                        {
                            {
                                let mut app = app_handle.borrow_mut();
                                app.load_folder();
                            }
                            refresh(selected_path);
                            show_toast(&ui, "Metadata tag(s) removed.");
                            return;
                        }
                    }
                    if pending_exif_removals_handle.borrow().contains(path) {
                        if let Err(err) =
                            remove_supported_metadata(path, ui.get_backup_before_changes())
                        {
                            show_message(&ui, "Remove Metadata Failed", &err);
                            return;
                        }
                        store_current_as_original(&ui);
                        pending_taken_dates.borrow_mut().clear();
                        pending_created_dates_handle.borrow_mut().clear();
                        pending_modified_dates_handle.borrow_mut().clear();
                        pending_exif_removals_handle.borrow_mut().clear();
                        pending_exif_tag_removals_handle.borrow_mut().clear();
                        update_metadata_dirty_state(&ui);
                        {
                            let mut app = app_handle.borrow_mut();
                            app.load_folder();
                        }
                        refresh(selected_path);
                        show_toast(&ui, "Metadata tags were removed.");
                        return;
                    }
                }
                if ui.get_selected_media_kind().as_str() == "mp4" && ui.get_taken_date_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if !is_mp4_path(path) {
                        show_message(&ui, "Apply Failed", "Selected file is not an MP4 video.");
                        return;
                    }
                    if let Err(err) = write_mp4_media_date(path, ui.get_taken_date().as_str()) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                    ui.set_original_taken_date(ui.get_taken_date());
                    ui.set_taken_date_dirty(false);
                    update_metadata_dirty_state(&ui);
                } else if ui.get_selected_media_kind().as_str() == "png"
                    && (ui.get_taken_date_dirty()
                        || ui.get_png_creation_time_dirty()
                        || ui.get_png_exif_date_time_original_dirty())
                {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    if !is_png_path(path) {
                        show_message(&ui, "Apply Failed", "Selected file is not a PNG image.");
                        return;
                    }
                    let result = if ui.get_taken_date_dirty() {
                        write_png_media_date(
                            path,
                            ui.get_taken_date().as_str(),
                            ui.get_backup_before_changes(),
                        )
                    } else {
                        let creation = ui
                            .get_png_creation_time_dirty()
                            .then(|| ui.get_png_creation_time());
                        let original = ui
                            .get_png_exif_date_time_original_dirty()
                            .then(|| ui.get_date_time_original());
                        write_png_date_sources(
                            path,
                            creation.as_ref().map(|value| value.as_str()),
                            original.as_ref().map(|value| value.as_str()),
                            ui.get_backup_before_changes(),
                        )
                    };
                    if let Err(err) = result {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                    if ui.get_taken_date_dirty() {
                        let synchronized = ui.get_taken_date();
                        ui.set_png_creation_time(synchronized.clone());
                        ui.set_date_time_original(synchronized.clone());
                        ui.set_original_taken_date(synchronized);
                        ui.set_taken_date_dirty(false);
                    }
                    update_png_date_source_conflict(&ui);
                    update_metadata_dirty_state(&ui);
                } else if has_exif_metadata_changes(&ui) {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };

                    let current_metadata = read_exif_metadata(path);
                    if !current_metadata.has_exif {
                        let metadata = collect_dirty_exif_metadata(&ui, current_metadata, None, false);
                        let backup_before_changes = ui.get_backup_before_changes();
                        let new_path = match rewrite_basic_exif_metadata(path, &metadata, backup_before_changes) {
                            Ok(path) => path,
                            Err(err) => {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
                        };
                        if backup_before_changes {
                            let backup_path = exif_backup_path(&new_path);
                            if let Err(err) = copy_file_times(&backup_path, &new_path) {
                                show_message(&ui, "Apply Failed", &err);
                                return;
                            }
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
                        let metadata = collect_dirty_exif_metadata(&ui, current_metadata, None, false);
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

                if ui.get_metadata_dirty() {
                    let Some(path) = apply_path.as_ref() else {
                        show_message(&ui, "Apply Failed", "Selected file could not be resolved.");
                        return;
                    };
                    let pending_created_dates_snapshot =
                        pending_created_dates_handle.borrow().clone();
                    let pending_modified_dates_snapshot =
                        pending_modified_dates_handle.borrow().clone();
                    let pending_created_dates_arg = (!pending_created_dates_snapshot.is_empty())
                        .then_some(&pending_created_dates_snapshot);
                    let pending_modified_dates_arg = (!pending_modified_dates_snapshot.is_empty())
                        .then_some(&pending_modified_dates_snapshot);
                    if let Err(err) = apply_metadata_changes_to_path(
                        &ui,
                        path,
                        None,
                        pending_created_dates_arg,
                        pending_modified_dates_arg,
                    ) {
                        show_message(&ui, "Apply Failed", &err);
                        return;
                    }
                }

                // Other metadata fields are UI-only until their EXIF writers are implemented.
                store_current_as_original(&ui);
                pending_taken_dates.borrow_mut().clear();
                pending_created_dates_handle.borrow_mut().clear();
                pending_modified_dates_handle.borrow_mut().clear();
                pending_exif_removals_handle.borrow_mut().clear();
                pending_exif_tag_removals_handle.borrow_mut().clear();
                update_metadata_dirty_state(&ui);
                {
                    let mut app = app_handle.borrow_mut();
                    app.load_folder();
                }
                refresh(selected_path);
            }
        });

        let previous_input = previous_taken_date_input.clone();
        let ui_handle = ui.as_weak();
        ui.on_taken_date_input_edited(move || {
            let Some(ui) = ui_handle.upgrade() else { return -1; };
            let current = ui.get_taken_date().to_string();
            let formatted = auto_format_date_edit(&previous_input.borrow(), &current);
            *previous_input.borrow_mut() = formatted.clone();
            if formatted != current {
                let formatted_cursor_position = i32::try_from(formatted.len()).unwrap_or(i32::MAX);
                ui.set_taken_date(formatted.into());
                formatted_cursor_position
            } else {
                -1
            }
        });

        let previous_input = previous_png_creation_time_input.clone();
        let mirrored_previous_input = previous_png_exif_original_input.clone();
        let ui_handle = ui.as_weak();
        ui.on_png_creation_time_input_edited(move || {
            let Some(ui) = ui_handle.upgrade() else { return -1; };
            let current = ui.get_png_creation_time().to_string();
            let previous = previous_input.borrow().clone();
            let formatted = auto_format_date_edit(&previous, &current);
            let opposite = ui.get_date_time_original().to_string();
            let should_follow = should_mirror_png_date_source(
                ui.get_original_png_exif_date_time_original().as_str(),
                &opposite,
                &previous,
            );
            *previous_input.borrow_mut() = formatted.clone();
            if formatted != current {
                ui.set_png_creation_time(formatted.clone().into());
            }
            if should_follow {
                ui.set_date_time_original(formatted.clone().into());
                *mirrored_previous_input.borrow_mut() = formatted.clone();
            }
            update_png_date_source_conflict(&ui);
            if formatted != current {
                i32::try_from(formatted.len()).unwrap_or(i32::MAX)
            } else {
                -1
            }
        });

        let previous_input = previous_png_exif_original_input.clone();
        let mirrored_previous_input = previous_png_creation_time_input.clone();
        let ui_handle = ui.as_weak();
        ui.on_png_exif_date_time_original_input_edited(move || {
            let Some(ui) = ui_handle.upgrade() else { return -1; };
            let current = ui.get_date_time_original().to_string();
            let previous = previous_input.borrow().clone();
            let formatted = auto_format_date_edit(&previous, &current);
            let opposite = ui.get_png_creation_time().to_string();
            let should_follow = should_mirror_png_date_source(
                ui.get_original_png_creation_time().as_str(),
                &opposite,
                &previous,
            );
            *previous_input.borrow_mut() = formatted.clone();
            if formatted != current {
                ui.set_date_time_original(formatted.clone().into());
            }
            if should_follow {
                ui.set_png_creation_time(formatted.clone().into());
                *mirrored_previous_input.borrow_mut() = formatted.clone();
            }
            update_png_date_source_conflict(&ui);
            if formatted != current {
                i32::try_from(formatted.len()).unwrap_or(i32::MAX)
            } else {
                -1
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
    set_selected_media_details(ui, app);

    let metadata = app
        .path_for_ui_index(index)
        .filter(|path| path.is_file() && is_jpeg_path(path))
        .map(|path| read_exif_metadata(&path))
        .unwrap_or_default();
    set_loaded_exif_metadata(ui, metadata);
    if let Some(path) = app.path_for_ui_index(index).filter(|path| path.is_file() && is_mp4_path(path)) {
        set_loaded_media_date(ui, scan_media_file(&path).media_date);
    }
    if let Some(path) = app.path_for_ui_index(index).filter(|path| path.is_file() && is_png_path(path)) {
        set_loaded_media_date(ui, scan_media_file(&path).media_date);
        set_loaded_png_date_sources(ui, &path);
    }
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

    if indices.len() > 5 {
        let (file_count, recyclable_count, has_dir) = app.selected_counts();
        let summary = format!("{} items selected", indices.len());
        ui.set_selected_index(*indices.last().unwrap_or(&-1));
        ui.set_selected_name(summary.clone().into());
        ui.set_selected_created(String::new().into());
        ui.set_selected_modified(String::new().into());
        ui.set_selected_name_status(String::new().into());
        ui.set_selected_created_status("Mixed".into());
        ui.set_selected_modified_status("Mixed".into());
        ui.set_original_selected_name(summary.into());
        ui.set_original_selected_created(String::new().into());
        ui.set_original_selected_modified(String::new().into());
        ui.set_selected_is_dir(has_dir);
        ui.set_selected_file_count(file_count as i32);
        ui.set_selected_recyclable_count(recyclable_count as i32);
        ui.set_selected_delete_message(delete_confirmation_message(recyclable_count as i32).into());
        set_media_details(ui, "mixed", "", "", "");
        set_loaded_exif_metadata(ui, ExifMetadata::default());
        return;
    }

    let mut names = Vec::new();
    let mut created_values = Vec::new();
    let mut modified_values = Vec::new();
    let mut has_dir = false;
    let mut metadata_values = Vec::new();
    let mut png_date_sources = Vec::new();

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
            .filter(|path| path.is_file() && is_jpeg_path(path))
            .map(|path| read_exif_metadata(&path))
            .unwrap_or_default();
        metadata_values.push(metadata);

        if let Some(path) = app
            .path_for_ui_index(*index)
            .filter(|path| path.is_file() && is_png_path(path))
        {
            png_date_sources.push(read_png_date_sources(&path));
        }
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
    set_selected_media_details(ui, app);

    set_loaded_exif_metadata(ui, join_metadata(&metadata_values));
    set_joined_metadata_statuses(ui, &metadata_values);
    if ui.get_selected_media_kind().as_str() == "png" {
        set_joined_png_date_sources(ui, &png_date_sources);
    }
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
    set_media_details(ui, "", "", "", "");
    ui.set_metadata_dirty(false);
    reset_metadata_dirty_flags(ui);
    set_loaded_exif_metadata(ui, ExifMetadata::default());
}

fn set_selected_media_details(ui: &MainWindow, app: &SlintApp) {
    let indices = app.selected_indices();
    if indices.is_empty() {
        set_media_details(ui, "", "", "", "");
        return;
    }

    if indices.len() == 1 {
        let (kind, media_type, media_date, metadata_status) = app.media_details_for_index(indices[0]);
        set_media_details(ui, &kind, &media_type, &media_date, &metadata_status);
        return;
    }

    let mut kinds: Vec<String> = indices
        .iter()
        .filter_map(|index| {
            let (kind, _, _, _) = app.media_details_for_index(*index);
            if kind == "folder" || kind.is_empty() {
                None
            } else {
                Some(kind)
            }
        })
        .collect();
    kinds.sort();
    kinds.dedup();

    if kinds.len() == 1 && kinds[0] == "jpeg" {
        set_media_details(ui, "jpeg", "", "", "");
    } else if kinds.len() == 1 && kinds[0] == "mp4" {
        set_media_details(ui, "mp4", "Multiple videos", "Mixed", "Mixed");
    } else if kinds.len() == 1 && kinds[0] == "png" {
        set_media_details(ui, "png", "Multiple PNG images", "Mixed", "Mixed");
    } else {
        set_media_details(ui, "mixed", "", "", "");
    }
}

fn set_media_details(
    ui: &MainWindow,
    kind: &str,
    media_type: &str,
    media_date: &str,
    metadata_status: &str,
) {
    ui.set_selected_media_kind(kind.into());
    ui.set_selected_media_type(media_type.into());
    ui.set_selected_media_date(media_date.into());
    ui.set_selected_metadata_status(metadata_status.into());
}

fn set_loaded_media_date(ui: &MainWindow, media_date: String) {
    let value = if media_date == "-" { String::new() } else { media_date };
    ui.set_taken_date(value.clone().into());
    ui.set_original_taken_date(value.clone().into());
    ui.set_taken_date_status(status_for_value(&value).into());
    ui.set_taken_date_dirty(false);
    ui.set_metadata_dirty(
        ui.get_selected_name_dirty()
            || ui.get_selected_created_dirty()
            || ui.get_selected_modified_dirty(),
    );
}

fn stage_taken_date_in_ui(ui: &MainWindow, value: &str) {
    ui.set_taken_date(value.into());
    if ui.get_selected_media_kind().as_str() == "png" {
        ui.set_png_creation_time(value.into());
        ui.set_date_time_original(value.into());
        update_png_date_source_conflict(ui);
    }
}

fn set_loaded_png_date_sources(ui: &MainWindow, path: &std::path::Path) {
    let sources = read_png_date_sources(path);
    ui.set_png_creation_time(sources.creation_time.clone().into());
    ui.set_original_png_creation_time(sources.creation_time.into());
    ui.set_date_time_original(sources.date_time_original.clone().into());
    ui.set_original_png_exif_date_time_original(sources.date_time_original.into());
    ui.set_date_time_digitized(sources.date_time_digitized.into());
    ui.set_image_date_time(sources.image_date_time.into());
    ui.set_png_creation_time_dirty(false);
    ui.set_png_exif_date_time_original_dirty(false);
    update_png_date_source_conflict(ui);
}

fn set_joined_png_date_sources(ui: &MainWindow, sources: &[PngDateSources]) {
    fn multiple_value(values: Vec<String>) -> String {
        let display = selection_display(values);
        if display.status == "Mixed" {
            "<Multiple values>".to_string()
        } else if display.value.is_empty() {
            "-".to_string()
        } else {
            display.value
        }
    }

    let creation_time = multiple_value(
        sources
            .iter()
            .map(|source| source.creation_time.clone())
            .collect(),
    );
    let date_time_original = multiple_value(
        sources
            .iter()
            .map(|source| source.date_time_original.clone())
            .collect(),
    );
    ui.set_png_creation_time(creation_time.clone().into());
    ui.set_original_png_creation_time(creation_time.into());
    ui.set_date_time_original(date_time_original.clone().into());
    ui.set_original_png_exif_date_time_original(date_time_original.into());
    ui.set_date_time_digitized(
        multiple_value(
            sources
                .iter()
                .map(|source| source.date_time_digitized.clone())
                .collect(),
        )
        .into(),
    );
    ui.set_image_date_time(
        multiple_value(
            sources
                .iter()
                .map(|source| source.image_date_time.clone())
                .collect(),
        )
        .into(),
    );
    ui.set_png_creation_time_dirty(false);
    ui.set_png_exif_date_time_original_dirty(false);
    ui.set_png_date_sources_conflict(false);
}

fn update_png_date_source_conflict(ui: &MainWindow) {
    let creation = ui.get_png_creation_time();
    let original = ui.get_date_time_original();
    ui.set_png_date_sources_conflict(
        !creation.trim().is_empty()
            && !original.trim().is_empty()
            && creation != original,
    );
}

fn set_exif_metadata(ui: &MainWindow, metadata: ExifMetadata) {
    let date_sources: Vec<&str> = [
        metadata.date_time_original.as_str(),
        metadata.date_time_digitized.as_str(),
        metadata.image_date_time.as_str(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect();
    let has_conflicting_date_sources = date_sources
        .first()
        .is_some_and(|first| date_sources.iter().skip(1).any(|value| value != first));

    ui.set_exif_available(metadata.has_exif);
    ui.set_png_creation_time("".into());
    ui.set_original_png_creation_time("".into());
    ui.set_original_png_exif_date_time_original("".into());
    ui.set_png_date_sources_conflict(false);
    ui.set_date_time_original(metadata.date_time_original.into());
    ui.set_date_time_digitized(metadata.date_time_digitized.into());
    ui.set_image_date_time(metadata.image_date_time.into());
    ui.set_exif_date_sources_visible(
        ui.get_selected_file_count() == 1 && has_conflicting_date_sources,
    );
    ui.set_taken_date(metadata.taken_date.into());
    ui.set_camera_make(metadata.camera_make.into());
    ui.set_camera_model(metadata.camera_model.into());
    ui.set_lens_model(metadata.lens_model.into());
    ui.set_software(metadata.software.into());
    ui.set_artist(metadata.artist.into());
    ui.set_image_description(metadata.image_description.into());
    ui.set_copyright(metadata.copyright.into());
    ui.set_exif_version(metadata.exif_version.into());
    ui.set_exposure_program(metadata.exposure_program.into());
    ui.set_white_balance(metadata.white_balance.into());
    ui.set_focal_length_35mm(metadata.focal_length_35mm.into());
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
        date_time_original: joined_selection_value(values.iter().map(|metadata| metadata.date_time_original.clone()).collect()),
        date_time_digitized: joined_selection_value(values.iter().map(|metadata| metadata.date_time_digitized.clone()).collect()),
        image_date_time: joined_selection_value(values.iter().map(|metadata| metadata.image_date_time.clone()).collect()),
        camera_make: joined_selection_value(values.iter().map(|metadata| metadata.camera_make.clone()).collect()),
        camera_model: joined_selection_value(values.iter().map(|metadata| metadata.camera_model.clone()).collect()),
        lens_model: joined_selection_value(values.iter().map(|metadata| metadata.lens_model.clone()).collect()),
        software: joined_selection_value(values.iter().map(|metadata| metadata.software.clone()).collect()),
        artist: joined_selection_value(values.iter().map(|metadata| metadata.artist.clone()).collect()),
        image_description: joined_selection_value(values.iter().map(|metadata| metadata.image_description.clone()).collect()),
        copyright: joined_selection_value(values.iter().map(|metadata| metadata.copyright.clone()).collect()),
        exif_version: joined_selection_value(values.iter().map(|metadata| metadata.exif_version.clone()).collect()),
        exposure_program: joined_selection_value(values.iter().map(|metadata| metadata.exposure_program.clone()).collect()),
        white_balance: joined_selection_value(values.iter().map(|metadata| metadata.white_balance.clone()).collect()),
        focal_length_35mm: joined_selection_value(values.iter().map(|metadata| metadata.focal_length_35mm.clone()).collect()),
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

fn filename_from_media_date(
    current_path: &std::path::Path,
    media_date: &str,
    show_extension: bool,
) -> Result<String, String> {
    filename_from_media_date_with_reserved(
        current_path,
        media_date,
        show_extension,
        &mut HashSet::new(),
    )
}

fn filename_from_media_date_with_reserved(
    current_path: &std::path::Path,
    media_date: &str,
    show_extension: bool,
    reserved_names: &mut HashSet<String>,
) -> Result<String, String> {
    let datetime = NaiveDateTime::parse_from_str(media_date.trim(), "%Y-%m-%d %H:%M:%S")
        .map_err(|_| "The selected file does not have a usable Media Date.".to_string())?;
    let base = datetime.format("%Y%m%d_%H%M%S").to_string();
    let extension = current_path.extension().and_then(|value| value.to_str());
    let parent = current_path
        .parent()
        .ok_or_else(|| "Selected file parent could not be resolved.".to_string())?;

    for index in 0..1000 {
        let stem = if index == 0 {
            base.clone()
        } else {
            format!("{base}_{index:03}")
        };
        let full_name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem}.{extension}"),
            _ => stem.clone(),
        };
        let candidate = parent.join(&full_name);
        let candidate_key = candidate.to_string_lossy().to_lowercase();
        if (candidate == current_path || !candidate.exists())
            && !reserved_names.contains(&candidate_key)
        {
            reserved_names.insert(candidate_key);
            return Ok(if show_extension { full_name } else { stem });
        }
    }

    Err("Could not find an available filename for the Media Date.".to_string())
}

fn apply_filename_rename_plan(
    plan: &HashMap<PathBuf, String>,
) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<_> = plan.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    let mut committed = Vec::with_capacity(entries.len());

    for (source, new_name) in entries {
        match rename_entry(source, new_name) {
            Ok(target) => committed.push((source.clone(), target)),
            Err(err) => {
                for (previous_source, previous_target) in committed.iter().rev() {
                    let _ = std::fs::rename(previous_target, previous_source);
                }
                return Err(format!("Failed to rename {}: {err}", source.display()));
            }
        }
    }

    Ok(committed
        .into_iter()
        .map(|(_, target)| target)
        .collect())
}

fn selected_file_paths(app: &SlintApp) -> Vec<PathBuf> {
    app.selected_indices()
        .iter()
        .filter_map(|index| app.path_for_ui_index(*index))
        .filter(|path| path.is_file())
        .collect()
}

fn is_jpeg_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
}

fn is_png_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

fn is_mp4_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
}

fn remove_supported_metadata(
    path: &std::path::Path,
    backup_before_changes: bool,
) -> Result<(), String> {
    if is_png_path(path) {
        remove_png_date_metadata(path, backup_before_changes)
    } else if is_jpeg_path(path) {
        remove_exif_metadata(path, backup_before_changes)
    } else {
        Err("Metadata removal is supported for JPEG and PNG files only.".to_string())
    }
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
    pending_created_dates: Option<&HashMap<PathBuf, SystemTime>>,
    pending_modified_dates: Option<&HashMap<PathBuf, SystemTime>>,
) -> Result<Option<PathBuf>, String> {
    let taken_date_override = pending_taken_dates
        .and_then(|values| values.get(path))
        .map(String::as_str);
    let has_pending_taken_date = taken_date_override.is_some();
    if is_mp4_path(path) {
        if has_exif_metadata_changes_without_taken_date(ui) {
            return Err("Only Media Date can be written to MP4 files.".to_string());
        }
        if let Some(taken_date) = taken_date_override {
            write_mp4_media_date(path, taken_date)?;
        } else if ui.get_selected_media_kind().as_str() == "mp4"
            && ui.get_taken_date_dirty()
            && pending_taken_dates.is_none()
        {
            write_mp4_media_date(path, ui.get_taken_date().as_str())?;
        }
        write_dirty_file_times(ui, path, pending_created_dates, pending_modified_dates)?;
        return Ok(None);
    }
    if is_png_path(path) {
        if has_exif_metadata_changes_without_taken_date(ui) {
            return Err("Only Media Date can be written to PNG files.".to_string());
        }
        if let Some(taken_date) = taken_date_override {
            write_png_media_date(path, taken_date, ui.get_backup_before_changes())?;
        } else if ui.get_selected_media_kind().as_str() == "png" && ui.get_taken_date_dirty() && pending_taken_dates.is_none() {
            write_png_media_date(
                path,
                ui.get_taken_date().as_str(),
                ui.get_backup_before_changes(),
            )?;
        }
        write_dirty_file_times(ui, path, pending_created_dates, pending_modified_dates)?;
        return Ok(None);
    }

    let has_exif_changes = if pending_taken_dates.is_some() && !has_pending_taken_date {
        has_exif_metadata_changes_without_taken_date(ui)
    } else {
        has_exif_metadata_changes(ui)
    };

    let target_path = if has_exif_changes || has_pending_taken_date {
        let current_metadata = read_exif_metadata(path);
        if !current_metadata.has_exif {
            let metadata = collect_dirty_exif_metadata(ui, current_metadata, taken_date_override, pending_taken_dates.is_some());
            let backup_before_changes = ui.get_backup_before_changes();
            let new_path = rewrite_basic_exif_metadata(path, &metadata, backup_before_changes)?;
            if backup_before_changes {
                let backup_path = exif_backup_path(&new_path);
                copy_file_times(&backup_path, &new_path)?;
            }
            Some(new_path)
        } else if is_generated_new_exif_path(path) {
            let metadata = collect_dirty_exif_metadata(ui, current_metadata, taken_date_override, pending_taken_dates.is_some());
            rewrite_generated_basic_exif_metadata(path, &metadata)?;
            None
        } else {
            if let Err(err) = write_dirty_exif_tags(ui, path, taken_date_override, pending_taken_dates.is_some()) {
                if is_missing_writable_exif_tag_error(&err) {
                    let current_metadata = read_exif_metadata(path);
                    let metadata = collect_dirty_exif_metadata(ui, current_metadata, taken_date_override, pending_taken_dates.is_some());
                    rewrite_repairable_exif_metadata(
                        path,
                        &metadata,
                        ui.get_backup_before_changes(),
                    )?;
                } else {
                    return Err(err);
                }
            }
            None
        }
    } else {
        None
    };

    let file_time_path = target_path.as_deref().unwrap_or(path);
    write_dirty_file_times(
        ui,
        file_time_path,
        pending_created_dates,
        pending_modified_dates,
    )?;
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

fn is_missing_writable_exif_tag_error(err: &str) -> bool {
    err.contains("No writable EXIF tag was found")
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
    pending_created_dates: Option<&HashMap<PathBuf, SystemTime>>,
    pending_modified_dates: Option<&HashMap<PathBuf, SystemTime>>,
) -> Result<(), String> {
    let pending_created_time = pending_created_dates.and_then(|values| values.get(path).copied());
    let pending_modified_time = pending_modified_dates.and_then(|values| values.get(path).copied());
    let has_pending_created_map = pending_created_dates.is_some();
    let has_pending_modified_map = pending_modified_dates.is_some();

    if (has_pending_created_map && pending_created_time.is_none())
        || (has_pending_modified_map && pending_modified_time.is_none())
    {
        return Ok(());
    }

    if !ui.get_selected_created_dirty()
        && !ui.get_selected_modified_dirty()
        && pending_created_time.is_none()
        && pending_modified_time.is_none()
    {
        return Ok(());
    }

    let created_time = if let Some(created_time) = pending_created_time {
        Some(created_time)
    } else if ui.get_selected_created_dirty() && !has_pending_created_map {
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
        date_time_original: ui.get_date_time_original().to_string(),
        date_time_digitized: ui.get_date_time_digitized().to_string(),
        image_date_time: ui.get_image_date_time().to_string(),
        camera_make: ui.get_camera_make().to_string(),
        camera_model: ui.get_camera_model().to_string(),
        lens_model: ui.get_lens_model().to_string(),
        software: ui.get_software().to_string(),
        artist: ui.get_artist().to_string(),
        image_description: ui.get_image_description().to_string(),
        copyright: ui.get_copyright().to_string(),
        exif_version: ui.get_exif_version().to_string(),
        exposure_program: ui.get_exposure_program().to_string(),
        white_balance: ui.get_white_balance().to_string(),
        focal_length_35mm: ui.get_focal_length_35mm().to_string(),
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

fn collect_dirty_exif_metadata(
    ui: &MainWindow,
    mut metadata: ExifMetadata,
    taken_date_override: Option<&str>,
    using_pending_taken_dates: bool,
) -> ExifMetadata {
    metadata.has_exif = true;
    if let Some(taken_date) = taken_date_override {
        metadata.taken_date = taken_date.to_string();
    } else if ui.get_taken_date_dirty() && !using_pending_taken_dates {
        metadata.taken_date = ui.get_taken_date().to_string();
    }
    if ui.get_camera_make_dirty() {
        metadata.camera_make = ui.get_camera_make().to_string();
    }
    if ui.get_camera_model_dirty() {
        metadata.camera_model = ui.get_camera_model().to_string();
    }
    if ui.get_lens_model_dirty() {
        metadata.lens_model = ui.get_lens_model().to_string();
    }
    if ui.get_software_dirty() {
        metadata.software = ui.get_software().to_string();
    }
    if ui.get_artist_dirty() {
        metadata.artist = ui.get_artist().to_string();
    }
    if ui.get_shutter_speed_dirty() {
        metadata.shutter_speed = ui.get_shutter_speed().to_string();
    }
    if ui.get_aperture_dirty() {
        metadata.aperture = ui.get_aperture().to_string();
    }
    if ui.get_iso_speed_dirty() {
        metadata.iso_speed = ui.get_iso_speed().to_string();
    }
    if ui.get_focal_length_dirty() {
        metadata.focal_length = ui.get_focal_length().to_string();
    }
    if ui.get_flash_fired_dirty() {
        metadata.flash_fired = ui.get_flash_fired().to_string();
    }
    if ui.get_metering_mode_dirty() {
        metadata.metering_mode = ui.get_metering_mode().to_string();
    }
    if ui.get_orientation_dirty() {
        metadata.orientation = ui.get_orientation().to_string();
    }
    if ui.get_color_space_dirty() {
        metadata.color_space = ui.get_color_space().to_string();
    }
    if ui.get_gps_latitude_dirty() {
        metadata.gps_latitude = ui.get_gps_latitude().to_string();
    }
    if ui.get_gps_longitude_dirty() {
        metadata.gps_longitude = ui.get_gps_longitude().to_string();
    }
    if ui.get_gps_altitude_dirty() {
        metadata.gps_altitude = ui.get_gps_altitude().to_string();
    }
    metadata
}

fn parse_media_date_shift(days: &str, hours: &str, minutes: &str, seconds: &str) -> Result<ChronoDuration, String> {
    fn parse_part(value: &str, label: &str) -> Result<i128, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(0);
        }
        let parsed = trimmed
            .parse::<i128>()
            .map_err(|_| format!("{label} must be a non-negative whole number."))?;
        if parsed < 0 {
            return Err(format!("{label} must be a non-negative whole number."));
        }
        Ok(parsed)
    }

    let days = parse_part(days, "Days")?;
    let hours = parse_part(hours, "Hours")?;
    let minutes = parse_part(minutes, "Minutes")?;
    let seconds = parse_part(seconds, "Seconds")?;
    let total = days
        .checked_mul(86_400)
        .and_then(|value| value.checked_add(hours.checked_mul(3_600)?))
        .and_then(|value| value.checked_add(minutes.checked_mul(60)?))
        .and_then(|value| value.checked_add(seconds))
        .ok_or_else(|| "The requested time shift is too large.".to_string())?;
    let seconds = i64::try_from(total).map_err(|_| "The requested time shift is too large.".to_string())?;
    ChronoDuration::try_seconds(seconds).ok_or_else(|| "The requested time shift is too large.".to_string())
}

fn media_date_for_shift(path: &std::path::Path, pending: &HashMap<PathBuf, String>) -> String {
    if let Some(value) = pending.get(path) {
        return value.clone();
    }
    if is_mp4_path(path) || is_png_path(path) {
        scan_media_file(path).media_date
    } else if is_jpeg_path(path) {
        read_exif_metadata(path).taken_date
    } else {
        String::new()
    }
}

fn shift_display_datetime(value: &str, duration: ChronoDuration, subtract: bool) -> Option<String> {
    let datetime = parse_display_datetime_or_date(value.trim())?;
    let shifted = if subtract {
        datetime.checked_sub_signed(duration)?
    } else {
        datetime.checked_add_signed(duration)?
    };
    Some(shifted.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn build_shift_media_date_preview(
    paths: &[PathBuf],
    pending: &HashMap<PathBuf, String>,
    days: &str,
    hours: &str,
    minutes: &str,
    seconds: &str,
    subtract: bool,
) -> String {
    let duration = match parse_media_date_shift(days, hours, minutes, seconds) {
        Ok(duration) => duration,
        Err(err) => return err,
    };
    let mut lines = Vec::new();
    let mut valid_count = 0usize;
    let mut skipped_count = 0usize;
    for path in paths {
        if !is_jpeg_path(path) && !is_png_path(path) && !is_mp4_path(path) {
            skipped_count += 1;
            continue;
        }
        let current = media_date_for_shift(path, pending);
        let Some(shifted) = shift_display_datetime(&current, duration, subtract) else {
            skipped_count += 1;
            continue;
        };
        valid_count += 1;
        if lines.len() < 3 {
            let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("(unknown)");
            lines.push(format!("{name}: {current}  ->  {shifted}"));
        }
    }
    if lines.is_empty() {
        return format!("No usable Media Date found.  Skipped: {skipped_count}");
    }
    lines.push(format!("Ready: {valid_count} file(s)   Skipped: {skipped_count}"));
    lines.join("\n")
}

fn parse_timestamp(value: &str) -> Result<SystemTime, String> {
    if value.trim().is_empty() || value == "N/A" || value == "-" {
        return Err("Invalid timestamp format.".to_string());
    }

    let naive = parse_display_datetime_or_date(value)
        .ok_or_else(|| "Expected datetime format: YYYY-MM-DD or YYYY-MM-DD HH:MM:SS".to_string())?;

    Local
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| "Invalid or ambiguous local datetime.".to_string())
        .map(|dt| dt.into())
}

fn parse_display_datetime_or_date(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
}

fn earliest_file_timestamp(path: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let timestamp = earliest_available_timestamp(
        metadata.created().ok(),
        metadata.modified().ok(),
    )?;
    let datetime: chrono::DateTime<Local> = timestamp.into();
    Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn earliest_available_timestamp(
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> Option<SystemTime> {
    match (created, modified) {
        (Some(created), Some(modified)) => Some(created.min(modified)),
        (Some(created), None) => Some(created),
        (None, Some(modified)) => Some(modified),
        (None, None) => None,
    }
}

fn auto_format_date_input(value: &str) -> String {
    let date_end = value
        .char_indices()
        .find_map(|(index, ch)| (ch.is_whitespace() || ch == 'T').then_some(index))
        .unwrap_or(value.len());
    let date = &value[..date_end];
    let suffix = &value[date_end..];
    if date.is_empty()
        || !date.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        return value.to_string();
    }

    let digits: String = date.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() < 4 || digits.len() > 8 {
        return value.to_string();
    }

    let formatted_date = match digits.len() {
        4 => format!("{digits}-"),
        5 => format!("{}-{}", &digits[..4], &digits[4..]),
        6 => format!("{}-{}-", &digits[..4], &digits[4..]),
        _ => format!("{}-{}-{}", &digits[..4], &digits[4..6], &digits[6..]),
    };
    let formatted_suffix = if suffix.is_empty() {
        String::new()
    } else {
        let separator_len = suffix
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
        let separator = &suffix[..separator_len];
        let time = &suffix[separator_len..];
        if time.is_empty() {
            suffix.to_string()
        } else if time.bytes().all(|byte| byte.is_ascii_digit() || byte == b':') {
            let digits: String = time.chars().filter(|ch| ch.is_ascii_digit()).collect();
            if digits.len() > 6 {
                suffix.to_string()
            } else {
                let formatted_time = match digits.len() {
                    0 | 1 => digits,
                    2 => format!("{digits}:"),
                    3 => format!("{}:{}", &digits[..2], &digits[2..]),
                    4 => format!("{}:{}:", &digits[..2], &digits[2..]),
                    _ => format!("{}:{}:{}", &digits[..2], &digits[2..4], &digits[4..]),
                };
                format!("{separator}{formatted_time}")
            }
        } else {
            suffix.to_string()
        }
    };

    format!("{formatted_date}{formatted_suffix}")
}

fn auto_format_date_edit(previous: &str, current: &str) -> String {
    if previous
        .strip_suffix('-')
        .or_else(|| previous.strip_suffix(':'))
        .is_some_and(|without_separator| without_separator == current)
    {
        current.to_string()
    } else {
        auto_format_date_input(current)
    }
}

fn should_mirror_png_date_source(
    original_opposite: &str,
    current_opposite: &str,
    previous_source: &str,
) -> bool {
    original_opposite.trim().is_empty()
        && (current_opposite.trim().is_empty() || current_opposite == previous_source)
}

fn trim_taken_date_before_save(ui: &MainWindow) {
    let current = ui.get_taken_date().to_string();
    let trimmed = current.trim();
    let normalized = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| trimmed.to_string());
    if normalized != current {
        ui.set_taken_date(normalized.into());
        update_metadata_dirty_state(ui);
    }
}

fn trim_png_date_sources_before_save(ui: &MainWindow) {
    fn normalize(value: &str) -> String {
        let trimmed = value.trim();
        NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|datetime| datetime.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| trimmed.to_string())
    }

    let creation = ui.get_png_creation_time().to_string();
    let normalized_creation = normalize(&creation);
    if normalized_creation != creation {
        ui.set_png_creation_time(normalized_creation.into());
    }
    let original = ui.get_date_time_original().to_string();
    let normalized_original = normalize(&original);
    if normalized_original != original {
        ui.set_date_time_original(normalized_original.into());
    }
    update_png_date_source_conflict(ui);
    update_metadata_dirty_state(ui);
}

fn should_apply_taken_date_candidate(existing: &str, candidate: &str) -> bool {
    let existing = existing.trim();
    if existing.is_empty() || existing == "N/A" || existing == "-" {
        return true;
    }

    let Some(existing_datetime) = parse_display_datetime_or_date(existing) else {
        return true;
    };
    let Some(candidate_datetime) = parse_display_datetime_or_date(candidate) else {
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
    ui.set_png_creation_time_dirty(false);
    ui.set_png_exif_date_time_original_dirty(false);
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
    ui.set_original_png_creation_time(ui.get_png_creation_time());
    ui.set_original_png_exif_date_time_original(ui.get_date_time_original());
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
    let png_creation_time_dirty =
        ui.get_png_creation_time() != ui.get_original_png_creation_time();
    let png_exif_date_time_original_dirty =
        ui.get_date_time_original() != ui.get_original_png_exif_date_time_original();

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
    ui.set_png_creation_time_dirty(png_creation_time_dirty);
    ui.set_png_exif_date_time_original_dirty(png_exif_date_time_original_dirty);

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
            || gps_altitude_dirty
            || png_creation_time_dirty
            || png_exif_date_time_original_dirty,
    );
}

fn show_toast(ui: &MainWindow, message: &str) {
    ui.set_activity_text(message.into());
}

fn stage_or_confirm_exif_paste(
    ui: &MainWindow,
    values: Vec<(String, String)>,
    pending: &Rc<RefCell<Option<PendingExifPaste>>>,
) -> bool {
    let has_existing_value = values.iter().any(|(key, _)| {
        let target_value = get_exif_value(ui, key);
        !target_value.trim().is_empty()
    });

    if has_existing_value {
        *pending.borrow_mut() = Some(PendingExifPaste { values });
        ui.set_message_title("Confirm Paste".into());
        ui.set_message_text("Destination contains a value. Paste anyway? (Y/N)".into());
        ui.set_confirm_yes_selected(false);
        ui.set_confirm_metadata_paste_visible(true);
        false
    } else {
        apply_exif_paste(ui, &values);
        show_toast(ui, "Pasted. Save to apply.");
        true
    }
}

fn apply_exif_paste(ui: &MainWindow, values: &[(String, String)]) {
    for (key, value) in values {
        set_exif_value(ui, key, value);
    }
    update_metadata_dirty_state(ui);
}

fn collect_exif_section(ui: &MainWindow, section: &str) -> Vec<(String, String)> {
    let keys: &[&str] = match section {
        "camera" => &["taken_date", "camera_make", "camera_model", "lens_model", "software", "artist"],
        "exposure" => &["shutter_speed", "aperture", "iso_speed", "focal_length", "flash_fired", "metering_mode"],
        "image" => &["orientation", "color_space"],
        "location" => &["gps_latitude", "gps_longitude", "gps_altitude"],
        _ => &[],
    };
    keys.iter()
        .map(|key| ((*key).to_string(), get_exif_value(ui, key)))
        .collect()
}

fn is_writable_exif_section(section: &str) -> bool {
    matches!(section, "camera" | "exposure" | "image")
}

fn is_writable_exif_key(key: &str) -> bool {
    matches!(
        key,
        "taken_date"
            | "camera_make"
            | "camera_model"
            | "lens_model"
            | "software"
            | "artist"
            | "shutter_speed"
            | "aperture"
            | "iso_speed"
            | "focal_length"
            | "flash_fired"
            | "metering_mode"
            | "orientation"
            | "color_space"
    )
}

fn is_writable_png_key(key: &str) -> bool {
    matches!(key, "png_creation_time" | "date_time_original")
}

fn is_removable_exif_key(key: &str) -> bool {
    matches!(
        key,
        "taken_date"
            | "date_time_original"
            | "date_time_digitized"
            | "image_date_time"
            | "camera_make"
            | "camera_model"
            | "lens_model"
            | "software"
            | "artist"
            | "shutter_speed"
            | "aperture"
            | "iso_speed"
            | "focal_length"
            | "flash_fired"
            | "metering_mode"
            | "image_width"
            | "image_height"
            | "orientation"
            | "color_space"
            | "gps_latitude"
            | "gps_longitude"
            | "gps_altitude"
            | "image_description"
            | "copyright"
            | "exif_version"
            | "exposure_program"
            | "white_balance"
            | "focal_length_35mm"
    )
}

fn is_writable_metadata_key(ui: &MainWindow, key: &str) -> bool {
    is_writable_exif_key(key)
        || (ui.get_selected_media_kind().as_str() == "png"
            && ui.get_selected_file_count() == 1
            && is_writable_png_key(key))
}

fn exif_key_label(key: &str) -> &'static str {
    match key {
        "taken_date" => "Taken Date",
        "png_creation_time" => "Creation Time",
        "date_time_original" => "DateTime Original",
        "date_time_digitized" => "DateTime Digitized",
        "image_date_time" => "Image Modified DateTime",
        "camera_make" => "Camera Make",
        "camera_model" => "Camera Model",
        "lens_model" => "Lens Model",
        "software" => "Software",
        "artist" => "Artist",
        "image_description" => "Image Description",
        "copyright" => "Copyright",
        "exif_version" => "EXIF Version",
        "exposure_program" => "Exposure Program",
        "white_balance" => "White Balance",
        "focal_length_35mm" => "35mm Focal Length",
        "shutter_speed" => "Shutter Speed",
        "aperture" => "Aperture",
        "iso_speed" => "ISO Speed",
        "focal_length" => "Focal Length",
        "flash_fired" => "Flash Fired",
        "metering_mode" => "Metering Mode",
        "orientation" => "Orientation",
        "color_space" => "Color Space",
        _ => "EXIF Tag",
    }
}

fn set_exif_key_dirty(ui: &MainWindow, key: &str, dirty: bool) {
    match key {
        "taken_date" => ui.set_taken_date_dirty(dirty),
        "camera_make" => ui.set_camera_make_dirty(dirty),
        "camera_model" => ui.set_camera_model_dirty(dirty),
        "lens_model" => ui.set_lens_model_dirty(dirty),
        "software" => ui.set_software_dirty(dirty),
        "artist" => ui.set_artist_dirty(dirty),
        "shutter_speed" => ui.set_shutter_speed_dirty(dirty),
        "aperture" => ui.set_aperture_dirty(dirty),
        "iso_speed" => ui.set_iso_speed_dirty(dirty),
        "focal_length" => ui.set_focal_length_dirty(dirty),
        "flash_fired" => ui.set_flash_fired_dirty(dirty),
        "metering_mode" => ui.set_metering_mode_dirty(dirty),
        "image_width" => ui.set_image_width_dirty(dirty),
        "image_height" => ui.set_image_height_dirty(dirty),
        "orientation" => ui.set_orientation_dirty(dirty),
        "color_space" => ui.set_color_space_dirty(dirty),
        "gps_latitude" => ui.set_gps_latitude_dirty(dirty),
        "gps_longitude" => ui.set_gps_longitude_dirty(dirty),
        "gps_altitude" => ui.set_gps_altitude_dirty(dirty),
        _ => {}
    }
}

fn exif_section_label(section: &str) -> &'static str {
    match section {
        "camera" => "CAMERA",
        "exposure" => "EXPOSURE",
        "image" => "IMAGE",
        "location" => "LOCATION",
        _ => "EXIF",
    }
}

fn get_exif_value(ui: &MainWindow, key: &str) -> String {
    match key {
        "taken_date" => ui.get_taken_date().to_string(),
        "png_creation_time" => ui.get_png_creation_time().to_string(),
        "date_time_original" => ui.get_date_time_original().to_string(),
        "date_time_digitized" => ui.get_date_time_digitized().to_string(),
        "image_date_time" => ui.get_image_date_time().to_string(),
        "camera_make" => ui.get_camera_make().to_string(),
        "camera_model" => ui.get_camera_model().to_string(),
        "lens_model" => ui.get_lens_model().to_string(),
        "software" => ui.get_software().to_string(),
        "artist" => ui.get_artist().to_string(),
        "image_description" => ui.get_image_description().to_string(),
        "copyright" => ui.get_copyright().to_string(),
        "exif_version" => ui.get_exif_version().to_string(),
        "exposure_program" => ui.get_exposure_program().to_string(),
        "white_balance" => ui.get_white_balance().to_string(),
        "focal_length_35mm" => ui.get_focal_length_35mm().to_string(),
        "shutter_speed" => ui.get_shutter_speed().to_string(),
        "aperture" => ui.get_aperture().to_string(),
        "iso_speed" => ui.get_iso_speed().to_string(),
        "focal_length" => ui.get_focal_length().to_string(),
        "flash_fired" => ui.get_flash_fired().to_string(),
        "metering_mode" => ui.get_metering_mode().to_string(),
        "image_width" => ui.get_image_width().to_string(),
        "image_height" => ui.get_image_height().to_string(),
        "orientation" => ui.get_orientation().to_string(),
        "color_space" => ui.get_color_space().to_string(),
        "gps_latitude" => ui.get_gps_latitude().to_string(),
        "gps_longitude" => ui.get_gps_longitude().to_string(),
        "gps_altitude" => ui.get_gps_altitude().to_string(),
        _ => String::new(),
    }
}

fn set_exif_value(ui: &MainWindow, key: &str, value: &str) {
    match key {
        "taken_date" => ui.set_taken_date(value.into()),
        "png_creation_time" => ui.set_png_creation_time(value.into()),
        "date_time_original" => ui.set_date_time_original(value.into()),
        "camera_make" => ui.set_camera_make(value.into()),
        "camera_model" => ui.set_camera_model(value.into()),
        "lens_model" => ui.set_lens_model(value.into()),
        "software" => ui.set_software(value.into()),
        "artist" => ui.set_artist(value.into()),
        "image_description" => ui.set_image_description(value.into()),
        "copyright" => ui.set_copyright(value.into()),
        "exif_version" => ui.set_exif_version(value.into()),
        "exposure_program" => ui.set_exposure_program(value.into()),
        "white_balance" => ui.set_white_balance(value.into()),
        "focal_length_35mm" => ui.set_focal_length_35mm(value.into()),
        "shutter_speed" => ui.set_shutter_speed(value.into()),
        "aperture" => ui.set_aperture(value.into()),
        "iso_speed" => ui.set_iso_speed(value.into()),
        "focal_length" => ui.set_focal_length(value.into()),
        "flash_fired" => ui.set_flash_fired(value.into()),
        "metering_mode" => ui.set_metering_mode(value.into()),
        "image_width" => ui.set_image_width(value.into()),
        "image_height" => ui.set_image_height(value.into()),
        "orientation" => ui.set_orientation(value.into()),
        "color_space" => ui.set_color_space(value.into()),
        "gps_latitude" => ui.set_gps_latitude(value.into()),
        "gps_longitude" => ui.set_gps_longitude(value.into()),
        "gps_altitude" => ui.set_gps_altitude(value.into()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_format_date_input,
        auto_format_date_edit,
        earliest_available_timestamp,
        apply_filename_rename_plan,
        filename_from_media_date,
        filename_from_media_date_with_reserved,
        parse_media_date_shift,
        shift_display_datetime,
        should_mirror_png_date_source,
    };
    use std::collections::{HashMap, HashSet};
    use std::time::{Duration, SystemTime};

    #[test]
    fn chooses_the_earlier_available_file_timestamp() {
        let created = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(
            earliest_available_timestamp(Some(created), Some(modified)),
            Some(modified)
        );
        assert_eq!(earliest_available_timestamp(Some(created), None), Some(created));
        assert_eq!(earliest_available_timestamp(None, Some(modified)), Some(modified));
    }

    #[test]
    fn shifts_media_dates_across_day_and_month_boundaries() {
        let shift = parse_media_date_shift("57", "10", "0", "0").unwrap();
        assert_eq!(
            shift_display_datetime("2015-05-15 03:00:00", shift, false).as_deref(),
            Some("2015-07-11 13:00:00")
        );
        assert_eq!(
            shift_display_datetime("2015-07-11 13:00:00", shift, true).as_deref(),
            Some("2015-05-15 03:00:00")
        );
    }

    #[test]
    fn rejects_negative_media_date_shift_parts() {
        assert!(parse_media_date_shift("0", "-1", "0", "0").is_err());
    }

    #[test]
    fn mirrors_png_date_edit_only_while_the_other_source_started_empty() {
        assert!(should_mirror_png_date_source("", "", "2015-"));
        assert!(should_mirror_png_date_source(
            "",
            "2015-12-",
            "2015-12-"
        ));
        assert!(!should_mirror_png_date_source(
            "2014-01-01 00:00:00",
            "2014-01-01 00:00:00",
            "2015-12-"
        ));
        assert!(!should_mirror_png_date_source(
            "",
            "2016-01-02 11:22:33",
            "2015-12-"
        ));
    }

    #[test]
    fn automatically_inserts_date_hyphens_while_typing_or_pasting() {
        assert_eq!(auto_format_date_input("2014"), "2014-");
        assert_eq!(auto_format_date_input("20260"), "2026-0");
        assert_eq!(auto_format_date_input("2026-05"), "2026-05-");
        assert_eq!(auto_format_date_input("2026-050"), "2026-05-0");
        assert_eq!(auto_format_date_input("20260509"), "2026-05-09");
        assert_eq!(auto_format_date_input("2026--05"), "2026-05-");
        assert_eq!(
            auto_format_date_input("20260509 12:34:56"),
            "2026-05-09 12:34:56"
        );
        assert_eq!(auto_format_date_input("2026-05-09"), "2026-05-09");
        assert_eq!(
            auto_format_date_input("2026-05-09 12"),
            "2026-05-09 12:"
        );
        assert_eq!(
            auto_format_date_input("2026-05-09 12:34"),
            "2026-05-09 12:34:"
        );
        assert_eq!(
            auto_format_date_input("2026-05-09 123456"),
            "2026-05-09 12:34:56"
        );
        assert_eq!(
            auto_format_date_input("2026-05-09 12::34"),
            "2026-05-09 12:34:"
        );
    }

    #[test]
    fn allows_backspace_to_remove_an_automatically_inserted_hyphen() {
        assert_eq!(auto_format_date_edit("2014-", "2014"), "2014");
        assert_eq!(auto_format_date_edit("2014-05-", "2014-05"), "2014-05");
        assert_eq!(auto_format_date_edit("201", "2014"), "2014-");
        assert_eq!(
            auto_format_date_edit("2014-05-09 12:", "2014-05-09 12"),
            "2014-05-09 12"
        );
    }

    #[test]
    fn filename_from_media_date_uses_compact_date_and_avoids_collisions() {
        let dir = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_media_filename_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let current = dir.join("original.jpg");
        let collision = dir.join("20120815_023000.jpg");
        std::fs::write(&current, []).unwrap();
        std::fs::write(&collision, []).unwrap();

        let result = filename_from_media_date(&current, "2012-08-15 02:30:00", true).unwrap();

        let _ = std::fs::remove_file(current);
        let _ = std::fs::remove_file(collision);
        let _ = std::fs::remove_dir(dir);
        assert_eq!(result, "20120815_023000_001.jpg");
    }

    #[test]
    fn filename_from_media_date_reserves_unique_names_for_a_batch() {
        let dir = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_media_filename_batch_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.jpg");
        let second = dir.join("second.jpg");
        std::fs::write(&first, []).unwrap();
        std::fs::write(&second, []).unwrap();

        let mut reserved = HashSet::new();
        let first_name = filename_from_media_date_with_reserved(
            &first,
            "2012-08-15 02:30:00",
            true,
            &mut reserved,
        )
        .unwrap();
        let second_name = filename_from_media_date_with_reserved(
            &second,
            "2012-08-15 02:30:00",
            true,
            &mut reserved,
        )
        .unwrap();
        let plan = HashMap::from([
            (first.clone(), first_name.clone()),
            (second.clone(), second_name.clone()),
        ]);

        let renamed = apply_filename_rename_plan(&plan).unwrap();

        assert_eq!(first_name, "20120815_023000.jpg");
        assert_eq!(second_name, "20120815_023000_001.jpg");
        assert_eq!(renamed.len(), 2);
        assert!(dir.join(first_name).is_file());
        assert!(dir.join(second_name).is_file());

        for path in renamed {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_dir(dir);
    }
}
