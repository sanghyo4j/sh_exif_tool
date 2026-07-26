use std::fs;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use chrono::{NaiveDate, NaiveDateTime};

const DISPLAY_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const EXIF_DATETIME_FORMAT: &str = "%Y:%m:%d %H:%M:%S";
const DEFAULT_WRITABLE_ASCII_LEN: usize = 64;

#[derive(Clone, Debug)]
pub struct ExifMetadata {
    pub has_exif: bool,
    pub taken_date: String,
    pub date_time_original: String,
    pub date_time_digitized: String,
    pub image_date_time: String,
    pub camera_make: String,
    pub camera_model: String,
    pub lens_model: String,
    pub software: String,
    pub artist: String,
    pub image_description: String,
    pub copyright: String,
    pub exif_version: String,
    pub exposure_program: String,
    pub white_balance: String,
    pub focal_length_35mm: String,
    pub shutter_speed: String,
    pub aperture: String,
    pub iso_speed: String,
    pub focal_length: String,
    pub flash_fired: String,
    pub metering_mode: String,
    pub image_width: String,
    pub image_height: String,
    pub orientation: String,
    pub color_space: String,
    pub gps_latitude: String,
    pub gps_longitude: String,
    pub gps_altitude: String,
}

impl Default for ExifMetadata {
    fn default() -> Self {
        Self {
            has_exif: false,
            taken_date: empty_value(),
            date_time_original: empty_value(),
            date_time_digitized: empty_value(),
            image_date_time: empty_value(),
            camera_make: empty_value(),
            camera_model: empty_value(),
            lens_model: empty_value(),
            software: empty_value(),
            artist: empty_value(),
            image_description: empty_value(),
            copyright: empty_value(),
            exif_version: empty_value(),
            exposure_program: empty_value(),
            white_balance: empty_value(),
            focal_length_35mm: empty_value(),
            shutter_speed: empty_value(),
            aperture: empty_value(),
            iso_speed: empty_value(),
            focal_length: empty_value(),
            flash_fired: empty_value(),
            metering_mode: empty_value(),
            image_width: empty_value(),
            image_height: empty_value(),
            orientation: empty_value(),
            color_space: empty_value(),
            gps_latitude: empty_value(),
            gps_longitude: empty_value(),
            gps_altitude: empty_value(),
        }
    }
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

#[derive(Clone, Copy)]
struct Entry<'a> {
    data: &'a [u8],
    endian: Endian,
    tag: u16,
    field_type: u16,
    count: u32,
    value_field: usize,
}

pub fn read_exif_metadata(path: &Path) -> ExifMetadata {
    let Ok(bytes) = fs::read(path) else {
        return ExifMetadata::default();
    };

    let Some(tiff) = find_exif_tiff(&bytes) else {
        return ExifMetadata::default();
    };

    parse_tiff(tiff).unwrap_or_default()
}

pub(crate) fn read_exif_metadata_for_scan(path: &Path) -> ExifMetadata {
    let Ok(file) = fs::File::open(path) else {
        return ExifMetadata::default();
    };
    let Some(tiff) = read_jpeg_exif_tiff(file) else {
        return ExifMetadata::default();
    };
    parse_tiff(&tiff).unwrap_or_default()
}

fn read_jpeg_exif_tiff(file: fs::File) -> Option<Vec<u8>> {
    let mut reader = BufReader::new(file);
    let mut signature = [0u8; 2];
    reader.read_exact(&mut signature).ok()?;
    if signature != [0xff, 0xd8] {
        return None;
    }

    loop {
        let mut marker_prefix = [0u8; 1];
        reader.read_exact(&mut marker_prefix).ok()?;
        while marker_prefix[0] != 0xff {
            reader.read_exact(&mut marker_prefix).ok()?;
        }

        let mut marker = [0u8; 1];
        reader.read_exact(&mut marker).ok()?;
        while marker[0] == 0xff {
            reader.read_exact(&mut marker).ok()?;
        }
        if marker[0] == 0x00 {
            continue;
        }
        if marker[0] == 0xda || marker[0] == 0xd9 {
            return None;
        }
        if marker[0] == 0x01 || (0xd0..=0xd7).contains(&marker[0]) {
            continue;
        }

        let mut length_bytes = [0u8; 2];
        reader.read_exact(&mut length_bytes).ok()?;
        let segment_len = usize::from(u16::from_be_bytes(length_bytes));
        if segment_len < 2 {
            return None;
        }
        let payload_len = segment_len - 2;

        if marker[0] == 0xe1 {
            let mut payload = vec![0u8; payload_len];
            reader.read_exact(&mut payload).ok()?;
            if let Some(tiff) = payload.strip_prefix(b"Exif\0\0") {
                return Some(tiff.to_vec());
            }
        } else {
            reader.seek(SeekFrom::Current(payload_len as i64)).ok()?;
        }
    }
}

fn write_exif_tag_values<F>(path: &Path, tags: &[u16], mut make_bytes: F) -> Result<(), String>
where
    F: FnMut(&Entry<'_>) -> Option<Vec<u8>>,
{
    let mut bytes = fs::read(path).map_err(|err| err.to_string())?;
    let tiff_start = find_exif_tiff_start(&bytes).ok_or_else(|| "EXIF data was not found.".to_string())?;
    let tiff = bytes
        .get(tiff_start..)
        .ok_or_else(|| "Invalid EXIF data offset.".to_string())?;
    let endian = read_tiff_endian(tiff).ok_or_else(|| "Invalid TIFF header.".to_string())?;
    let ifd0_offset = read_u32(tiff, 4, endian).ok_or_else(|| "Invalid IFD0 offset.".to_string())? as usize;

    let mut writes = Vec::new();
    let ifd0_entries = read_ifd_entries(tiff, ifd0_offset, endian)
        .ok_or_else(|| "Unable to read IFD0 entries.".to_string())?;

    for entry in &ifd0_entries {
        if tags.contains(&entry.tag) {
            if let Some(value_bytes) = make_bytes(entry) {
                if let Some(range) = writable_entry_range(entry, value_bytes.len()) {
                    writes.push((range, value_bytes));
                }
            }
        }
    }

    if let Some(exif_ifd_offset) = ifd0_entries
        .iter()
        .find(|entry| entry.tag == 0x8769)
        .and_then(|entry| entry.as_long())
        .map(|offset| offset as usize)
    {
        if let Some(exif_entries) = read_ifd_entries(tiff, exif_ifd_offset, endian) {
            for entry in &exif_entries {
                if tags.contains(&entry.tag) {
                    if let Some(value_bytes) = make_bytes(entry) {
                        if let Some(range) = writable_entry_range(entry, value_bytes.len()) {
                            writes.push((range, value_bytes));
                        }
                    }
                }
            }
        }
    }

    if writes.is_empty() {
        return Err(format!(
            "No writable EXIF tag was found for tag(s) {:?}. This tool only overwrites existing EXIF entries and does not create missing tags.",
            tags
        ));
    }

    for ((relative_start, len), value_bytes) in writes {
        let start = tiff_start
            .checked_add(relative_start)
            .ok_or_else(|| "Invalid EXIF write offset.".to_string())?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| "Invalid EXIF write length.".to_string())?;
        let target = bytes
            .get_mut(start..end)
            .ok_or_else(|| "EXIF write range is outside the file.".to_string())?;
        target.fill(0);
        target[..value_bytes.len()].copy_from_slice(&value_bytes);
    }

    fs::write(path, bytes).map_err(|err| err.to_string())
}

fn writable_entry_range(entry: &Entry<'_>, value_len: usize) -> Option<(usize, usize)> {
    match entry.field_type {
        2 => writable_ascii_range(entry, value_len),
        3 if entry.count == 1 && value_len <= 2 => entry.data.get(entry.value_field..entry.value_field + 2).map(|_| (entry.value_field, 2)),
        4 if entry.count == 1 && value_len <= 4 => entry.data.get(entry.value_field..entry.value_field + 4).map(|_| (entry.value_field, 4)),
        5 if entry.count == 1 => {
            let offset = read_u32(entry.data, entry.value_field, entry.endian)? as usize;
            entry.data.get(offset..offset + 8).map(|_| (offset, 8))
        }
        _ => None,
    }
}

fn write_exif_ascii_tags(path: &Path, tags: &[u16], value: &str) -> Result<(), String> {
    let mut value_bytes = value.as_bytes().to_vec();
    value_bytes.push(0);
    write_exif_tag_values(path, tags, |_| Some(value_bytes.clone()))
}

fn write_exif_short_tag(path: &Path, tags: &[u16], value: u16) -> Result<(), String> {
    write_exif_tag_values(path, tags, |entry| {
        if entry.field_type == 3 && entry.count == 1 {
            let bytes = match entry.endian {
                Endian::Little => value.to_le_bytes(),
                Endian::Big => value.to_be_bytes(),
            };
            Some(bytes.to_vec())
        } else {
            None
        }
    })
}

fn write_exif_rational_tag(path: &Path, tags: &[u16], numerator: u32, denominator: u32) -> Result<(), String> {
    write_exif_tag_values(path, tags, |entry| {
        if entry.field_type == 5 && entry.count == 1 {
            let mut bytes = Vec::with_capacity(8);
            match entry.endian {
                Endian::Little => {
                    bytes.extend_from_slice(&numerator.to_le_bytes());
                    bytes.extend_from_slice(&denominator.to_le_bytes());
                }
                Endian::Big => {
                    bytes.extend_from_slice(&numerator.to_be_bytes());
                    bytes.extend_from_slice(&denominator.to_be_bytes());
                }
            }
            Some(bytes)
        } else {
            None
        }
    })
}

pub fn write_taken_date(path: &Path, display_value: &str) -> Result<(), String> {
    let exif_value = display_datetime_to_exif(display_value)?;
    write_exif_ascii_tags(path, &[0x0132, 0x9003, 0x9004], &exif_value)
}

pub fn write_camera_make(path: &Path, value: &str) -> Result<(), String> {
    write_exif_ascii_tags(path, &[0x010f], value)
}

pub fn write_camera_model(path: &Path, value: &str) -> Result<(), String> {
    write_exif_ascii_tags(path, &[0x0110], value)
}

pub fn write_lens_model(path: &Path, value: &str) -> Result<(), String> {
    write_exif_ascii_tags(path, &[0xa434], value)
}

pub fn write_software(path: &Path, value: &str) -> Result<(), String> {
    write_exif_ascii_tags(path, &[0x0131], value)
}

pub fn write_artist(path: &Path, value: &str) -> Result<(), String> {
    write_exif_ascii_tags(path, &[0x013b], value)
}

pub fn write_shutter_speed(path: &Path, value: &str) -> Result<(), String> {
    let (num, den) = parse_exif_rational_value(value)?;
    write_exif_rational_tag(path, &[0x829a], num, den)
}

pub fn write_aperture(path: &Path, value: &str) -> Result<(), String> {
    let (num, den) = parse_aperture_value(value)?;
    write_exif_rational_tag(path, &[0x829d], num, den)
}

