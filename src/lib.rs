pub mod core;
#[cfg_attr(feature = "gui-win32", allow(dead_code))]
pub mod exif;
pub mod fs;
#[cfg_attr(feature = "gui-win32", allow(dead_code))]
pub mod media;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_NUMBER: &str = "67";
pub const APP_TITLE: &str = concat!(
    "SH148 Photo Management Tool v",
    env!("CARGO_PKG_VERSION"),
    " (Build 67)"
);

#[cfg(feature = "gui-slint")]
mod gui_slint;

#[cfg(all(windows, feature = "gui-win32"))]
mod gui_win32;

pub trait GuiRunner {
    fn run() -> Result<(), Box<dyn std::error::Error>>;
}

#[cfg(all(windows, feature = "gui-win32"))]
use gui_win32::Win32Runner as SelectedRunner;

#[cfg(all(not(windows), feature = "gui-win32"))]
compile_error!("The gui-win32 feature is only supported on Windows.");

#[cfg(all(not(feature = "gui-win32"), feature = "gui-slint"))]
use gui_slint::SlintRunner as SelectedRunner;

#[cfg(all(feature = "gui-win32", feature = "gui-slint"))]
compile_error!("Select only one GUI feature: gui-win32 or gui-slint.");

#[cfg(not(any(feature = "gui-win32", feature = "gui-slint")))]
compile_error!("Select a GUI feature: gui-win32 or gui-slint.");

pub fn run_gui_app() -> Result<(), Box<dyn std::error::Error>> {
    SelectedRunner::run()
}
