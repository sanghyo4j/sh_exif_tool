use std::fs;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};

use crate::exif::{
    create_date_only_exif_tiff,
    exif_backup_path,
    read_exif_tiff_metadata,
    rewrite_exif_tiff_dates,
};

use super::MediaScanResult;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const CREATION_TIME_KEYWORD: &[u8] = b"Creation Time";
const DISPLAY_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const WINDOWS_CREATION_TIME_FORMAT: &str = "%Y:%m:%d %H:%M:%S";

#[derive(Clone, Debug, Default)]
pub(crate) struct PngDateSources {
    pub creation_time: String,
    pub date_time_original: String,
    pub date_time_digitized: String,
    pub image_date_time: String,
}

impl PngDateSources {
    pub(crate) fn effective_media_date(&self) -> String {
        [
            self.date_time_original.as_str(),
            self.creation_time.as_str(),
            self.date_time_digitized.as_str(),
            self.image_date_time.as_str(),
        ]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or("-")
        .to_string()
    }

    pub(crate) fn has_existing_date(&self) -> bool {
        [
            &self.creation_time,
            &self.date_time_original,
            &self.date_time_digitized,
            &self.image_date_time,
        ]
        .into_iter()
        .any(|value| !value.is_empty())
    }
}

pub(super) fn has_png_signature(bytes: &[u8]) -> bool {
    bytes.len() >= PNG_SIGNATURE.len() && &bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE
}

pub(super) fn scan(path: &Path) -> MediaScanResult {
    let sources = read_date_sources(path);
    let media_date = sources.effective_media_date();
    let metadata_status = if sources.has_existing_date() { "O" } else { "X" };
    MediaScanResult {
        media_kind: "png".to_string(),
        media_type: "PNG image".to_string(),
        media_date,
        metadata_status: metadata_status.to_string(),
    }
}

pub(crate) fn read_date_sources(path: &Path) -> PngDateSources {
    let Ok(bytes) = fs::read(path) else {
        return PngDateSources::default();
    };
    read_date_sources_from_bytes(&bytes)
}

fn read_date_sources_from_bytes(bytes: &[u8]) -> PngDateSources {
    let creation_time = find_text_chunk(bytes, CREATION_TIME_KEYWORD)
        .and_then(|value| creation_time_to_display(&value))
        .unwrap_or_default();
    let exif = find_chunk(bytes, b"eXIf")
        .map(read_exif_tiff_metadata)
        .unwrap_or_default();
    PngDateSources {
        creation_time,
        date_time_original: exif.date_time_original,
        date_time_digitized: exif.date_time_digitized,
        image_date_time: exif.image_date_time,
    }
}

pub(super) fn write_media_date(
    path: &Path,
    display_value: &str,
    backup_before_changes: bool,
) -> Result<(), String> {
    write_date_sources(
        path,
        Some(display_value),
        Some(display_value),
        backup_before_changes,
    )
}

pub(super) fn write_date_sources(
    path: &Path,
    creation_time_value: Option<&str>,
    exif_original_value: Option<&str>,
    backup_before_changes: bool,
) -> Result<(), String> {
    if creation_time_value.is_none() && exif_original_value.is_none() {
        return Ok(());
    }
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let original_file_metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    let original_created = original_file_metadata.created().ok();
    let original_modified = original_file_metadata.modified().ok();
    let expected_creation = creation_time_value
        .map(display_to_creation_time)
        .transpose()?
        .map(|value| {
            let display = creation_time_to_display(&value)
                .ok_or_else(|| "PNG Creation Time normalization failed.".to_string())?;
            Ok::<_, String>((value, display))
        })
        .transpose()?;
    let expected_exif = exif_original_value
        .map(display_to_creation_time)
        .transpose()?
        .map(|value| {
            creation_time_to_display(&value)
                .ok_or_else(|| "PNG EXIF DateTimeOriginal normalization failed.".to_string())
        })
        .transpose()?;

    let with_creation_time = match expected_creation.as_ref() {
        Some((stored, _)) => replace_or_insert_creation_time_chunk(&bytes, stored)?,
        None => bytes.clone(),
    };
    let updated = match expected_exif.as_ref() {
        Some(display) => replace_or_insert_exif_chunk(&with_creation_time, display)?,
        None => with_creation_time,
    };
    if backup_before_changes {
        let backup_path = exif_backup_path(path);
        if !backup_path.exists() {
            fs::copy(path, &backup_path).map_err(|err| err.to_string())?;
        }
    }

    if let Err(err) = fs::write(path, &updated) {
        let _ = fs::write(path, &bytes);
        return Err(err.to_string());
    }
    if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
        let _ = fs::write(path, &bytes);
        let _ = crate::fs::set_file_times(path, original_created, original_modified);
        return Err(err);
    }

    let written = read_date_sources(path);
    let creation_verified = expected_creation
        .as_ref()
        .is_none_or(|(_, display)| written.creation_time == *display);
    let exif_verified = expected_exif
        .as_ref()
        .is_none_or(|display| written.date_time_original == *display);
    if !creation_verified || !exif_verified {
        let _ = fs::write(path, &bytes);
        let _ = crate::fs::set_file_times(path, original_created, original_modified);
        return Err("PNG date metadata verification failed.".to_string());
    }

    Ok(())
}

