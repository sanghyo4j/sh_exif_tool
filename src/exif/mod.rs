use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ExifMetadata {
    pub has_exif: bool,
    pub taken_date: String,
    pub camera_make: String,
    pub camera_model: String,
    pub lens_model: String,
    pub software: String,
    pub artist: String,
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
            camera_make: empty_value(),
            camera_model: empty_value(),
            lens_model: empty_value(),
            software: empty_value(),
            artist: empty_value(),
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

fn parse_tiff(data: &[u8]) -> Option<ExifMetadata> {
    if data.len() < 8 {
        return None;
    }

    let endian = match &data[0..2] {
        b"II" => Endian::Little,
        b"MM" => Endian::Big,
        _ => return None,
    };

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
            0x010f => meta.camera_make = entry.as_ascii().unwrap_or_else(empty_value),
            0x0110 => meta.camera_model = entry.as_ascii().unwrap_or_else(empty_value),
            0x0112 => meta.orientation = entry.as_short().map(format_orientation).unwrap_or_else(empty_value),
            0x0131 => meta.software = entry.as_ascii().unwrap_or_else(empty_value),
            0x0132 => {
                if meta.taken_date.is_empty() {
                    meta.taken_date = entry.as_ascii().unwrap_or_else(empty_value);
                }
            }
            0x013b => meta.artist = entry.as_ascii().unwrap_or_else(empty_value),
            0x8769 => exif_ifd = entry.as_long().map(|v| v as usize),
            0x8825 => gps_ifd = entry.as_long().map(|v| v as usize),
            _ => {}
        }
    }

    if let Some(offset) = exif_ifd {
        parse_exif_ifd(data, offset, endian, &mut meta);
    }

    if let Some(offset) = gps_ifd {
        parse_gps_ifd(data, offset, endian, &mut meta);
    }

    Some(meta)
}

fn parse_exif_ifd(data: &[u8], offset: usize, endian: Endian, meta: &mut ExifMetadata) {
    let Some(entries) = read_ifd_entries(data, offset, endian) else {
        return;
    };

    for entry in entries {
        match entry.tag {
            0x829a => meta.shutter_speed = entry.as_rational().map(format_shutter).unwrap_or_else(empty_value),
            0x829d => meta.aperture = entry.as_rational().map(format_aperture).unwrap_or_else(empty_value),
            0x8827 => meta.iso_speed = entry.as_short().map(|v| v.to_string()).unwrap_or_else(empty_value),
            0x9003 => meta.taken_date = entry.as_ascii().unwrap_or_else(empty_value),
            0x9004 => {
                if meta.taken_date.is_empty() {
                    meta.taken_date = entry.as_ascii().unwrap_or_else(empty_value);
                }
            }
            0x9207 => meta.metering_mode = entry.as_short().map(format_metering).unwrap_or_else(empty_value),
            0x9209 => meta.flash_fired = entry.as_short().map(format_flash).unwrap_or_else(empty_value),
            0x920a => meta.focal_length = entry.as_rational().map(format_focal_length).unwrap_or_else(empty_value),
            0xa001 => meta.color_space = entry.as_short().map(format_color_space).unwrap_or_else(empty_value),
            0xa002 => meta.image_width = entry.as_number().map(|v| v.to_string()).unwrap_or_else(empty_value),
            0xa003 => meta.image_height = entry.as_number().map(|v| v.to_string()).unwrap_or_else(empty_value),
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
            return Some(&segment[6..]);
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

fn empty_value() -> String {
    String::new()
}
