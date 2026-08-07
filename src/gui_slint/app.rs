use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use slint::{ModelRc, StandardListViewItem, VecModel};
use super::FileEntry as UiFileEntry; 
use crate::exif::read_exif_metadata;
use crate::fs::{read_directory, FileSystemEntry};
use crate::media::{MediaScanJob, MediaScanResult};

#[derive(Clone, Debug)]
pub struct CachedMediaScan {
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub result: MediaScanResult,
}

pub struct SlintApp {
    pub current_path: String,
    pub files: Vec<FileSystemEntry>,
    pub selected_indices: Vec<i32>,
    pub file_filter: i32,
    pub show_only_missing_media_date: bool,
    pub scan_results: Arc<Mutex<HashMap<PathBuf, CachedMediaScan>>>,
    pub scan_epoch: Arc<AtomicU64>,
    selection_anchor: Option<i32>,
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
            scan_results: Arc::new(Mutex::new(HashMap::new())),
            scan_epoch: Arc::new(AtomicU64::new(0)),
            selection_anchor: None,
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

    pub fn get_ui_model(&self) -> ModelRc<UiFileEntry> {
        let mut ui_items: Vec<UiFileEntry> = Vec::new();

        ui_items.push(UiFileEntry {
            name: "[..]".into(),
            size: "-".into(),
            modified: "-".into(),
            created: "-".into(),
            is_dir: true,
            selected: self.selected_indices.contains(&0),
            media_kind: "folder".into(),
            media_date: "-".into(),
            metadata_status: "-".into(),
        });

        let visible_files = self.visible_files();
        let cache = self.scan_results.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for (visible_index, f) in visible_files.into_iter().enumerate() {
            let name_str = f.path.file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let ui_index = i32::try_from(visible_index + 1).unwrap_or(i32::MAX);
            
            let scan = scan_result_from_cache(&cache, f);
            ui_items.push(UiFileEntry {
                name: name_str.into(),
                size: if f.is_dir { "-".into() } else { format_file_size(f.size).into() },
                modified: f.modified.map(format_time).unwrap_or_else(|| "-".into()).into(),
                created: f.created.map(format_time).unwrap_or_else(|| "-".into()).into(),
                is_dir: f.is_dir,
                selected: self.selected_indices.contains(&ui_index),
                media_kind: if f.is_dir { "folder" } else { scan.as_ref().map(|value| value.media_kind.as_str()).unwrap_or("pending") }.into(),
                media_date: if f.is_dir { "-" } else { scan.as_ref().map(|value| value.media_date.as_str()).unwrap_or("…") }.into(),
                metadata_status: if f.is_dir { "-" } else { scan.as_ref().map(|value| value.metadata_status.as_str()).unwrap_or("…") }.into(),
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
            let name = f.path.file_name()
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
            let modified = f.modified.map(format_time).unwrap_or_else(|| "-".to_string());
            let scan = self.scan_result_for(f);
            let media_date = scan.as_ref().map(|value| value.media_date.as_str()).unwrap_or("…");
            let metadata = scan.as_ref().map(|value| value.metadata_status.as_str()).unwrap_or("…");

            rows.push(ModelRc::new(VecModel::from(vec![
                StandardListViewItem::from(display_name.as_str()),
                StandardListViewItem::from(size.as_str()),
                StandardListViewItem::from(modified.as_str()),
                StandardListViewItem::from(media_date),
                StandardListViewItem::from(metadata),
            ])));
        }

        ModelRc::new(VecModel::from(rows))
    }

    pub fn file_count(&self) -> usize {
        self.visible_files().iter().filter(|entry| !entry.is_dir).count()
    }

    pub fn visible_entry_count(&self) -> usize {
        self.visible_files().len()
    }

    pub fn path_for_ui_index(&self, index: i32) -> Option<PathBuf> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some(PathBuf::from(&self.current_path).parent()?.to_path_buf());
        }

        self.visible_files().get(idx - 1).map(|entry| entry.path.clone())
    }

    pub fn ui_index_for_path(&self, path: &std::path::Path) -> Option<i32> {
        self.visible_files()
            .into_iter()
            .position(|entry| entry.path == path)
            .and_then(|index| i32::try_from(index + 1).ok())
    }