pub fn write_iso_speed(path: &Path, value: &str) -> Result<(), String> {
    let iso = parse_u16_value(value)?;
    write_exif_short_tag(path, &[0x8827], iso)
}

pub fn write_focal_length(path: &Path, value: &str) -> Result<(), String> {
    let (num, den) = parse_exif_rational_value(value)?;
    write_exif_rational_tag(path, &[0x920a], num, den)
}

pub fn write_flash_fired(path: &Path, value: &str) -> Result<(), String> {
    let flash = parse_flash_value(value)?;
    write_exif_short_tag(path, &[0x9209], flash)
}

pub fn write_metering_mode(path: &Path, value: &str) -> Result<(), String> {
    let mode = parse_metering_mode_value(value)?;
    write_exif_short_tag(path, &[0x9207], mode)
}

pub fn write_orientation(path: &Path, value: &str) -> Result<(), String> {
    let orientation = parse_u16_value(value)?;
    write_exif_short_tag(path, &[0x0112], orientation)
}

pub fn write_color_space(path: &Path, value: &str) -> Result<(), String> {
    let color_space = parse_color_space_value(value)?;
    write_exif_short_tag(path, &[0xa001], color_space)
}

pub fn remove_gps_information(path: &Path) -> Result<(), String> {
    let mut bytes = fs::read(path).map_err(|err| err.to_string())?;
    let tiff_start = find_exif_tiff_start(&bytes).ok_or_else(|| "EXIF data was not found.".to_string())?;

    let ranges = {
        let tiff = bytes
            .get(tiff_start..)
            .ok_or_else(|| "Invalid EXIF data offset.".to_string())?;
        let endian = read_tiff_endian(tiff).ok_or_else(|| "Invalid TIFF header.".to_string())?;
        let ifd0_offset = read_u32(tiff, 4, endian).ok_or_else(|| "Invalid IFD0 offset.".to_string())? as usize;
        let ifd0_entries = read_ifd_entries(tiff, ifd0_offset, endian)
            .ok_or_else(|| "Unable to read IFD0 entries.".to_string())?;

        let Some(gps_entry) = ifd0_entries.iter().find(|entry| entry.tag == 0x8825) else {
            return Ok(());
        };

        let Some(gps_offset) = gps_entry.as_long().map(|value| value as usize).filter(|value| *value > 0) else {
            return Ok(());
        };

        let mut ranges = vec![(gps_entry.value_field, 4usize)];

        if let Some(gps_entries) = read_ifd_entries(tiff, gps_offset, endian) {
            for entry in &gps_entries {
                if let Some(range) = entry_storage_range(entry) {
                    ranges.push(range);
                }
            }
        }

        if let Some(count) = read_u16(tiff, gps_offset, endian).map(|value| value as usize) {
            if let Some(ifd_len) = 2usize
                .checked_add(count.saturating_mul(12))
                .and_then(|value| value.checked_add(4))
            {
                if gps_offset
                    .checked_add(ifd_len)
                    .and_then(|end| tiff.get(gps_offset..end))
                    .is_some()
                {
                    ranges.push((gps_offset, ifd_len));
                }
            }
        }

        ranges
    };

    for (relative_start, len) in ranges {
        let start = tiff_start
            .checked_add(relative_start)
            .ok_or_else(|| "Invalid GPS write offset.".to_string())?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| "Invalid GPS write length.".to_string())?;
        if let Some(target) = bytes.get_mut(start..end) {
            target.fill(0);
        }
    }

    fs::write(path, bytes).map_err(|err| err.to_string())
}

pub fn remove_exif_metadata(path: &Path, backup_before_changes: bool) -> Result<(), String> {
    let original = fs::read(path).map_err(|err| err.to_string())?;
    if original.len() < 4 || original[0] != 0xff || original[1] != 0xd8 {
        return Err("Only JPEG files are supported for EXIF removal.".to_string());
    }

    let mut updated = original.clone();
    let mut removed = false;
    while let Some((start, end)) = find_exif_app1_range(&updated) {
        updated.drain(start..end);
        removed = true;
    }
    if !removed {
        return Ok(());
    }

    let original_file_metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    let original_created = original_file_metadata.created().ok();
    let original_modified = original_file_metadata.modified().ok();

    if !backup_before_changes {
        fs::write(path, &updated).map_err(|err| err.to_string())?;
        if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
            let _ = fs::write(path, &original);
            let _ = crate::fs::set_file_times(path, original_created, original_modified);
            return Err(err);
        }
        return Ok(());
    }

    let backup_path = exif_backup_path(path);
    if backup_path.exists() {
        return Err(format!("Backup file already exists: {}", backup_path.display()));
    }
    fs::rename(path, &backup_path).map_err(|err| err.to_string())?;
    if let Err(err) = fs::write(path, updated) {
        let _ = fs::rename(&backup_path, path);
        return Err(err.to_string());
    }
    if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
        let _ = fs::remove_file(path);
        let _ = fs::rename(&backup_path, path);
        return Err(err);
    }
    Ok(())
}

pub fn rewrite_basic_exif_metadata(
    path: &Path,
    metadata: &ExifMetadata,
    backup_before_changes: bool,
) -> Result<PathBuf, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    if find_exif_tiff_start(&bytes).is_some() {
        return Err("Refusing to rewrite an existing EXIF structure. Existing EXIF files only support editing tags that are already present.".to_string());
    }
    let exif_segment = build_basic_exif_app1(metadata)?;
    let updated = replace_or_insert_exif_app1(&bytes, &exif_segment)?;
    if !backup_before_changes {
        let original_file_metadata = fs::metadata(path).map_err(|err| err.to_string())?;
        let original_created = original_file_metadata.created().ok();
        let original_modified = original_file_metadata.modified().ok();
        if let Err(err) = fs::write(path, updated) {
            return Err(err.to_string());
        }
        if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
            let _ = fs::write(path, &bytes);
            let _ = crate::fs::set_file_times(path, original_created, original_modified);
            return Err(err);
        }
        return Ok(path.to_path_buf());
    }

    let backup_path = exif_backup_path(path);
    if backup_path.exists() {
        return Err(format!("Backup file already exists: {}", backup_path.display()));
    }

    fs::rename(path, &backup_path).map_err(|err| err.to_string())?;
    if let Err(err) = fs::write(path, updated) {
        let _ = fs::rename(&backup_path, path);
        return Err(err.to_string());
    }
    Ok(path.to_path_buf())
}

pub fn rewrite_generated_basic_exif_metadata(path: &Path, metadata: &ExifMetadata) -> Result<(), String> {
    if !is_generated_new_exif_path(path) {
        return Err("Refusing to rewrite EXIF in place unless this is a generated EXIF file.".to_string());
    }

    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    if find_exif_tiff_start(&bytes).is_none() {
        return Err("Generated EXIF file does not contain an EXIF structure.".to_string());
    }

    let exif_segment = build_basic_exif_app1(metadata)?;
    let updated = replace_or_insert_exif_app1(&bytes, &exif_segment)?;
    fs::write(path, updated).map_err(|err| err.to_string())
}

pub fn rewrite_repairable_exif_metadata(
    path: &Path,
    metadata: &ExifMetadata,
    backup_before_changes: bool,
) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let tiff = find_exif_tiff(&bytes).ok_or_else(|| "EXIF data was not found.".to_string())?;
    if !is_basic_generated_tiff(tiff) {
        return Err(
            "The requested tag is not present. The file contains additional EXIF tags, so a basic EXIF rebuild was refused to prevent metadata loss."
                .to_string(),
        );
    }
    if find_exif_tiff_start(&bytes).is_none() {
        return Err("EXIF data was not found.".to_string());
    }

    let exif_segment = build_basic_exif_app1(metadata)?;
    let updated = replace_or_insert_exif_app1(&bytes, &exif_segment)?;
    if backup_before_changes {
        let backup_path = exif_backup_path(path);
        if !backup_path.exists() {
            fs::copy(path, &backup_path).map_err(|err| err.to_string())?;
        }
    }

    if let Err(err) = fs::write(path, updated) {
        let _ = fs::write(path, &bytes);
        return Err(err.to_string());
    }
    Ok(())
}

pub fn is_generated_new_exif_path(path: &Path) -> bool {
    has_basic_generated_exif_shape(path)
}

pub fn remove_exif_tag(path: &Path, key: &str, backup_before_changes: bool) -> Result<(), String> {
    let (ifd0_tags, exif_tags, gps_tags): (&[u16], &[u16], &[u16]) = match key {
        "taken_date" => (&[0x0132], &[0x9003, 0x9004], &[]),
        "date_time_original" => (&[], &[0x9003], &[]),
        "date_time_digitized" => (&[], &[0x9004], &[]),
        "image_date_time" => (&[0x0132], &[], &[]),
        "camera_make" => (&[0x010f], &[], &[]),
        "image_description" => (&[0x010e], &[], &[]),
        "copyright" => (&[0x8298], &[], &[]),
        "camera_model" => (&[0x0110], &[], &[]),
        "orientation" => (&[0x0112], &[], &[]),
        "software" => (&[0x0131], &[], &[]),
        "artist" => (&[0x013b], &[], &[]),
        "shutter_speed" => (&[], &[0x829a], &[]),
        "exposure_program" => (&[], &[0x8822], &[]),
        "exif_version" => (&[], &[0x9000], &[]),
        "aperture" => (&[], &[0x829d], &[]),
        "iso_speed" => (&[], &[0x8827], &[]),
        "focal_length" => (&[], &[0x920a], &[]),
        "flash_fired" => (&[], &[0x9209], &[]),
        "metering_mode" => (&[], &[0x9207], &[]),
        "image_width" => (&[], &[0xa002], &[]),
        "image_height" => (&[], &[0xa003], &[]),
        "color_space" => (&[], &[0xa001], &[]),
        "lens_model" => (&[], &[0xa434], &[]),
        "white_balance" => (&[], &[0xa403], &[]),
        "focal_length_35mm" => (&[], &[0xa405], &[]),
        "gps_latitude" => (&[], &[], &[0x0001, 0x0002]),
        "gps_longitude" => (&[], &[], &[0x0003, 0x0004]),
        "gps_altitude" => (&[], &[], &[0x0005, 0x0006]),
        _ => return Err("This metadata row cannot be removed individually.".to_string()),
    };

    let original_file_metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    let original_created = original_file_metadata.created().ok();
    let original_modified = original_file_metadata.modified().ok();
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let tiff = find_exif_tiff(&bytes).ok_or_else(|| "EXIF data was not found.".to_string())?;
    let updated_tiff = rewrite_exif_tiff_removing_tags(tiff, ifd0_tags, exif_tags, gps_tags)?;
    let segment = build_exif_app1_from_tiff(&updated_tiff)?;
    let updated = replace_or_insert_exif_app1(&bytes, &segment)?;
    if backup_before_changes {
        let backup_path = exif_backup_path(path);
        if !backup_path.exists() {
            fs::copy(path, backup_path).map_err(|err| err.to_string())?;
        }
    }
    fs::write(path, updated).map_err(|err| err.to_string())?;
    if let Err(err) = crate::fs::set_file_times(path, original_created, original_modified) {
        let _ = fs::write(path, bytes);
        let _ = crate::fs::set_file_times(path, original_created, original_modified);
        return Err(err);
    }
    Ok(())
}

