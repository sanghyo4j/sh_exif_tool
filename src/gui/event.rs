use std::path::PathBuf;
use fltk::{dialog::FileDialogType, dialog::NativeFileChooser, prelude::*};
use crate::gui::{state::GuiState, layout::UiWidgets};

pub fn connect_events(state: &mut GuiState, widgets: &mut UiWidgets) {
    let mut input_path = widgets.input_path.clone();
    let mut output = widgets.output.clone();

    widgets.btn_browse.set_callback(move |_| {
        let mut dialog = NativeFileChooser::new(FileDialogType::BrowseDir);
        dialog.show();
        let path_buf = dialog.filename();
        if let Some(path) = path_buf.to_str() {
            if !path.is_empty() {
                input_path.set_value(path); // ✅ 이제 오류 없음
            }
        }
    });

    let input_path = widgets.input_path.clone();
    widgets.btn_process.set_callback({
        let mut output = output.clone();
        let state = state.clone();
        move |_| {
            let dir = input_path.value();
            let path = PathBuf::from(dir.trim());
            if !path.exists() || !path.is_dir() {
                output.set_label("Invalid directory.");
                return;
            }

            // exif 처리 함수 호출 가능

            output.set_label("Processing complete.");
        }
    });
}
