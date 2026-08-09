use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};

use super::MediaScanResult;

const QUICKTIME_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;
const DISPLAY_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub(super) fn has_mp4_signature(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp"
}

pub(super) fn has_quicktime_signature(bytes: &[u8]) -> bool {
    bytes.len() >= 8
        && matches!(
            &bytes[4..8],
            b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip" | b"pnot"
        )
}

pub(super) fn scan(
    file: &mut File,
    allow_quicktime_without_ftyp: bool,
) -> Result<MediaScanResult, String> {
    let summary = scan_atoms(file, allow_quicktime_without_ftyp)?;
    Ok(MediaScanResult {
        media_kind: "mp4".to_string(),
        media_type: "MPEG-4 media".to_string(),
        media_date: summary.media_date.unwrap_or_else(|| "-".to_string()),
        metadata_status: if summary.has_metadata { "O" } else { "X" }.to_string(),
        time_interpretation: summary.time_interpretation,
        exif_metadata: None,
    })
}

pub(super) fn write_media_date(path: &Path, display_value: &str) -> Result<(), String> {
    let datetime = parse_display_datetime_or_date(display_value).ok_or_else(|| {
        "Media Date must be formatted as YYYY-MM-DD or YYYY-MM-DD HH:MM:SS.".to_string()
    })?;
    let expected_display = datetime.format(DISPLAY_DATETIME_FORMAT).to_string();
    let local_datetime = Local
        .from_local_datetime(&datetime)
        .single()
        .ok_or_else(|| {
            "Media Date is ambiguous or invalid in the local time zone.".to_string()
        })?;
    let quicktime_seconds = local_datetime
        .with_timezone(&Utc)
        .timestamp()
        .checked_add(QUICKTIME_UNIX_EPOCH_OFFSET)
        .filter(|value| *value > 0)
        .ok_or_else(|| "Media Date is outside the QuickTime time range.".to_string())?
        as u64;

    let original_metadata = path.metadata().map_err(|err| err.to_string())?;
    let original_len = original_metadata.len();
    let original_created = original_metadata.created().ok();
    let original_modified = original_metadata.modified().ok();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    let allow_quicktime_without_ftyp = is_mov_path(path);
    let targets = find_creation_time_fields(&mut file, allow_quicktime_without_ftyp)?;
    if targets.is_empty() {
        return Err("No writable ISO media creation time field was found.".to_string());
    }
    if targets
        .iter()
        .any(|field| field.width == 4 && quicktime_seconds > u32::MAX as u64)
    {
        return Err(
            "Media Date is outside the range of an existing version 0 date field.".to_string(),
        );
    }

    for field in &targets {
        if let Err(err) = write_creation_time_value(&mut file, field, quicktime_seconds) {
            drop(file);
            let _ =
                rollback_creation_time_values(path, &targets, original_created, original_modified);
            return Err(err);
        }
    }
    if let Err(err) = file.flush() {
        drop(file);
        let _ = rollback_creation_time_values(path, &targets, original_created, original_modified);
        return Err(err.to_string());
    }
    drop(file);

    if path.metadata().map_err(|err| err.to_string())?.len() != original_len {
        let _ = rollback_creation_time_values(path, &targets, original_created, original_modified);
        return Err("File size changed unexpectedly while writing Media Date.".to_string());
    }
    let mut verification_file = File::open(path).map_err(|err| err.to_string())?;
    let written = scan(&mut verification_file, allow_quicktime_without_ftyp)?.media_date;
    if written != expected_display {
        drop(verification_file);
        let _ = rollback_creation_time_values(path, &targets, original_created, original_modified);
        return Err("Media Date verification failed.".to_string());
    }
    drop(verification_file);

    if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
        let _ = rollback_creation_time_values(path, &targets, original_created, original_modified);
        return Err(err);
    }
    Ok(())
}

fn parse_display_datetime_or_date(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim();
    NaiveDateTime::parse_from_str(value, DISPLAY_DATETIME_FORMAT)
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
}

#[derive(Default)]
struct Mp4Summary {
    media_date: Option<String>,
    has_metadata: bool,
    time_interpretation: String,
    fallback_creation_seconds: Option<u64>,
    movie_header: Option<MovieHeaderTimeInfo>,
}

#[derive(Clone, Copy, Debug)]
struct MovieHeaderTimeInfo {
    creation_seconds: u64,
    modification_seconds: u64,
    duration_seconds: Option<f64>,
}