pub(super) fn remove_date_metadata(
    path: &Path,
    backup_before_changes: bool,
) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    if !has_png_signature(&bytes) {
        return Err("The selected file is not a valid PNG image.".to_string());
    }

    let mut updated = Vec::with_capacity(bytes.len());
    updated.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    let mut removed = false;
    while offset
        .checked_add(12)
        .is_some_and(|minimum| minimum <= bytes.len())
    {
        let length = read_be_u32(&bytes[offset..offset + 4])
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())?
            as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| "PNG chunk length overflow.".to_string())?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or_else(|| "PNG chunk length overflow.".to_string())?;
        if chunk_end > bytes.len() {
            return Err("PNG chunk is truncated.".to_string());
        }

        let chunk_type = &bytes[offset + 4..offset + 8];
        let data = &bytes[data_start..data_end];
        let is_creation_time =
            chunk_type == b"tEXt" && text_chunk_value(data, CREATION_TIME_KEYWORD).is_some();
        let remove = chunk_type == b"eXIf" || is_creation_time;
        if remove {
            removed = true;
        } else {
            updated.extend_from_slice(&bytes[offset..chunk_end]);
        }
        offset = chunk_end;
        if chunk_type == b"IEND" {
            updated.extend_from_slice(&bytes[offset..]);
            offset = bytes.len();
            break;
        }
    }
    if offset != bytes.len() {
        return Err("PNG contains invalid trailing chunk data.".to_string());
    }
    if !removed {
        return Ok(());
    }

    let original_file_metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    let original_created = original_file_metadata.created().ok();
    let original_modified = original_file_metadata.modified().ok();
    if backup_before_changes {
        let backup_path = exif_backup_path(path);
        if backup_path.exists() {
            return Err(format!("Backup file already exists: {}", backup_path.display()));
        }
        fs::copy(path, &backup_path).map_err(|err| err.to_string())?;
    }

    if let Err(err) = fs::write(path, &updated) {
        let _ = fs::write(path, &bytes);
        return Err(err.to_string());
    }
    if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
        let _ = fs::write(path, &bytes);
        let _ = crate::fs::set_file_times(path, original_created, original_modified);
        return Err(err);
    }
    if read_date_sources(path).has_existing_date() {
        let _ = fs::write(path, &bytes);
        let _ = crate::fs::set_file_times(path, original_created, original_modified);
        return Err("PNG date metadata removal verification failed.".to_string());
    }
    Ok(())
}

fn find_text_chunk(bytes: &[u8], keyword: &[u8]) -> Option<String> {
    if !has_png_signature(bytes) {
        return None;
    }

    let mut offset = PNG_SIGNATURE.len();
    while offset.checked_add(12)? <= bytes.len() {
        let length = read_be_u32(bytes.get(offset..offset + 4)?)? as usize;
        let chunk_type = bytes.get(offset + 4..offset + 8)?;
        let data_start = offset + 8;
        let data_end = data_start.checked_add(length)?;
        let chunk_end = data_end.checked_add(4)?;
        if chunk_end > bytes.len() {
            return None;
        }
        if chunk_type == b"tEXt" {
            let data = bytes.get(data_start..data_end)?;
            if let Some(value) = text_chunk_value(data, keyword) {
                return Some(value.to_string());
            }
        }
        if chunk_type == b"IEND" {
            break;
        }
        offset = chunk_end;
    }

    None
}

