use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, Utc};

use crate::fs::{read_directory, FileSystemEntry};
use crate::media::{is_image_path, is_jpeg_path, is_png_path, is_video_path, MediaScanResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileFilter {
    Supported,
    Images,
    Jpeg,
    Png,
    Videos,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
    MediaDate,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TimeDisplayMode {
    #[default]
    Recorded,
    Utc,
    Kst,
    Pdt,
    Pst,
}

#[derive(Clone, Debug)]
pub struct FileRow {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub media_date: String,
    pub scan_pending: bool,
    pub scan_result: Option<MediaScanResult>,
    pub icon_index: i32,
}

impl FileRow {
    pub fn from_entry(entry: FileSystemEntry) -> Self {
        let name = entry
            .path
            .file_name()
            .unwrap_or(entry.path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let supported = is_supported(&entry.path);
        Self {
            path: entry.path,
            name,
            is_dir: entry.is_dir,
            is_parent: false,
            size: entry.size,
            modified: entry.modified,
            created: entry.created,
            media_date: if supported { "..." } else { "-" }.to_string(),
            scan_pending: supported,
            scan_result: None,
            icon_index: -1,
        }
    }

    pub fn apply_scan(&mut self, result: MediaScanResult, mode: TimeDisplayMode) {
        self.media_date = display_media_date(&result, mode);
        self.scan_pending = false;
        self.scan_result = Some(result);
    }

    pub fn text(&self, column: usize) -> String {
        match column {
            0 => self.name.clone(),
            1 if self.is_dir => "-".to_string(),
            1 => format_size(self.size),
            2 => format_time(self.modified),
            3 => self.media_date.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Debug)]
pub struct FileListModel {
    pub directory: PathBuf,
    /// Complete directory snapshot. Metadata-dependent filters derive `rows`
    /// from this collection and must never destructively remove entries here.
    pub all_rows: Vec<FileRow>,
    pub rows: Vec<FileRow>,
    pub total_files: usize,
    pub unsupported_files: usize,
    pub filter: FileFilter,
    pub sort_column: Option<SortColumn>,
    pub ascending: bool,
    pub time_display_mode: TimeDisplayMode,
    pub show_only_missing_date: bool,
    pub show_only_missing_offset: bool,
    pub show_only_duplicate_date: bool,
}

impl FileListModel {
    pub fn load(
        directory: PathBuf,
        filter: FileFilter,
        time_display_mode: TimeDisplayMode,
    ) -> Result<Self, String> {
        let entries = read_directory(&directory)?;
        let total_files = entries.iter().filter(|entry| !entry.is_dir).count();
        let unsupported_files = entries
            .iter()
            .filter(|entry| !entry.is_dir && !is_supported(&entry.path))
            .count();
        let mut rows: Vec<_> = entries
            .into_iter()
            .filter(|entry| entry.is_dir || filter.matches(&entry.path))
            .map(FileRow::from_entry)
            .collect();
        if let Some(parent) = directory.parent() {
            rows.push(FileRow {
                path: parent.to_path_buf(),
                name: "[..]".to_string(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                created: None,
                media_date: "-".to_string(),
                scan_pending: false,
                scan_result: None,
                icon_index: -1,
            });
        }
        sort_rows(&mut rows, SortColumn::Name, true, time_display_mode);
        Ok(Self {
            directory,
            all_rows: rows.clone(),
            rows,
            total_files,
            unsupported_files,
            filter,
            sort_column: None,
            ascending: true,
            time_display_mode,
            show_only_missing_date: false,
            show_only_missing_offset: false,
            show_only_duplicate_date: false,
        })
    }

    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == Some(column) && !self.ascending {
            self.sort_column = None;
            self.ascending = true;
            sort_rows(
                &mut self.all_rows,
                SortColumn::Name,
                true,
                self.time_display_mode,
            );
            self.rebuild_visible_rows();
            return;
        }
        if self.sort_column == Some(column) {
            self.ascending = !self.ascending;
        } else {
            self.sort_column = Some(column);
            self.ascending = true;
        }
        sort_rows(
            &mut self.all_rows,
            column,
            self.ascending,
            self.time_display_mode,
        );
        self.rebuild_visible_rows();
    }

    pub fn find_path(&self, path: &Path) -> Option<usize> {
        self.rows.iter().position(|row| row.path == path)
    }

    pub fn set_time_display_mode(&mut self, mode: TimeDisplayMode) {
        self.time_display_mode = mode;
        for row in &mut self.all_rows {
            if let Some(scan) = row.scan_result.as_ref() {
                row.media_date = display_media_date(scan, mode);
            }
        }
        if let Some(column) = self.sort_column {
            sort_rows(
                &mut self.all_rows,
                column,
                self.ascending,
                self.time_display_mode,
            );
        }
        self.rebuild_visible_rows();
    }

    pub fn apply_scan_to_source(&mut self, path: &Path, result: MediaScanResult) {
        if let Some(row) = self.all_rows.iter_mut().find(|row| row.path == path) {
            row.apply_scan(result, self.time_display_mode);
        }
    }

    pub fn refresh_after_scan_batch(&mut self) {
        if self.sort_column.is_some() {
            let column = self.sort_column.unwrap();
            sort_rows(
                &mut self.all_rows,
                column,
                self.ascending,
                self.time_display_mode,
            );
        }
        self.rebuild_visible_rows();
    }

    pub fn set_special_filters(
        &mut self,
        missing_date: bool,
        missing_offset: bool,
        duplicate_date: bool,
    ) {
        self.show_only_missing_date = missing_date;
        self.show_only_missing_offset = missing_offset;
        self.show_only_duplicate_date = duplicate_date;
        self.rebuild_visible_rows();
    }

    pub fn sorted_value_is_duplicated(&self, index: usize) -> bool {
        let Some(column) = self.sort_column else {
            return false;
        };
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        if row.is_dir || row.is_parent {
            return false;
        }
        index
            .checked_sub(1)
            .and_then(|other| self.rows.get(other))
            .is_some_and(|other| same_sort_value(row, other, column, self.time_display_mode))
            || self
                .rows
                .get(index + 1)
                .is_some_and(|other| same_sort_value(row, other, column, self.time_display_mode))
    }

    pub fn sort_subitem(&self) -> Option<i32> {
        self.sort_column.map(|column| match column {
            SortColumn::Name => 0,
            SortColumn::Size => 1,
            SortColumn::Modified => 2,
            SortColumn::MediaDate => 3,
        })
    }

    fn rebuild_visible_rows(&mut self) {
        let filters_active = self.show_only_missing_date
            || self.show_only_missing_offset
            || self.show_only_duplicate_date;
        if !filters_active {
            self.rows = self.all_rows.clone();
            return;
        }
        let mut counts = std::collections::HashMap::<i64, usize>::new();
        if self.show_only_duplicate_date {
            for row in &self.all_rows {
                if !row.is_dir {
                    if let Some(date) = media_date_sort_key(row, self.time_display_mode) {
                        *counts.entry(date).or_default() += 1;
                    }
                }
            }
        }
        self.rows = self
            .all_rows
            .iter()
            .filter(|row| {
                if row.is_dir {
                    return true;
                }
                let Some(scan) = row.scan_result.as_ref() else {
                    // Pending scan results are unknown, not missing.
                    return false;
                };
                let date = raw_media_date(row);
                let missing_date = !usable_media_date(date);
                let missing_offset = is_jpeg_path(&row.path)
                    && !missing_date
                    && scan.recorded_offset_minutes.is_none();
                let duplicate = media_date_sort_key(row, self.time_display_mode)
                    .and_then(|key| counts.get(&key))
                    .is_some_and(|count| *count > 1);
                (self.show_only_missing_date && missing_date)
                    || (self.show_only_missing_offset && missing_offset)
                    || (self.show_only_duplicate_date && duplicate)
            })
            .cloned()
            .collect();
    }
}

fn same_sort_value(
    left: &FileRow,
    right: &FileRow,
    column: SortColumn,
    time_display_mode: TimeDisplayMode,
) -> bool {
    if right.is_dir || right.is_parent {
        return false;
    }
    match column {
        SortColumn::Name => left.name.eq_ignore_ascii_case(&right.name),
        SortColumn::Size => left.size == right.size,
        SortColumn::Modified => left.modified.is_some() && left.modified == right.modified,
        SortColumn::MediaDate => {
            usable_media_date(raw_media_date(left))
                && media_date_sort_key(left, time_display_mode)
                    == media_date_sort_key(right, time_display_mode)
        }
    }
}

fn raw_media_date(row: &FileRow) -> &str {
    row.scan_result
        .as_ref()
        .map(|scan| {
            if scan.recorded_media_date.trim().is_empty() {
                scan.media_date.as_str()
            } else {
                scan.recorded_media_date.as_str()
            }
        })
        .unwrap_or(row.media_date.as_str())
}

fn usable_media_date(value: &str) -> bool {
    !value.trim().is_empty() && !matches!(value.trim(), "-" | "...")
}

impl FileFilter {
    fn matches(self, path: &Path) -> bool {
        match self {
            Self::Supported => is_supported(path),
            Self::Images => is_image_path(path),
            Self::Jpeg => is_jpeg_path(path),
            Self::Png => is_png_path(path),
            Self::Videos => is_video_path(path),
            Self::All => true,
        }
    }
}

pub fn is_supported(path: &Path) -> bool {
    is_image_path(path) || is_video_path(path)
}

fn sort_rows(
    rows: &mut [FileRow],
    column: SortColumn,
    ascending: bool,
    time_display_mode: TimeDisplayMode,
) {
    rows.sort_by(|left, right| {
        let parents = right.is_parent.cmp(&left.is_parent);
        if parents != Ordering::Equal {
            return parents;
        }
        let folders = right.is_dir.cmp(&left.is_dir);
        if folders != Ordering::Equal {
            return folders;
        }
        if column == SortColumn::MediaDate {
            return option_cmp_missing_last(
                media_date_sort_key(left, time_display_mode),
                media_date_sort_key(right, time_display_mode),
                !ascending,
            )
            .then_with(|| compare_names(&left.name, &right.name));
        }
        let order = match column {
            SortColumn::Name => compare_names(&left.name, &right.name),
            SortColumn::Size => left.size.cmp(&right.size),
            SortColumn::Modified => left.modified.cmp(&right.modified),
            SortColumn::MediaDate => Ordering::Equal,
        }
        .then_with(|| compare_names(&left.name, &right.name));
        if ascending {
            order
        } else {
            order.reverse()
        }
    });
}

fn option_cmp_missing_last<T: Ord>(
    left: Option<T>,
    right: Option<T>,
    descending: bool,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            let order = left.cmp(&right);
            if descending {
                order.reverse()
            } else {
                order
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn media_date_sort_key(row: &FileRow, mode: TimeDisplayMode) -> Option<i64> {
    let Some(scan) = row.scan_result.as_ref() else {
        return None;
    };
    if let Some(timestamp) = scan.media_date_utc {
        return Some(timestamp);
    }
    let recorded = recorded_media_date(scan);
    let wall_clock = NaiveDateTime::parse_from_str(&recorded, "%Y-%m-%d %H:%M:%S").ok()?;
    let assumed_offset_minutes = match mode {
        TimeDisplayMode::Kst => 540,
        TimeDisplayMode::Pdt => -420,
        TimeDisplayMode::Pst => -480,
        TimeDisplayMode::Recorded | TimeDisplayMode::Utc => 0,
    };
    Some(wall_clock.and_utc().timestamp() - i64::from(assumed_offset_minutes) * 60)
}

pub fn display_media_date(scan: &MediaScanResult, mode: TimeDisplayMode) -> String {
    let recorded = recorded_media_date(scan);
    if recorded == "-" || recorded == "..." || recorded.trim().is_empty() {
        return recorded;
    }
    if mode == TimeDisplayMode::Recorded || scan.media_date_utc.is_none() {
        let basis = scan
            .recorded_offset_minutes
            .map(format_offset_label)
            .unwrap_or_else(|| "Local?".to_string());
        return format!("{recorded} {basis}");
    }
    let timestamp = scan.media_date_utc.unwrap();
    let (minutes, label) = match mode {
        TimeDisplayMode::Kst => (540, "KST"),
        TimeDisplayMode::Pdt => (-420, "PDT"),
        TimeDisplayMode::Pst => (-480, "PST"),
        _ => (0, "UTC"),
    };
    let Some(utc) = DateTime::<Utc>::from_timestamp(timestamp, 0) else {
        return format!("{recorded} Local?");
    };
    let Some(offset) = FixedOffset::east_opt(minutes * 60) else {
        return format!("{recorded} Local?");
    };
    format!(
        "{} {label}",
        utc.with_timezone(&offset).format("%Y-%m-%d %H:%M:%S")
    )
}

fn recorded_media_date(scan: &MediaScanResult) -> String {
    if scan.recorded_media_date.trim().is_empty() {
        scan.media_date.clone()
    } else {
        scan.recorded_media_date.clone()
    }
}

fn format_offset_label(minutes: i32) -> String {
    match minutes {
        540 => "KST".to_string(),
        -420 => "PDT".to_string(),
        -480 => "PST".to_string(),
        120 => "CEST".to_string(),
        60 => "CET".to_string(),
        0 => "UTC".to_string(),
        _ => {
            let sign = if minutes < 0 { '-' } else { '+' };
            let absolute = minutes.abs();
            format!("UTC{sign}{:02}:{:02}", absolute / 60, absolute % 60)
        }
    }
}

fn compare_names(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

fn format_time(value: Option<SystemTime>) -> String {
    value
        .map(DateTime::<Local>::from)
        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_model(rows: Vec<FileRow>) -> FileListModel {
        FileListModel {
            directory: PathBuf::from("."),
            all_rows: rows.clone(),
            rows,
            total_files: 0,
            unsupported_files: 0,
            filter: FileFilter::Supported,
            sort_column: None,
            ascending: true,
            time_display_mode: TimeDisplayMode::Recorded,
            show_only_missing_date: false,
            show_only_missing_offset: false,
            show_only_duplicate_date: false,
        }
    }

    fn scanned_jpeg(name: &str, date: &str, offset: Option<i32>) -> FileRow {
        let result = MediaScanResult {
            media_kind: "jpeg".to_string(),
            media_date: date.to_string(),
            recorded_media_date: date.to_string(),
            recorded_offset_minutes: offset,
            ..MediaScanResult::default()
        };
        let mut row = FileRow {
            path: PathBuf::from(name),
            name: name.to_string(),
            is_dir: false,
            is_parent: false,
            size: 0,
            modified: None,
            created: None,
            media_date: String::new(),
            scan_pending: false,
            scan_result: None,
            icon_index: -1,
        };
        row.apply_scan(result, TimeDisplayMode::Recorded);
        row
    }

    fn scanned_jpeg_with_utc(name: &str, date: &str, offset: i32, utc_timestamp: i64) -> FileRow {
        let mut row = scanned_jpeg(name, date, Some(offset));
        row.scan_result.as_mut().unwrap().media_date_utc = Some(utc_timestamp);
        row
    }

    #[test]
    fn supported_filter_recognizes_all_current_media_extensions() {
        for name in [
            "a.jpg", "a.JPEG", "a.png", "a.mp4", "a.mov", "a.3gp", "a.m2ts",
        ] {
            assert!(is_supported(Path::new(name)), "{name}");
        }
        assert!(!is_supported(Path::new("notes.txt")));
    }

    #[test]
    fn size_format_is_compact() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "2 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn recorded_media_date_uses_the_files_explicit_basis() {
        let scan = MediaScanResult {
            media_date: "2018-09-07 07:07:50".to_string(),
            recorded_media_date: "2018-09-07 07:07:50".to_string(),
            recorded_offset_minutes: Some(-420),
            ..MediaScanResult::default()
        };
        assert_eq!(
            display_media_date(&scan, TimeDisplayMode::Recorded),
            "2018-09-07 07:07:50 PDT"
        );
    }

    #[test]
    fn special_filters_are_non_destructive_and_combine_as_a_union() {
        let rows = vec![
            scanned_jpeg("missing-date.jpg", "-", None),
            scanned_jpeg("missing-offset.jpg", "2020-01-01 12:00:00", None),
            scanned_jpeg("complete.jpg", "2021-01-01 12:00:00", Some(540)),
        ];
        let mut model = test_model(rows);
        model.set_special_filters(true, true, false);
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.all_rows.len(), 3);

        model.set_special_filters(false, false, false);
        assert_eq!(model.rows.len(), 3);
    }

    #[test]
    fn media_date_sort_uses_absolute_time_when_offsets_are_known() {
        let rows = vec![
            scanned_jpeg_with_utc("kst-1053.jpg", "2018-09-05 10:53:54", 540, 100),
            scanned_jpeg_with_utc("pdt-1107.jpg", "2018-09-05 11:07:56", -420, 300),
            scanned_jpeg_with_utc("kst-1111.jpg", "2018-09-05 11:11:18", 540, 200),
        ];
        let mut model = test_model(rows);
        model.sort_by(SortColumn::MediaDate);
        assert_eq!(
            model
                .rows
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["kst-1053.jpg", "kst-1111.jpg", "pdt-1107.jpg"]
        );
    }

    #[test]
    fn local_media_date_is_not_forced_behind_known_utc_dates() {
        let known = scanned_jpeg_with_utc("known.jpg", "2019-01-05 08:30:00", 540, 1_546_644_600);
        let local = scanned_jpeg("local.jpg", "2019-01-05 08:31:00", None);
        let mut model = test_model(vec![local, known]);
        model.time_display_mode = TimeDisplayMode::Kst;
        model.sort_by(SortColumn::MediaDate);
        assert_eq!(
            model
                .rows
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["known.jpg", "local.jpg"]
        );
    }

    #[test]
    fn duplicate_highlight_only_marks_equal_values_in_the_active_sort_column() {
        let mut first = scanned_jpeg("a.jpg", "2020-01-01 10:00:00", Some(540));
        first.size = 123;
        let mut second = scanned_jpeg("b.jpg", "2020-01-01 11:00:00", Some(540));
        second.size = 123;
        let mut third = scanned_jpeg("c.jpg", "2020-01-01 12:00:00", Some(540));
        third.size = 456;
        let mut model = test_model(vec![first, second, third]);
        model.sort_by(SortColumn::Size);
        assert!(model.sorted_value_is_duplicated(0));
        assert!(model.sorted_value_is_duplicated(1));
        assert!(!model.sorted_value_is_duplicated(2));
        assert_eq!(model.sort_subitem(), Some(1));
    }
}
