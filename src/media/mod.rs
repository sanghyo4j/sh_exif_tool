mod jpeg;
mod mp4;
mod png;

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
        return failed_result("Unavailable");
    };
    let read_len = file.read(&mut signature).unwrap_or(0);

    if jpeg::has_jpeg_signature(&signature[..read_len]) {
        return jpeg::scan(path);
    }

    if mp4::has_mp4_signature(&signature[..read_len]) {
        return mp4::scan(&mut file).unwrap_or_else(|_| failed_result("MPEG-4 media"));
    }

    if png::has_png_signature(&signature[..read_len]) {
        return png::scan(path);
    }

    let expected_supported = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "mp4" | "png"));

    MediaScanResult {
        media_kind: "other".to_string(),
        media_type: "Unknown".to_string(),
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

pub fn write_mp4_media_date(path: &Path, display_value: &str) -> Result<(), String> {
    mp4::write_media_date(path, display_value)
}

fn failed_result(media_type: &str) -> MediaScanResult {
    MediaScanResult {
        media_kind: "other".to_string(),
        media_type: media_type.to_string(),
        media_date: "-".to_string(),
        metadata_status: "!".to_string(),
    }
}