fn find_chunk<'a>(bytes: &'a [u8], wanted_type: &[u8; 4]) -> Option<&'a [u8]> {
    if !has_png_signature(bytes) {
        return None;
    }
    let mut offset = PNG_SIGNATURE.len();
    while offset.checked_add(12)? <= bytes.len() {
        let length = read_be_u32(bytes.get(offset..offset + 4)?)? as usize;
        let chunk_type = bytes.get(offset + 4..offset + 8)?;
        let data_start = offset + 8;
        let data_end = data_start.checked_add(length)?;
        let chunk_end = data_end.checked_add(4)?;
        if chunk_end > bytes.len() {
            return None;
        }
        if chunk_type == wanted_type {
            return bytes.get(data_start..data_end);
        }
        if chunk_type == b"IEND" {
            break;
        }
        offset = chunk_end;
    }
    None
}

fn replace_or_insert_exif_chunk(bytes: &[u8], display_value: &str) -> Result<Vec<u8>, String> {
    let existing_tiff = find_chunk(bytes, b"eXIf");
    let tiff = match existing_tiff {
        Some(value) => rewrite_exif_tiff_dates(value, display_value)?,
        None => create_date_only_exif_tiff(display_value)?,
    };
    replace_or_insert_chunk(bytes, b"eXIf", &tiff)
}

fn replace_or_insert_chunk(
    bytes: &[u8],
    wanted_type: &[u8; 4],
    data: &[u8],
) -> Result<Vec<u8>, String> {
    if !has_png_signature(bytes) {
        return Err("Selected file is not a PNG image.".to_string());
    }
    let mut output = Vec::with_capacity(bytes.len().saturating_add(data.len()).saturating_add(12));
    output.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    let mut inserted = false;
    let mut saw_ihdr = false;
    while offset + 12 <= bytes.len() {
        let length = read_be_u32(&bytes[offset..offset + 4])
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())? as usize;
        let chunk_type = bytes
            .get(offset + 4..offset + 8)
            .ok_or_else(|| "Invalid PNG chunk type.".to_string())?;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())?;
        if chunk_end > bytes.len() {
            return Err("PNG chunk extends beyond the end of the file.".to_string());
        }
        if chunk_type == b"IHDR" {
            saw_ihdr = true;
        }
        if chunk_type == wanted_type {
            if !inserted {
                write_chunk(&mut output, wanted_type, data)?;
                inserted = true;
            }
            offset = chunk_end;
            continue;
        }
        if !inserted && chunk_type == b"IDAT" {
            if !saw_ihdr {
                return Err("PNG IHDR chunk was not found.".to_string());
            }
            write_chunk(&mut output, wanted_type, data)?;
            inserted = true;
        }
        output.extend_from_slice(&bytes[offset..chunk_end]);
        if chunk_type == b"IEND" {
            if !inserted {
                write_chunk(&mut output, wanted_type, data)?;
            }
            return Ok(output);
        }
        offset = chunk_end;
    }
    Err("PNG IEND chunk was not found.".to_string())
}

