pub mod layout;
pub mod event;
pub mod state;
pub mod config;

use fltk::app;
use fltk::prelude::*;
use crate::gui::layout::build_ui;
use crate::gui::state::GuiState;
use crate::gui::event::connect_events;
use crate::gui::config::AppConfig;

pub fn launch_gui() {
    let app = app::App::default();
    let (mut wind, mut widgets) = build_ui();
    let mut state = GuiState::default();
    let config = AppConfig::default();

    connect_events(&config, &mut state, &mut widgets);
    wind.show();
    app.run().unwrap();
}
