use std::{
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
    cell::RefCell,
};
use fltk::prelude::*;
use crate::gui::{layout::UiWidgets, state::GuiState, config::AppConfig};

pub fn connect_events(config: &AppConfig, state: &mut GuiState, widgets: &mut UiWidgets) {
    let config = Rc::new(AppConfig {
        image_extensions: config.image_extensions.clone(),
    });
    let state = Rc::new(RefCell::new(state.clone()));
    let widgets = Rc::new(RefCell::new(widgets.clone()));

    let current_dir = env::current_dir().unwrap();
    update_view(&config, &state, &widgets, &current_dir);

    let mut tree = widgets.borrow().file_tree.clone();
    let config_clone = config.clone();
    let state_clone = state.clone();
    let widgets_clone = widgets.clone();

    tree.set_callback(move |t| {
        if let Some(item) = t.callback_item() {
            if let Ok(path_str) = t.item_pathname(&item) {
                let path = PathBuf::from(path_str);
                if path.is_dir() {
                    update_view(&config_clone, &state_clone, &widgets_clone, &path);
                }
            }
        }
    });
}

fn update_view(
    config: &Rc<AppConfig>,
    state: &Rc<RefCell<GuiState>>,
    widgets: &Rc<RefCell<UiWidgets>>,
    dir: &Path,
) {
    let mut widgets = widgets.borrow_mut();

    let current_str = dir.to_string_lossy().to_string();
    widgets.path_display.set_value(&current_str);

    widgets.file_tree.clear();
    widgets.file_tree.set_show_root(false);

    let mut path = dir;
    let mut all_paths = Vec::new();
    while let Some(parent) = path.parent() {
        all_paths.push(path.to_path_buf());
        path = parent;
    }
    all_paths.push(path.to_path_buf());
    all_paths.reverse();

    let mut opened = PathBuf::new();
    for p in &all_paths {
        let item_str = p.to_string_lossy().replace("\\", "/");
        widgets.file_tree.add(item_str.as_str());
        widgets.file_tree.open(item_str.as_str(), true);
        opened = p.clone();
    }

    if let Ok(entries) = fs::read_dir(&opened) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let item_str = path.to_string_lossy().replace("\\", "/");
                widgets.file_tree.add(item_str.as_str());
                widgets.file_tree.close(item_str.as_str(), true);
            }
        }
    }

    widgets.file_list.clear();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if config.image_extensions.contains(&ext.to_lowercase()) {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        widgets.file_list.add(name);
                    }
                }
            }
        }
    }
}
