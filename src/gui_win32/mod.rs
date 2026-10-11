mod model;
mod window;

use crate::GuiRunner;

pub struct Win32Runner;

impl GuiRunner for Win32Runner {
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        window::run()
    }
}
