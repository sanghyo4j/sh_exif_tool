use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::path::PathBuf;
use std::time::SystemTime;

pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub is_dir: bool,
}

pub struct GuiApp {
    pub current_path: String,
    pub files: Vec<FileEntry>,
    pub selected: Option<usize>,
}

impl Default for GuiApp {
    fn default() -> Self {
        let path = std::env::current_dir().unwrap();
        let mut app = Self {
            current_path: path.to_string_lossy().to_string(),
            files: Vec::new(),
            selected: None,
        };
        app.load_folder();
        app
    }
}

impl GuiApp {
    fn load_folder(&mut self) {
        self.selected = None;
        self.files.clear();
        let path = PathBuf::from(&self.current_path);
        if let Ok(read_dir) = std::fs::read_dir(&path) {
            for entry in read_dir.flatten() {
                let p = entry.path();
                if let Ok(meta) = entry.metadata() {
                    self.files.push(FileEntry {
                        path: p,
                        size: meta.len(),
                        modified: meta.modified().ok(),
                        created: meta.created().ok(),
                        accessed: meta.accessed().ok(),
                        is_dir: meta.is_dir(),
                    });
                }
            }
        }
    }

    fn format_size_kb(size: u64) -> String {
        if size == 0 { "0 KB".to_string() } else { format!("{} KB", (size + 1023) / 1024) }
    }

    fn format_time(t: SystemTime) -> String {
        use chrono::{DateTime, Local};
        let dt: DateTime<Local> = t.into();
        dt.format("%Y-%m-%d %H:%M").to_string()
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("path_bar").show(ctx, |ui| {
            let resp = ui.text_edit_singleline(&mut self.current_path);
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.load_folder();
            }
        });

        egui::SidePanel::right("info_panel").default_width(260.0).show(ctx, |ui| {
            ui.heading("File Info");
            ui.separator();
            if let Some(index) = self.selected {
                let entry = &self.files[index];
                let name = entry.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(name);
                ui.separator();
                let size_label = if entry.is_dir { "Folder".to_owned() } else { Self::format_size_kb(entry.size) };
                ui.label(size_label);
                if let Some(t) = entry.created { ui.label(format!("Created: {}", Self::format_time(t))); }
                if let Some(t) = entry.modified { ui.label(format!("Modified: {}", Self::format_time(t))); }
                if let Some(t) = entry.accessed { ui.label(format!("Accessed: {}", Self::format_time(t))); }
            } else { ui.label("No file selected"); }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut open_dir: Option<PathBuf> = None;
            let mut go_parent = false;

            TableBuilder::new(ui)
                .striped(true)
                .column(Column::remainder())
                .column(Column::exact(90.0))
                .column(Column::exact(170.0))
                .column(Column::exact(170.0))
                .header(20.0, |mut header| {
                    header.col(|ui| { ui.label("Name"); });
                    header.col(|ui| { ui.label("Size"); });
                    header.col(|ui| { ui.label("Modified"); });
                    header.col(|ui| { ui.label("Created"); });
                })
                .body(|mut body| {
                    body.row(22.0, |mut row| {
                        row.col(|ui| { let r = ui.selectable_label(false, "[..]"); if r.double_clicked() { go_parent = true; } });
                        row.col(|_ui| {});
                        row.col(|_ui| {});
                        row.col(|_ui| {});
                    });

                    body.rows(22.0, self.files.len(), |mut row| {
                        let index = row.index();
                        let entry = &self.files[index];
                        let label = entry.path.file_name().map(|s| s.to_string_lossy().into_owned()).map(|s| if entry.is_dir { format!("[{}]", s) } else { s }).unwrap_or_default();
                        let selected = self.selected == Some(index);

                        row.col(|ui| {
                            let r = ui.selectable_label(selected, label);
                            if r.clicked() { self.selected = Some(index); }
                            if r.double_clicked() && entry.is_dir { open_dir = Some(entry.path.clone()); }
                        });

                        row.col(|ui| { if entry.is_dir { ui.label("-"); } else { ui.label(Self::format_size_kb(entry.size)); } });
                        row.col(|ui| { if let Some(t) = entry.modified { ui.label(Self::format_time(t)); } else { ui.label("-"); } });
                        row.col(|ui| { if let Some(t) = entry.created { ui.label(Self::format_time(t)); } else { ui.label("-"); } });
                    });
                });

            if go_parent {
                if let Some(parent) = PathBuf::from(&self.current_path).parent() {
                    self.current_path = parent.to_string_lossy().to_string();
                    self.load_folder();
                }
            }

            if let Some(dir) = open_dir {
                self.current_path = dir.to_string_lossy().to_string();
                self.load_folder();
            }
        });
    }
}