use eframe::egui;
use std::fs;
use std::time::UNIX_EPOCH;
use chrono::NaiveDateTime;

fn main() {
    let mut native_options = eframe::NativeOptions::default();
    native_options.default_theme = eframe::Theme::Light;
    eframe::run_native(
        "폴더 뷰어",
        native_options,
        Box::new(|_cc| Box::new(MyApp::new())),
    );
}

struct FileItem {
    name: String,
    size: u64,
    created: String,
}

struct MyApp {
    path: String,
    files: Vec<FileItem>,
    selected: Option<usize>,
}

impl MyApp {
    fn new() -> Self {
        let path = std::env::current_dir().unwrap().to_string_lossy().to_string();
        let files = fs::read_dir(&path)
            .map(|r| {
                r.filter_map(|e| {
                    if let Ok(e) = e {
                        let meta = e.metadata().ok()?;
                        let size = meta.len();
                        let created = meta.created().unwrap_or(UNIX_EPOCH);
                        let secs = created.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                        let created_str = NaiveDateTime::from_timestamp_opt(secs as i64, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| "-".to_string());
                        Some(FileItem {
                            name: e.file_name().to_string_lossy().to_string(),
                            size,
                            created: created_str,
                        })
                    } else {
                        None
                    }
                })
                .collect()
            })
            .unwrap_or_else(|_| vec![]);
        Self { path, files, selected: None }
    }
}

fn format_size_kb(size: u64) -> String {
    let kb = size / 1024;
    let s = kb.to_string();
    let mut out = String::new();
    let chars: Vec<_> = s.chars().collect();
    for (i, c) in chars.iter().rev().enumerate() {
        if i != 0 && i % 3 == 0 {
            out.insert(0, ',');
        }
        out.insert(0, *c);
    }
    format!("{out} KB")
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            std::process::exit(0);
        }

        ctx.set_visuals(egui::Visuals::light());

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_sized([ui.available_width(), 32.0], egui::TextEdit::singleline(&mut self.path));
        });

        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            let cnt = self.files.len();
            let selected = self.selected.map(|i| &self.files[i].name).map_or("-", |v| v);
            ui.horizontal(|ui| {
                ui.label(format!("파일 수: {cnt}"));
                ui.separator();
                ui.label(format!("선택: {selected}"));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    let col_widths = [220.0, 90.0, 180.0];
                    ui.columns(3, |cols| {
                        cols[0].label("File Name");
                        cols[1].label("File Size");
                        cols[2].label("Created");
                    });
                    ui.separator();



                    for (i, file) in self.files.iter().enumerate() {
                        let is_selected = Some(i) == self.selected;
                        let row_height = 28.0;
                        let col_widths = [220.0, 90.0, 180.0];

                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::click(),
                        );

                        let bg = if is_selected {
                            ui.visuals().selection.bg_fill
                        } else if resp.hovered() {
                            ui.visuals().selection.bg_fill.linear_multiply(1.2)
                        } else {
                            egui::Color32::WHITE
                        };
                        ui.painter().rect_filled(rect, 0.0, bg);

                        // 텍스트를 painter로 직접 그림
                        let mut x = rect.left();
                        let y = rect.center().y;
                        let text_color = if is_selected { ui.visuals().selection.stroke.color } else { egui::Color32::BLACK };
                        let row_values = [
                            &file.name,
                            &format_size_kb(file.size),
                            &file.created,
                        ];
                        for (w, val) in col_widths.iter().zip(row_values.iter()) {
                            let pos = egui::pos2(x + 4.0, y - row_height * 0.5 + 7.0);
                            ui.painter().text(
                                pos,
                                egui::Align2::LEFT_TOP,
                                val,
                                egui::TextStyle::Body.resolve(ui.style()),
                                text_color,
                            );
                            x += *w;
                        }

                        if resp.clicked() {
                            self.selected = Some(i);
                            ctx.request_repaint();
                        }
                    }



                });
            });
        });
    }
}
