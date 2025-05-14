mod exif_tag;

use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use exif_tag::ensure_date_taken;
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

    for file in files {
        let ext = file.extension().unwrap_or_default().to_string_lossy().to_lowercase();
    
        if ext == "jpg" || ext == "jpeg" {
            print_file_info(&file);
        } else {
            if let Some(jpg_path) = convert_to_jpeg(&file) {
                print_file_info(&jpg_path);
            } else {
                println!("Failed to convert: {}", file.display());
            }
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

fn print_file_info(path: &PathBuf) {
    let file_size_kb = fs::metadata(path).unwrap().len() / 1024;
    let filename_cow = path.file_name().unwrap().to_string_lossy();
    let filename = filename_cow.as_ref();

    let width = UnicodeWidthStr::width(filename);
    let padding = if width < 40 { 40 - width } else { 0 };
    let padded = format!("{}{}", filename, " ".repeat(padding));

    let date_taken = ensure_date_taken(path).unwrap_or_else(|| "N/A".to_string());

    println!("{} {:>6} KB   {}", padded, file_size_kb, date_taken);
}

fn convert_to_jpeg(path: &Path) -> Option<PathBuf> {
    let img = ImageReader::open(path).ok()?.decode().ok()?;
    let new_path = path.with_extension("jpg");

    let file = File::create(&new_path).ok()?;
    let mut encoder = JpegEncoder::new_with_quality(file, 100);
    encoder.encode_image(&img).ok()?;

    Some(new_path)
}