    pub fn ui_details_for_index(&self, index: i32) -> Option<(String, String, String, bool)> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some((
                "[..]".to_string(),
                "-".to_string(),
                "-".to_string(),
                true,
            ));
        }

        let visible = self.visible_files();
        let entry = visible.get(idx - 1)?;
        let name = entry.path.file_name()
            .map(|os_str| os_str.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let modified = entry.modified.map(format_time).unwrap_or_else(|| "-".to_string());
        let created = entry.created.map(format_time).unwrap_or_else(|| "-".to_string());

        Some((name, created, modified, entry.is_dir))
    }

    pub fn media_details_for_index(&self, index: i32) -> (String, String, String, String) {
        let Some(idx) = usize::try_from(index).ok() else {
            return empty_media_details();
        };
        if idx == 0 {
            return (
                "folder".to_string(),
                "Folder".to_string(),
                "-".to_string(),
                "-".to_string(),
            );
        }

        let visible = self.visible_files();
        let Some(entry) = visible.get(idx - 1) else {
            return empty_media_details();
        };
        if entry.is_dir {
            return (
                "folder".to_string(),
                "Folder".to_string(),
                "-".to_string(),
                "-".to_string(),
            );
        }

        if let Some(scan) = self.scan_result_for(entry) {
            return (
                scan.media_kind,
                scan.media_type,
                scan.media_date,
                media_metadata_label(&scan.metadata_status).to_string(),
            );
        }

        if is_mp4_file(&entry.path) {
            return (
                "mp4".to_string(),
                "Scanning...".to_string(),
                "Scanning...".to_string(),
                "Scanning...".to_string(),
            );
        }
        if is_jpeg_file(&entry.path) {
            return (
                "jpeg".to_string(),
                "JPEG image".to_string(),
                "Scanning...".to_string(),
                "Scanning...".to_string(),
            );
        }
        if is_png_file(&entry.path) {
            return (
                "png".to_string(),
                "PNG image".to_string(),
                "Scanning...".to_string(),
                "Scanning...".to_string(),
            );
        }

        empty_media_details()
    }

    pub fn select_ui_index(&mut self, index: i32, ctrl: bool, shift: bool) {
        if index < 0 || usize::try_from(index).map_or(true, |idx| idx >= self.visible_files().len() + 1) {
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
            if let Some(position) = self.selected_indices.iter().position(|selected| *selected == index) {
                self.selected_indices.remove(position);
            } else {
                self.selected_indices.push(index);
            }
            self.selection_anchor = Some(index);
        } else {
            self.selected_indices.clear();
            self.selected_indices.push(index);
            self.selection_anchor = Some(index);
        }

        self.selected_indices.sort_unstable();
        self.selected_indices.dedup();
    }

    pub fn selected_indices(&self) -> &[i32] {
        &self.selected_indices
    }

    pub fn selected_counts(&self) -> (usize, usize, bool) {
        let visible = self.visible_files();
        let mut file_count = 0;
        let mut recyclable_count = 0;
        let mut has_dir = false;
        for index in &self.selected_indices {
            let Ok(index) = usize::try_from(*index) else {
                continue;
            };
            if index == 0 {
                has_dir = true;
                continue;
            }
            let Some(entry) = visible.get(index - 1) else {
                continue;
            };
            recyclable_count += 1;
            if entry.is_dir {
                has_dir = true;
            } else {
                file_count += 1;
            }
        }
        (file_count, recyclable_count, has_dir)
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
            .filter(|(_, entry)| !entry.is_dir && is_jpeg_file(&entry.path) && !file_has_exif(&entry.path, entry.is_dir))
            .filter_map(|(index, _)| i32::try_from(index + 1).ok())
            .collect();
        self.selection_anchor = self.selected_indices.first().copied();
    }

    pub fn prepare_scan_jobs(&self) -> (u64, Vec<MediaScanJob>) {
        let epoch = self.scan_epoch.load(Ordering::Relaxed);
        let selected_paths: HashSet<_> = self.selected_indices
            .iter()
            .filter_map(|index| self.path_for_ui_index(*index))
            .collect();
        let visible_files = self.visible_files();
        let cache = self.scan_results.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        jobs.sort_by_key(|job| (!selected_paths.contains(&job.path), job.size));
        (epoch, jobs)
    }

    pub fn restart_scan(&self) {
        self.scan_epoch.fetch_add(1, Ordering::Relaxed);
    }

    fn scan_result_for(&self, entry: &FileSystemEntry) -> Option<MediaScanResult> {
        let cache = self.scan_results.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        scan_result_from_cache(&cache, entry)
    }

    fn visible_files(&self) -> Vec<&FileSystemEntry> {
        let cache = self.scan_results.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.files
            .iter()
            .filter(|entry| {
                if entry.is_dir {
                    return true;
                }
                let matches_type_filter = matches_file_filter(self.file_filter, &entry.path);
                if !matches_type_filter {
                    return false;
                }
                if !self.show_only_missing_media_date {
                    return true;
                }
                if !is_image_file(&entry.path) && !is_video_file(&entry.path) {
                    return false;
                }
                let scan = scan_result_from_cache(&cache, entry);
                media_date_is_missing(scan.as_ref())
            })
            .collect()
    }
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

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn is_image_file(path: &std::path::Path) -> bool {
    is_jpeg_file(path) || is_png_file(path)
}

fn matches_file_filter(filter: i32, path: &std::path::Path) -> bool {
    match filter {
        0 => is_image_file(path) || is_video_file(path),
        1 => is_image_file(path),
        2 => is_video_file(path),
        4 => is_jpeg_file(path),
        5 => is_png_file(path),
        _ => true,
    }
}

fn is_video_file(path: &std::path::Path) -> bool {
    is_mp4_file(path)
}

fn is_mp4_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
}

fn file_has_exif(path: &std::path::Path, is_dir: bool) -> bool {
    !is_dir && is_jpeg_file(path) && read_exif_metadata(path).has_exif
}

fn is_jpeg_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
}

fn is_png_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

#[cfg(test)]
mod tests {
    use super::{is_image_file, is_video_file, matches_file_filter, media_date_is_missing};
    use crate::media::MediaScanResult;
    use std::path::Path;

    #[test]
    fn supported_media_is_the_union_of_images_and_videos() {
        for name in ["photo.jpg", "photo.JPEG", "image.png", "clip.mp4", "clip.MP4"] {
            let path = Path::new(name);
            assert!(is_image_file(path) || is_video_file(path), "{name} should be supported");
        }
        assert!(!is_image_file(Path::new("notes.txt")));
        assert!(!is_video_file(Path::new("notes.txt")));
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
    fn image_subfilters_separate_jpeg_and_png_files() {
        assert!(matches_file_filter(4, Path::new("photo.jpg")));
        assert!(matches_file_filter(4, Path::new("photo.JPEG")));
        assert!(!matches_file_filter(4, Path::new("image.png")));
        assert!(matches_file_filter(5, Path::new("image.PNG")));
        assert!(!matches_file_filter(5, Path::new("photo.jpeg")));
    }
}

fn empty_media_details() -> (String, String, String, String) {
    (
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    )
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