pub fn exif_backup_path(path: &Path) -> PathBuf {
    path.with_extension("bak_exif")
}

fn has_basic_generated_exif_shape(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let Some(tiff) = find_exif_tiff(&bytes) else {
        return false;
    };
    is_basic_generated_tiff(tiff)
}

fn is_basic_generated_tiff(tiff: &[u8]) -> bool {
    let Some(endian) = read_tiff_endian(tiff) else {
        return false;
    };
    if !matches!(endian, Endian::Little) || read_u16(tiff, 2, endian) != Some(42) {
        return false;
    }

    let Some(ifd0_offset) = read_u32(tiff, 4, endian).map(|value| value as usize) else {
        return false;
    };
    let Some(ifd0_entries) = read_ifd_entries(tiff, ifd0_offset, endian) else {
        return false;
    };
    if ifd0_entries.is_empty() {
        return false;
    }

    const BASIC_IFD0_TAGS: &[u16] = &[0x010f, 0x0110, 0x0112, 0x0131, 0x0132, 0x013b, 0x8769];
    const BASIC_EXIF_TAGS: &[u16] = &[0x829a, 0x829d, 0x8827, 0x9003, 0x9004, 0x9207, 0x9209, 0x920a, 0xa001, 0xa434];

    if ifd0_entries.iter().any(|entry| !BASIC_IFD0_TAGS.contains(&entry.tag)) {
        return false;
    }

    if let Some(exif_ifd_offset) = ifd0_entries
        .iter()
        .find(|entry| entry.tag == 0x8769)
        .and_then(|entry| entry.as_long())
        .map(|offset| offset as usize)
    {
        let Some(exif_entries) = read_ifd_entries(tiff, exif_ifd_offset, endian) else {
            return false;
        };
        if exif_entries.iter().any(|entry| !BASIC_EXIF_TAGS.contains(&entry.tag)) {
            return false;
        }
    }

    true
}

fn parse_u16_value(value: &str) -> Result<u16, String> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|_| "Expected an integer value.".to_string())
}

fn parse_optional_u16_value(value: &str) -> Result<Option<u16>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_u16_value(value).map(Some)
}

fn parse_exif_rational_value(value: &str) -> Result<(u32, u32), String> {
    let value = value.trim();
    let value = value.strip_suffix('s').unwrap_or(value).trim();
    if let Some((num_str, den_str)) = value.split_once('/') {
        let num = num_str.trim().parse::<u32>().map_err(|_| "Expected rational value like 1/125 or 3/2.".to_string())?;
        let den = den_str.trim().parse::<u32>().map_err(|_| "Expected rational value like 1/125 or 3/2.".to_string())?;
        return Ok((num, den));
    }

    let float_value = value
        .trim_start_matches('f')
        .trim_start_matches('/')
        .trim_end_matches("mm")
        .trim()
        .parse::<f64>()
        .map_err(|_| "Expected numeric or fraction value.".to_string())?;

    if float_value <= 0.0 {
        return Err("Expected a positive numeric value.".to_string());
    }

    let denom = 1000u32;
    let num = (float_value * denom as f64).round() as u32;
    Ok((num, denom))
}

fn parse_aperture_value(value: &str) -> Result<(u32, u32), String> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix('f').unwrap_or(trimmed).trim();
    let trimmed = trimmed.strip_prefix('/').unwrap_or(trimmed).trim();
    let float_value = trimmed
        .parse::<f64>()
        .map_err(|_| "Expected aperture like f/2.8 or 2.8.".to_string())?;

    if float_value <= 0.0 {
        return Err("Expected a positive aperture value.".to_string());
    }

    let denom = 10u32;
    let num = (float_value * denom as f64).round() as u32;
    Ok((num, denom))
}

fn parse_flash_value(value: &str) -> Result<u16, String> {
    let lowered = value.trim().to_lowercase();
    if lowered == "yes" || lowered == "true" || lowered == "1" {
        return Ok(1);
    }
    if lowered == "no" || lowered == "false" || lowered == "0" {
        return Ok(0);
    }
    parse_u16_value(value)
}

fn parse_optional_flash_value(value: &str) -> Result<Option<u16>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_flash_value(value).map(Some)
}

fn parse_metering_mode_value(value: &str) -> Result<u16, String> {
    let lowered = value.trim().to_lowercase();
    let mode = match lowered.as_str() {
        "average" => 1,
        "center-weighted average" => 2,
        "spot" => 3,
        "multi-spot" => 4,
        "pattern" => 5,
        "partial" => 6,
        "other" => 255,
        _ => parse_u16_value(value)?,
    };
    Ok(mode)
}

fn parse_optional_metering_mode_value(value: &str) -> Result<Option<u16>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_metering_mode_value(value).map(Some)
}

fn parse_orientation_value(value: &str) -> Result<u16, String> {
    let lowered = value.trim().to_lowercase();
    let orientation = match lowered.as_str() {
        "normal" => 1,
        "mirrored horizontal" => 2,
        "rotated 180" => 3,
        "mirrored vertical" => 4,
        "mirrored horizontal rotated 270" => 5,
        "rotated 90" => 6,
        "mirrored horizontal rotated 90" => 7,
        "rotated 270" => 8,
        _ => parse_u16_value(value)?,
    };
    Ok(orientation)
}

fn parse_optional_orientation_value(value: &str) -> Result<Option<u16>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_orientation_value(value).map(Some)
}

fn parse_color_space_value(value: &str) -> Result<u16, String> {
    let lowered = value.trim().to_lowercase();
    let color_space = match lowered.as_str() {
        "srgb" => 1,
        "uncalibrated" => 0xffff,
        _ => parse_u16_value(value)?,
    };
    Ok(color_space)
}

fn parse_optional_color_space_value(value: &str) -> Result<Option<u16>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_color_space_value(value).map(Some)
}

fn build_basic_exif_app1(metadata: &ExifMetadata) -> Result<Vec<u8>, String> {
    let tiff = build_basic_tiff(metadata)?;
    let segment_len = tiff
        .len()
        .checked_add(8)
        .ok_or_else(|| "EXIF segment is too large.".to_string())?;
    if segment_len > u16::MAX as usize {
        return Err("EXIF segment is too large for JPEG APP1.".to_string());
    }

    let mut segment = Vec::with_capacity(segment_len + 2);
    segment.extend_from_slice(&[0xff, 0xe1]);
    segment.extend_from_slice(&(segment_len as u16).to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(&tiff);
    Ok(segment)
}

fn build_basic_tiff(metadata: &ExifMetadata) -> Result<Vec<u8>, String> {
    let mut ifd0 = TiffIfdBuilder::new();
    let mut exif_ifd = TiffIfdBuilder::new();

    ifd0.add_writable_ascii(0x010f, &metadata.camera_make, DEFAULT_WRITABLE_ASCII_LEN);
    ifd0.add_writable_ascii(0x0110, &metadata.camera_model, DEFAULT_WRITABLE_ASCII_LEN);
    if let Some(orientation) = parse_optional_orientation_value(&metadata.orientation)? {
        ifd0.add_short(0x0112, orientation);
    }
    ifd0.add_writable_ascii(0x0131, &metadata.software, DEFAULT_WRITABLE_ASCII_LEN);
    if let Some(datetime) = optional_exif_datetime(&metadata.taken_date)? {
        ifd0.add_ascii_value(0x0132, datetime.clone());
        exif_ifd.add_ascii_value(0x9003, datetime.clone());
        exif_ifd.add_ascii_value(0x9004, datetime);
    }
    ifd0.add_writable_ascii(0x013b, &metadata.artist, DEFAULT_WRITABLE_ASCII_LEN);

    if let Some((num, den)) = parse_optional_rational_value(&metadata.shutter_speed)? {
        exif_ifd.add_rational(0x829a, num, den);
    }
    if let Some((num, den)) = parse_optional_aperture_value(&metadata.aperture)? {
        exif_ifd.add_rational(0x829d, num, den);
    }
    if let Some(iso) = parse_optional_u16_value(&metadata.iso_speed)? {
        exif_ifd.add_short(0x8827, iso);
    }
    if let Some(mode) = parse_optional_metering_mode_value(&metadata.metering_mode)? {
        exif_ifd.add_short(0x9207, mode);
    }
    if let Some(flash) = parse_optional_flash_value(&metadata.flash_fired)? {
        exif_ifd.add_short(0x9209, flash);
    }
    if let Some((num, den)) = parse_optional_rational_value(&metadata.focal_length)? {
        exif_ifd.add_rational(0x920a, num, den);
    }
    if let Some(color_space) = parse_optional_color_space_value(&metadata.color_space)? {
        exif_ifd.add_short(0xa001, color_space);
    }
    exif_ifd.add_writable_ascii(0xa434, &metadata.lens_model, DEFAULT_WRITABLE_ASCII_LEN);

    let include_exif_ifd = !exif_ifd.entries.is_empty();
    if include_exif_ifd {
        ifd0.add_long(0x8769, 0);
    }

    let ifd0_count = ifd0.entries.len();
    let ifd0_size = 2 + ifd0_count * 12 + 4;
    let exif_ifd_offset = 8 + ifd0_size + ifd0.extra_len();
    if include_exif_ifd {
        ifd0.set_long_value(0x8769, exif_ifd_offset as u32);
    }

    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes());
    ifd0.write_to(&mut tiff, 8);

    if include_exif_ifd {
        while tiff.len() < exif_ifd_offset {
            tiff.push(0);
        }
        exif_ifd.write_to(&mut tiff, exif_ifd_offset);
    }

    Ok(tiff)
}

fn replace_or_insert_exif_app1(bytes: &[u8], exif_segment: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err("Only JPEG files are supported for EXIF creation.".to_string());
    }

    if let Some((start, end)) = find_exif_app1_range(bytes) {
        let mut updated = Vec::with_capacity(bytes.len() - (end - start) + exif_segment.len());
        updated.extend_from_slice(&bytes[..start]);
        updated.extend_from_slice(exif_segment);
        updated.extend_from_slice(&bytes[end..]);
        return Ok(updated);
    }

    let insert_pos = jpeg_exif_insert_pos(bytes)?;
    let mut updated = Vec::with_capacity(bytes.len() + exif_segment.len());
    updated.extend_from_slice(&bytes[..insert_pos]);
    updated.extend_from_slice(exif_segment);
    updated.extend_from_slice(&bytes[insert_pos..]);
    Ok(updated)
}

fn find_exif_app1_range(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }

    let mut pos = 2usize;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xff {
            return None;
        }

        let marker = bytes[pos + 1];
        if marker == 0xda || marker == 0xd9 {
            return None;
        }

        let len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > bytes.len() {
            return None;
        }

        let segment_start = pos + 4;
        let segment_end = pos + 2 + len;
        if marker == 0xe1 && bytes.get(segment_start..segment_end)?.starts_with(b"Exif\0\0") {
            return Some((pos, segment_end));
        }

        pos = segment_end;
    }

    None
}

