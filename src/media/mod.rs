mod jpeg;
mod mp4;
mod png;

pub(crate) use png::PngDateSources;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, Default)]
pub struct MediaScanResult {
    pub media_kind: String,
    pub media_type: String,
    pub media_date: String,
    pub metadata_status: String,
}

#[derive(Clone, Debug)]
pub struct MediaScanJob {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub fn scan_media_file(path: &Path) -> MediaScanResult {
    let mut signature = [0u8; 12];
    let Ok(mut file) = File::open(path) else {
        return failed_result_for_path(path, "Unavailable");
    };
    let read_len = file.read(&mut signature).unwrap_or(0);

    if jpeg::has_jpeg_signature(&signature[..read_len]) {
        return jpeg::scan(path);
    }

    if mp4::has_mp4_signature(&signature[..read_len]) {
        return mp4::scan(&mut file)
            .unwrap_or_else(|_| failed_result("mp4", "MPEG-4 media"));
    }

    if png::has_png_signature(&signature[..read_len]) {
        return png::scan(path);
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let (media_kind, media_type, expected_supported) = match extension.as_deref() {
        Some("jpg" | "jpeg") => ("jpeg", "JPEG image", true),
        Some("png") => ("png", "PNG image", true),
        Some("mp4") => ("mp4", "MPEG-4 media", true),
        _ => ("other", "Unknown", false),
    };

    MediaScanResult {
        media_kind: media_kind.to_string(),
        media_type: media_type.to_string(),
        media_date: "-".to_string(),
        metadata_status: if expected_supported { "!" } else { "-" }.to_string(),
    }
}

pub fn write_png_media_date(
    path: &Path,
    display_value: &str,
    backup_before_changes: bool,
) -> Result<(), String> {
    png::write_media_date(path, display_value, backup_before_changes)
}

pub(crate) fn read_png_date_sources(path: &Path) -> png::PngDateSources {
    png::read_date_sources(path)
}

pub(crate) fn write_png_date_sources(
    path: &Path,
    creation_time: Option<&str>,
    exif_date_time_original: Option<&str>,
    backup_before_changes: bool,
) -> Result<(), String> {
    png::write_date_sources(
        path,
        creation_time,
        exif_date_time_original,
        backup_before_changes,
    )
}

pub(crate) fn remove_png_date_source(
    path: &Path,
    key: &str,
    backup_before_changes: bool,
) -> Result<(), String> {
    png::remove_date_source(path, key, backup_before_changes)
}

pub(crate) fn remove_png_date_metadata(
    path: &Path,
    backup_before_changes: bool,
) -> Result<(), String> {
    png::remove_date_metadata(path, backup_before_changes)
}

pub fn write_mp4_media_date(path: &Path, display_value: &str) -> Result<(), String> {
    mp4::write_media_date(path, display_value)
}

fn failed_result(media_kind: &str, media_type: &str) -> MediaScanResult {
    MediaScanResult {
        media_kind: media_kind.to_string(),
        media_type: media_type.to_string(),
        media_date: "-".to_string(),
        metadata_status: "!".to_string(),
    }
}

fn failed_result_for_path(path: &Path, fallback_type: &str) -> MediaScanResult {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => failed_result("jpeg", "JPEG image"),
        Some("png") => failed_result("png", "PNG image"),
        Some("mp4") => failed_result("mp4", "MPEG-4 media"),
        _ => failed_result("other", fallback_type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_mp4_kind_when_metadata_scan_fails() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_failed_scan_{}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, b"not an mp4 container").unwrap();

        let result = scan_media_file(&path);
        let _ = std::fs::remove_file(path);

        assert_eq!(result.media_kind, "mp4");
        assert_eq!(result.media_type, "MPEG-4 media");
        assert_eq!(result.metadata_status, "!");
    }
}