fn scan_atoms(file: &mut File, allow_quicktime_without_ftyp: bool) -> Result<Mp4Summary, String> {
    let file_metadata = file.metadata().map_err(|err| err.to_string())?;
    let file_len = file_metadata.len();
    let file_modified = file_metadata.modified().ok();
    let mut position = 0u64;
    let mut found_ftyp = false;
    let mut summary = Mp4Summary::default();

    while position + 8 <= file_len {
        let atom = read_atom_header(file, position, file_len)?;
        match &atom.kind {
            b"ftyp" => found_ftyp = true,
            b"moov" => {
                scan_atom_children(file, atom.payload_start, atom.end, 0, &mut summary)?;
                // Some Samsung files append an SEFT footer after the complete
                // moov atom. It is not an ISO BMFF atom stream and must remain
                // opaque. All standard creation-time fields are inside moov,
                // so no later top-level data needs to be parsed.
                break;
            }
            _ => {}
        }
        position = atom.end;
    }

    if !found_ftyp && !allow_quicktime_without_ftyp {
        return Err("MP4/QuickTime ftyp atom was not found.".to_string());
    }
    let creation_seconds = summary
        .movie_header
        .map(|header| header.creation_seconds)
        .or(summary.fallback_creation_seconds);
    if let Some(seconds) = creation_seconds {
        let (media_date, interpretation) =
            interpret_quicktime_creation_date(seconds, summary.movie_header, file_modified);
        summary.media_date = media_date;
        summary.time_interpretation = interpretation;
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
    file.seek(SeekFrom::Start(position))
        .map_err(|err| err.to_string())?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header)
        .map_err(|err| err.to_string())?;
    let size32 = u32::from_be_bytes(header[..4].try_into().unwrap());
    let kind: [u8; 4] = header[4..8].try_into().unwrap();

    let (size, header_len) = match size32 {
        0 => (parent_end.saturating_sub(position), 8u64),
        1 => {
            let mut extended = [0u8; 8];
            file.read_exact(&mut extended)
                .map_err(|err| err.to_string())?;
            (u64::from_be_bytes(extended), 16u64)
        }
        value => (u64::from(value), 8u64),
    };

    if size < header_len {
        return Err("Invalid MP4/QuickTime atom size.".to_string());
    }
    let end = position
        .checked_add(size)
        .ok_or_else(|| "MP4/QuickTime atom size overflow.".to_string())?;
    if end > parent_end || end <= position {
        return Err("MP4/QuickTime atom is outside its parent.".to_string());
    }

    Ok(AtomHeader {
        kind,
        payload_start: position + header_len,
        end,
    })
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
            b"mvhd" => {
                summary.has_metadata = true;
                summary.movie_header =
                    read_movie_header_time_info(file, atom.payload_start, atom.end)?;
            }
            b"tkhd" | b"mdhd" => {
                summary.has_metadata = true;
                if summary.fallback_creation_seconds.is_none() {
                    summary.fallback_creation_seconds =
                        read_quicktime_creation_seconds(file, atom.payload_start, atom.end)?;
                }
            }
            // Creation timestamps only live in mvhd (under moov), tkhd
            // (under trak), and mdhd (under mdia). Other containers may carry
            // vendor-specific or non-atom payloads and must remain opaque.
            b"trak" | b"mdia" => {
                scan_atom_children(file, atom.payload_start, atom.end, depth + 1, summary)?;
            }
            _ => {}
        }
        position = atom.end;
    }
    Ok(())
}

fn find_creation_time_fields(
    file: &mut File,
    allow_quicktime_without_ftyp: bool,
) -> Result<Vec<CreationTimeField>, String> {
    let file_len = file.metadata().map_err(|err| err.to_string())?.len();
    let mut position = 0u64;
    let mut found_ftyp = false;
    let mut fields = Vec::new();
    while position + 8 <= file_len {
        let atom = read_atom_header(file, position, file_len)?;
        match &atom.kind {
            b"ftyp" => found_ftyp = true,
            b"moov" => {
                collect_creation_time_fields(file, atom.payload_start, atom.end, 0, &mut fields)?;
                break;
            }
            _ => {}
        }
        position = atom.end;
    }
    if !found_ftyp && !allow_quicktime_without_ftyp {
        return Err("MP4/QuickTime ftyp atom was not found.".to_string());
    }
    Ok(fields)
}

