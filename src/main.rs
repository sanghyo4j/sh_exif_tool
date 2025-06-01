mod cli;
mod gui;
mod utils;

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        cli::run_cli(&args);
    } else {
        gui::run_gui();
    }
}
