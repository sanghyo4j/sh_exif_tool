pub mod layout;
pub mod event;
pub mod state;

use fltk::prelude::*;
use layout::build_ui;
use event::connect_events;
use state::GuiState;

pub fn run_app() {
    let mut state = GuiState::default();
    let (mut wind, mut widgets) = build_ui();
    connect_events(&mut state, &mut widgets);

    wind.show();
    fltk::app::run().unwrap();
}
