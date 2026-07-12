use std::path::PathBuf;
use std::time::SystemTime;
use slint::{ModelRc, StandardListViewItem, VecModel};
use super::FileEntry as UiFileEntry; 
use crate::fs::{read_directory, FileSystemEntry};

pub struct SlintApp {
    pub current_path: String,
    pub files: Vec<FileSystemEntry>,
    pub selected_indices: Vec<i32>,
    pub show_only_supported_images: bool,
    selection_anchor: Option<i32>,
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
            selected_indices: Vec::new(),
            show_only_supported_images: true,
            selection_anchor: None,
        };
        app.load_folder();
        app
    }

    pub fn load_folder(&mut self) {
        let path = std::path::Path::new(&self.current_path);
        match read_directory(path) {
            Ok(entries) => {
                self.files = entries;
                self.selected_indices.clear();
                self.selection_anchor = None;
            }
            Err(err) => {
                self.files.clear();
                self.selected_indices.clear();
                self.selection_anchor = None;
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
            selected: self.selected_indices.contains(&0),
            is_supported_image: false,
        });

        for (visible_index, f) in self.visible_files().into_iter().enumerate() {
            let name_str = f.path.file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let ui_index = i32::try_from(visible_index + 1).unwrap_or(i32::MAX);
            
            ui_items.push(UiFileEntry {
                name: name_str.into(),
                size: if f.is_dir { "-".into() } else { format!("{} KB", (f.size + 1023) / 1024).into() },
                modified: f.modified.map(format_time).unwrap_or_else(|| "-".into()).into(),
                created: f.created.map(format_time).unwrap_or_else(|| "-".into()).into(),
                is_dir: f.is_dir,
                selected: self.selected_indices.contains(&ui_index),
                is_supported_image: is_supported_image_file(&f.path),
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

        for f in self.visible_files() {
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
        self.visible_files().iter().filter(|entry| !entry.is_dir).count()
    }

    pub fn path_for_ui_index(&self, index: i32) -> Option<PathBuf> {
        let idx = usize::try_from(index).ok()?;
        if idx == 0 {
            return Some(PathBuf::from(&self.current_path).parent()?.to_path_buf());
        }

        self.visible_files().get(idx - 1).map(|entry| entry.path.clone())
    }

    pub fn ui_index_for_path(&self, path: &std::path::Path) -> Option<i32> {
        self.visible_files()
            .into_iter()
            .position(|entry| entry.path == path)
            .and_then(|index| i32::try_from(index + 1).ok())
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

        let visible = self.visible_files();
        let entry = visible.get(idx - 1)?;
        let name = entry.path.file_name()
            .map(|os_str| os_str.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let modified = entry.modified.map(format_time).unwrap_or_else(|| "-".to_string());
        let created = entry.created.map(format_time).unwrap_or_else(|| "-".to_string());

        Some((name, created, modified, entry.is_dir))
    }

    pub fn select_ui_index(&mut self, index: i32, ctrl: bool, shift: bool) {
        if index < 0 || usize::try_from(index).map_or(true, |idx| idx >= self.visible_files().len() + 1) {
            self.selected_indices.clear();
            self.selection_anchor = None;
            return;
        }

        if shift {
            let anchor = self.selection_anchor.unwrap_or(index);
            let start = anchor.min(index);
            let end = anchor.max(index);
            self.selected_indices = (start..=end).collect();
        } else if ctrl {
            if let Some(position) = self.selected_indices.iter().position(|selected| *selected == index) {
                self.selected_indices.remove(position);
            } else {
                self.selected_indices.push(index);
            }
            self.selection_anchor = Some(index);
        } else {
            self.selected_indices.clear();
            self.selected_indices.push(index);
            self.selection_anchor = Some(index);
        }

        self.selected_indices.sort_unstable();
        self.selected_indices.dedup();
    }

    pub fn selected_indices(&self) -> &[i32] {
        &self.selected_indices
    }

    fn visible_files(&self) -> Vec<&FileSystemEntry> {
        self.files
            .iter()
            .filter(|entry| {
                !self.show_only_supported_images
                    || entry.is_dir
                    || is_supported_image_file(&entry.path)
            })
            .collect()
    }
}

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn is_supported_image_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
        .unwrap_or(false)
}
