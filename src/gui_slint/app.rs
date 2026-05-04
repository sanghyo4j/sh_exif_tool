use std::path::PathBuf;
use std::time::SystemTime;
use slint::{ModelRc, StandardListViewItem, VecModel};
use super::FileEntry as UiFileEntry; 

pub struct LocalFileData {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub is_dir: bool,
}

pub struct SlintApp {
    pub current_path: String,
    pub files: Vec<LocalFileData>,
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
        self.files.clear();

        let path = std::path::Path::new(&self.current_path);
        if let Ok(read_dir) = std::fs::read_dir(path) {
            let mut entries: Vec<LocalFileData> = read_dir
                .filter_map(|res| res.ok())
                .filter_map(|entry| {
                    let meta = entry.metadata().ok()?;
                    Some(LocalFileData {
                        path: entry.path(),
                        size: meta.len(),
                        modified: meta.modified().ok(),
                        created: meta.created().ok(),
                        is_dir: meta.is_dir(),
                    })
                })
                .collect();

            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.path.cmp(&b.path)));
            self.files = entries;
        } else {
            eprintln!("Failed to read directory: {}", self.current_path);
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
}

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}
