pub mod core;
pub mod exif;
pub mod fs;
pub mod media;

#[cfg(feature = "gui-slint")]
mod gui_slint;

pub trait GuiRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>>;
}

#[cfg(feature = "gui-slint")]
use gui_slint::SlintRunner as SelectedRunner;

pub fn run_gui_app() -> Result<(), Box<dyn std::error::Error>> {
    SelectedRunner::run()
}
