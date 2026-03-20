use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut options = eframe::NativeOptions::default();
    options.viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([900.0, 600.0]);

    eframe::run_native(
        "gui_egui",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    core: core::AppCore,
    cwd: PathBuf,
    entries: Vec<String>,
    selected: Option<usize>,
    tags: String,
    show_about: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_visuals(&cc.egui_ctx);
        let cwd = std::env::current_dir().unwrap();
        let entries = read_entries(&cwd);
        Self {
            core: core::AppCore::new(),
            cwd,
            entries,
            selected: None,
            tags: String::new(),
            show_about: false,
        }
    }
}

fn setup_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_rounding = egui::Rounding::same(10.0);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    ctx.set_style(style);
}

fn read_entries(dir: &PathBuf) -> Vec<String> {
    let mut v = Vec::new();
    v.push("..".to_string());
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Some(n) = e.file_name().to_str() {
                v.push(n.to_string());
            }
        }
    }
    v
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |_ui| {});
                ui.menu_button("Setting", |_ui| {});
                ui.menu_button("Help", |ui| {
                    if ui.button("Version").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::top("path").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.cwd.to_string_lossy());
            });
        });

        egui::SidePanel::left("left").resizable(true).show(ctx, |ui| {
            ui.heading("Files");
            ui.separator();
            for (i, name) in self.entries.iter().enumerate() {
                let sel = self.selected == Some(i);
                if ui.selectable_label(sel, name).clicked() {
                    self.selected = Some(i);
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::TopBottomPanel::top("right_top").show_inside(ui, |ui| {
                ui.heading("Thumbnail");
                ui.separator();
                ui.allocate_space(ui.available_size());
            });

            egui::TopBottomPanel::bottom("right_bottom").show_inside(ui, |ui| {
                ui.heading("Tags");
                ui.separator();
                ui.add(
                    egui::TextEdit::multiline(&mut self.tags)
                        .desired_rows(4)
                        .hint_text("comma separated"),
                );
            });
        });

        egui::Window::new("Version")
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("core {}", self.core.version_str()));
            });
    }
}
