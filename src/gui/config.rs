use std::collections::HashSet;

#[derive(Clone)]
pub struct AppConfig {
    pub image_extensions: HashSet<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut exts = HashSet::new();
        exts.insert("jpg".to_string());
        exts.insert("jpeg".to_string());
        exts.insert("png".to_string());
        exts.insert("gif".to_string());
        AppConfig {
            image_extensions: exts,
        }
    }
}