fn jpeg_exif_insert_pos(bytes: &[u8]) -> Result<usize, String> {
    let mut pos = 2usize;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xff {
            return Ok(2);
        }

        let marker = bytes[pos + 1];
        if marker == 0xda || marker == 0xd9 {
            return Ok(pos);
        }

        let len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > bytes.len() {
            return Err("Invalid JPEG segment length.".to_string());
        }

        if marker == 0xe0 {
            pos += 2 + len;
            continue;
        }

        return Ok(pos);
    }

    Ok(2)
}

fn optional_exif_datetime(value: &str) -> Result<Option<String>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    display_datetime_to_exif(value).map(Some)
}

fn parse_optional_rational_value(value: &str) -> Result<Option<(u32, u32)>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_exif_rational_value(value).map(Some)
}

fn parse_optional_aperture_value(value: &str) -> Result<Option<(u32, u32)>, String> {
    if is_empty_metadata_value(value) {
        return Ok(None);
    }
    parse_aperture_value(value).map(Some)
}

fn is_empty_metadata_value(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == "N/A" || trimmed == "-"
}

struct TiffIfdBuilder {
    entries: Vec<TiffEntryBuilder>,
}

impl TiffIfdBuilder {
    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn add_writable_ascii(&mut self, tag: u16, value: &str, min_len: usize) {
        let value = if is_empty_metadata_value(value) {
            String::new()
        } else {
            value.trim().to_string()
        };
        self.add_ascii_value_with_min_len(tag, value, min_len);
    }

    fn add_ascii_value(&mut self, tag: u16, value: String) {
        self.add_ascii_value_with_min_len(tag, value, 0);
    }

    fn add_ascii_value_with_min_len(&mut self, tag: u16, value: String, min_len: usize) {
        let mut bytes = value.into_bytes();
        bytes.push(0);
        if bytes.len() < min_len {
            bytes.resize(min_len, 0);
        }
        self.entries.push(TiffEntryBuilder {
            tag,
            field_type: 2,
            count: bytes.len() as u32,
            value: bytes,
        });
    }

    fn add_short(&mut self, tag: u16, value: u16) {
        self.entries.push(TiffEntryBuilder {
            tag,
            field_type: 3,
            count: 1,
            value: value.to_le_bytes().to_vec(),
        });
    }

    fn add_long(&mut self, tag: u16, value: u32) {
        self.entries.push(TiffEntryBuilder {
            tag,
            field_type: 4,
            count: 1,
            value: value.to_le_bytes().to_vec(),
        });
    }

    fn add_rational(&mut self, tag: u16, numerator: u32, denominator: u32) {
        let mut value = Vec::with_capacity(8);
        value.extend_from_slice(&numerator.to_le_bytes());
        value.extend_from_slice(&denominator.to_le_bytes());
        self.entries.push(TiffEntryBuilder {
            tag,
            field_type: 5,
            count: 1,
            value,
        });
    }

    fn set_long_value(&mut self, tag: u16, value: u32) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.tag == tag) {
            entry.value = value.to_le_bytes().to_vec();
        }
    }

    fn extra_len(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.value.len() > 4)
            .map(|entry| entry.value.len())
            .sum()
    }

    fn write_to(&mut self, tiff: &mut Vec<u8>, ifd_offset: usize) {
        self.entries.sort_by_key(|entry| entry.tag);
        let count = self.entries.len();
        let entries_start = ifd_offset + 2;
        let next_ifd_offset = entries_start + count * 12;
        let mut data_offset = next_ifd_offset + 4;

        while tiff.len() < ifd_offset {
            tiff.push(0);
        }

        tiff.extend_from_slice(&(count as u16).to_le_bytes());
        let mut extra_data = Vec::new();

        for entry in &self.entries {
            tiff.extend_from_slice(&entry.tag.to_le_bytes());
            tiff.extend_from_slice(&entry.field_type.to_le_bytes());
            tiff.extend_from_slice(&entry.count.to_le_bytes());

            if entry.value.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..entry.value.len()].copy_from_slice(&entry.value);
                tiff.extend_from_slice(&inline);
            } else {
                tiff.extend_from_slice(&(data_offset as u32).to_le_bytes());
                extra_data.extend_from_slice(&entry.value);
                data_offset += entry.value.len();
            }
        }

        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&extra_data);
    }
}

struct TiffEntryBuilder {
    tag: u16,
    field_type: u16,
    count: u32,
    value: Vec<u8>,
}

pub fn extract_datetime_from_filename(path: &Path) -> Option<String> {
    let filename = path.file_stem()?.to_str()?;
    let chars: Vec<char> = filename.chars().collect();
    let mut candidates = Vec::new();

    for start in 0..chars.len() {
        if start + 19 <= chars.len() {
            let separated: String = chars[start..start + 19].iter().collect();
            if let Some(parsed) = parse_separated_filename_datetime(&separated) {
                candidates.push(parsed);
            }
        }

        let mut digits = String::new();
        let mut candidate_10 = None;
        let mut candidate_12 = None;

        for ch in chars.iter().skip(start) {
            if ch.is_ascii_digit() {
                digits.push(*ch);
            } else if (*ch == '_' || *ch == '-') && digits.len() == 8 {
                continue;
            } else {
                break;
            }

            if digits.len() == 10 {
                candidate_10 = parse_compact_filename_datetime(&digits);
            } else if digits.len() == 12 {
                candidate_12 = parse_compact_filename_datetime(&digits);
                candidate_10 = None;
            } else if digits.len() == 14 {
                if let Some(parsed) = parse_compact_filename_datetime(&digits) {
                    candidates.push(parsed);
                }
                candidate_10 = None;
                candidate_12 = None;
            }

            if digits.len() >= 14 {
                break;
            }
        }

        if let Some(parsed) = candidate_12 {
            candidates.push(parsed);
        } else if let Some(parsed) = candidate_10 {
            candidates.push(parsed);
        }
    }

    let earliest = candidates.into_iter().min()?;
    Some(format_datetime_for_display(earliest))
}

fn parse_separated_filename_datetime(value: &str) -> Option<NaiveDateTime> {
    let bytes = value.as_bytes();
    let digit_positions = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    if bytes.len() != 19
        || !digit_positions
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
        || !matches!(bytes[4], b'-' | b'.' | b'_')
        || !matches!(bytes[7], b'-' | b'.' | b'_')
        || !matches!(bytes[10], b' ' | b'-' | b'_' | b'T')
        || !matches!(bytes[13], b':' | b';' | b'.' | b'-' | b'_')
        || !matches!(bytes[16], b':' | b';' | b'.' | b'-' | b'_')
    {
        return None;
    }

    let digits: String = digit_positions
        .iter()
        .map(|index| bytes[*index] as char)
        .collect();
    parse_compact_filename_datetime(&digits)
}

pub fn exif_datetime_to_display(value: &str) -> String {
    parse_known_datetime(value)
        .map(format_datetime_for_display)
        .unwrap_or_else(|| value.to_string())
}

pub fn display_datetime_to_exif(value: &str) -> Result<String, String> {
    parse_known_datetime(value)
        .map(|datetime| datetime.format(EXIF_DATETIME_FORMAT).to_string())
        .ok_or_else(|| "Expected date format: YYYY-MM-DD HH:MM:SS".to_string())
}

pub(crate) fn read_exif_tiff_metadata(tiff: &[u8]) -> ExifMetadata {
    parse_tiff(tiff).unwrap_or_default()
}

pub(crate) fn create_date_only_exif_tiff(display_value: &str) -> Result<Vec<u8>, String> {
    let mut tiff = Vec::with_capacity(14);
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&0u16.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    rewrite_exif_tiff_dates(&tiff, display_value)
}

pub(crate) fn rewrite_exif_tiff_dates(
    tiff: &[u8],
    display_value: &str,
) -> Result<Vec<u8>, String> {
    let exif_value = display_datetime_to_exif(display_value)?;
    let mut date_bytes = exif_value.into_bytes();
    date_bytes.push(0);

    let endian = read_tiff_endian(tiff).ok_or_else(|| "Invalid PNG eXIf TIFF header.".to_string())?;
    if read_u16(tiff, 2, endian) != Some(42) {
        return Err("Invalid PNG eXIf TIFF marker.".to_string());
    }
    let old_ifd0_offset = read_u32(tiff, 4, endian)
        .map(|value| value as usize)
        .ok_or_else(|| "Invalid PNG eXIf IFD0 offset.".to_string())?;
    let old_ifd0 = raw_ifd_entries(tiff, old_ifd0_offset, endian)?;
    let old_next_ifd = raw_ifd_next_offset(tiff, old_ifd0_offset, old_ifd0.len(), endian)?;
    let old_exif_offset = read_ifd_entries(tiff, old_ifd0_offset, endian)
        .and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.tag == 0x8769)
                .and_then(|entry| entry.as_long())
                .map(|value| value as usize)
        });
    let old_exif = match old_exif_offset {
        Some(offset) => raw_ifd_entries(tiff, offset, endian)?,
        None => Vec::new(),
    };
    let old_exif_next = match old_exif_offset {
        Some(offset) => raw_ifd_next_offset(tiff, offset, old_exif.len(), endian)?,
        None => 0,
    };

    let mut ifd0_entries: Vec<(u16, [u8; 12])> = old_ifd0
        .into_iter()
        .filter(|(tag, _)| *tag != 0x8769)
        .collect();
    let mut exif_entries: Vec<(u16, [u8; 12])> = old_exif
        .into_iter()
        .filter(|(tag, _)| *tag != 0x9003)
        .collect();

    let mut output = tiff.to_vec();
    if output.len() % 2 != 0 {
        output.push(0);
    }
    let new_ifd0_offset = output.len();
    let ifd0_count = ifd0_entries
        .len()
        .checked_add(1)
        .ok_or_else(|| "PNG eXIf contains too many IFD0 entries.".to_string())?;
    let ifd0_table_len = 2usize
        .checked_add(ifd0_count.checked_mul(12).ok_or_else(|| "PNG eXIf IFD0 is too large.".to_string())?)
        .and_then(|value| value.checked_add(4))
        .ok_or_else(|| "PNG eXIf IFD0 is too large.".to_string())?;
    let new_exif_offset = new_ifd0_offset
        .checked_add(ifd0_table_len)
        .map(|value| (value + 1) & !1)
        .ok_or_else(|| "PNG eXIf is too large.".to_string())?;

    ifd0_entries.push((
        0x8769,
        make_tiff_entry(0x8769, 4, 1, new_exif_offset as u32, endian),
    ));
    ifd0_entries.sort_by_key(|(tag, _)| *tag);
    append_raw_ifd(&mut output, new_ifd0_offset, &ifd0_entries, old_next_ifd, endian)?;
    while output.len() < new_exif_offset {
        output.push(0);
    }

    let exif_count = exif_entries
        .len()
        .checked_add(1)
        .ok_or_else(|| "PNG eXIf contains too many Exif IFD entries.".to_string())?;
    let exif_table_len = 2usize
        .checked_add(exif_count.checked_mul(12).ok_or_else(|| "PNG eXIf Exif IFD is too large.".to_string())?)
        .and_then(|value| value.checked_add(4))
        .ok_or_else(|| "PNG eXIf Exif IFD is too large.".to_string())?;
    let date_offset = new_exif_offset
        .checked_add(exif_table_len)
        .ok_or_else(|| "PNG eXIf is too large.".to_string())?;
    exif_entries.push((
        0x9003,
        make_tiff_entry(0x9003, 2, date_bytes.len() as u32, date_offset as u32, endian),
    ));
    exif_entries.sort_by_key(|(tag, _)| *tag);
    append_raw_ifd(&mut output, new_exif_offset, &exif_entries, old_exif_next, endian)?;
    output.extend_from_slice(&date_bytes);
    write_u32_at(&mut output, 4, new_ifd0_offset as u32, endian)?;
    Ok(output)
}

