use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use chrono::{DateTime, Utc};

use super::MediaScanResult;

const QUICKTIME_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;

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
    Ok(Some(date.format("%Y-%m-%d %H:%M:%S").to_string()))
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
        assert_eq!(result.media_date, "2012-08-15 01:00:00");
        assert_eq!(result.metadata_status, "O");
    }
}
