use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use regex::Regex;
use rexiv2::Metadata;
use time::{OffsetDateTime, PrimitiveDateTime, Date, Time};
use time::format_description::FormatItem;
use time::format_description::parse;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn get_date_taken(path: &Path) -> Option<String> {
    if let Ok(meta) = Metadata::new_from_path(path) {
        if let Ok(tag) = meta.get_tag_string("Exif.Photo.DateTimeOriginal") {
            return Some(tag);
        }

        if let Ok(tag) = meta.get_tag_string("Exif.Image.DateTime") {
            return Some(tag);
        }
    }

    None
}

pub fn extract_datetime_from_filename(path: &Path) -> Option<String> {
    let filename = path.file_stem()?.to_str()?;

    let re = Regex::new(r"\d{8}[_-]?\d{4,6}").unwrap();
    let mut candidates = Vec::new();

    for mat in re.find_iter(filename) {
        let raw = mat.as_str().chars().filter(|c| c.is_ascii_digit()).collect::<String>();

        let padded = match raw.len() {
            14 => raw,
            12 => format!("{}00", raw),
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

pub fn append_creator_suffix(path: &Path) {
    println!("→ append_creator_suffix 호출: {}", path.display());

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().unwrap().to_string_lossy();
    let ext = path.extension().unwrap_or_default().to_string_lossy();

    if stem.ends_with("SH148") {
        println!("이미 SH148이 붙어 있음 → 변경 없음");
        return;
    }

    let new_name = format!("{}_SH148.{}", stem, ext);
    let new_path = parent.join(&new_name);

    println!("→ 파일명 변경: {} → {}", path.display(), new_path.display());

    match fs::rename(path, &new_path) {
        Ok(_) => println!("→ 파일명 변경 성공"),
        Err(e) => println!("→ 파일명 변경 실패: {:?}", e),
    }
}

pub fn set_exif_datetime_and_software(path: &Path, dt: &str) -> Result<(), String> {
    let mut meta = Metadata::new_from_path(path).map_err(|e| format!("{:?}", e))?;

    meta.set_tag_string("Exif.Photo.DateTimeOriginal", dt)
        .map_err(|e| format!("{:?}", e))?;

    let suffix = format!("SH148 EXIF TAG CREATOR v{}", env!("CARGO_PKG_VERSION"));
    let new_software = match meta.get_tag_string("Exif.Image.Software") {
        Ok(orig) => format!("{orig} (+{suffix})"),
        Err(_) => suffix,
    };

    meta.set_tag_string("Exif.Image.Software", &new_software)
        .map_err(|e| format!("{:?}", e))?;

    meta.save_to_file(path).map_err(|e| format!("{:?}", e))?;

    Ok(())
}

pub fn check_datetime_tags(path: &Path) -> Result<(), String> {
    let mut meta = Metadata::new_from_path(path).map_err(|e| format!("{:?}", e))?;

    let orig = meta.get_tag_string("Exif.Photo.DateTimeOriginal").ok();
    let dt = meta.get_tag_string("Exif.Image.DateTime").ok();
    let dig = meta.get_tag_string("Exif.Photo.DateTimeDigitized").ok();

    if orig.is_some() && dt.is_none() && dig.is_none() {
        return Ok(());
    }

    let mut dates = Vec::new();
    for tag in [&orig, &dt, &dig] {
        if let Some(s) = tag {
            if let Ok(p) = time::PrimitiveDateTime::parse(s, &time::format_description::parse("[year]:[month]:[day] [hour]:[minute]:[second]").unwrap()) {
                dates.push(p);
            }
        }
    }

    let earliest = dates.into_iter().min().ok_or("날짜 태그 파싱 실패")?;

    let fmt = parse("[year]:[month]:[day] [hour]:[minute]:[second]").unwrap();
    let value = earliest.format(&fmt).map_err(|e| format!("{:?}", e))?;

    meta.set_tag_string("Exif.Photo.DateTimeOriginal", &value)
        .map_err(|e| format!("{:?}", e))?;

    if dt.is_some() {
        meta.set_tag_string("Exif.Image.DateTime", &value).ok();
    }

    if dig.is_some() {
        meta.set_tag_string("Exif.Photo.DateTimeDigitized", &value).ok();
    }

    meta.save_to_file(path).map_err(|e| format!("{:?}", e))?;

    Ok(())
}