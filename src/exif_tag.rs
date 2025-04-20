use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use rexiv2::Metadata;

pub fn get_or_create_metadata(path: &Path) -> Option<Metadata> {
    match Metadata::new_from_path(path) {
        Ok(meta) => Some(meta),
        Err(_) => {
            let meta = Metadata::new();
            if meta.save_to_file(path).is_ok() {
                Some(meta)
            } else {
                None
            }
        }
    }
}

pub fn get_date_taken(path: &Path) -> Option<String> {
    get_or_create_metadata(path).and_then(|meta| {
        meta.get_tag_string("Exif.Photo.DateTimeOriginal").ok()
    })
}

pub fn ensure_date_taken(path: &Path) -> Option<String> {
    match get_date_taken(path) {
        Some(date) => Some(date),
        None => {
            if let Ok(created) = fs::metadata(path).and_then(|m| m.created()) {
                if let Ok(formatted) = format_system_time(created) {
                    if let Some(mut meta) = get_or_create_metadata(path) {
                        let _ = meta.set_tag_string("Exif.Photo.DateTimeOriginal", &formatted);
                        let _ = meta.save_to_file(path);
                        return Some(formatted);
                    }
                }
            }
            None
        }
    }
}

fn format_system_time(time: SystemTime) -> Result<String, ()> {
    let datetime = time.duration_since(UNIX_EPOCH).map_err(|_| ())?;

    let secs = datetime.as_secs();
    let naive = time_t_to_ymdhms(secs);
    Ok(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        naive.0, naive.1, naive.2, naive.3, naive.4, naive.5
    ))
}

fn time_t_to_ymdhms(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    use std::time::Duration;
    use time::OffsetDateTime;

    let t = UNIX_EPOCH + Duration::from_secs(secs);
    let odt = OffsetDateTime::from(t);
    (
        odt.year() as u32,
        odt.month() as u32,
        odt.day() as u32,
        odt.hour() as u32,
        odt.minute() as u32,
        odt.second() as u32,
    )
}
