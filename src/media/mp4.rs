use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

use super::MediaScanResult;

const QUICKTIME_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;
const DISPLAY_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub(super) fn has_mp4_signature(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp"
}

pub(super) fn scan(file: &mut File) -> Result<MediaScanResult, String> {
    let summary = scan_atoms(file)?;
    Ok(MediaScanResult {
        media_kind: "mp4".to_string(),
        media_type: "MPEG-4 media".to_string(),
        media_date: summary.media_date.unwrap_or_else(|| "-".to_string()),
        metadata_status: if summary.has_metadata { "O" } else { "X" }.to_string(),
    })
}

pub(super) fn write_media_date(path: &Path, display_value: &str) -> Result<(), String> {
    let datetime = NaiveDateTime::parse_from_str(display_value.trim(), DISPLAY_DATETIME_FORMAT)
        .map_err(|_| "MP4 Media Date must be formatted as YYYY-MM-DD HH:MM:SS.".to_string())?;
    let local_datetime = Local
        .from_local_datetime(&datetime)
        .single()
        .ok_or_else(|| "MP4 Media Date is ambiguous or invalid in the local time zone.".to_string())?;
    let quicktime_seconds = local_datetime
        .with_timezone(&Utc)
        .timestamp()
        .checked_add(QUICKTIME_UNIX_EPOCH_OFFSET)
        .filter(|value| *value > 0)
        .ok_or_else(|| "MP4 Media Date is outside the QuickTime time range.".to_string())? as u64;

    let original_metadata = path.metadata().map_err(|err| err.to_string())?;
    let original_len = original_metadata.len();
    let original_created = original_metadata.created().ok();
    let original_modified = original_metadata.modified().ok();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    let fields = find_creation_time_fields(&mut file)?;
    let targets: Vec<_> = fields.into_iter().filter(|field| field.original != 0).collect();
    if targets.is_empty() {
        return Err("No existing MP4 creation time value was found.".to_string());
    }
    if targets.iter().any(|field| field.width == 4 && quicktime_seconds > u32::MAX as u64) {
        return Err("MP4 Media Date is outside the range of an existing version 0 date field.".to_string());
    }

    for field in &targets {
        if let Err(err) = write_creation_time_value(&mut file, field, quicktime_seconds) {
            drop(file);
            let _ = rollback_creation_time_values(
                path,
                &targets,
                original_created,
                original_modified,
            );
            return Err(err);
        }
    }
    if let Err(err) = file.flush() {
        drop(file);
        let _ = rollback_creation_time_values(
            path,
            &targets,
            original_created,
            original_modified,
        );
        return Err(err.to_string());
    }
    drop(file);

    if path.metadata().map_err(|err| err.to_string())?.len() != original_len {
        let _ = rollback_creation_time_values(
            path,
            &targets,
            original_created,
            original_modified,
        );
        return Err("MP4 file size changed unexpectedly while writing Media Date.".to_string());
    }
    let mut verification_file = File::open(path).map_err(|err| err.to_string())?;
    let written = scan(&mut verification_file)?.media_date;
    if written != display_value.trim() {
        drop(verification_file);
        let _ = rollback_creation_time_values(
            path,
            &targets,
            original_created,
            original_modified,
        );
        return Err("MP4 Media Date verification failed.".to_string());
    }
    drop(verification_file);

    if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
        let _ = rollback_creation_time_values(
            path,
            &targets,
            original_created,
            original_modified,
        );
        return Err(err);
    }
    Ok(())
}

#[derive(Default)]
struct Mp4Summary {
    media_date: Option<String>,
    has_metadata: bool,
}

