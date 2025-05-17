mod exif_tag;

use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use exif_tag::{get_date_taken, extract_datetime_from_filename, append_creator_suffix};

use unicode_width::UnicodeWidthStr;
use image::{DynamicImage, ImageFormat, io::Reader as ImageReader};
use image::codecs::jpeg::JpegEncoder;


fn main() {
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

    let files = list_image_files(&dir_path);

    if files.is_empty() {
        println!("No file found: {}", dir_path.to_string_lossy());
        return;
    }

    let mut missing_date_files = Vec::new();

    for file in files {
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

        let has_date = print_file_info(&target_path);
        if !has_date {
            missing_date_files.push(target_path);
        }
    }

    for file in missing_date_files {
        if let Some(updated) = try_update_date_from_filename(&file) {
            println!("→ EXIF 작성 완료: {} ({})", file.display(), updated);
        } else {
            println!("→ EXIF 작성 실패: {}", file.display());
        }
    }
}

fn list_image_files(dir: &Path) -> Vec<PathBuf> {
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

fn try_update_date_from_filename(path: &Path) -> Option<String> {
    if let Some(dt) = extract_datetime_from_filename(path) {
        if let Ok(mut meta) = rexiv2::Metadata::new_from_path(path) {
            let r1 = meta.set_tag_string("Exif.Photo.DateTimeOriginal", &dt);
            let r2 = meta.set_tag_string("Exif.Image.Software", "SH EXIF TAG CREATOR");

            if r1.is_ok() && r2.is_ok() && meta.save_to_file(path).is_ok() {
                append_creator_suffix(path);
                return Some(dt);
            }
        }
    }
    None
}