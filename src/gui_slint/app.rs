use super::FileEntry as UiFileEntry;
use crate::exif::{read_exif_metadata, ExifMetadata};
use crate::fs::{read_directory, FileSystemEntry};
use crate::media::{
    fallback_video_media_type, is_image_path, is_jpeg_path, is_mp4_path, is_mpeg_ts_path,
    is_png_path, is_video_path, MediaScanJob, MediaScanResult,
};
use slint::{ModelRc, StandardListViewItem, VecModel};
use std::cmp::Ordering as CmpOrdering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const DISPLAY_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const TIME_DISPLAY_RECORDED: i32 = 0;
const TIME_DISPLAY_KST: i32 = 2;

#[derive(Clone, Debug)]
pub struct CachedMediaScan {
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub result: MediaScanResult,
}

#[derive(Clone, Debug)]
pub struct SelectedEntryDetails {
    pub path: Option<PathBuf>,
    pub name: String,
    pub exact_size: String,
    pub created: String,
    pub modified: String,
    pub is_dir: bool,
    pub media_kind: String,
    pub media_type: String,
    pub media_date: String,
    pub metadata_status: String,
    pub time_interpretation: String,
    pub exif_metadata: Option<ExifMetadata>,
}

pub struct SlintApp {
    pub current_path: String,
    pub files: Vec<FileSystemEntry>,
    pub selected_indices: Vec<i32>,
    pub file_filter: i32,
    pub show_only_missing_media_date: bool,
    pub show_only_missing_time_zone_offset: bool,
    pub show_only_duplicate_media_date: bool,
    pub time_display_mode: i32,
    pub sort_column: i32,
    pub sort_direction: i32,
    pub scan_results: Arc<Mutex<HashMap<PathBuf, CachedMediaScan>>>,
    pub scan_epoch: Arc<AtomicU64>,
    selection_anchor: Option<i32>,
    dynamic_sort_ready: bool,
}

impl SlintApp {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string();

