use std::path::Path;
use rexiv2::Metadata;

pub fn get_date_taken(path: &Path) -> String {
    match Metadata::new_from_path(path) {
        Ok(meta) => meta
            .get_tag_string("Exif.Photo.DateTimeOriginal")
            .unwrap_or_else(|_| "N/A".to_string()),
        Err(_) => "N/A".to_string(),
    }
}