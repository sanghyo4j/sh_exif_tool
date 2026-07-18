use std::fs;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};

use crate::exif::exif_backup_path;

use super::MediaScanResult;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const CREATION_TIME_KEYWORD: &[u8] = b"Creation Time";
const DISPLAY_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const WINDOWS_CREATION_TIME_FORMAT: &str = "%Y:%m:%d %H:%M:%S";

pub(super) fn has_png_signature(bytes: &[u8]) -> bool {
    bytes.len() >= PNG_SIGNATURE.len() && &bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE
}

pub(super) fn scan(path: &Path) -> MediaScanResult {
    let creation_time = read_png_creation_time(path);
    let metadata_status = if creation_time.is_some() { "O" } else { "X" };
    MediaScanResult {
        media_kind: "png".to_string(),
        media_type: "PNG image".to_string(),
        media_date: creation_time.unwrap_or_else(|| "-".to_string()),
        metadata_status: metadata_status.to_string(),
    }
}

pub(super) fn write_media_date(
    path: &Path,
    display_value: &str,
    backup_before_changes: bool,
) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let creation_time = display_to_creation_time(display_value)?;
    let expected_display = creation_time_to_display(&creation_time)
        .ok_or_else(|| "PNG Media Date normalization failed.".to_string())?;
    let updated = replace_or_insert_creation_time_chunk(&bytes, &creation_time)?;
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

    let written = read_png_creation_time(path);
    if written.as_deref() != Some(expected_display.as_str()) {
        let _ = fs::write(path, &bytes);
        return Err("PNG Creation Time verification failed.".to_string());
    }

    Ok(())
}

fn read_png_creation_time(path: &Path) -> Option<String> {
    let Ok(bytes) = fs::read(path) else {
        return None;
    };
    find_text_chunk(&bytes, CREATION_TIME_KEYWORD)
        .and_then(|value| creation_time_to_display(&value))
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

        let backup_path = exif_backup_path(&path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(backup_path);

        assert_eq!(result.media_kind, "png");
        assert_eq!(result.media_type, "PNG image");
        assert_eq!(result.media_date, "2012-08-15 02:30:00");
        assert_eq!(result.metadata_status, "O");
        assert_eq!(
            find_text_chunk(&written_bytes, CREATION_TIME_KEYWORD).as_deref(),
            Some("2012:08:15 02:30:00")
        );
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