        let mut app = Self {
            current_path: current_dir,
            files: Vec::new(),
            selected_indices: Vec::new(),
            file_filter: 0,
            show_only_missing_media_date: false,
            show_only_missing_time_zone_offset: false,
            show_only_duplicate_media_date: false,
            time_display_mode: TIME_DISPLAY_KST,
            sort_column: 0,
            sort_direction: 0,
            scan_results: Arc::new(Mutex::new(HashMap::new())),
            scan_epoch: Arc::new(AtomicU64::new(0)),
            selection_anchor: None,
            dynamic_sort_ready: true,
        };
        app.load_folder();
        app
    }

    pub fn load_folder(&mut self) {
        self.scan_epoch.fetch_add(1, Ordering::Relaxed);
        self.scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        let path = std::path::Path::new(&self.current_path);
        match read_directory(path) {
            Ok(entries) => {
                self.files = entries;
                self.selected_indices.clear();
                self.selection_anchor = None;
            }
            Err(err) => {
                self.files.clear();
                self.selected_indices.clear();
                self.selection_anchor = None;
                eprintln!("Failed to read directory: {} ({err})", self.current_path);
            }
        }
    }

    /// Reloads filesystem fields without throwing away valid media scans.
    /// Changed files are deliberately left uncached so the regular background
    /// scanner refreshes them without blocking the UI thread after Save.
    pub fn reload_folder_after_changes(&mut self, changed_paths: &[PathBuf]) {
        self.scan_epoch.fetch_add(1, Ordering::Relaxed);
        let path = std::path::Path::new(&self.current_path);
        let Ok(entries) = read_directory(path) else {
            self.files.clear();
            self.selected_indices.clear();
            self.selection_anchor = None;
            return;
        };

        let changed: HashSet<&PathBuf> = changed_paths.iter().collect();
        let current: HashMap<_, _> = entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| (entry.path.clone(), (entry.size, entry.modified)))
            .collect();
        {
            let mut cache = self
                .scan_results
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.retain(|cached_path, cached| {
                !changed.contains(cached_path)
                    && current.get(cached_path).is_some_and(|(size, modified)| {
                        cached.size == *size && cached.modified == *modified
                    })
            });
        }

        self.files = entries;
        self.selected_indices.clear();
        self.selection_anchor = None;

        self.dynamic_sort_ready = false;
    }

    pub fn remove_deleted_paths(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        self.scan_epoch.fetch_add(1, Ordering::Relaxed);
        let deleted: HashSet<&PathBuf> = paths.iter().collect();
        self.files.retain(|entry| !deleted.contains(&entry.path));
        self.scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|path, _| !deleted.contains(path));
        self.selected_indices.clear();
        self.selection_anchor = None;
    }

    pub fn get_ui_model(&self) -> ModelRc<UiFileEntry> {
        let mut ui_items: Vec<UiFileEntry> = Vec::new();
        // Selection checks happen once per displayed row. Keeping the selected
        // indices in a Vec made every model rebuild O(rows * selections), which
        // became especially expensive while Ctrl+A and background scan refreshes
        // were active.
        let selected_indices: HashSet<i32> = self.selected_indices.iter().copied().collect();

        ui_items.push(UiFileEntry {
            name: "[..]".into(),
            size: "-".into(),
            modified: "-".into(),
            created: "-".into(),
            is_dir: true,
            selected: selected_indices.contains(&0),
            media_kind: "folder".into(),
            media_date: "-".into(),
            metadata_status: "-".into(),
            duplicate_name: false,
            duplicate_size: false,
            duplicate_modified: false,
            duplicate_media_date: false,
        });

        let visible_files = self.visible_files();
        let cache = self
            .scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (duplicate_groups, duplicate_cells) = if self.sort_direction != 0
            && matches!(self.sort_column, 1..=4)
            && (self.dynamic_sort_ready || !matches!(self.sort_column, 4 | 5))
        {
            let mut groups = HashMap::new();
            let mut cells = HashMap::new();
            for entry in &visible_files {
                if let Some(group_key) =
                    active_sort_value(entry, self.sort_column, self.time_display_mode, &cache)
                {
                    *groups.entry(group_key.clone()).or_insert(0usize) += 1;
                    for column in 1..=4 {
                        if let Some(value) =
                            active_sort_value(entry, column, self.time_display_mode, &cache)
                        {
                            *cells
                                .entry((group_key.clone(), column, value))
                                .or_insert(0usize) += 1;
                        }
                    }
                }
            }
            (groups, cells)
        } else {
            (HashMap::new(), HashMap::new())
        };
        for (visible_index, f) in visible_files.into_iter().enumerate() {
            let name_str = f
                .path
                .file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let ui_index = i32::try_from(visible_index + 1).unwrap_or(i32::MAX);

            let scan = scan_result_from_cache(&cache, f);
            let group_key = active_sort_value(f, self.sort_column, self.time_display_mode, &cache);
            let group_is_duplicate = group_key
                .as_ref()
                .and_then(|key| duplicate_groups.get(key))
                .is_some_and(|count| *count > 1);
            let duplicate_in_column = |column| {
                group_is_duplicate
                    && group_key.as_ref().is_some_and(|group| {
                        active_sort_value(f, column, self.time_display_mode, &cache)
                            .and_then(|value| {
                                duplicate_cells
                                    .get(&(group.clone(), column, value))
                                    .copied()
                            })
                            .is_some_and(|count| count > 1)
                    })
            };
            ui_items.push(UiFileEntry {
                name: name_str.into(),
                size: if f.is_dir {
                    "-".into()
                } else {
                    format_file_size(f.size).into()
                },
                modified: f
                    .modified
                    .map(format_time)
                    .unwrap_or_else(|| "-".into())
                    .into(),
                created: f
                    .created
                    .map(format_time)
                    .unwrap_or_else(|| "-".into())
                    .into(),
                is_dir: f.is_dir,
                selected: selected_indices.contains(&ui_index),
                media_kind: if f.is_dir {
                    "folder"
                } else {
                    scan.as_ref()
                        .map(|value| value.media_kind.as_str())
                        .unwrap_or("pending")
                }
                .into(),
                media_date: if f.is_dir {
                    "-".to_string()
                } else {
                    scan.as_ref()
                        .map(|value| display_media_date(value, self.time_display_mode).0)
                        .unwrap_or_else(|| "…".to_string())
                }
                .into(),
                metadata_status: if f.is_dir {
                    "-"
                } else {
                    scan.as_ref()
                        .map(|value| value.metadata_status.as_str())
                        .unwrap_or("…")
                }
                .into(),
                duplicate_name: duplicate_in_column(1),
                duplicate_size: duplicate_in_column(2),
                duplicate_modified: duplicate_in_column(3),
                duplicate_media_date: duplicate_in_column(4),
            });
        }

        ModelRc::new(VecModel::from(ui_items))
    }

    #[allow(dead_code)]
    pub fn get_table_model(&self) -> ModelRc<ModelRc<StandardListViewItem>> {
        let mut rows: Vec<ModelRc<StandardListViewItem>> = Vec::new();

        rows.push(ModelRc::new(VecModel::from(vec![
            StandardListViewItem::from("[..]"),
            StandardListViewItem::from("-"),
            StandardListViewItem::from("-"),
            StandardListViewItem::from("-"),
            StandardListViewItem::from("-"),
        ])));

        for f in self.visible_files() {
            let name = f
                .path
                .file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let display_name = if f.is_dir {
                format!("[{}]", name)
            } else {
                name
            };
            let size = if f.is_dir {
                "-".to_string()
            } else {
                format_file_size(f.size)
            };
            let modified = f
                .modified
                .map(format_time)
                .unwrap_or_else(|| "-".to_string());
            let scan = self.scan_result_for(f);
            let media_date = scan
                .as_ref()
                .map(|value| display_media_date(value, self.time_display_mode).0)
                .unwrap_or_else(|| "…".to_string());
            let metadata = scan
                .as_ref()
                .map(|value| value.metadata_status.as_str())
                .unwrap_or("…");

            rows.push(ModelRc::new(VecModel::from(vec![
                StandardListViewItem::from(display_name.as_str()),
                StandardListViewItem::from(size.as_str()),
                StandardListViewItem::from(modified.as_str()),
                StandardListViewItem::from(media_date.as_str()),
                StandardListViewItem::from(metadata),
            ])));
        }

        ModelRc::new(VecModel::from(rows))
    }

    pub fn file_count(&self) -> usize {
        self.visible_files()
            .iter()
            .filter(|entry| !entry.is_dir)
            .count()
    }

    pub fn folder_file_counts(&self) -> (usize, usize, usize) {
        let total = self.files.iter().filter(|entry| !entry.is_dir).count();
        let supported = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir && matches_file_filter(0, &entry.path))
            .count();
        (total, supported, total.saturating_sub(supported))
    }

    pub fn visible_entry_count(&self) -> usize {
        self.visible_files().len()
    }

    pub fn path_for_ui_index(&self, index: i32) -> Option<PathBuf> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some(PathBuf::from(&self.current_path).parent()?.to_path_buf());
        }

        self.visible_files()
            .get(idx - 1)
            .map(|entry| entry.path.clone())
    }

    pub fn ui_index_for_path(&self, path: &std::path::Path) -> Option<i32> {
        self.visible_files()
            .into_iter()
            .position(|entry| entry.path == path)
            .and_then(|index| i32::try_from(index + 1).ok())
    }

    pub fn ui_index_for_filename_prefix(&self, query: &str, current_index: i32) -> Option<i32> {
        let query = query.to_lowercase();
        if query.is_empty() {
            return None;
        }
        let visible = self.visible_files();
        if visible.is_empty() {
            return None;
        }

        let include_current = query.chars().count() > 1;
        let current_position = usize::try_from(current_index.saturating_sub(1)).unwrap_or(0);
        let start = if include_current {
            current_position.min(visible.len() - 1)
        } else if current_index > 0 {
            (current_position + 1) % visible.len()
        } else {
            0
        };

        (0..visible.len()).find_map(|offset| {
            let position = (start + offset) % visible.len();
            let name = visible[position].path.file_name()?.to_string_lossy();
            name.to_lowercase()
                .starts_with(&query)
                .then(|| i32::try_from(position + 1).ok())
                .flatten()
        })
    }

    pub fn ui_details_for_index(&self, index: i32) -> Option<(String, String, String, bool)> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some(("[..]".to_string(), "-".to_string(), "-".to_string(), true));
        }

        let visible = self.visible_files();
        let entry = visible.get(idx - 1)?;
        let name = entry
            .path
            .file_name()
            .map(|os_str| os_str.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let modified = entry
            .modified
            .map(format_time)
            .unwrap_or_else(|| "-".to_string());
        let created = entry
            .created
            .map(format_time)
            .unwrap_or_else(|| "-".to_string());

        Some((name, created, modified, entry.is_dir))
    }

    pub fn select_ui_index(&mut self, index: i32, ctrl: bool, shift: bool) {
        if index < 0
            // A visible row can never exceed the complete directory entry
            // count. Avoid rebuilding and sorting the visible list merely to
            // validate an index that came from that same UI model.
            || usize::try_from(index).map_or(true, |idx| idx > self.files.len())
        {
            self.selected_indices.clear();
            self.selection_anchor = None;
            return;
        }

        if shift {
            let anchor = self.selection_anchor.unwrap_or(index);
            let start = anchor.min(index);
            let end = anchor.max(index);
            self.selected_indices = (start..=end).collect();
        } else if ctrl {
            match self.selected_indices.binary_search(&index) {
                Ok(position) => {
                    self.selected_indices.remove(position);
                }
                Err(position) => {
                    self.selected_indices.insert(position, index);
                }
            }
            self.selection_anchor = Some(index);
        } else {
            self.selected_indices.clear();
            self.selected_indices.push(index);
            self.selection_anchor = Some(index);
        }

        // Shift produces an ordered range, Ctrl inserts at the binary-search
        // position, and a plain click contains one item. No full re-sort is
        // needed for every Ctrl click.
    }

    pub fn selected_indices(&self) -> &[i32] {
        &self.selected_indices
    }

    /// Takes one stable snapshot of every selected row. This deliberately
    /// performs filtering/sorting and locks the media cache only once; the
    /// Details panel used to repeat both operations for every displayed tag.
    pub fn selected_entry_details(&self) -> Vec<SelectedEntryDetails> {
        let visible = self.visible_files();
        let cache = self
            .scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        self.selected_indices
            .iter()
            .filter_map(|selected| {
                let index = usize::try_from(*selected).ok()?;
                if index == 0 {
                    return Some(SelectedEntryDetails {
                        path: PathBuf::from(&self.current_path)
                            .parent()
                            .map(PathBuf::from),
                        name: "[..]".to_string(),
                        exact_size: "-".to_string(),
                        created: "-".to_string(),
                        modified: "-".to_string(),
                        is_dir: true,
                        media_kind: "folder".to_string(),
                        media_type: "Folder".to_string(),
                        media_date: "-".to_string(),
                        metadata_status: "-".to_string(),
                        time_interpretation: String::new(),
                        exif_metadata: None,
                    });
                }

                let entry = *visible.get(index - 1)?;
                let name = entry
                    .path
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                let scan = scan_result_from_cache(&cache, entry);
                let (
                    media_kind,
                    media_type,
                    media_date,
                    metadata_status,
                    time_interpretation,
                    exif_metadata,
                ) = if entry.is_dir {
                    (
                        "folder".to_string(),
                        "Folder".to_string(),
                        "-".to_string(),
                        "-".to_string(),
                        String::new(),
                        None,
                    )
                } else if let Some(scan) = scan {
                    let (display_date, display_basis) =
                        display_media_date(&scan, self.time_display_mode);
                    (
                        scan.media_kind,
                        scan.media_type,
                        display_date,
                        media_metadata_label(&scan.metadata_status).to_string(),
                        display_basis,
                        scan.exif_metadata,
                    )
                } else if is_jpeg_path(&entry.path) {
                    (
                        "jpeg".to_string(),
                        "JPEG image".to_string(),
                        "Scanning...".to_string(),
                        "Scanning...".to_string(),
                        String::new(),
                        None,
                    )
                } else if is_png_path(&entry.path) {
                    (
                        "png".to_string(),
                        "PNG image".to_string(),
                        "Scanning...".to_string(),
                        "Scanning...".to_string(),
                        String::new(),
                        None,
                    )
                } else if is_mp4_path(&entry.path) {
                    let media_type = fallback_video_media_type(&entry.path);
                    (
                        "mp4".to_string(),
                        media_type.to_string(),
                        "Scanning...".to_string(),
                        "Scanning...".to_string(),
                        String::new(),
                        None,
                    )
                } else if is_mpeg_ts_path(&entry.path) {
                    (
                        "mts".to_string(),
                        "AVCHD transport stream".to_string(),
                        "Scanning...".to_string(),
                        "Scanning...".to_string(),
                        String::new(),
                        None,
                    )
                } else {
                    (
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        None,
                    )
                };

                Some(SelectedEntryDetails {
                    path: Some(entry.path.clone()),
                    name,
                    exact_size: if entry.is_dir {
                        "-".to_string()
                    } else {
                        format!("{} Byte", format_number_with_commas(entry.size))
                    },
                    created: entry
                        .created
                        .map(format_time)
                        .unwrap_or_else(|| "-".to_string()),
                    modified: entry
                        .modified
                        .map(format_time)
                        .unwrap_or_else(|| "-".to_string()),
                    is_dir: entry.is_dir,
                    media_kind,
                    media_type,
                    media_date,
                    metadata_status,
                    time_interpretation,
                    exif_metadata,
                })
            })
            .collect()
    }

    /// Returns the latest cached media fields for one visible UI row. The
    /// file-list model is a snapshot, while Details reads the cache directly;
    /// callers use this to keep the selected row in sync between full model
    /// refreshes.
    pub fn cached_media_fields_for_ui_index(&self, index: i32) -> Option<(String, String, String)> {
        let index = usize::try_from(index).ok()?;
        if index == 0 {
            return None;
        }
        let visible = self.visible_files();
        let entry = *visible.get(index - 1)?;
        if entry.is_dir {
            return None;
        }
        let scan = self.scan_result_for(entry)?;
        Some((scan.media_kind, scan.media_date, scan.metadata_status))
    }

    pub fn select_all_visible_entries(&mut self) {
        self.selected_indices = (1..=self.visible_files().len())
            .filter_map(|index| i32::try_from(index).ok())
            .collect();
        self.selection_anchor = self.selected_indices.first().copied();
    }

    pub fn select_files_without_exif(&mut self) {
        self.selected_indices = self
            .visible_files()
            .into_iter()
            .enumerate()
            .filter(|(_, entry)| {
                !entry.is_dir
                    && is_jpeg_path(&entry.path)
                    && !file_has_exif(&entry.path, entry.is_dir)
            })
            .filter_map(|(index, _)| i32::try_from(index + 1).ok())
            .collect();
        self.selection_anchor = self.selected_indices.first().copied();
    }

    pub fn prepare_scan_jobs(&self) -> (u64, Vec<MediaScanJob>) {
        let epoch = self.scan_epoch.load(Ordering::Relaxed);
        let selected_paths: HashSet<_> = self
            .selected_indices
            .iter()
            .filter_map(|index| self.path_for_ui_index(*index))
            .collect();
        // Dynamic date filters must not prevent their own background scan.
        // The duplicate-only view hides uncached rows, but every matching file
        // still has to be inspected before duplicate groups can be known.
        let visible_files: Vec<_> = if self.show_only_duplicate_media_date {
            self.files
                .iter()
                .filter(|entry| !entry.is_dir && matches_file_filter(self.file_filter, &entry.path))
                .collect()
        } else {
            self.visible_files()
        };
        let cache = self
            .scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut jobs: Vec<_> = visible_files
            .into_iter()
            .filter(|entry| !entry.is_dir)
            .filter(|entry| {
                cache.get(&entry.path).is_none_or(|cached| {
                    cached.size != entry.size || cached.modified != entry.modified
                })
            })
            .map(|entry| MediaScanJob {
                path: entry.path.clone(),
                size: entry.size,
                modified: entry.modified,
            })
            .collect();
        // Keep the same order the user sees in the file table. Selected rows
        // form the only leading group; the stable sort preserves display order
        // inside both groups.
        jobs.sort_by_key(|job| !selected_paths.contains(&job.path));
        (epoch, jobs)
    }

    pub fn scan_priority_paths_for_ui_range(&self, first: i32, last: i32) -> Vec<PathBuf> {
        let visible = self.visible_files();
        let mut paths = Vec::new();
        let mut seen = HashSet::new();
        // A small explicit selection should jump ahead of the viewport. For a
        // large selection (most notably Ctrl+A), prepare_scan_jobs() already
        // preserves display order for the whole selected group. Copying every
        // selected path into the live priority list made every scan dequeue
        // progressively more expensive without changing that order.
        if self.selected_indices.len() <= 5 {
            for index in &self.selected_indices {
                let Ok(index) = usize::try_from(*index) else {
                    continue;
                };
                let Some(entry) = index.checked_sub(1).and_then(|index| visible.get(index)) else {
                    continue;
                };
                if !entry.is_dir && seen.insert(entry.path.clone()) {
                    paths.push(entry.path.clone());
                }
            }
        }
        if last >= first {
            let first = first.max(1) as usize;
            let last = last.max(0) as usize;
            for entry in (first..=last).filter_map(|ui_index| visible.get(ui_index - 1)) {
                if !entry.is_dir && seen.insert(entry.path.clone()) {
                    paths.push(entry.path.clone());
                }
            }
        }
        paths
    }

    pub fn restart_scan(&self) {
        self.scan_epoch.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_dynamic_sort_ready(&mut self, ready: bool) {
        if self.dynamic_sort_ready == ready {
            return;
        }
        let selected_paths = self.selected_paths();
        self.dynamic_sort_ready = ready;
        self.restore_selected_paths(&selected_paths);
    }

    pub fn complete_dynamic_sort(&mut self) {
        if self.dynamic_sort_ready {
            return;
        }
        let selected_paths = self.selected_paths();
        self.dynamic_sort_ready = true;
        self.restore_selected_paths(&selected_paths);
    }

    pub fn cycle_sort(&mut self, column: i32) {
        let selected_paths = self.selected_paths();
        if self.sort_column != column || self.sort_direction == 0 {
            self.sort_column = column;
            self.sort_direction = 1;
        } else if self.sort_direction == 1 {
            self.sort_direction = 2;
        } else {
            self.sort_column = 0;
            self.sort_direction = 0;
        }
        self.restore_selected_paths(&selected_paths);
    }

    pub fn set_time_display_mode(&mut self, mode: i32) {
        let mode = if mode == 1 { 1 } else { TIME_DISPLAY_KST };
        if self.time_display_mode == mode {
            return;
        }
        let selected_paths = self.selected_paths();
        self.time_display_mode = mode;
        self.restore_selected_paths(&selected_paths);
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_indices
            .iter()
            .filter(|index| **index > 0)
            .filter_map(|index| self.path_for_ui_index(*index))
            .collect()
    }

    fn restore_selected_paths(&mut self, paths: &[PathBuf]) {
        self.selected_indices = paths
            .iter()
            .filter_map(|path| self.ui_index_for_path(path))
            .collect();
        self.selected_indices.sort_unstable();
        self.selected_indices.dedup();
        self.selection_anchor = self.selected_indices.first().copied();
    }

    fn scan_result_for(&self, entry: &FileSystemEntry) -> Option<MediaScanResult> {
        let cache = self
            .scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        scan_result_from_cache(&cache, entry)
    }

    fn visible_files(&self) -> Vec<&FileSystemEntry> {
        let cache = self
            .scan_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut date_counts = HashMap::<String, usize>::new();
        if self.show_only_duplicate_media_date {
            for entry in self
                .files
                .iter()
                .filter(|entry| !entry.is_dir && matches_file_filter(self.file_filter, &entry.path))
            {
                let Some(scan) = scan_result_from_cache(&cache, entry) else {
                    continue;
                };
                let displayed = display_media_date(&scan, self.time_display_mode).0;
                let date = displayed.trim();
                if !media_date_value_is_present(date) {
                    continue;
                }
                *date_counts.entry(date.to_string()).or_default() += 1;
            }
        }

        let dynamic_filter_enabled = self.show_only_missing_media_date
            || self.show_only_missing_time_zone_offset
            || self.show_only_duplicate_media_date;
        let mut visible: Vec<_> = self
            .files
            .iter()
            .filter(|entry| {
                if entry.is_dir {
                    return true;
                }
                if !matches_file_filter(self.file_filter, &entry.path) {
                    return false;
                }
                if !dynamic_filter_enabled {
                    return true;
                }
                let Some(scan) = scan_result_from_cache(&cache, entry) else {
                    return self.show_only_missing_media_date
                        || self.show_only_missing_time_zone_offset;
                };
                let is_missing = media_date_is_missing(Some(&scan));
                let is_missing_offset = scan.media_kind == "jpeg"
                    && !is_missing
                    && scan.recorded_offset_minutes.is_none();
                let displayed = display_media_date(&scan, self.time_display_mode).0;
                let is_duplicate = media_date_value_is_present(displayed.trim())
                    && date_counts
                        .get(displayed.trim())
                        .is_some_and(|count| *count > 1);
                (self.show_only_missing_media_date && is_missing)
                    || (self.show_only_missing_time_zone_offset && is_missing_offset)
                    || (self.show_only_duplicate_media_date && is_duplicate)
            })
            .collect();

        let dynamic_sort_allowed = self.dynamic_sort_ready || !matches!(self.sort_column, 4 | 5);
        if self.sort_direction != 0 && dynamic_sort_allowed {
            visible.sort_by(|left, right| {
                compare_file_entries(
                    left,
                    right,
                    self.sort_column,
                    self.sort_direction,
                    self.time_display_mode,
                    &cache,
                )
            });
        }
        visible
    }
}