pub(crate) fn remove_exif_tiff_date_time_original(tiff: &[u8]) -> Result<Vec<u8>, String> {
    rewrite_exif_tiff_removing_tags(tiff, &[], &[0x9003], &[])
}

fn rewrite_exif_tiff_removing_tags(
    tiff: &[u8],
    ifd0_tags: &[u16],
    exif_tags: &[u16],
    gps_tags: &[u16],
) -> Result<Vec<u8>, String> {
    let endian = read_tiff_endian(tiff).ok_or_else(|| "Invalid EXIF TIFF header.".to_string())?;
    if read_u16(tiff, 2, endian) != Some(42) {
        return Err("Invalid EXIF TIFF marker.".to_string());
    }
    let old_ifd0_offset = read_u32(tiff, 4, endian)
        .map(|value| value as usize)
        .ok_or_else(|| "Invalid EXIF IFD0 offset.".to_string())?;
    let old_ifd0 = raw_ifd_entries(tiff, old_ifd0_offset, endian)?;
    let old_ifd0_next = raw_ifd_next_offset(tiff, old_ifd0_offset, old_ifd0.len(), endian)?;
    let parsed_ifd0 = read_ifd_entries(tiff, old_ifd0_offset, endian)
        .ok_or_else(|| "Unable to read EXIF IFD0 entries.".to_string())?;
    let old_exif_offset = parsed_ifd0
        .iter()
        .find(|entry| entry.tag == 0x8769)
        .and_then(Entry::as_long)
        .map(|value| value as usize);
    let old_gps_offset = parsed_ifd0
        .iter()
        .find(|entry| entry.tag == 0x8825)
        .and_then(Entry::as_long)
        .filter(|value| *value > 0)
        .map(|value| value as usize);

    let mut output = tiff.to_vec();
    if output.len() % 2 != 0 {
        output.push(0);
    }

    let new_exif_offset = if let Some(offset) = old_exif_offset {
        let entries: Vec<_> = raw_ifd_entries(tiff, offset, endian)?
            .into_iter()
            .filter(|(tag, _)| !exif_tags.contains(tag))
            .collect();
        let next = raw_ifd_next_offset(tiff, offset, raw_ifd_entries(tiff, offset, endian)?.len(), endian)?;
        let new_offset = output.len();
        append_raw_ifd(&mut output, new_offset, &entries, next, endian)?;
        Some(new_offset)
    } else {
        None
    };

    if output.len() % 2 != 0 {
        output.push(0);
    }
    let new_gps_offset = if let Some(offset) = old_gps_offset {
        let old_entries = raw_ifd_entries(tiff, offset, endian)?;
        let next = raw_ifd_next_offset(tiff, offset, old_entries.len(), endian)?;
        let entries: Vec<_> = old_entries
            .into_iter()
            .filter(|(tag, _)| !gps_tags.contains(tag))
            .collect();
        let new_offset = output.len();
        append_raw_ifd(&mut output, new_offset, &entries, next, endian)?;
        Some(new_offset)
    } else {
        None
    };

    if output.len() % 2 != 0 {
        output.push(0);
    }
    let mut new_ifd0: Vec<_> = old_ifd0
        .into_iter()
        .filter(|(tag, _)| {
            !ifd0_tags.contains(tag)
                && *tag != 0x8769
                && *tag != 0x8825
        })
        .collect();
    if let Some(offset) = new_exif_offset {
        let offset = u32::try_from(offset).map_err(|_| "EXIF data is too large.".to_string())?;
        new_ifd0.push((0x8769, make_tiff_entry(0x8769, 4, 1, offset, endian)));
    }
    if let Some(offset) = new_gps_offset {
        let offset = u32::try_from(offset).map_err(|_| "EXIF data is too large.".to_string())?;
        new_ifd0.push((0x8825, make_tiff_entry(0x8825, 4, 1, offset, endian)));
    }
    new_ifd0.sort_by_key(|(tag, _)| *tag);
    let new_ifd0_offset = output.len();
    append_raw_ifd(&mut output, new_ifd0_offset, &new_ifd0, old_ifd0_next, endian)?;
    write_u32_at(
        &mut output,
        4,
        u32::try_from(new_ifd0_offset).map_err(|_| "EXIF data is too large.".to_string())?,
        endian,
    )?;
    Ok(output)
}

fn build_exif_app1_from_tiff(tiff: &[u8]) -> Result<Vec<u8>, String> {
    let segment_len = tiff
        .len()
        .checked_add(8)
        .ok_or_else(|| "EXIF segment is too large.".to_string())?;
    if segment_len > u16::MAX as usize {
        return Err("EXIF segment is too large for JPEG APP1.".to_string());
    }
    let mut segment = Vec::with_capacity(segment_len + 2);
    segment.extend_from_slice(&[0xff, 0xe1]);
    segment.extend_from_slice(&(segment_len as u16).to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(tiff);
    Ok(segment)
}

fn raw_ifd_entries(
    tiff: &[u8],
    offset: usize,
    endian: Endian,
) -> Result<Vec<(u16, [u8; 12])>, String> {
    let count = read_u16(tiff, offset, endian)
        .map(|value| value as usize)
        .ok_or_else(|| "Invalid PNG eXIf IFD entry count.".to_string())?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let start = offset
            .checked_add(2)
            .and_then(|value| value.checked_add(index.checked_mul(12)?))
            .ok_or_else(|| "Invalid PNG eXIf IFD offset.".to_string())?;
        let raw: [u8; 12] = tiff
            .get(start..start + 12)
            .ok_or_else(|| "PNG eXIf IFD entry is truncated.".to_string())?
            .try_into()
            .map_err(|_| "PNG eXIf IFD entry is invalid.".to_string())?;
        let tag = read_u16(&raw, 0, endian).ok_or_else(|| "Invalid PNG eXIf tag.".to_string())?;
        entries.push((tag, raw));
    }
    Ok(entries)
}

fn raw_ifd_next_offset(
    tiff: &[u8],
    offset: usize,
    count: usize,
    endian: Endian,
) -> Result<u32, String> {
    let position = offset
        .checked_add(2)
        .and_then(|value| value.checked_add(count.checked_mul(12)?))
        .ok_or_else(|| "Invalid PNG eXIf next IFD offset.".to_string())?;
    read_u32(tiff, position, endian).ok_or_else(|| "PNG eXIf next IFD offset is truncated.".to_string())
}

fn make_tiff_entry(
    tag: u16,
    field_type: u16,
    count: u32,
    value: u32,
    endian: Endian,
) -> [u8; 12] {
    let mut entry = [0u8; 12];
    write_u16_bytes(&mut entry[0..2], tag, endian);
    write_u16_bytes(&mut entry[2..4], field_type, endian);
    write_u32_bytes(&mut entry[4..8], count, endian);
    write_u32_bytes(&mut entry[8..12], value, endian);
    entry
}

fn append_raw_ifd(
    output: &mut Vec<u8>,
    offset: usize,
    entries: &[(u16, [u8; 12])],
    next_ifd: u32,
    endian: Endian,
) -> Result<(), String> {
    if entries.len() > u16::MAX as usize {
        return Err("PNG eXIf IFD has too many entries.".to_string());
    }
    while output.len() < offset {
        output.push(0);
    }
    if output.len() != offset {
        return Err("PNG eXIf IFD append offset is invalid.".to_string());
    }
    let mut count = [0u8; 2];
    write_u16_bytes(&mut count, entries.len() as u16, endian);
    output.extend_from_slice(&count);
    for (_, entry) in entries {
        output.extend_from_slice(entry);
    }
    let mut next = [0u8; 4];
    write_u32_bytes(&mut next, next_ifd, endian);
    output.extend_from_slice(&next);
    Ok(())
}

fn write_u32_at(
    output: &mut [u8],
    offset: usize,
    value: u32,
    endian: Endian,
) -> Result<(), String> {
    let target = output
        .get_mut(offset..offset + 4)
        .ok_or_else(|| "PNG eXIf TIFF header is truncated.".to_string())?;
    write_u32_bytes(target, value, endian);
    Ok(())
}

fn write_u16_bytes(target: &mut [u8], value: u16, endian: Endian) {
    let bytes = match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    };
    target.copy_from_slice(&bytes);
}

fn write_u32_bytes(target: &mut [u8], value: u32, endian: Endian) {
    let bytes = match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    };
    target.copy_from_slice(&bytes);
}

fn parse_compact_filename_datetime(value: &str) -> Option<NaiveDateTime> {
    let padded = match value.len() {
        14 => value.to_string(),
        12 => format!("{value}00"),
        10 => {
            let yy = value[0..2].parse::<i32>().ok()?;
            let year = if yy <= 79 { 2000 + yy } else { 1900 + yy };
            format!("{year:04}{}00", &value[2..])
        }
        _ => return None,
    };

    let year = padded[0..4].parse().ok()?;
    let month = padded[4..6].parse().ok()?;
    let day = padded[6..8].parse().ok()?;
    let hour = padded[8..10].parse().ok()?;
    let minute = padded[10..12].parse().ok()?;
    let second = padded[12..14].parse().ok()?;

    NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, second)
}

