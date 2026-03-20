use std::path::PathBuf;
use std::time::SystemTime;
use slint::{ModelRc, VecModel, SharedString};
// mod.rs에서 slint::include_modules!()를 통해 생성된 FileEntry를 가져옵니다.
use super::FileEntry as UiFileEntry; 

/// 시스템의 실제 파일 정보를 담는 로컬 구조체
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
        if let Ok(read_dir) = std::fs::read_dir(&self.current_path) {
            let mut entries: Vec<LocalFileData> = read_dir
                .flatten()
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

            // 디렉토리가 위로 오도록 정렬 (선택 사항)
            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.path.cmp(&b.path)));
            self.files = entries;
        }
    }

    pub fn get_ui_model(&self) -> ModelRc<UiFileEntry> {
        let items: Vec<UiFileEntry> = self.files.iter().map(|f| {
            let name = f.path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            
            UiFileEntry {
                name: SharedString::from(if f.is_dir { format!("[{}]", name) } else { name }),
                size: SharedString::from(if f.is_dir { "-".into() } else { format!("{} KB", (f.size + 1023) / 1024) }),
                modified: SharedString::from(f.modified.map(format_time).unwrap_or_else(|| "-".into())),
                created: SharedString::from(f.created.map(format_time).unwrap_or_else(|| "-".into())),
                is_dir: f.is_dir,
            }
        }).collect();
        
        ModelRc::new(VecModel::from(items))
    }
}

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}