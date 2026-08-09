use std::fs::File;

use super::MediaScanResult;

const TS_PACKET_SIZE: usize = 188;
const M2TS_PACKET_SIZE: usize = 192;

pub(super) fn has_transport_stream_signature(bytes: &[u8]) -> bool {
    has_sync_bytes(bytes, 0, TS_PACKET_SIZE) || has_sync_bytes(bytes, 4, M2TS_PACKET_SIZE)
}

fn has_sync_bytes(bytes: &[u8], first: usize, packet_size: usize) -> bool {
    (0..3).all(|packet| {
        let offset = first + packet * packet_size;
        bytes.get(offset).is_some_and(|value| *value == 0x47)
    })
}

pub(super) fn scan(file: &File) -> Result<MediaScanResult, String> {
    let mut bytes = [0u8; M2TS_PACKET_SIZE * 3];
    use std::io::{Read, Seek, SeekFrom};
    let mut file = file.try_clone().map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
    let count = file.read(&mut bytes).map_err(|error| error.to_string())?;
    if !has_transport_stream_signature(&bytes[..count]) {
        return Err("MPEG transport-stream sync bytes were not found.".to_string());
    }

    Ok(MediaScanResult {
        media_kind: "mts".to_string(),
        media_type: "AVCHD transport stream".to_string(),
        media_date: "-".to_string(),
        metadata_status: "X".to_string(),
        time_interpretation: "No standard embedded capture date".to_string(),
        exif_metadata: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_188_and_192_byte_transport_stream_packets() {
        let mut ts = vec![0u8; TS_PACKET_SIZE * 3];
        for offset in [0, TS_PACKET_SIZE, TS_PACKET_SIZE * 2] {
            ts[offset] = 0x47;
        }
        assert!(has_transport_stream_signature(&ts));

        let mut m2ts = vec![0u8; M2TS_PACKET_SIZE * 3];
        for offset in [4, 4 + M2TS_PACKET_SIZE, 4 + M2TS_PACKET_SIZE * 2] {
            m2ts[offset] = 0x47;
        }
        assert!(has_transport_stream_signature(&m2ts));
        assert!(!has_transport_stream_signature(&[0u8; M2TS_PACKET_SIZE * 3]));
    }
}
