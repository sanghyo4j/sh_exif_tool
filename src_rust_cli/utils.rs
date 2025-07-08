use std::fs;
use std::path::{Path, PathBuf};

pub fn list_image_files(dir: &Path) -> Vec<PathBuf> {
    let exts = ["png", "jpg", "jpeg", "gif"];
    let mut result = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if exts.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                        result.push(path);
                    }
                }
            }
        }
    }

    result
}