fn parse_tiff(data: &[u8]) -> Option<ExifMetadata> {
    if data.len() < 8 {
        return None;
    }

    let endian = read_tiff_endian(data)?;

    if read_u16(data, 2, endian)? != 42 {
        return None;
    }

    let ifd0_offset = read_u32(data, 4, endian)? as usize;
    let mut meta = ExifMetadata::default();
    meta.has_exif = true;
    let mut exif_ifd = None;
    let mut gps_ifd = None;

    for entry in read_ifd_entries(data, ifd0_offset, endian)? {
        match entry.tag {
            0x010e => meta.image_description = entry.as_ascii().unwrap_or_else(empty_value),
            0x010f => meta.camera_make = entry.as_ascii().unwrap_or_else(empty_value),
            0x0110 => meta.camera_model = entry.as_ascii().unwrap_or_else(empty_value),
            0x0112 => meta.orientation = entry.as_short().map(format_orientation).unwrap_or_else(empty_value),
            0x0131 => meta.software = entry.as_ascii().unwrap_or_else(empty_value),
            0x0132 => {
                meta.image_date_time = entry
                    .as_ascii()
                    .map(|value| exif_datetime_to_display(&value))
                    .unwrap_or_else(empty_value);
            }
            0x013b => meta.artist = entry.as_ascii().unwrap_or_else(empty_value),
            0x8298 => meta.copyright = entry.as_ascii().unwrap_or_else(empty_value),
            0x8769 => exif_ifd = entry.as_long().map(|v| v as usize),
            0x8825 => gps_ifd = entry.as_long().filter(|v| *v > 0).map(|v| v as usize),
            _ => {}
        }
    }

    if let Some(offset) = exif_ifd {
        parse_exif_ifd(data, offset, endian, &mut meta);
    }

    meta.taken_date = [
        &meta.date_time_original,
        &meta.date_time_digitized,
        &meta.image_date_time,
    ]
    .into_iter()
    .find(|value| !value.is_empty())
    .cloned()
    .unwrap_or_else(empty_value);

    if let Some(offset) = gps_ifd {
        parse_gps_ifd(data, offset, endian, &mut meta);
    }

    Some(meta)
}

fn read_tiff_endian(data: &[u8]) -> Option<Endian> {
    if data.len() < 2 {
        return None;
    }

    match &data[0..2] {
        b"II" => Some(Endian::Little),
        b"MM" => Some(Endian::Big),
        _ => None,
    }
}

fn parse_exif_ifd(data: &[u8], offset: usize, endian: Endian, meta: &mut ExifMetadata) {
    let Some(entries) = read_ifd_entries(data, offset, endian) else {
        return;
    };

    for entry in entries {
        match entry.tag {
            0x8822 => {
                meta.exposure_program = entry
                    .as_short()
                    .map(format_exposure_program)
                    .unwrap_or_else(empty_value)
            }
            0x829a => meta.shutter_speed = entry.as_rational().map(format_shutter).unwrap_or_else(empty_value),
            0x829d => meta.aperture = entry.as_rational().map(format_aperture).unwrap_or_else(empty_value),
            0x8827 => meta.iso_speed = entry.as_short().map(|v| v.to_string()).unwrap_or_else(empty_value),
            0x9003 => {
                meta.date_time_original = entry
                    .as_ascii()
                    .map(|value| exif_datetime_to_display(&value))
                    .unwrap_or_else(empty_value)
            }
            0x9004 => {
                meta.date_time_digitized = entry
                    .as_ascii()
                    .map(|value| exif_datetime_to_display(&value))
                    .unwrap_or_else(empty_value);
            }
            0x9000 => meta.exif_version = entry.as_version_string().unwrap_or_else(empty_value),
            0x9207 => meta.metering_mode = entry.as_short().map(format_metering).unwrap_or_else(empty_value),
            0x9209 => meta.flash_fired = entry.as_short().map(format_flash).unwrap_or_else(empty_value),
            0x920a => meta.focal_length = entry.as_rational().map(format_focal_length).unwrap_or_else(empty_value),
            0xa001 => meta.color_space = entry.as_short().map(format_color_space).unwrap_or_else(empty_value),
            0xa002 => meta.image_width = entry.as_number().map(|v| v.to_string()).unwrap_or_else(empty_value),
            0xa003 => meta.image_height = entry.as_number().map(|v| v.to_string()).unwrap_or_else(empty_value),
            0xa403 => {
                meta.white_balance = entry
                    .as_short()
                    .map(format_white_balance)
                    .unwrap_or_else(empty_value)
            }
            0xa405 => {
                meta.focal_length_35mm = entry
                    .as_short()
                    .map(|value| format!("{value} mm"))
                    .unwrap_or_else(empty_value)
            }
            0xa434 => meta.lens_model = entry.as_ascii().unwrap_or_else(empty_value),
            _ => {}
        }
    }
}

fn parse_gps_ifd(data: &[u8], offset: usize, endian: Endian, meta: &mut ExifMetadata) {
    let Some(entries) = read_ifd_entries(data, offset, endian) else {
        return;
    };

    let mut lat_ref = None;
    let mut lon_ref = None;
    let mut lat = None;
    let mut lon = None;
    let mut alt_ref = 0u8;
    let mut alt = None;

    for entry in entries {
        match entry.tag {
            0x0001 => lat_ref = entry.as_ascii(),
            0x0002 => lat = entry.as_rational_array3(),
            0x0003 => lon_ref = entry.as_ascii(),
            0x0004 => lon = entry.as_rational_array3(),
            0x0005 => alt_ref = entry.as_byte().unwrap_or(0),
            0x0006 => alt = entry.as_rational(),
            _ => {}
        }
    }

    if let (Some(reference), Some(value)) = (lat_ref, lat) {
        meta.gps_latitude = format_gps_coord(&reference, value);
    }
    if let (Some(reference), Some(value)) = (lon_ref, lon) {
        meta.gps_longitude = format_gps_coord(&reference, value);
    }
    if let Some(value) = alt {
        let meters = rational_to_f64(value);
        meta.gps_altitude = if alt_ref == 1 {
            format!("-{meters:.2} m")
        } else {
            format!("{meters:.2} m")
        };
    }
}

fn find_exif_tiff(bytes: &[u8]) -> Option<&[u8]> {
    bytes.get(find_exif_tiff_start(bytes)?..)
}

fn find_exif_tiff_start(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }

    let mut pos = 2usize;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xff {
            return None;
        }

        let marker = bytes[pos + 1];
        pos += 2;

        if marker == 0xda || marker == 0xd9 {
            return None;
        }

        let len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        if len < 2 || pos + len > bytes.len() {
            return None;
        }

        let segment = &bytes[pos + 2..pos + len];
        if marker == 0xe1 && segment.starts_with(b"Exif\0\0") {
            return Some(pos + 2 + 6);
        }

        pos += len;
    }

    None
}

fn read_ifd_entries(data: &[u8], offset: usize, endian: Endian) -> Option<Vec<Entry<'_>>> {
    let count = read_u16(data, offset, endian)? as usize;
    let entries_start = offset.checked_add(2)?;
    let entries_len = count.checked_mul(12)?;
    if entries_start + entries_len > data.len() {
        return None;
    }

    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let entry_offset = entries_start + i * 12;
        entries.push(Entry {
            data,
            endian,
            tag: read_u16(data, entry_offset, endian)?,
            field_type: read_u16(data, entry_offset + 2, endian)?,
            count: read_u32(data, entry_offset + 4, endian)?,
            value_field: entry_offset + 8,
        });
    }

    Some(entries)
}

impl<'a> Entry<'a> {
    fn value_bytes(&self) -> Option<&'a [u8]> {
        let size = type_size(self.field_type)?;
        let len = size.checked_mul(self.count as usize)?;
        if len <= 4 {
            return self.data.get(self.value_field..self.value_field + len);
        }

        let offset = read_u32(self.data, self.value_field, self.endian)? as usize;
        self.data.get(offset..offset + len)
    }

    fn as_ascii(&self) -> Option<String> {
        if self.field_type != 2 {
            return None;
        }

        let bytes = self.value_bytes()?;
        let trimmed = bytes.split(|b| *b == 0).next().unwrap_or(bytes);
        let value = String::from_utf8_lossy(trimmed).trim().to_string();
        (!value.is_empty()).then_some(value)
    }

    fn as_byte(&self) -> Option<u8> {
        if self.field_type != 1 {
            return None;
        }
        self.value_bytes()?.first().copied()
    }

    fn as_version_string(&self) -> Option<String> {
        if self.field_type != 7 {
            return None;
        }
        let bytes = self.value_bytes()?;
        let value: String = bytes
            .iter()
            .copied()
            .filter(u8::is_ascii_digit)
            .map(char::from)
            .collect();
        match value.len() {
            4 => Some(format!("{}.{}", &value[..2], &value[2..])),
            _ if !value.is_empty() => Some(value),
            _ => None,
        }
    }

    fn as_short(&self) -> Option<u16> {
        if self.field_type != 3 {
            return None;
        }
        read_u16(self.value_bytes()?, 0, self.endian)
    }

    fn as_long(&self) -> Option<u32> {
        match self.field_type {
            3 => self.as_short().map(u32::from),
            4 => read_u32(self.value_bytes()?, 0, self.endian),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<u32> {
        self.as_long()
    }

    fn as_rational(&self) -> Option<(u32, u32)> {
        if self.field_type != 5 {
            return None;
        }
        let bytes = self.value_bytes()?;
        Some((
            read_u32(bytes, 0, self.endian)?,
            read_u32(bytes, 4, self.endian)?,
        ))
    }

    fn as_rational_array3(&self) -> Option<[(u32, u32); 3]> {
        if self.field_type != 5 || self.count < 3 {
            return None;
        }
        let bytes = self.value_bytes()?;
        Some([
            (read_u32(bytes, 0, self.endian)?, read_u32(bytes, 4, self.endian)?),
            (read_u32(bytes, 8, self.endian)?, read_u32(bytes, 12, self.endian)?),
            (read_u32(bytes, 16, self.endian)?, read_u32(bytes, 20, self.endian)?),
        ])
    }
}

fn writable_ascii_range(entry: &Entry<'_>, value_len: usize) -> Option<(usize, usize)> {
    if entry.field_type != 2 {
        return None;
    }

    let len = type_size(entry.field_type)?.checked_mul(entry.count as usize)?;
    if len < value_len {
        return None;
    }

    if len <= 4 {
        Some((entry.value_field, len))
    } else {
        let offset = read_u32(entry.data, entry.value_field, entry.endian)? as usize;
        entry.data.get(offset..offset + len)?;
        Some((offset, len))
    }
}

fn entry_storage_range(entry: &Entry<'_>) -> Option<(usize, usize)> {
    let size = type_size(entry.field_type)?;
    let len = size.checked_mul(entry.count as usize)?;
    if len <= 4 {
        entry
            .data
            .get(entry.value_field..entry.value_field + 4)
            .map(|_| (entry.value_field, 4))
    } else {
        let offset = read_u32(entry.data, entry.value_field, entry.endian)? as usize;
        entry.data.get(offset..offset + len).map(|_| (offset, len))
    }
}

fn read_u16(data: &[u8], offset: usize, endian: Endian) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(match endian {
        Endian::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
        Endian::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
    })
}

fn read_u32(data: &[u8], offset: usize, endian: Endian) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(match endian {
        Endian::Little => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        Endian::Big => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    })
}

fn type_size(field_type: u16) -> Option<usize> {
    match field_type {
        1 | 2 | 7 => Some(1),
        3 => Some(2),
        4 | 9 => Some(4),
        5 | 10 => Some(8),
        _ => None,
    }
}

