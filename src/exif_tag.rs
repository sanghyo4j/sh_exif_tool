use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use regex::Regex;
use rexiv2::Metadata;
use time::{OffsetDateTime, PrimitiveDateTime, Date, Time};

pub fn get_date_taken(path: &Path) -> Option<String> {
    if let Ok(meta) = Metadata::new_from_path(path) {
        if let Ok(tag) = meta.get_tag_string("Exif.Photo.DateTimeOriginal") {
            return Some(tag);
        }
    }

    if let Some(dt) = extract_datetime_from_filename(path) {
        if let Ok(mut meta) = Metadata::new_from_path(path) {
            let r1 = meta.set_tag_string("Exif.Photo.DateTimeOriginal", &dt);
            let r2 = meta.set_tag_string("Exif.Image.Software", "SH148 EXIF TAG CREATOR v0.1.2");

            if r1.is_ok() && r2.is_ok() && meta.save_to_file(path).is_ok() {
                append_creator_suffix(path);
                return Some(dt);
            }
        }
    }

    None
}

pub fn ensure_date_taken(path: &Path) -> Option<String> {
    get_date_taken(path)
}

fn extract_datetime_from_filename(path: &Path) -> Option<String> {
    let filename = path.file_stem()?.to_str()?;
    println!("→ 파일명 분석 대상: {}", filename);

    let re = Regex::new(r"\d{12,14}").unwrap();
    let mut candidates = Vec::new();

    for mat in re.find_iter(filename) {
        let raw = mat.as_str();
        println!("→ 숫자 패턴 추출됨: {}", raw);

        let padded = match raw.len() {
            14 => raw.to_string(),          // yyyyMMddHHmmss
            12 => format!("{}00", raw),     // yyyyMMddHHmm → 초를 00으로
            _ => continue,
        };

        if let Ok(parsed) = parse_datetime_from_compact(&padded) {
            candidates.push(parsed);
        }
    }

    let earliest = candidates.into_iter().min()?;
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        earliest.year(),
        earliest.month() as u8,
        earliest.day(),
        earliest.hour(),
        earliest.minute(),
        earliest.second()
    ))
}

fn parse_datetime_from_compact(s: &str) -> Result<OffsetDateTime, ()> {
    if s.len() != 14 {
        return Err(());
    }

    let y = s[0..4].parse::<i32>().map_err(|_| ())?;
    let mo = s[4..6].parse::<u8>().map_err(|_| ())?;
    let d = s[6..8].parse::<u8>().map_err(|_| ())?;
    let h = s[8..10].parse::<u8>().map_err(|_| ())?;
    let mi = s[10..12].parse::<u8>().map_err(|_| ())?;
    let s = s[12..14].parse::<u8>().map_err(|_| ())?;

    let date = Date::from_calendar_date(y, mo.try_into().map_err(|_| ())?, d).map_err(|_| ())?;
    let time = Time::from_hms(h, mi, s).map_err(|_| ())?;
    let primitive = PrimitiveDateTime::new(date, time);

    Ok(primitive.assume_utc())
}

fn append_creator_suffix(path: &Path) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().unwrap().to_string_lossy();
    let ext = path.extension().unwrap_or_default().to_string_lossy();

    if stem.ends_with("_SH_EXIF_TAG_CREATOR") {
        return;
    }

    let new_filename = format!("{}_SH_EXIF_TAG_CREATOR.{}", stem, ext);
    let new_path = parent.join(new_filename);

    if let Err(e) = fs::rename(path, &new_path) {
        eprintln!("파일명 변경 실패: {} -> {} ({})", path.display(), new_path.display(), e);
    } else {
        println!("파일명 변경 완료: {}", new_path.display());
    }
}
