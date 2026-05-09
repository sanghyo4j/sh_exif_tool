use std::path::PathBuf;
use std::time::SystemTime;
use slint::{ModelRc, StandardListViewItem, VecModel};
use super::FileEntry as UiFileEntry; 
use crate::fs::{read_directory, FileSystemEntry};

pub struct SlintApp {
    pub current_path: String,
    pub files: Vec<FileSystemEntry>,
}

impl SlintApp {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string();

        let mut app = Self {
            current_path: current_dir,
            files: Vec::new(),
        };
        app.load_folder();
        app
    }

    pub fn load_folder(&mut self) {
        let path = std::path::Path::new(&self.current_path);
        match read_directory(path) {
            Ok(entries) => self.files = entries,
            Err(err) => {
                self.files.clear();
                eprintln!("Failed to read directory: {} ({err})", self.current_path);
            }
        }
    }

    pub fn get_ui_model(&self) -> ModelRc<UiFileEntry> {
        let mut ui_items: Vec<UiFileEntry> = Vec::new();

        ui_items.push(UiFileEntry {
            name: "[..]".into(),
            size: "-".into(),
            modified: "-".into(),
            created: "-".into(),
            is_dir: true,
        });

        for f in &self.files {
            let name_str = f.path.file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            
            ui_items.push(UiFileEntry {
                name: name_str.into(),
                size: if f.is_dir { "-".into() } else { format!("{} KB", (f.size + 1023) / 1024).into() },
                modified: f.modified.map(format_time).unwrap_or_else(|| "-".into()).into(),
                created: f.created.map(format_time).unwrap_or_else(|| "-".into()).into(),
                is_dir: f.is_dir,
            });
        }

        ModelRc::new(VecModel::from(ui_items))
    }

    pub fn get_table_model(&self) -> ModelRc<ModelRc<StandardListViewItem>> {
        let mut rows: Vec<ModelRc<StandardListViewItem>> = Vec::new();

        rows.push(ModelRc::new(VecModel::from(vec![
            StandardListViewItem::from("[..]"),
            StandardListViewItem::from("-"),
            StandardListViewItem::from("-"),
        ])));

        for f in &self.files {
            let name = f.path.file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let display_name = if f.is_dir {
                format!("[{}]", name)
            } else {
                name
            };
            let size = if f.is_dir {
                "-".to_string()
            } else {
                format!("{} KB", (f.size + 1023) / 1024)
            };
            let modified = f.modified.map(format_time).unwrap_or_else(|| "-".to_string());

            rows.push(ModelRc::new(VecModel::from(vec![
                StandardListViewItem::from(display_name.as_str()),
                StandardListViewItem::from(size.as_str()),
                StandardListViewItem::from(modified.as_str()),
            ])));
        }

        ModelRc::new(VecModel::from(rows))
    }

    pub fn file_count(&self) -> usize {
        self.files.iter().filter(|entry| !entry.is_dir).count()
    }

    pub fn path_for_ui_index(&self, index: i32) -> Option<PathBuf> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some(PathBuf::from(&self.current_path).parent()?.to_path_buf());
        }

        self.files.get(idx - 1).map(|entry| entry.path.clone())
    }

    pub fn ui_details_for_index(&self, index: i32) -> Option<(String, String, String, bool)> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some((
                "[..]".to_string(),
                "-".to_string(),
                "-".to_string(),
                true,
            ));
        }

        let entry = self.files.get(idx - 1)?;
        let name = entry.path.file_name()
            .map(|os_str| os_str.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let modified = entry.modified.map(format_time).unwrap_or_else(|| "-".to_string());
        let created = entry.created.map(format_time).unwrap_or_else(|| "-".to_string());

        Some((name, created, modified, entry.is_dir))
    }
}

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}