fn rational_to_f64((numerator, denominator): (u32, u32)) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn format_shutter(value: (u32, u32)) -> String {
    let seconds = rational_to_f64(value);
    if seconds > 0.0 && seconds < 1.0 {
        format!("1/{:.0} s", 1.0 / seconds)
    } else {
        format!("{seconds:.2} s")
    }
}

fn format_aperture(value: (u32, u32)) -> String {
    format!("f/{:.1}", rational_to_f64(value))
}

fn format_focal_length(value: (u32, u32)) -> String {
    format!("{:.1} mm", rational_to_f64(value))
}

fn format_flash(value: u16) -> String {
    if value & 1 == 1 {
        "Yes".to_string()
    } else {
        "No".to_string()
    }
}

fn format_metering(value: u16) -> String {
    match value {
        1 => "Average",
        2 => "Center-weighted average",
        3 => "Spot",
        4 => "Multi-spot",
        5 => "Pattern",
        6 => "Partial",
        255 => "Other",
        _ => "Unknown",
    }
    .to_string()
}

fn format_exposure_program(value: u16) -> String {
    match value {
        0 => "Not defined".to_string(),
        1 => "Manual".to_string(),
        2 => "Normal program".to_string(),
        3 => "Aperture priority".to_string(),
        4 => "Shutter priority".to_string(),
        5 => "Creative program".to_string(),
        6 => "Action program".to_string(),
        7 => "Portrait mode".to_string(),
        8 => "Landscape mode".to_string(),
        _ => value.to_string(),
    }
}

fn format_white_balance(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
        _ => value.to_string(),
    }
}

fn format_orientation(value: u16) -> String {
    match value {
        1 => "Normal",
        2 => "Mirrored horizontal",
        3 => "Rotated 180",
        4 => "Mirrored vertical",
        5 => "Mirrored horizontal rotated 270",
        6 => "Rotated 90",
        7 => "Mirrored horizontal rotated 90",
        8 => "Rotated 270",
        _ => "Unknown",
    }
    .to_string()
}

fn format_color_space(value: u16) -> String {
    match value {
        1 => "sRGB",
        0xffff => "Uncalibrated",
        _ => "Unknown",
    }
    .to_string()
}

fn format_gps_coord(reference: &str, parts: [(u32, u32); 3]) -> String {
    let degrees = rational_to_f64(parts[0]);
    let minutes = rational_to_f64(parts[1]);
    let seconds = rational_to_f64(parts[2]);
    let mut decimal = degrees + minutes / 60.0 + seconds / 3600.0;
    if reference == "S" || reference == "W" {
        decimal = -decimal;
    }
    format!("{decimal:.6}")
}

fn parse_known_datetime(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, EXIF_DATETIME_FORMAT)
        .or_else(|_| NaiveDateTime::parse_from_str(value, DISPLAY_DATETIME_FORMAT))
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
}

fn format_datetime_for_display(datetime: NaiveDateTime) -> String {
    datetime.format(DISPLAY_DATETIME_FORMAT).to_string()
}

