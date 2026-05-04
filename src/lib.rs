pub mod core;
pub mod exif;

#[cfg(feature = "gui-egui")]
mod gui_egui;

#[cfg(feature = "gui-slint")]
mod gui_slint;

pub trait GuiRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>>;
}

// 두 기능이 동시에 켜졌을 때 우선순위 결정 또는 에러 처리
#[cfg(all(feature = "gui-slint", feature = "gui-egui"))]
compile_error!("Only one GUI feature can be enabled at a time.");

#[cfg(feature = "gui-slint")]
use gui_slint::SlintRunner as SelectedRunner;

#[cfg(all(feature = "gui-egui", not(feature = "gui-slint")))]
use gui_egui::EguiRunner as SelectedRunner;

pub fn run_gui_app() -> Result<(), Box<dyn std::error::Error>> {
    SelectedRunner::run()
}

#[cfg(not(any(feature = "gui-slint", feature = "gui-egui")))]
compile_error!("Please select a GUI feature: 'gui-slint' or 'gui-egui'");