fn replace_or_insert_creation_time_chunk(
    bytes: &[u8],
    creation_time: &str,
) -> Result<Vec<u8>, String> {
    if !has_png_signature(bytes) {
        return Err("Selected file is not a PNG image.".to_string());
    }

    let text_data = creation_time_text_data(creation_time);
    let mut output = Vec::with_capacity(
        bytes
            .len()
            .saturating_add(text_data.len())
            .saturating_add(12),
    );
    output.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    let mut inserted = false;
    let mut saw_ihdr = false;

    while offset + 12 <= bytes.len() {
        let length = read_be_u32(&bytes[offset..offset + 4])
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())? as usize;
        let chunk_type = bytes
            .get(offset + 4..offset + 8)
            .ok_or_else(|| "Invalid PNG chunk type.".to_string())?;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or_else(|| "Invalid PNG chunk length.".to_string())?;
        if chunk_end > bytes.len() {
            return Err("PNG chunk extends beyond the end of the file.".to_string());
        }

        if chunk_type == b"IHDR" {
            saw_ihdr = true;
        }

        if chunk_type == b"tEXt" {
            if let Some(data) = bytes.get(data_start..data_end) {
                if text_chunk_value(data, CREATION_TIME_KEYWORD).is_some() {
                    offset = chunk_end;
                    continue;
                }
            }
        }

        if !inserted && (chunk_type == b"IDAT" || chunk_type == b"IEND") {
            if !saw_ihdr {
                return Err("PNG IHDR chunk was not found.".to_string());
            }
            write_chunk(&mut output, b"tEXt", &text_data)?;
            inserted = true;
        }

        output.extend_from_slice(&bytes[offset..chunk_end]);

        if chunk_type == b"IEND" {
            if !inserted {
                write_chunk(&mut output, b"tEXt", &text_data)?;
            }
            return Ok(output);
        }

        offset = chunk_end;
    }

    Err("PNG IEND chunk was not found.".to_string())
}

fn creation_time_text_data(creation_time: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(CREATION_TIME_KEYWORD.len() + 1 + creation_time.len());
    data.extend_from_slice(CREATION_TIME_KEYWORD);
    data.push(0);
    data.extend_from_slice(creation_time.as_bytes());
    data
}

fn text_chunk_value<'a>(data: &'a [u8], keyword: &[u8]) -> Option<&'a str> {
    let separator = data.iter().position(|byte| *byte == 0)?;
    if data.get(..separator)? != keyword {
        return None;
    }
    std::str::from_utf8(data.get(separator + 1..)?).ok()
}

fn display_to_creation_time(display_value: &str) -> Result<String, String> {
    let value = display_value.trim();
    let datetime = NaiveDateTime::parse_from_str(value, DISPLAY_DATETIME_FORMAT)
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
        .ok_or_else(|| "PNG Media Date must be formatted as YYYY-MM-DD or YYYY-MM-DD HH:MM:SS.".to_string())?;
    Ok(datetime.format(WINDOWS_CREATION_TIME_FORMAT).to_string())
}

fn creation_time_to_display(value: &str) -> Option<String> {
    if let Ok(datetime) = NaiveDateTime::parse_from_str(value.trim(), WINDOWS_CREATION_TIME_FORMAT)
    {
        return Some(datetime.format(DISPLAY_DATETIME_FORMAT).to_string());
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value.trim()) {
        return Some(
            datetime
                .naive_utc()
                .format(DISPLAY_DATETIME_FORMAT)
                .to_string(),
        );
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc2822(value.trim()) {
        return Some(
            datetime
                .naive_utc()
                .format(DISPLAY_DATETIME_FORMAT)
                .to_string(),
        );
    }
    NaiveDateTime::parse_from_str(value.trim(), DISPLAY_DATETIME_FORMAT)
        .ok()
        .map(|datetime| datetime.format(DISPLAY_DATETIME_FORMAT).to_string())
}

fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) -> Result<(), String> {
    let length =
        u32::try_from(data.len()).map_err(|_| "PNG EXIF chunk is too large.".to_string())?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);
    let crc = png_crc32(chunk_type, data);
    output.extend_from_slice(&crc.to_be_bytes());
    Ok(())
}

fn read_be_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}

