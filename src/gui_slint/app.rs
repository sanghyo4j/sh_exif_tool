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
        
        // 디버깅용 출력 (터미널에서 확인해 보세요)
        println!("Loading folder: {}", self.current_path);

        let path = std::path::Path::new(&self.current_path);
        if let Ok(read_dir) = std::fs::read_dir(path) {
            let mut entries: Vec<LocalFileData> = read_dir
                .filter_map(|res| res.ok()) // flatten() 대신 명시적으로 ok() 처리
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

            // 디버깅용: 읽어온 파일 개수 확인
            println!("Found {} entries", entries.len());

            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.path.cmp(&b.path)));
            self.files = entries;
        } else {
            eprintln!("Failed to read directory: {}", self.current_path);
        }
    }

    pub fn get_ui_model(&self) -> ModelRc<UiFileEntry> {
        let mut ui_items: Vec<UiFileEntry> = Vec::new();

        // 1. 상위 폴더 아이템 추가 (필드 명칭: name, size, modified, created, is_dir)
        ui_items.push(UiFileEntry {
            name: "[..]".into(), // Slint의 SharedString으로 자동 변환됨
            size: "-".into(),
            modified: "-".into(),
            created: "-".into(),
            is_dir: true,
        });

        // 2. 실제 파일 목록 추가
        for f in &self.files {
            let name_str = f.path.file_name()
                .map(|os_str| os_str.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            
            println!("DEBUG: Mapping file name -> '{}'", name_str);

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
}

fn format_time(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}