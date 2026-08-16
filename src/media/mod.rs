mod jpeg;
mod mp4;
mod mpeg_ts;
mod png;

use crate::exif::ExifMetadata;
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
    pub time_interpretation: String,
    pub exif_metadata: Option<ExifMetadata>,
}

#[derive(Clone, Debug)]
pub struct MediaScanJob {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub(crate) fn is_jpeg_path(path: &Path) -> bool {
    has_extension(path, &["jpg", "jpeg"])
}

pub(crate) fn is_png_path(path: &Path) -> bool {
    has_extension(path, &["png"])
}

pub(crate) fn is_image_path(path: &Path) -> bool {
    is_jpeg_path(path) || is_png_path(path)
}

pub(crate) fn is_mp4_path(path: &Path) -> bool {
    has_extension(path, &["mp4", "mov", "m4v", "3gp", "3g2", "qt"])
}

pub(crate) fn is_mpeg_ts_path(path: &Path) -> bool {
    has_extension(path, &["mts", "m2ts"])
}

pub(crate) fn is_video_path(path: &Path) -> bool {
    is_mp4_path(path) || is_mpeg_ts_path(path)
}

pub(crate) fn fallback_video_media_type(path: &Path) -> &'static str {
    match lowercase_extension(path).as_deref() {
        Some("mov" | "qt") => "QuickTime movie",
        Some("m4v") => "MPEG-4 video",
        Some("3gp") => "3GPP media",
        Some("3g2") => "3GPP2 media",
        Some("mts" | "m2ts") => "AVCHD transport stream",
        _ => "MPEG-4 media",
    }
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    lowercase_extension(path)
        .as_deref()
        .is_some_and(|extension| extensions.contains(&extension))
}

fn lowercase_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
}

pub fn scan_media_file(path: &Path) -> MediaScanResult {
    let mut signature = [0u8; 192 * 3];
    let Ok(mut file) = File::open(path) else {
        return failed_result_for_path(path, "Unavailable");
    };
    let read_len = file.read(&mut signature).unwrap_or(0);

    if jpeg::has_jpeg_signature(&signature[..read_len]) {
        return jpeg::scan(path);
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let allow_quicktime_without_ftyp = matches!(extension.as_deref(), Some("mov" | "qt"));
    if mp4::has_mp4_signature(&signature[..read_len])
        || (allow_quicktime_without_ftyp && mp4::has_quicktime_signature(&signature[..read_len]))
    {
        let media_type = iso_media_type(extension.as_deref(), &signature[..read_len]);
        return mp4::scan(&mut file, allow_quicktime_without_ftyp)
            .map(|mut result| {
                result.media_type = media_type.to_string();
                result
            })
            .unwrap_or_else(|_| failed_result("mp4", media_type));
    }

    if matches!(extension.as_deref(), Some("mts" | "m2ts"))
        && mpeg_ts::has_transport_stream_signature(&signature[..read_len])
    {
        return mpeg_ts::scan(&file)
            .unwrap_or_else(|_| failed_result("mts", "AVCHD transport stream"));
    }

    if png::has_png_signature(&signature[..read_len]) {
        return png::scan(path);
    }

    let (media_kind, media_type, expected_supported) = match extension.as_deref() {
        Some("jpg" | "jpeg") => ("jpeg", "JPEG image", true),
        Some("png") => ("png", "PNG image", true),
        Some("mp4") => ("mp4", "MPEG-4 media", true),
        Some("mov") => ("mp4", "QuickTime movie", true),
        Some("m4v") => ("mp4", "MPEG-4 video", true),
        Some("3gp") => ("mp4", "3GPP media", true),
        Some("3g2") => ("mp4", "3GPP2 media", true),
        Some("qt") => ("mp4", "QuickTime movie", true),
        Some("mts" | "m2ts") => ("mts", "AVCHD transport stream", true),
        _ => ("other", "Unknown", false),
    };

    MediaScanResult {
        media_kind: media_kind.to_string(),
        media_type: media_type.to_string(),
        media_date: "-".to_string(),
        metadata_status: if expected_supported { "!" } else { "-" }.to_string(),
        time_interpretation: String::new(),
        exif_metadata: None,
    }
}

fn iso_media_type(extension: Option<&str>, signature: &[u8]) -> &'static str {
    let brand = signature.get(8..12).unwrap_or_default();
    if brand.starts_with(b"3gp") {
        "3GPP media"
    } else if brand.starts_with(b"3g2") {
        "3GPP2 media"
    } else if brand == b"qt  " || matches!(extension, Some("mov" | "qt")) {
        "QuickTime movie"
    } else if matches!(extension, Some("m4v")) {
        "MPEG-4 video"
    } else {
        "MPEG-4 media"
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
        time_interpretation: String::new(),
        exif_metadata: None,
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
        Some("mov") => failed_result("mp4", "QuickTime movie"),
        Some("m4v") => failed_result("mp4", "MPEG-4 video"),
        Some("3gp") => failed_result("mp4", "3GPP media"),
        Some("3g2") => failed_result("mp4", "3GPP2 media"),
        Some("qt") => failed_result("mp4", "QuickTime movie"),
        Some("mts" | "m2ts") => failed_result("mts", "AVCHD transport stream"),
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

    #[test]
    fn classifies_3gpp_brand_and_avchd_transport_stream() {
        let temp = std::env::temp_dir();
        let gp_path = temp.join(format!("sh148_media_{}.3gp", std::process::id()));
        let mut ftyp = Vec::new();
        ftyp.extend_from_slice(&20u32.to_be_bytes());
        ftyp.extend_from_slice(b"ftyp");
        ftyp.extend_from_slice(b"3gp5");
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        ftyp.extend_from_slice(b"3gp5");
        std::fs::write(&gp_path, ftyp).unwrap();
        let gp = scan_media_file(&gp_path);
        let _ = std::fs::remove_file(&gp_path);
        assert_eq!(gp.media_kind, "mp4");
        assert_eq!(gp.media_type, "3GPP media");

        let mts_path = temp.join(format!("sh148_media_{}.m2ts", std::process::id()));
        let mut packets = vec![0u8; 192 * 3];
        for offset in [4, 196, 388] {
            packets[offset] = 0x47;
        }
        std::fs::write(&mts_path, packets).unwrap();
        let mts = scan_media_file(&mts_path);
        let _ = std::fs::remove_file(&mts_path);
        assert_eq!(mts.media_kind, "mts");
        assert_eq!(mts.media_type, "AVCHD transport stream");
        assert_eq!(mts.media_date, "-");
    }
}