fn png_crc32(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in chunk_type.iter().chain(data.iter()) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_media_date_without_a_time() {
        assert_eq!(
            display_to_creation_time("2014-02-04").unwrap(),
            "2014:02:04 00:00:00"
        );
    }
    use crate::media::scan_media_file;

    #[test]
    fn writes_and_reads_png_creation_time_media_date() {
        let path =
            std::env::temp_dir().join(format!("sh_exif_tool_png_exif_{}.png", std::process::id()));
        fs::write(&path, minimal_png()).unwrap();

        write_media_date(&path, "2012-08-15 02:30:00", true).unwrap();
        let result = scan_media_file(&path);
        let written_bytes = fs::read(&path).unwrap();
        let sources = read_date_sources(&path);

        let backup_path = exif_backup_path(&path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(backup_path);

        assert_eq!(result.media_kind, "png");
        assert_eq!(result.media_type, "PNG image");
        assert_eq!(result.media_date, "2012-08-15 02:30:00");
        assert_eq!(result.metadata_status, "O");
        assert_eq!(sources.creation_time, "2012-08-15 02:30:00");
        assert_eq!(sources.date_time_original, "2012-08-15 02:30:00");
        assert_eq!(
            find_text_chunk(&written_bytes, CREATION_TIME_KEYWORD).as_deref(),
            Some("2012:08:15 02:30:00")
        );
        assert!(find_chunk(&written_bytes, b"eXIf").is_some());
    }

    #[test]
    fn writes_png_media_date_without_backup_when_disabled() {
        let path = std::env::temp_dir().join(format!(
            "sh_exif_tool_png_no_backup_{}.png",
            std::process::id()
        ));
        let backup_path = exif_backup_path(&path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&backup_path);
        fs::write(&path, minimal_png()).unwrap();

        write_media_date(&path, "2014-02-04 14:40:01", false).unwrap();

        assert!(!backup_path.exists());
        assert_eq!(scan_media_file(&path).media_date, "2014-02-04 14:40:01");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn updates_both_png_date_sources_when_exif_already_exists() {
        let path = std::env::temp_dir().join(format!(
            "sh_exif_tool_png_dual_date_{}.png",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, minimal_png()).unwrap();

        write_media_date(&path, "2015-12-26 03:53:22", false).unwrap();
        write_media_date(&path, "2016-01-02 11:22:33", false).unwrap();
        let sources = read_date_sources(&path);

        assert_eq!(sources.creation_time, "2016-01-02 11:22:33");
        assert_eq!(sources.date_time_original, "2016-01-02 11:22:33");
        assert_eq!(sources.date_time_digitized, "");
        assert_eq!(sources.image_date_time, "");
        assert_eq!(sources.effective_media_date(), "2016-01-02 11:22:33");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn edits_png_date_sources_independently() {
        let path = std::env::temp_dir().join(format!(
            "sh_exif_tool_png_independent_date_{}.png",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, minimal_png()).unwrap();

        write_date_sources(&path, Some("2015-12-26 03:53:22"), None, false).unwrap();
        write_date_sources(&path, None, Some("2016-01-02 11:22:33"), false).unwrap();
        let sources = read_date_sources(&path);

        assert_eq!(sources.creation_time, "2015-12-26 03:53:22");
        assert_eq!(sources.date_time_original, "2016-01-02 11:22:33");
        assert_eq!(sources.effective_media_date(), "2016-01-02 11:22:33");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn removes_png_date_metadata_but_preserves_unrelated_text() {
        let path = std::env::temp_dir().join(format!(
            "sh_exif_tool_png_remove_dates_{}.png",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let mut bytes = minimal_png();
        let iend_start = bytes.len() - chunk(b"IEND", &[]).len();
        bytes.splice(
            iend_start..iend_start,
            chunk(b"tEXt", b"Comment\0keep this text"),
        );
        fs::write(&path, bytes).unwrap();
        write_media_date(&path, "2015-12-26 03:53:22", false).unwrap();

        remove_date_metadata(&path, false).unwrap();

        let updated = fs::read(&path).unwrap();
        assert!(!read_date_sources(&path).has_existing_date());
        assert_eq!(
            find_text_chunk(&updated, b"Comment").as_deref(),
            Some("keep this text")
        );
        assert!(find_chunk(&updated, b"eXIf").is_none());

        let _ = fs::remove_file(path);
    }

    fn minimal_png() -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        bytes.extend_from_slice(&chunk(b"IDAT", &[0x78, 0x9c, 0x63, 0, 0, 0, 2, 0, 1]));
        bytes.extend_from_slice(&chunk(b"IEND", &[]));
        bytes
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&png_crc32(kind, data).to_be_bytes());
        bytes
    }
}