fn compare_file_entries(
    left: &FileSystemEntry,
    right: &FileSystemEntry,
    column: i32,
    direction: i32,
    time_display_mode: i32,
    cache: &HashMap<PathBuf, CachedMediaScan>,
) -> CmpOrdering {
    if left.is_dir != right.is_dir {
        return right.is_dir.cmp(&left.is_dir);
    }

    let left_name = file_name_for_sort(left);
    let right_name = file_name_for_sort(right);
    if left.is_dir {
        return natural_name_cmp(&left_name, &right_name);
    }

    let descending = direction == 2;
    let primary = match column {
        1 => directional_cmp(natural_name_cmp(&left_name, &right_name), descending),
        2 => directional_cmp(left.size.cmp(&right.size), descending),
        3 => option_cmp_missing_last(left.modified, right.modified, descending),
        4 => {
            let left_date = scan_result_from_cache(cache, left).and_then(|scan| {
                usable_sort_value(&display_media_date(&scan, time_display_mode).0)
            });
            let right_date = scan_result_from_cache(cache, right).and_then(|scan| {
                usable_sort_value(&display_media_date(&scan, time_display_mode).0)
            });
            option_cmp_missing_last(left_date, right_date, descending)
        }
        5 => {
            let left_status = scan_result_from_cache(cache, left)
                .map(|scan| metadata_sort_rank(&scan.metadata_status));
            let right_status = scan_result_from_cache(cache, right)
                .map(|scan| metadata_sort_rank(&scan.metadata_status));
            option_cmp_missing_last(left_status, right_status, descending)
        }
        _ => CmpOrdering::Equal,
    };
    primary.then_with(|| natural_name_cmp(&left_name, &right_name))
}