fn scan_atoms(file: &mut File) -> Result<Mp4Summary, String> {
    let file_len = file.metadata().map_err(|err| err.to_string())?.len();
    let mut position = 0u64;
    let mut found_ftyp = false;
    let mut summary = Mp4Summary::default();

    while position + 8 <= file_len {
        let atom = read_atom_header(file, position, file_len)?;
        match &atom.kind {
            b"ftyp" => found_ftyp = true,
            b"moov" => scan_atom_children(file, atom.payload_start, atom.end, 0, &mut summary)?,
            _ => {}
        }
        position = atom.end;
    }

    if !found_ftyp {
        return Err("MP4 ftyp atom was not found.".to_string());
    }
    Ok(summary)
}

struct AtomHeader {
    kind: [u8; 4],
    payload_start: u64,
    end: u64,
}

#[derive(Clone, Debug)]
struct CreationTimeField {
    offset: u64,
    width: u8,
    original: u64,
}

fn read_atom_header(file: &mut File, position: u64, parent_end: u64) -> Result<AtomHeader, String> {
    file.seek(SeekFrom::Start(position)).map_err(|err| err.to_string())?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header).map_err(|err| err.to_string())?;
    let size32 = u32::from_be_bytes(header[..4].try_into().unwrap());
    let kind: [u8; 4] = header[4..8].try_into().unwrap();

    let (size, header_len) = match size32 {
        0 => (parent_end.saturating_sub(position), 8u64),
        1 => {
            let mut extended = [0u8; 8];
            file.read_exact(&mut extended).map_err(|err| err.to_string())?;
            (u64::from_be_bytes(extended), 16u64)
        }
        value => (u64::from(value), 8u64),
    };

    if size < header_len {
        return Err("Invalid MP4 atom size.".to_string());
    }
    let end = position.checked_add(size).ok_or_else(|| "MP4 atom size overflow.".to_string())?;
    if end > parent_end || end <= position {
        return Err("MP4 atom is outside its parent.".to_string());
    }

    Ok(AtomHeader { kind, payload_start: position + header_len, end })
}

fn scan_atom_children(
    file: &mut File,
    start: u64,
    end: u64,
    depth: usize,
    summary: &mut Mp4Summary,
) -> Result<(), String> {
    if depth > 8 {
        return Ok(());
    }

    let mut position = start;
    while position + 8 <= end {
        let atom = read_atom_header(file, position, end)?;
        match &atom.kind {
            b"mvhd" | b"tkhd" | b"mdhd" => {
                summary.has_metadata = true;
                if summary.media_date.is_none() {
                    summary.media_date = read_quicktime_creation_date(file, atom.payload_start, atom.end)?;
                }
            }
            b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" => {
                scan_atom_children(file, atom.payload_start, atom.end, depth + 1, summary)?;
            }
            _ => {}
        }
        position = atom.end;
    }
    Ok(())
}

fn find_creation_time_fields(file: &mut File) -> Result<Vec<CreationTimeField>, String> {
    let file_len = file.metadata().map_err(|err| err.to_string())?.len();
    let mut position = 0u64;
    let mut found_ftyp = false;
    let mut fields = Vec::new();
    while position + 8 <= file_len {
        let atom = read_atom_header(file, position, file_len)?;
        match &atom.kind {
            b"ftyp" => found_ftyp = true,
            b"moov" => collect_creation_time_fields(
                file,
                atom.payload_start,
                atom.end,
                0,
                &mut fields,
            )?,
            _ => {}
        }
        position = atom.end;
    }
    if !found_ftyp {
        return Err("MP4 ftyp atom was not found.".to_string());
    }
    Ok(fields)
}

fn collect_creation_time_fields(
    file: &mut File,
    start: u64,
    end: u64,
    depth: usize,
    fields: &mut Vec<CreationTimeField>,
) -> Result<(), String> {
    if depth > 8 {
        return Ok(());
    }
    let mut position = start;
    while position + 8 <= end {
        let atom = read_atom_header(file, position, end)?;
        match &atom.kind {
            b"mvhd" | b"tkhd" | b"mdhd" => {
                fields.push(read_creation_time_field(file, atom.payload_start, atom.end)?);
            }
            b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" => {
                collect_creation_time_fields(
                    file,
                    atom.payload_start,
                    atom.end,
                    depth + 1,
                    fields,
                )?;
            }
            _ => {}
        }
        position = atom.end;
    }
    Ok(())
}