fn is_mov_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "mov" | "qt")
        })
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
                fields.push(read_creation_time_field(
                    file,
                    atom.payload_start,
                    atom.end,
                )?);
            }
            b"trak" | b"mdia" => {
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
    file.seek(SeekFrom::Start(start))
        .map_err(|err| err.to_string())?;
    let mut version_and_flags = [0u8; 4];
    file.read_exact(&mut version_and_flags)
        .map_err(|err| err.to_string())?;
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
    file.seek(SeekFrom::Start(field.offset))
        .map_err(|err| err.to_string())?;
    if field.width == 4 {
        let value = u32::try_from(value)
            .map_err(|_| "MP4 Media Date does not fit an existing version 0 field.".to_string())?;
        file.write_all(&value.to_be_bytes())
            .map_err(|err| err.to_string())
    } else {
        file.write_all(&value.to_be_bytes())
            .map_err(|err| err.to_string())
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

fn read_quicktime_creation_seconds(
    file: &mut File,
    start: u64,
    end: u64,
) -> Result<Option<u64>, String> {
    if end.saturating_sub(start) < 8 {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|err| err.to_string())?;
    let mut version_and_flags = [0u8; 4];
    file.read_exact(&mut version_and_flags)
        .map_err(|err| err.to_string())?;

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
    Ok(Some(seconds))
}

fn read_movie_header_time_info(
    file: &mut File,
    start: u64,
    end: u64,
) -> Result<Option<MovieHeaderTimeInfo>, String> {
    if end.saturating_sub(start) < 20 {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|err| err.to_string())?;
    let mut version_and_flags = [0u8; 4];
    file.read_exact(&mut version_and_flags)
        .map_err(|err| err.to_string())?;
    let (creation_seconds, modification_seconds, timescale, duration) = match version_and_flags[0] {
        0 => {
            let mut values = [0u8; 16];
            file.read_exact(&mut values)
                .map_err(|err| err.to_string())?;
            (
                u64::from(u32::from_be_bytes(values[0..4].try_into().unwrap())),
                u64::from(u32::from_be_bytes(values[4..8].try_into().unwrap())),
                u32::from_be_bytes(values[8..12].try_into().unwrap()),
                u64::from(u32::from_be_bytes(values[12..16].try_into().unwrap())),
            )
        }
        1 => {
            if end.saturating_sub(start) < 32 {
                return Ok(None);
            }
            let mut values = [0u8; 28];
            file.read_exact(&mut values)
                .map_err(|err| err.to_string())?;
            (
                u64::from_be_bytes(values[0..8].try_into().unwrap()),
                u64::from_be_bytes(values[8..16].try_into().unwrap()),
                u32::from_be_bytes(values[16..20].try_into().unwrap()),
                u64::from_be_bytes(values[20..28].try_into().unwrap()),
            )
        }
        _ => return Ok(None),
    };
    if creation_seconds == 0 || creation_seconds > i64::MAX as u64 {
        return Ok(None);
    }
    Ok(Some(MovieHeaderTimeInfo {
        creation_seconds,
        modification_seconds,
        duration_seconds: (timescale > 0).then_some(duration as f64 / f64::from(timescale)),
    }))
}

fn interpret_quicktime_creation_date(
    creation_seconds: u64,
    movie_header: Option<MovieHeaderTimeInfo>,
    file_modified: Option<std::time::SystemTime>,
) -> (Option<String>, String) {
    let unix_seconds = creation_seconds as i64 - QUICKTIME_UNIX_EPOCH_OFFSET;
    let Some(utc_creation) = DateTime::<Utc>::from_timestamp(unix_seconds, 0) else {
        return (None, String::new());
    };
    let raw_creation = utc_creation.naive_utc();
    let standard_creation = utc_creation.with_timezone(&Local);

    let use_camera_local = movie_header
        .zip(file_modified)
        .and_then(|(header, modified)| {
            let raw_mod_unix = i64::try_from(header.modification_seconds)
                .ok()?
                .checked_sub(QUICKTIME_UNIX_EPOCH_OFFSET)?;
            let raw_mod = DateTime::<Utc>::from_timestamp(raw_mod_unix, 0)?.naive_utc();
            let standard_mod = DateTime::<Utc>::from_timestamp(raw_mod_unix, 0)?
                .with_timezone(&Local)
                .naive_local();
            let file_mod = DateTime::<Local>::from(modified).naive_local();
            Some(should_use_camera_local_time(
                raw_creation,
                raw_mod,
                standard_mod,
                file_mod,
                header.duration_seconds,
            ))
        })
        .unwrap_or(false);

    if use_camera_local {
        return (
            Some(raw_creation.format(DISPLAY_DATETIME_FORMAT).to_string()),
            "Camera local time".to_string(),
        );
    }
    (
        Some(
            standard_creation
                .format(DISPLAY_DATETIME_FORMAT)
                .to_string(),
        ),
        "UTC (standard)".to_string(),
    )
}

fn should_use_camera_local_time(
    raw_creation: NaiveDateTime,
    raw_modification: NaiveDateTime,
    standard_modification: NaiveDateTime,
    file_modified: NaiveDateTime,
    duration_seconds: Option<f64>,
) -> bool {
    let camera_difference = (file_modified - raw_modification).num_seconds().abs();
    let standard_difference = (file_modified - standard_modification).num_seconds().abs();
    let timeline_is_coherent = duration_seconds.map_or(true, |duration| {
        let recorded_span = (raw_modification - raw_creation).num_seconds() as f64;
        (recorded_span - duration).abs() <= 300.0
    });
    timeline_is_coherent
        && camera_difference <= 300
        && standard_difference >= 3_600
        && camera_difference + 60 < standard_difference
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_media_date_without_a_time() {
        assert_eq!(
            parse_display_datetime_or_date("2014-02-04")
                .unwrap()
                .format(DISPLAY_DATETIME_FORMAT)
                .to_string(),
            "2014-02-04 00:00:00"
        );
    }

    #[test]
    fn detects_a_coherent_camera_local_quicktime_timeline() {
        let raw_creation =
            NaiveDateTime::parse_from_str("2016-07-18 21:02:36", DISPLAY_DATETIME_FORMAT).unwrap();
        let raw_modification =
            NaiveDateTime::parse_from_str("2016-07-18 21:07:48", DISPLAY_DATETIME_FORMAT).unwrap();
        let standard_modification =
            NaiveDateTime::parse_from_str("2016-07-19 06:07:48", DISPLAY_DATETIME_FORMAT).unwrap();
        let file_modified =
            NaiveDateTime::parse_from_str("2016-07-18 21:07:50", DISPLAY_DATETIME_FORMAT).unwrap();

        assert!(should_use_camera_local_time(
            raw_creation,
            raw_modification,
            standard_modification,
            file_modified,
            Some(311.311),
        ));
    }

    #[test]
    fn does_not_override_standard_time_when_the_timeline_is_ambiguous() {
        let raw_creation =
            NaiveDateTime::parse_from_str("2016-07-18 21:02:36", DISPLAY_DATETIME_FORMAT).unwrap();
        let raw_modification = raw_creation + chrono::Duration::seconds(312);

        assert!(!should_use_camera_local_time(
            raw_creation,
            raw_modification,
            raw_modification,
            raw_modification,
            Some(311.311),
        ));
    }
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
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_mp4_scan_{}.mp4",
            std::process::id()
        ));
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
    fn reads_and_writes_quicktime_mov_without_ftyp_atom() {
        let unix_2012 = 1_344_992_400u32;
        let quicktime_2012 = unix_2012 + QUICKTIME_UNIX_EPOCH_OFFSET as u32;
        let mvhd = atom(b"mvhd", &version_zero_date_payload(quicktime_2012));
        let moov = atom(b"moov", &mvhd);
        let mdat = atom(b"mdat", &[0x42; 16]);
        let path = std::env::temp_dir().join(format!(
            "sh148_photo_management_quicktime_{}.mov",
            std::process::id()
        ));
        std::fs::write(&path, [moov, mdat].concat()).unwrap();

        let original = scan_media_file(&path);
        assert_eq!(original.media_kind, "mp4");
        assert_eq!(original.media_type, "QuickTime movie");
        assert_eq!(original.metadata_status, "O");

        let updated = "2013-12-18 05:15:08";
        write_media_date(&path, updated).unwrap();
        let result = scan_media_file(&path);
        let _ = std::fs::remove_file(path);
        assert_eq!(result.media_date, updated);
    }

    #[test]
    fn writes_all_existing_creation_date_fields_in_place_including_zero_values() {
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
            "sh148_exif_file_tool_mp4_write_{}.mp4",
            std::process::id()
        ));
        let original_bytes = [ftyp, mdat, moov].concat();
        std::fs::write(&path, &original_bytes).unwrap();

        let display_value = "2013-12-18 05:15:08";
        write_media_date(&path, display_value).unwrap();

        let updated_bytes = std::fs::read(&path).unwrap();
        let result = scan_media_file(&path);
        let mut file = File::open(&path).unwrap();
        let fields = find_creation_time_fields(&mut file, false).unwrap();
        let local = Local
            .from_local_datetime(
                &NaiveDateTime::parse_from_str(display_value, DISPLAY_DATETIME_FORMAT).unwrap(),
            )
            .single()
            .unwrap();
        let expected = (local.with_timezone(&Utc).timestamp() + QUICKTIME_UNIX_EPOCH_OFFSET) as u64;

        assert_eq!(updated_bytes.len(), original_bytes.len());
        assert_eq!(
            &updated_bytes[updated_bytes.len() - 16..],
            &original_bytes[original_bytes.len() - 16..]
        );
        assert_eq!(result.media_date, display_value);
        assert_eq!(fields.iter().filter(|field| field.original == 0).count(), 0);
        assert_eq!(
            fields
                .iter()
                .filter(|field| field.original == expected)
                .count(),
            5
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_an_existing_zero_creation_date_in_place() {
        let ftyp = atom(b"ftyp", b"isom\0\0\0\0isom");
        let mdat = atom(b"mdat", &[0x5a; 32]);
        let moov = atom(b"moov", &atom(b"mvhd", &version_zero_date_payload(0)));
        let original_bytes = [ftyp, mdat, moov].concat();
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_mp4_zero_date_write_{}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, &original_bytes).unwrap();

        let display_value = "2016-04-27 18:10:53";
        write_media_date(&path, display_value).unwrap();

        let updated_bytes = std::fs::read(&path).unwrap();
        let result = scan_media_file(&path);
        assert_eq!(updated_bytes.len(), original_bytes.len());
        assert_eq!(result.media_date, display_value);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ignores_opaque_vendor_metadata_while_writing_creation_date() {
        let ftyp = atom(b"ftyp", b"isom\0\0\0\0isom");
        let mdat = atom(b"mdat", &[0x5a; 32]);
        let mvhd = atom(b"mvhd", &version_zero_date_payload(0));
        // This is intentionally not an atom stream. Some cameras place
        // proprietary payloads inside udta, which is irrelevant to the
        // standard header creation timestamps.
        let opaque_udta = atom(b"udta", &[0xff, 0xff, 0xff, 0xff, b'v', b'e', b'n', b'd']);
        let moov = atom(b"moov", &[mvhd, opaque_udta].concat());
        let original_bytes = [ftyp, mdat, moov].concat();
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_mp4_opaque_metadata_{}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, &original_bytes).unwrap();

        let display_value = "2016-05-19 20:31:26";
        write_media_date(&path, display_value).unwrap();

        let updated_bytes = std::fs::read(&path).unwrap();
        let result = scan_media_file(&path);
        assert_eq!(updated_bytes.len(), original_bytes.len());
        assert_eq!(result.media_date, display_value);
        assert_eq!(
            &updated_bytes[updated_bytes.len() - 16..],
            &original_bytes[original_bytes.len() - 16..]
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn preserves_samsung_seft_footer_after_moov() {
        let ftyp = atom(b"ftyp", b"mp42\0\0\0\0isommp42");
        let mdat = atom(b"mdat", &[0x5a; 32]);
        let moov = atom(b"moov", &atom(b"mvhd", &version_zero_date_payload(0)));
        let seft_footer = b"\0\0A\n\x12\0\0\0BackupRestore_Data_test_SEFHe\0\0\0\x01\0\0\0\0\0A\n,\0\0\0,\0\0\0\x18\0\0\0SEFT";
        let original_bytes = [
            ftyp.as_slice(),
            mdat.as_slice(),
            moov.as_slice(),
            seft_footer.as_slice(),
        ]
        .concat();
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_mp4_seft_footer_{}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, &original_bytes).unwrap();

        let display_value = "2016-05-19 20:31:26";
        write_media_date(&path, display_value).unwrap();

        let updated_bytes = std::fs::read(&path).unwrap();
        let result = scan_media_file(&path);
        assert_eq!(updated_bytes.len(), original_bytes.len());
        assert_eq!(result.media_date, display_value);
        assert_eq!(
            &updated_bytes[updated_bytes.len() - seft_footer.len()..],
            seft_footer
        );

        let _ = std::fs::remove_file(path);
    }
}
