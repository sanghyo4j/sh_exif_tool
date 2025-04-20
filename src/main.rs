mod exif_tag;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use exif_tag::get_date_taken;
use unicode_width::UnicodeWidthStr;

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

    let files = list_jpg_files(&dir_path);

    for file in files {
        print_file_info(&file);
    }
}

fn list_jpg_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ext == "jpg" || ext == "jpeg" {
                    files.push(path);
                }
            }
        }
    }

    files
}

use exif_tag::ensure_date_taken;

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
