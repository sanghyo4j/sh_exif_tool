use std::path::Path;

use crate::exif::read_exif_metadata_for_scan;

use super::MediaScanResult;

pub(super) fn has_jpeg_signature(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xff, 0xd8, 0xff]
}

pub(super) fn scan(path: &Path) -> MediaScanResult {
    let metadata = read_exif_metadata_for_scan(path);
    let media_date = if metadata.taken_date.trim().is_empty() {
        "-".to_string()
    } else {
        metadata.taken_date.clone()
    };
    MediaScanResult {
        media_kind: "jpeg".to_string(),
        media_type: "JPEG image".to_string(),
        media_date,
        metadata_status: if metadata.has_exif { "O" } else { "X" }.to_string(),
        time_interpretation: String::new(),
        exif_metadata: Some(metadata),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exif::read_exif_metadata;

    #[test]
    fn fast_scan_matches_existing_reader_for_basic_exif() {
        let path = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_jpeg_scan_equivalence_{}.jpg",
            std::process::id()
        ));
        let bytes = minimal_jpeg_with_taken_date("2012:08:15 02:30:00");
        std::fs::write(&path, bytes).unwrap();

        let existing = read_exif_metadata(&path);
        let scanned = scan(&path);
        let _ = std::fs::remove_file(path);

        assert!(existing.has_exif);
        assert_eq!(scanned.media_date, existing.taken_date);
        assert_eq!(scanned.metadata_status, "O");
    }

    fn minimal_jpeg_with_taken_date(value: &str) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0132u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        let mut value_bytes = value.as_bytes().to_vec();
        value_bytes.push(0);
        tiff.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&value_bytes);

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }
}
