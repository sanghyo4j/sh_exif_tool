use eframe::NativeOptions;
use sh_exif_tool::gui_egui::GuiApp;

fn main() -> eframe::Result<()> {
    let options = NativeOptions::default();
    
    eframe::run_native(
        "sh_exif_tool GUI Prototype",
        options,
        Box::new(|_cc| Ok(Box::<GuiApp>::default())),
    )
}