fn read_creation_time_field(
    file: &mut File,
    start: u64,
    end: u64,
) -> Result<CreationTimeField, String> {
    if end.saturating_sub(start) < 8 {
        return Err("MP4 date atom is too short.".to_string());
    }
    file.seek(SeekFrom::Start(start)).map_err(|err| err.to_string())?;
    let mut version_and_flags = [0u8; 4];
    file.read_exact(&mut version_and_flags).map_err(|err| err.to_string())?;
    let (width, original) = match version_and_flags[0] {
        0 => {
            let mut value = [0u8; 4];
            file.read_exact(&mut value).map_err(|err| err.to_string())?;
            (4, u64::from(u32::from_be_bytes(value)))
        }
        1 => {
            if end.saturating_sub(start) < 12 {
                return Err("MP4 version 1 date atom is too short.".to_string());
            }
            let mut value = [0u8; 8];
            file.read_exact(&mut value).map_err(|err| err.to_string())?;
            (8, u64::from_be_bytes(value))
        }
        version => return Err(format!("Unsupported MP4 date atom version: {version}.")),
    };
    Ok(CreationTimeField {
        offset: start + 4,
        width,
        original,
    })
}

fn write_creation_time_value(
    file: &mut File,
    field: &CreationTimeField,
    value: u64,
) -> Result<(), String> {
    file.seek(SeekFrom::Start(field.offset)).map_err(|err| err.to_string())?;
    if field.width == 4 {
        let value = u32::try_from(value)
            .map_err(|_| "MP4 Media Date does not fit an existing version 0 field.".to_string())?;
        file.write_all(&value.to_be_bytes()).map_err(|err| err.to_string())
    } else {
        file.write_all(&value.to_be_bytes()).map_err(|err| err.to_string())
    }
}

fn restore_creation_time_values(path: &Path, fields: &[CreationTimeField]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    for field in fields {
        write_creation_time_value(&mut file, field, field.original)?;
    }
    file.flush().map_err(|err| err.to_string())
}

fn rollback_creation_time_values(
    path: &Path,
    fields: &[CreationTimeField],
    created: Option<std::time::SystemTime>,
    modified: Option<std::time::SystemTime>,
) -> Result<(), String> {
    restore_creation_time_values(path, fields)?;
    crate::fs::set_file_times(path, created, modified)
}

