use std::fs;
use std::path::Path;
use std::time::SystemTime;
use rexiv2::Metadata;
use regex::Regex;
use time::OffsetDateTime;

pub fn get_date_taken(path: &Path) -> Option<String> {
    if let Ok(meta) = Metadata::new_from_path(path) {
        if let Ok(tag) = meta.get_tag_string("Exif.Photo.DateTimeOriginal") {
            return Some(tag);
        }
    }

    if let Some(dt) = extract_datetime_from_filename(path) {
        if let Ok(mut meta) = Metadata::new_from_path(path) {
            let _ = meta.set_tag_string("Exif.Photo.DateTimeOriginal", &dt);
            let _ = meta.save_to_file(path);
            return Some(dt);
        }
    }

    None
}

pub fn ensure_date_taken(path: &Path) -> Option<String> {
    if let Some(date) = get_date_taken(path) {
        return Some(date);
    }

    if let Ok(created) = fs::metadata(path).and_then(|m| m.created()) {
        if let Ok(formatted) = format_system_time(created) {
            if let Ok(mut meta) = Metadata::new_from_path(path) {
                let _ = meta.set_tag_string("Exif.Photo.DateTimeOriginal", &formatted);
                let _ = meta.save_to_file(path);
                return Some(formatted);
            }
        }
    }

    None
}

fn extract_datetime_from_filename(path: &Path) -> Option<String> {
    let filename = path.file_stem()?.to_str()?;
    println!("→ 파일명 분석 대상: {}", filename); // 디버깅용

    let re = Regex::new(r"^(\d{8})_(\d{6})").ok()?;
    let caps = re.captures(filename)?;

    let date = &caps[1];
    let time = &caps[2];

    let result = format!(
        "{}:{}:{} {}:{}:{}",
        &date[0..4], &date[4..6], &date[6..8],
        &time[0..2], &time[2..4], &time[4..6]
    );

    println!("→ 파싱 성공: {}", result); // 디버깅용
    Some(result)
}

fn format_system_time(system_time: SystemTime) -> Result<String, ()> {
    let datetime: OffsetDateTime = system_time.into();
    let formatted = format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        datetime.year(),
        datetime.month() as u8,
        datetime.day(),
        datetime.hour(),
        datetime.minute(),
        datetime.second()
    );
    Ok(formatted)
}