fn active_sort_value(
    entry: &FileSystemEntry,
    column: i32,
    time_display_mode: i32,
    cache: &HashMap<PathBuf, CachedMediaScan>,
) -> Option<String> {
    if entry.is_dir {
        return None;
    }
    match column {
        1 => Some(file_name_for_sort(entry).to_lowercase()),
        2 => Some(entry.size.to_string()),
        3 => entry.modified.map(format_time),
        4 => scan_result_from_cache(cache, entry)
            .map(|scan| display_media_date(&scan, time_display_mode).0)
            .filter(|value| value != "-" && value != "…" && !value.trim().is_empty()),
        _ => None,
    }
}

fn display_media_date(scan: &MediaScanResult, mode: i32) -> (String, String) {
    let recorded = if scan.recorded_media_date.trim().is_empty() {
        scan.media_date.clone()
    } else {
        scan.recorded_media_date.clone()
    };
    let recorded_basis = scan
        .recorded_offset_minutes
        .map(format_offset_label)
        .unwrap_or_else(|| "Local?".to_string());
    if mode == TIME_DISPLAY_RECORDED || scan.media_date_utc.is_none() {
        return (recorded, recorded_basis);
    }

    let timestamp = scan.media_date_utc.unwrap();
    let offset_minutes = if mode == TIME_DISPLAY_KST { 9 * 60 } else { 0 };
    let Some(utc) = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0) else {
        return (recorded, recorded_basis);
    };
    let Some(offset) = chrono::FixedOffset::east_opt(offset_minutes * 60) else {
        return (recorded, recorded_basis);
    };
    (
        utc.with_timezone(&offset)
            .format(DISPLAY_DATETIME_FORMAT)
            .to_string(),
        if mode == TIME_DISPLAY_KST {
            "KST"
        } else {
            "UTC"
        }
        .to_string(),
    )
}