fn empty_value() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_jpeg_with_camera_make(value: &str) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());

        let ifd0_offset = 8usize;
        let make_offset = 26u32;

        assert_eq!(tiff.len(), ifd0_offset);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x010fu16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&((value.len() + 1) as u32).to_le_bytes());
        tiff.extend_from_slice(&make_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), make_offset as usize);
        tiff.extend_from_slice(value.as_bytes());
        tiff.push(0);

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let segment_len = u16::try_from(payload.len() + 2).unwrap();

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    fn minimal_jpeg_with_camera_model(value: &str) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());

        let ifd0_offset = 8usize;
        let model_offset = 26u32;

        assert_eq!(tiff.len(), ifd0_offset);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0110u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&((value.len() + 1) as u32).to_le_bytes());
        tiff.extend_from_slice(&model_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), model_offset as usize);
        tiff.extend_from_slice(value.as_bytes());
        tiff.push(0);

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let segment_len = u16::try_from(payload.len() + 2).unwrap();

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    fn minimal_jpeg_with_lens_model(value: &str) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());

        let ifd0_offset = 8usize;
        let exif_ifd_offset = 26u32;
        let lens_offset = 44u32;

        assert_eq!(tiff.len(), ifd0_offset);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x8769u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&exif_ifd_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), exif_ifd_offset as usize);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0xa434u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&((value.len() + 1) as u32).to_le_bytes());
        tiff.extend_from_slice(&lens_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), lens_offset as usize);
        tiff.extend_from_slice(value.as_bytes());
        tiff.push(0);

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let segment_len = u16::try_from(payload.len() + 2).unwrap();

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    fn minimal_jpeg_with_gps() -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());

        let gps_ifd_offset = 26u32;
        let lat_offset = 104u32;
        let lon_offset = 128u32;
        let alt_offset = 152u32;

        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x8825u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&gps_ifd_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), gps_ifd_offset as usize);
        tiff.extend_from_slice(&6u16.to_le_bytes());
        tiff.extend_from_slice(&0x0001u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&2u32.to_le_bytes());
        tiff.extend_from_slice(&[b'N', 0, 0, 0]);
        tiff.extend_from_slice(&0x0002u16.to_le_bytes());
        tiff.extend_from_slice(&5u16.to_le_bytes());
        tiff.extend_from_slice(&3u32.to_le_bytes());
        tiff.extend_from_slice(&lat_offset.to_le_bytes());
        tiff.extend_from_slice(&0x0003u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&2u32.to_le_bytes());
        tiff.extend_from_slice(&[b'E', 0, 0, 0]);
        tiff.extend_from_slice(&0x0004u16.to_le_bytes());
        tiff.extend_from_slice(&5u16.to_le_bytes());
        tiff.extend_from_slice(&3u32.to_le_bytes());
        tiff.extend_from_slice(&lon_offset.to_le_bytes());
        tiff.extend_from_slice(&0x0005u16.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0]);
        tiff.extend_from_slice(&0x0006u16.to_le_bytes());
        tiff.extend_from_slice(&5u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&alt_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), lat_offset as usize);
        for (num, den) in [(37u32, 1u32), (34, 1), (12, 1)] {
            tiff.extend_from_slice(&num.to_le_bytes());
            tiff.extend_from_slice(&den.to_le_bytes());
        }
        assert_eq!(tiff.len(), lon_offset as usize);
        for (num, den) in [(127u32, 1u32), (1, 1), (30, 1)] {
            tiff.extend_from_slice(&num.to_le_bytes());
            tiff.extend_from_slice(&den.to_le_bytes());
        }
        assert_eq!(tiff.len(), alt_offset as usize);
        tiff.extend_from_slice(&42u32.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let segment_len = u16::try_from(payload.len() + 2).unwrap();

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    #[test]
    fn converts_exif_datetime_to_display_datetime() {
        assert_eq!(
            exif_datetime_to_display("2012:08:27 00:29:55"),
            "2012-08-27 00:29:55"
        );
    }

    #[test]
    fn converts_display_datetime_to_exif_datetime() {
        assert_eq!(
            display_datetime_to_exif("2012-08-27 00:29:55").unwrap(),
            "2012:08:27 00:29:55"
        );
    }

    #[test]
    fn png_date_rewrite_preserves_existing_exif_metadata() {
        let original = build_basic_tiff(&ExifMetadata {
            has_exif: true,
            taken_date: "2015-12-26 03:53:22".to_string(),
            camera_make: "Preserved Camera".to_string(),
            camera_model: "Preserved Model".to_string(),
            ..ExifMetadata::default()
        })
        .unwrap();

        let rewritten = rewrite_exif_tiff_dates(&original, "2016-01-02 11:22:33").unwrap();
        let metadata = read_exif_tiff_metadata(&rewritten);

        assert_eq!(metadata.camera_make, "Preserved Camera");
        assert_eq!(metadata.camera_model, "Preserved Model");
        assert_eq!(metadata.date_time_original, "2016-01-02 11:22:33");
        assert_eq!(metadata.date_time_digitized, "2015-12-26 03:53:22");
        assert_eq!(metadata.image_date_time, "2015-12-26 03:53:22");
    }

    #[test]
    fn retains_individual_exif_date_sources_and_prefers_original() {
        let source = "2012:08:27 00:29:55";
        let mut tiff = build_basic_tiff(&ExifMetadata {
            has_exif: true,
            taken_date: "2012-08-27 00:29:55".to_string(),
            ..ExifMetadata::default()
        })
        .unwrap();
        let offsets: Vec<usize> = tiff
            .windows(source.len())
            .enumerate()
            .filter_map(|(index, value)| (value == source.as_bytes()).then_some(index))
            .collect();
        assert_eq!(offsets.len(), 3);

        for (offset, value) in offsets.into_iter().zip([
            "2014:01:30 09:10:00",
            "2013:12:20 15:30:00",
            "2013:12:21 16:40:00",
        ]) {
            tiff[offset..offset + value.len()].copy_from_slice(value.as_bytes());
        }

        let metadata = parse_tiff(&tiff).unwrap();
        assert_eq!(metadata.image_date_time, "2014-01-30 09:10:00");
        assert_eq!(metadata.date_time_original, "2013-12-20 15:30:00");
        assert_eq!(metadata.date_time_digitized, "2013-12-21 16:40:00");
        assert_eq!(metadata.taken_date, "2013-12-20 15:30:00");
    }

    #[test]
    fn accepts_exif_datetime_when_converting_for_write() {
        assert_eq!(
            display_datetime_to_exif("2012:08:27 00:29:55").unwrap(),
            "2012:08:27 00:29:55"
        );
    }

    #[test]
    fn expands_display_date_to_midnight_when_converting_for_write() {
        assert_eq!(
            display_datetime_to_exif("2026-01-01").unwrap(),
            "2026:01:01 00:00:00"
        );
    }

    #[test]
    fn extracts_datetime_from_filename_with_sequence_suffix() {
        assert_eq!(
            extract_datetime_from_filename(Path::new("20130401_195241_001.jpg")).as_deref(),
            Some("2013-04-01 19:52:41")
        );
        assert_eq!(
            extract_datetime_from_filename(Path::new("20130401_195241_65.jpg")).as_deref(),
            Some("2013-04-01 19:52:41")
        );
    }

    #[test]
    fn extracts_datetime_from_separated_filename_patterns() {
        assert_eq!(
            extract_datetime_from_filename(Path::new("IMG-2014-02-20-11-53-59.png")).as_deref(),
            Some("2014-02-20 11:53:59")
        );
        assert_eq!(
            extract_datetime_from_filename(Path::new("2014-02-20 11;53;59.PNG")).as_deref(),
            Some("2014-02-20 11:53:59")
        );
        assert_eq!(
            extract_datetime_from_filename(Path::new("2014-02-04 14.40.01.png")).as_deref(),
            Some("2014-02-04 14:40:01")
        );
    }

    #[test]
    fn writes_existing_taken_date_tag_in_place() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_taken_date_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_datetime_original("2012:08:27 00:29:55"))
            .unwrap();

        write_taken_date(&path, "2026-05-09 12:34:56").unwrap();

        let metadata = read_exif_metadata(&path);
        assert_eq!(metadata.taken_date, "2026-05-09 12:34:56");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_existing_camera_make_tag_in_place() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_camera_make_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_camera_make("Canon"))
            .unwrap();

        write_camera_make(&path, "Sony").unwrap();

        let metadata = read_exif_metadata(&path);
        assert_eq!(metadata.camera_make, "Sony");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_existing_camera_model_tag_in_place() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_camera_model_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_camera_model("Canon"))
            .unwrap();

        write_camera_model(&path, "Sony").unwrap();

        let metadata = read_exif_metadata(&path);
        assert_eq!(metadata.camera_model, "Sony");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn generated_exif_file_allows_later_camera_model_write() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_generated_camera_model_test_{}.jpg",
            std::process::id()
        ));
        let backup_path = exif_backup_path(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xd9]).unwrap();

        let output_path = rewrite_basic_exif_metadata(&path, &ExifMetadata::default(), true).unwrap();
        write_camera_model(&output_path, "HTC-X315E").unwrap();

        let metadata = read_exif_metadata(&output_path);
        assert_eq!(metadata.camera_model, "HTC-X315E");

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(backup_path);
    }

    #[test]
    fn writes_existing_lens_model_tag_in_place() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_lens_model_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_lens_model("EF24-70mm"))
            .unwrap();

        write_lens_model(&path, "RF24-70mm").unwrap();

        let metadata = read_exif_metadata(&path);
        assert_eq!(metadata.lens_model, "RF24-70mm");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removes_gps_information() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_remove_gps_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_gps()).unwrap();

        let before = read_exif_metadata(&path);
        assert_eq!(before.gps_latitude, "37.570000");
        assert_eq!(before.gps_longitude, "127.025000");
        assert_eq!(before.gps_altitude, "42.00 m");

        remove_gps_information(&path).unwrap();

        let after = read_exif_metadata(&path);
        assert_eq!(after.gps_latitude, "");
        assert_eq!(after.gps_longitude, "");
        assert_eq!(after.gps_altitude, "");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn creates_new_exif_file_when_jpeg_has_no_exif() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_no_exif_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xd9]).unwrap();

        let metadata = ExifMetadata {
            has_exif: true,
            taken_date: "2026-05-09 12:34:56".to_string(),
            date_time_original: String::new(),
            date_time_digitized: String::new(),
            image_date_time: String::new(),
            camera_make: "Sony".to_string(),
            camera_model: "A7C".to_string(),
            lens_model: "FE 35mm F1.8".to_string(),
            software: "SH148 EXIF-File Tool".to_string(),
            artist: "tester".to_string(),
            image_description: String::new(),
            copyright: String::new(),
            exif_version: String::new(),
            exposure_program: String::new(),
            white_balance: String::new(),
            focal_length_35mm: String::new(),
            shutter_speed: "1/125".to_string(),
            aperture: "f/2.8".to_string(),
            iso_speed: "400".to_string(),
            focal_length: "35 mm".to_string(),
            flash_fired: "No".to_string(),
            metering_mode: "Pattern".to_string(),
            image_width: String::new(),
            image_height: String::new(),
            orientation: "Normal".to_string(),
            color_space: "sRGB".to_string(),
            gps_latitude: String::new(),
            gps_longitude: String::new(),
            gps_altitude: String::new(),
        };

        let output_path = rewrite_basic_exif_metadata(&path, &metadata, true).unwrap();
        let backup_path = exif_backup_path(&output_path);

        assert!(path.exists());
        assert!(output_path.exists());
        assert_eq!(path, output_path);
        assert!(backup_path.exists());
        assert!(!read_exif_metadata(&backup_path).has_exif);

        let written = read_exif_metadata(&output_path);
        assert!(written.has_exif);
        assert_eq!(written.taken_date, "2026-05-09 12:34:56");
        assert_eq!(written.camera_make, "Sony");
        assert_eq!(written.camera_model, "A7C");
        assert_eq!(written.lens_model, "FE 35mm F1.8");
        assert_eq!(written.iso_speed, "400");

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(backup_path);
    }

    #[test]
    fn creates_exif_without_backup_when_disabled() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_no_backup_test_{}.jpg",
            std::process::id()
        ));
        let backup_path = exif_backup_path(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xd9]).unwrap();

        let metadata = ExifMetadata {
            camera_make: "Sony".to_string(),
            ..ExifMetadata::default()
        };
        rewrite_basic_exif_metadata(&path, &metadata, false).unwrap();

        assert!(path.exists());
        assert!(!backup_path.exists());
        assert_eq!(read_exif_metadata(&path).camera_make, "Sony");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_to_add_missing_tag_to_existing_exif() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_missing_tag_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_camera_make("Canon")).unwrap();

        let err = write_lens_model(&path, "RF24-70mm").unwrap_err();

        assert!(err.contains("does not create missing tags"));
        assert_eq!(read_exif_metadata(&path).camera_make, "Canon");
        assert_eq!(read_exif_metadata(&path).lens_model, "");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn individual_tag_removal_preserves_other_raw_exif_entries() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_remove_one_tag_test_{}.jpg",
            std::process::id()
        ));
        let mut jpeg = minimal_jpeg_with_camera_make("Preserve me");
        let tag_position = jpeg
            .windows(2)
            .position(|bytes| bytes == [0x0f, 0x01])
            .expect("camera make tag");
        jpeg[tag_position..tag_position + 2].copy_from_slice(&0x010eu16.to_le_bytes());
        std::fs::write(&path, jpeg).unwrap();

        remove_exif_tag(&path, "camera_model", false).unwrap();
        assert_eq!(
            read_exif_metadata(&path).image_description,
            "Preserve me"
        );

        remove_exif_tag(&path, "image_description", false).unwrap();
        assert_eq!(read_exif_metadata(&path).image_description, "");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn repairs_existing_exif_when_camera_model_tag_is_missing() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_repair_missing_camera_model_test_{}.jpg",
            std::process::id()
        ));
        let backup_path = exif_backup_path(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
        std::fs::write(&path, minimal_jpeg_with_datetime_original("2012:09:02 14:39:12"))
            .unwrap();

        let mut metadata = read_exif_metadata(&path);
        metadata.camera_model = "HTC-X315E".to_string();

        rewrite_repairable_exif_metadata(&path, &metadata, true).unwrap();

        let written = read_exif_metadata(&path);
        assert_eq!(written.taken_date, "2012-09-02 14:39:12");
        assert_eq!(written.camera_model, "HTC-X315E");
        assert!(backup_path.exists());

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(backup_path);
    }

    #[test]
    fn repairs_existing_exif_without_backup_when_disabled() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_repair_no_backup_test_{}.jpg",
            std::process::id()
        ));
        let backup_path = exif_backup_path(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
        std::fs::write(&path, minimal_jpeg_with_datetime_original("2012:09:02 14:39:12"))
            .unwrap();

        let mut metadata = read_exif_metadata(&path);
        metadata.camera_model = "HTC-X315E".to_string();
        rewrite_repairable_exif_metadata(&path, &metadata, false).unwrap();

        assert!(!backup_path.exists());
        assert_eq!(read_exif_metadata(&path).camera_model, "HTC-X315E");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_to_rewrite_existing_exif_structure() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_existing_exif_rewrite_test_{}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, minimal_jpeg_with_camera_make("Canon")).unwrap();

        let mut metadata = ExifMetadata::default();
        metadata.camera_make = "Sony".to_string();
        let err = rewrite_basic_exif_metadata(&path, &metadata, true).unwrap_err();

        assert!(err.contains("Refusing to rewrite an existing EXIF structure"));
        assert_eq!(read_exif_metadata(&path).camera_make, "Canon");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rewrites_generated_exif_file_in_place() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_generated_exif_test_{}.jpg",
            std::process::id()
        ));
        let backup_path = exif_backup_path(&path);
        let _ = std::fs::remove_file(&backup_path);
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xd9]).unwrap();

        let metadata = ExifMetadata {
            camera_make: "Sony".to_string(),
            camera_model: "A7C".to_string(),
            ..ExifMetadata::default()
        };
        let output_path = rewrite_basic_exif_metadata(&path, &metadata, true).unwrap();
        assert_eq!(output_path, path);
        assert!(backup_path.exists());
        let mut updated = read_exif_metadata(&output_path);
        updated.lens_model = "FE 24-70mm F2.8 GM II".to_string();

        rewrite_generated_basic_exif_metadata(&output_path, &updated).unwrap();

        let written = read_exif_metadata(&output_path);
        assert_eq!(written.camera_make, "Sony");
        assert_eq!(written.camera_model, "A7C");
        assert_eq!(written.lens_model, "FE 24-70mm F2.8 GM II");

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(backup_path);
    }

    #[test]
    fn rewrites_basic_generated_exif_shape_in_place() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_basic_generated_shape_{}.jpg",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, minimal_jpeg_with_camera_make("Canon")).unwrap();

        let mut metadata = read_exif_metadata(&path);
        metadata.camera_model = "HTC-X315E".to_string();

        rewrite_generated_basic_exif_metadata(&path, &metadata).unwrap();
        write_camera_model(&path, "HTC-X315E").unwrap();

        let written = read_exif_metadata(&path);
        assert_eq!(written.camera_make, "Canon");
        assert_eq!(written.camera_model, "HTC-X315E");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removes_every_exif_app1_segment_but_preserves_other_app1_data() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_remove_all_exif_{}.jpg",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let first = minimal_jpeg_with_camera_make("Canon");
        let exif_segment = &first[2..first.len() - 2];
        let xmp_payload = b"http://ns.adobe.com/xap/1.0/\0<xmp>keep</xmp>";
        let xmp_len = u16::try_from(xmp_payload.len() + 2).unwrap();
        let mut jpeg = vec![0xff, 0xd8];
        jpeg.extend_from_slice(exif_segment);
        jpeg.extend_from_slice(&[0xff, 0xe1]);
        jpeg.extend_from_slice(&xmp_len.to_be_bytes());
        jpeg.extend_from_slice(xmp_payload);
        jpeg.extend_from_slice(exif_segment);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        std::fs::write(&path, jpeg).unwrap();

        remove_exif_metadata(&path, false).unwrap();

        let updated = std::fs::read(&path).unwrap();
        assert!(find_exif_app1_range(&updated).is_none());
        assert!(updated
            .windows(xmp_payload.len())
            .any(|window| window == xmp_payload));
        assert!(!read_exif_metadata(&path).has_exif);

        let _ = std::fs::remove_file(path);
    }

    fn minimal_jpeg_with_datetime_original(value: &str) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());

        let ifd0_offset = 8usize;
        let exif_ifd_offset = 26u32;
        let datetime_offset = 44u32;

        assert_eq!(tiff.len(), ifd0_offset);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x8769u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&exif_ifd_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), exif_ifd_offset as usize);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x9003u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&20u32.to_le_bytes());
        tiff.extend_from_slice(&datetime_offset.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), datetime_offset as usize);
        tiff.extend_from_slice(value.as_bytes());
        tiff.push(0);

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let segment_len = u16::try_from(payload.len() + 2).unwrap();

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }
}