fn read_quicktime_creation_date(file: &mut File, start: u64, end: u64) -> Result<Option<String>, String> {
    if end.saturating_sub(start) < 8 {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(start)).map_err(|err| err.to_string())?;
    let mut version_and_flags = [0u8; 4];
    file.read_exact(&mut version_and_flags).map_err(|err| err.to_string())?;

    let seconds = if version_and_flags[0] == 1 {
        let mut value = [0u8; 8];
        file.read_exact(&mut value).map_err(|err| err.to_string())?;
        u64::from_be_bytes(value)
    } else {
        let mut value = [0u8; 4];
        file.read_exact(&mut value).map_err(|err| err.to_string())?;
        u64::from(u32::from_be_bytes(value))
    };

    if seconds == 0 || seconds > i64::MAX as u64 {
        return Ok(None);
    }
    let unix_seconds = seconds as i64 - QUICKTIME_UNIX_EPOCH_OFFSET;
    let Some(date) = DateTime::<Utc>::from_timestamp(unix_seconds, 0) else {
        return Ok(None);
    };
    Ok(Some(date.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::scan_media_file;

    fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn version_zero_date_payload(created: u32) -> Vec<u8> {
        let mut payload = vec![0, 0, 0, 0];
        payload.extend_from_slice(&created.to_be_bytes());
        payload.extend_from_slice(&created.to_be_bytes());
        payload.extend_from_slice(&[0; 16]);
        payload
    }

    #[test]
    fn reads_creation_date_without_reading_media_payload() {
        let unix_2012 = 1_344_992_400u32;
        let quicktime_2012 = unix_2012 + QUICKTIME_UNIX_EPOCH_OFFSET as u32;
        let mut mvhd_payload = vec![0, 0, 0, 0];
        mvhd_payload.extend_from_slice(&quicktime_2012.to_be_bytes());
        mvhd_payload.extend_from_slice(&[0; 16]);
        let moov = atom(b"moov", &atom(b"mvhd", &mvhd_payload));
        let ftyp = atom(b"ftyp", b"isom\0\0\0\0isom");
        let mdat = atom(b"mdat", &[0; 32]);
        let path = std::env::temp_dir().join(format!("sh_exif_tool_mp4_scan_{}.mp4", std::process::id()));
        let mut bytes = ftyp;
        bytes.extend_from_slice(&mdat);
        bytes.extend_from_slice(&moov);
        std::fs::write(&path, bytes).unwrap();

        let result = scan_media_file(&path);
        let _ = std::fs::remove_file(path);

        assert_eq!(result.media_type, "MPEG-4 media");
        let expected = DateTime::<Utc>::from_timestamp(unix_2012 as i64, 0)
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(result.media_date, expected);
        assert_eq!(result.metadata_status, "O");
    }

    #[test]
    fn writes_only_existing_nonzero_creation_dates_in_place() {
        let original_unix = 1_344_992_400u32;
        let original_quicktime = original_unix + QUICKTIME_UNIX_EPOCH_OFFSET as u32;
        let mvhd = atom(b"mvhd", &version_zero_date_payload(original_quicktime));
        let empty_video_track = atom(
            b"trak",
            &[
                atom(b"tkhd", &version_zero_date_payload(0)),
                atom(b"mdia", &atom(b"mdhd", &version_zero_date_payload(0))),
            ]
            .concat(),
        );
        let dated_audio_track = atom(
            b"trak",
            &[
                atom(b"tkhd", &version_zero_date_payload(original_quicktime)),
                atom(
                    b"mdia",
                    &atom(b"mdhd", &version_zero_date_payload(original_quicktime)),
                ),
            ]
            .concat(),
        );
        let moov = atom(
            b"moov",
            &[mvhd, empty_video_track, dated_audio_track].concat(),
        );
        let ftyp = atom(b"ftyp", b"isom\0\0\0\0isom");
        let mdat = atom(b"mdat", &[0x5a; 64]);
        let path = std::env::temp_dir().join(format!(
            "sh_exif_tool_mp4_write_{}.mp4",
            std::process::id()
        ));
        let original_bytes = [ftyp, mdat, moov].concat();
        std::fs::write(&path, &original_bytes).unwrap();

        let display_value = "2013-12-18 05:15:08";
        write_media_date(&path, display_value).unwrap();

        let updated_bytes = std::fs::read(&path).unwrap();
        let result = scan_media_file(&path);
        let mut file = File::open(&path).unwrap();
        let fields = find_creation_time_fields(&mut file).unwrap();
        let local = Local
            .from_local_datetime(
                &NaiveDateTime::parse_from_str(display_value, DISPLAY_DATETIME_FORMAT).unwrap(),
            )
            .single()
            .unwrap();
        let expected = (local.with_timezone(&Utc).timestamp() + QUICKTIME_UNIX_EPOCH_OFFSET) as u64;

        assert_eq!(updated_bytes.len(), original_bytes.len());
        assert_eq!(&updated_bytes[updated_bytes.len() - 16..], &original_bytes[original_bytes.len() - 16..]);
        assert_eq!(result.media_date, display_value);
        assert_eq!(fields.iter().filter(|field| field.original == 0).count(), 2);
        assert_eq!(fields.iter().filter(|field| field.original == expected).count(), 3);

        let _ = std::fs::remove_file(path);
    }
}