fn format_offset_label(minutes: i32) -> String {
    if minutes == 0 {
        return "UTC".to_string();
    }
    let sign = if minutes < 0 { '-' } else { '+' };
    let absolute = minutes.abs();
    format!("UTC{sign}{:02}:{:02}", absolute / 60, absolute % 60)
}

fn format_number_with_commas(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

fn file_name_for_sort(entry: &FileSystemEntry) -> String {
    entry
        .path
        .file_name()
        .unwrap_or(entry.path.as_os_str())
        .to_string_lossy()
        .to_string()
}

fn directional_cmp(ordering: CmpOrdering, descending: bool) -> CmpOrdering {
    if descending {
        ordering.reverse()
    } else {
        ordering
    }
}

fn option_cmp_missing_last<T: Ord>(
    left: Option<T>,
    right: Option<T>,
    descending: bool,
) -> CmpOrdering {
    match (left, right) {
        (Some(left), Some(right)) => directional_cmp(left.cmp(&right), descending),
        (Some(_), None) => CmpOrdering::Less,
        (None, Some(_)) => CmpOrdering::Greater,
        (None, None) => CmpOrdering::Equal,
    }
}

fn usable_sort_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !matches!(value, "-" | "N/A")).then(|| value.to_string())
}

fn metadata_sort_rank(value: &str) -> i32 {
    match value.trim() {
        "O" | "Available" => 0,
        "X" | "Not Found" => 1,
        "!" | "Error" => 2,
        _ => 3,
    }
}

