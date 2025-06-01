use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use crate::exif_tag::{get_date_taken, extract_datetime_from_filename, append_creator_suffix, set_exif_datetime_and_software, check_datetime_tags};

use unicode_width::UnicodeWidthStr;
use image::{DynamicImage, ImageFormat, io::Reader as ImageReader};
use image::codecs::jpeg::JpegEncoder;
use rexiv2::{Metadata, Rexiv2Error};


pub fn run_cli(args: &[String]) {
    rexiv2::initialize();

    let args: Vec<String> = env::args().collect();
    let dir_path = if args.len() == 2 {
        Path::new(&args[1]).to_path_buf()
    } else {
        env::current_dir().unwrap()
    };

    if !dir_path.is_dir() {
        eprintln!("Error: {} is not a valid directory.", dir_path.display());
        return;
    }

    let input_files = list_image_files(&dir_path);

    if input_files.is_empty() {
        println!("No file found: {}", dir_path.to_string_lossy());
        return;
    }

    let mut jpeg_files = Vec::new();
    let mut updated_files = Vec::new();
    let mut failed_files = Vec::new();

    for file in input_files {
        let ext = file.extension().unwrap_or_default().to_string_lossy().to_lowercase();

        let target_path = if ext == "jpg" || ext == "jpeg" {
            file.clone()
        } else {
            match convert_to_jpeg(&file) {
                Some(jpg_path) => jpg_path,
                None => {
                    println!("Failed to convert: {}", file.display());
                    continue;
                }
            }
        };

        jpeg_files.push(target_path);
    }

    for file in &jpeg_files {
        match check_datetime_tags(file) {
            Ok(()) => {
                println!("{} → OK", file.display());
                updated_files.push(file.clone());
            }
            Err(e) => {
                println!("{} → 날짜 없음 또는 점검 실패: {}", file.display(), e);
                match try_update_date_from_filename(file) {
                    Ok(dt) => {
                        println!("→ EXIF 작성 완료: {} ({})", file.display(), dt);
                        updated_files.push(file.clone());
                    }
                    Err(e) => {
                        println!("→ EXIF 작성 실패: {} → {}", file.display(), e);
                        failed_files.push(file.clone());
                    }
                }
            }
        }
    }

    println!("→ EXIF 처리 완료: {}개", updated_files.len());
    println!("→ 실패한 파일: {}개", failed_files.len());
}

pub fn list_image_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ["jpg", "jpeg", "png", "gif"].contains(&ext.as_str()) {
                    files.push(path);
                }
            }
        }
    }

    files
}

fn print_file_info(path: &PathBuf) -> bool {
    let file_size_kb = fs::metadata(path).unwrap().len() / 1024;
    let filename_cow = path.file_name().unwrap().to_string_lossy();
    let filename = filename_cow.as_ref();
    let date_taken = get_date_taken(path).unwrap_or_else(|| "N/A".to_string());

    let width = UnicodeWidthStr::width(filename);
    let padding = if width < 40 { 40 - width } else { 0 };
    let padded = format!("{}{}", filename, " ".repeat(padding));

    println!("{} {:>6} KB   {}", padded, file_size_kb, date_taken);

    date_taken != "N/A"
}

fn convert_to_jpeg(path: &Path) -> Option<PathBuf> {
    let img = ImageReader::open(path).ok()?.decode().ok()?;
    let new_path = path.with_extension("jpg");

    let file = File::create(&new_path).ok()?;
    let mut encoder = JpegEncoder::new_with_quality(file, 100);
    encoder.encode_image(&img).ok()?;

    Some(new_path)
}

pub fn try_update_date_from_filename(path: &Path) -> Result<String, String> {
    println!("→ try_update_date_from_filename 진입: {}", path.display());

    if let Some(dt) = extract_datetime_from_filename(path) {
        
        println!("→ 날짜 추출됨: {dt}");
        
        set_exif_datetime_and_software(path, &dt)?;
        append_creator_suffix(path);
        Ok(dt)
    } else {
        Err("날짜 추출 실패".to_string())
    }
}