fn natural_name_cmp(left: &str, right: &str) -> CmpOrdering {
    let left_lower = left.to_lowercase();
    let right_lower = right.to_lowercase();
    let left_bytes = left_lower.as_bytes();
    let right_bytes = right_lower.as_bytes();
    let (mut left_index, mut right_index) = (0usize, 0usize);

    while left_index < left_bytes.len() && right_index < right_bytes.len() {
        if left_bytes[left_index].is_ascii_digit() && right_bytes[right_index].is_ascii_digit() {
            let left_end = digit_run_end(left_bytes, left_index);
            let right_end = digit_run_end(right_bytes, right_index);
            let left_digits = &left_lower[left_index..left_end];
            let right_digits = &right_lower[right_index..right_end];
            let left_trimmed = left_digits.trim_start_matches('0');
            let right_trimmed = right_digits.trim_start_matches('0');
            let left_number = if left_trimmed.is_empty() {
                "0"
            } else {
                left_trimmed
            };
            let right_number = if right_trimmed.is_empty() {
                "0"
            } else {
                right_trimmed
            };
            let number_order = left_number
                .len()
                .cmp(&right_number.len())
                .then_with(|| left_number.cmp(right_number))
                .then_with(|| left_digits.len().cmp(&right_digits.len()));
            if number_order != CmpOrdering::Equal {
                return number_order;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let left_char = left_lower[left_index..].chars().next().unwrap();
        let right_char = right_lower[right_index..].chars().next().unwrap();
        let character_order = left_char.cmp(&right_char);
        if character_order != CmpOrdering::Equal {
            return character_order;
        }
        left_index += left_char.len_utf8();
        right_index += right_char.len_utf8();
    }

    left_bytes
        .len()
        .cmp(&right_bytes.len())
        .then_with(|| left.cmp(right))
}

fn digit_run_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    index
}

fn scan_result_from_cache(
    cache: &HashMap<PathBuf, CachedMediaScan>,
    entry: &FileSystemEntry,
) -> Option<MediaScanResult> {
    cache
        .get(&entry.path)
        .filter(|cached| cached.size == entry.size && cached.modified == entry.modified)
        .map(|cached| cached.result.clone())
}

fn media_date_is_missing(scan: Option<&MediaScanResult>) -> bool {
    scan.is_none_or(|result| {
        let value = result.media_date.trim();
        value.is_empty() || matches!(value, "-" | "N/A")
    })
}

fn media_date_value_is_present(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "-" | "N/A" | "Scanning...")
}

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn matches_file_filter(filter: i32, path: &std::path::Path) -> bool {
    match filter {
        0 => is_image_path(path) || is_video_path(path),
        1 => is_image_path(path),
        2 => is_video_path(path),
        4 => is_jpeg_path(path),
        5 => is_png_path(path),
        _ => true,
    }
}

fn file_has_exif(path: &std::path::Path, is_dir: bool) -> bool {
    !is_dir && is_jpeg_path(path) && read_exif_metadata(path).has_exif
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::{
        display_media_date, format_number_with_commas, matches_file_filter, media_date_is_missing,
        natural_name_cmp, CachedMediaScan, SlintApp,
    };
    use crate::fs::FileSystemEntry;
    use crate::media::{is_image_path, is_video_path, MediaScanResult};
    use slint::Model;
    use std::cmp::Ordering as CmpOrdering;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};

    fn sortable_app() -> SlintApp {
        let entry = |name: &str| FileSystemEntry {
            path: PathBuf::from(name),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        };
        SlintApp {
            current_path: ".".to_string(),
            files: vec![entry("IMG_10.jpg"), entry("IMG_2.jpg")],
            selected_indices: vec![1],
            file_filter: 0,
            show_only_missing_media_date: false,
            show_only_missing_time_zone_offset: false,
            show_only_duplicate_media_date: false,
            time_display_mode: 0,
            sort_column: 0,
            sort_direction: 0,
            scan_results: Arc::new(Mutex::new(HashMap::new())),
            scan_epoch: Arc::new(AtomicU64::new(0)),
            selection_anchor: Some(1),
            dynamic_sort_ready: true,
        }
    }

    #[test]
    fn filename_sort_is_natural_and_cycles_ascending_descending_off() {
        assert_eq!(
            natural_name_cmp("IMG_2.jpg", "img_10.jpg"),
            CmpOrdering::Less
        );

        let mut app = sortable_app();
        app.cycle_sort(1);
        assert_eq!((app.sort_column, app.sort_direction), (1, 1));
        assert_eq!(
            app.path_for_ui_index(1).unwrap(),
            PathBuf::from("IMG_2.jpg")
        );
        assert_eq!(app.selected_indices, vec![2]);

        app.cycle_sort(1);
        assert_eq!((app.sort_column, app.sort_direction), (1, 2));
        assert_eq!(
            app.path_for_ui_index(1).unwrap(),
            PathBuf::from("IMG_10.jpg")
        );
        assert_eq!(app.selected_indices, vec![1]);

        app.cycle_sort(1);
        assert_eq!((app.sort_column, app.sort_direction), (0, 0));
        assert_eq!(
            app.path_for_ui_index(1).unwrap(),
            PathBuf::from("IMG_10.jpg")
        );
    }

    #[test]
    fn filename_prefix_search_keeps_growing_queries_and_wraps_single_keys() {
        let mut app = sortable_app();
        app.files.push(FileSystemEntry {
            path: PathBuf::from("2017_0318_110404.mov"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        app.files.push(FileSystemEntry {
            path: PathBuf::from("2017_0318_122327.mov"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });

        assert_eq!(app.ui_index_for_filename_prefix("2017_0318_11", 1), Some(3));
        assert_eq!(app.ui_index_for_filename_prefix("i", 1), Some(2));
        assert_eq!(app.ui_index_for_filename_prefix("i", 2), Some(1));
    }

    #[test]
    fn supported_media_is_the_union_of_images_and_videos() {
        for name in [
            "photo.jpg",
            "photo.JPEG",
            "image.png",
            "clip.mp4",
            "clip.MP4",
            "clip.mov",
            "clip.MOV",
            "clip.m4v",
            "clip.3gp",
            "clip.3g2",
            "clip.qt",
            "clip.mts",
            "clip.m2ts",
        ] {
            let path = Path::new(name);
            assert!(
                is_image_path(path) || is_video_path(path),
                "{name} should be supported"
            );
        }
        assert!(!is_image_path(Path::new("notes.txt")));
        assert!(!is_video_path(Path::new("notes.txt")));
    }

    #[test]
    fn missing_media_date_filter_keeps_pending_and_empty_dates_only() {
        let result = |media_date: &str| MediaScanResult {
            media_date: media_date.to_string(),
            ..MediaScanResult::default()
        };
        assert!(media_date_is_missing(None));
        assert!(media_date_is_missing(Some(&result("-"))));
        assert!(media_date_is_missing(Some(&result("N/A"))));
        assert!(!media_date_is_missing(Some(&result("2014-02-04 12:00:00"))));
    }

    #[test]
    fn duplicate_media_date_filter_keeps_only_known_duplicates() {
        let mut app = sortable_app();
        app.files.push(FileSystemEntry {
            path: PathBuf::from("IMG_3.jpg"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        app.files.push(FileSystemEntry {
            path: PathBuf::from("IMG_4.jpg"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        {
            let mut cache = app.scan_results.lock().unwrap();
            for (path, media_date) in [
                ("IMG_10.jpg", "2017-09-01 12:00:00"),
                ("IMG_2.jpg", "2017-09-01 12:00:00"),
                ("IMG_3.jpg", "2017-09-02 12:00:00"),
            ] {
                cache.insert(
                    PathBuf::from(path),
                    CachedMediaScan {
                        size: 0,
                        modified: None,
                        result: MediaScanResult {
                            media_kind: "jpeg".to_string(),
                            media_date: media_date.to_string(),
                            ..MediaScanResult::default()
                        },
                    },
                );
            }
        }
        app.show_only_duplicate_media_date = true;

        let model = app.get_ui_model();
        let names: Vec<_> = (1..model.row_count())
            .filter_map(|index| model.row_data(index))
            .map(|row| row.name.to_string())
            .collect();
        assert_eq!(names, vec!["IMG_10.jpg", "IMG_2.jpg"]);
    }

    #[test]
    fn missing_and_duplicate_filters_form_a_union_and_toggle_independently() {
        let mut app = sortable_app();
        app.files.push(FileSystemEntry {
            path: PathBuf::from("missing.jpg"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        {
            let mut cache = app.scan_results.lock().unwrap();
            for (path, media_date) in [
                ("IMG_10.jpg", "2017-09-01 12:00:00"),
                ("IMG_2.jpg", "2017-09-01 12:00:00"),
                ("missing.jpg", "-"),
            ] {
                cache.insert(
                    PathBuf::from(path),
                    CachedMediaScan {
                        size: 0,
                        modified: None,
                        result: MediaScanResult {
                            media_date: media_date.to_string(),
                            recorded_media_date: media_date.to_string(),
                            ..MediaScanResult::default()
                        },
                    },
                );
            }
        }
        app.show_only_missing_media_date = true;
        app.show_only_duplicate_media_date = true;

        let names = |app: &SlintApp| {
            let model = app.get_ui_model();
            (1..model.row_count())
                .filter_map(|index| model.row_data(index))
                .map(|row| row.name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&app), vec!["IMG_10.jpg", "IMG_2.jpg", "missing.jpg"]);

        app.show_only_missing_media_date = false;
        assert_eq!(names(&app), vec!["IMG_10.jpg", "IMG_2.jpg"]);
    }

    #[test]
    fn missing_time_zone_filter_requires_a_date_and_combines_with_other_filters() {
        let mut app = sortable_app();
        app.files.push(FileSystemEntry {
            path: PathBuf::from("missing_date.jpg"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        {
            let mut cache = app.scan_results.lock().unwrap();
            for (path, media_date, offset) in [
                ("IMG_10.jpg", "2018-09-06 00:15:53", None),
                ("IMG_2.jpg", "2025-01-19 17:45:59", Some(60)),
                ("missing_date.jpg", "-", None),
            ] {
                cache.insert(
                    PathBuf::from(path),
                    CachedMediaScan {
                        size: 0,
                        modified: None,
                        result: MediaScanResult {
                            media_kind: "jpeg".to_string(),
                            media_date: media_date.to_string(),
                            recorded_media_date: media_date.to_string(),
                            recorded_offset_minutes: offset,
                            ..MediaScanResult::default()
                        },
                    },
                );
            }
        }
        let names = |app: &SlintApp| {
            let model = app.get_ui_model();
            (1..model.row_count())
                .filter_map(|index| model.row_data(index))
                .map(|row| row.name.to_string())
                .collect::<Vec<_>>()
        };

        app.show_only_missing_time_zone_offset = true;
        assert_eq!(names(&app), vec!["IMG_10.jpg"]);

        app.show_only_missing_media_date = true;
        assert_eq!(names(&app), vec!["IMG_10.jpg", "missing_date.jpg"]);

        app.show_only_missing_time_zone_offset = false;
        assert_eq!(names(&app), vec!["missing_date.jpg"]);
    }

    #[test]
    fn time_display_converts_only_files_with_an_absolute_instant() {
        let utc_timestamp =
            chrono::NaiveDateTime::parse_from_str("2025-01-19 15:51:24", "%Y-%m-%d %H:%M:%S")
                .unwrap()
                .and_utc()
                .timestamp();
        let paris = MediaScanResult {
            media_date: "2025-01-19 16:51:24".to_string(),
            recorded_media_date: "2025-01-19 16:51:24".to_string(),
            media_date_utc: Some(utc_timestamp),
            recorded_offset_minutes: Some(60),
            ..MediaScanResult::default()
        };
        assert_eq!(
            display_media_date(&paris, 0),
            ("2025-01-19 16:51:24".to_string(), "UTC+01:00".to_string())
        );
        assert_eq!(
            display_media_date(&paris, 1),
            ("2025-01-19 15:51:24".to_string(), "UTC".to_string())
        );
        assert_eq!(
            display_media_date(&paris, 2),
            ("2025-01-20 00:51:24".to_string(), "KST".to_string())
        );

        let unknown = MediaScanResult {
            media_date: "2019-01-19 20:57:47".to_string(),
            recorded_media_date: "2019-01-19 20:57:47".to_string(),
            ..MediaScanResult::default()
        };
        assert_eq!(
            display_media_date(&unknown, 2),
            ("2019-01-19 20:57:47".to_string(), "Local?".to_string())
        );
    }

    #[test]
    fn image_subfilters_separate_jpeg_and_png_files() {
        assert!(matches_file_filter(4, Path::new("photo.jpg")));
        assert!(matches_file_filter(4, Path::new("photo.JPEG")));
        assert!(!matches_file_filter(4, Path::new("image.png")));
        assert!(matches_file_filter(5, Path::new("image.PNG")));
        assert!(!matches_file_filter(5, Path::new("photo.jpeg")));
    }

    #[test]
    fn folder_counts_separate_visible_supported_and_unsupported_files() {
        let mut app = sortable_app();
        app.files.push(FileSystemEntry {
            path: PathBuf::from("notes.txt"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        app.file_filter = 4;

        let (total, supported, unsupported) = app.folder_file_counts();
        assert_eq!(total, 3);
        assert_eq!(supported, 2);
        assert_eq!(unsupported, 1);
        assert_eq!(app.file_count(), 2);
    }

    #[test]
    fn exact_byte_counts_use_thousands_separators() {
        assert_eq!(format_number_with_commas(5_234_415), "5,234,415");
        assert_eq!(format_number_with_commas(999), "999");
    }

    #[test]
    fn scan_jobs_follow_display_order_instead_of_file_size() {
        let mut app = sortable_app();
        app.selected_indices.clear();
        app.files[0].size = 8_000_000;
        app.files[1].size = 1;

        let (_, jobs) = app.prepare_scan_jobs();
        let paths: Vec<_> = jobs.into_iter().map(|job| job.path).collect();
        assert_eq!(
            paths,
            vec![PathBuf::from("IMG_10.jpg"), PathBuf::from("IMG_2.jpg")]
        );
    }

    #[test]
    fn scan_priority_is_selected_row_then_visible_rows_in_display_order() {
        let mut app = sortable_app();
        app.files.push(FileSystemEntry {
            path: PathBuf::from("IMG_30.jpg"),
            size: 0,
            modified: None,
            created: None,
            is_dir: false,
        });
        app.selected_indices = vec![3];

        assert_eq!(
            app.scan_priority_paths_for_ui_range(1, 2),
            vec![
                PathBuf::from("IMG_30.jpg"),
                PathBuf::from("IMG_10.jpg"),
                PathBuf::from("IMG_2.jpg"),
            ]
        );
    }

    #[test]
    fn large_selection_keeps_only_viewport_paths_in_live_scan_priority() {
        let mut app = sortable_app();
        for index in 3..=8 {
            app.files.push(FileSystemEntry {
                path: PathBuf::from(format!("IMG_{index}.jpg")),
                size: 0,
                modified: None,
                created: None,
                is_dir: false,
            });
        }
        app.selected_indices = (1..=8).collect();

        assert_eq!(
            app.scan_priority_paths_for_ui_range(2, 3),
            vec![PathBuf::from("IMG_2.jpg"), PathBuf::from("IMG_3.jpg")]
        );
    }

    #[test]
    fn equal_values_in_the_active_sort_group_are_marked_per_column() {
        let mut app = sortable_app();
        app.files[0].size = 1_024;
        app.files[1].size = 1_024;
        app.cycle_sort(2);

        let model = app.get_ui_model();
        let first_file = model.row_data(1).unwrap();
        let second_file = model.row_data(2).unwrap();
        assert!(first_file.duplicate_size);
        assert!(second_file.duplicate_size);
        assert!(!first_file.duplicate_name);
        assert!(!second_file.duplicate_name);
    }

    #[test]
    fn deleting_rows_preserves_the_remaining_media_cache() {
        let mut app = sortable_app();
        let deleted = app.files[0].path.clone();
        let retained = app.files[1].path.clone();
        {
            let mut cache = app.scan_results.lock().unwrap();
            for path in [&deleted, &retained] {
                cache.insert(
                    path.clone(),
                    CachedMediaScan {
                        size: 0,
                        modified: None,
                        result: MediaScanResult {
                            media_date: "2015-07-11 14:31:13".to_string(),
                            ..MediaScanResult::default()
                        },
                    },
                );
            }
        }

        app.remove_deleted_paths(std::slice::from_ref(&deleted));

        assert_eq!(app.files.len(), 1);
        assert_eq!(app.files[0].path, retained);
        let cache = app.scan_results.lock().unwrap();
        assert!(!cache.contains_key(&deleted));
        assert!(cache.contains_key(&retained));
        assert!(app.selected_indices.is_empty());
    }
}

fn media_metadata_label(status: &str) -> &'static str {
    match status {
        "O" => "Available",
        "X" => "Not Found",
        "!" => "Read Error",
        "…" => "Scanning...",
        _ => "-",
    }
}

fn format_file_size(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let size = size as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else {
        format!("{} KB", (size / KB).ceil() as u64)
    }
}
