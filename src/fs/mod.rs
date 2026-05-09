use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct FileSystemEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub is_dir: bool,
}

pub fn read_directory(path: &Path) -> Result<Vec<FileSystemEntry>, String> {
    let read_dir = std::fs::read_dir(path).map_err(|err| err.to_string())?;
    let mut entries: Vec<FileSystemEntry> = read_dir
        .filter_map(|res| res.ok())
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            Some(FileSystemEntry {
                path: entry.path(),
                size: meta.len(),
                modified: meta.modified().ok(),
                created: meta.created().ok(),
                is_dir: meta.is_dir(),
            })
        })
        .collect();

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.path.cmp(&b.path)));
    Ok(entries)
}

pub fn rename_entry(current_path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("File name cannot be empty.".to_string());
    }

    if new_name.contains('\\') || new_name.contains('/') {
        return Err("File name cannot contain path separators.".to_string());
    }

    let parent = current_path
        .parent()
        .ok_or_else(|| "Selected file parent could not be resolved.".to_string())?;
    let new_path = parent.join(new_name);

    if new_path != current_path && new_path.exists() {
        return Err("A file with that name already exists.".to_string());
    }

    std::fs::rename(current_path, &new_path).map_err(|err| err.to_string())?;
    Ok(new_path)
}

pub fn save_file_copy(source_path: &Path) -> Result<PathBuf, String> {
    if !source_path.is_file() {
        return Err("Select a file before saving a copy.".to_string());
    }

    let parent = source_path
        .parent()
        .ok_or_else(|| "Selected file parent could not be resolved.".to_string())?;
    let stem = source_path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "copy".to_string());
    let extension = source_path
        .extension()
        .map(|value| value.to_string_lossy().to_string());

    for index in 1..1000 {
        let suffix = if index == 1 {
            "_copy".to_string()
        } else {
            format!("_copy_{index}")
        };
        let file_name = match &extension {
            Some(extension) => format!("{stem}{suffix}.{extension}"),
            None => format!("{stem}{suffix}"),
        };
        let target_path = parent.join(file_name);

        if !target_path.exists() {
            std::fs::copy(source_path, &target_path).map_err(|err| err.to_string())?;
            return Ok(target_path);
        }
    }

    Err("Could not find an available copy filename.".to_string())
}

pub fn move_file_to_recycle_bin(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("Select a file before deleting.".to_string());
    }

    move_path_to_recycle_bin(path)
}

#[cfg(windows)]
fn move_path_to_recycle_bin(path: &Path) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    const FO_DELETE: u32 = 0x0003;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_NOERRORUI: u16 = 0x0400;

    #[repr(C)]
    struct ShFileOpStructW {
        hwnd: *mut c_void,
        w_func: u32,
        p_from: *const u16,
        p_to: *const u16,
        f_flags: u16,
        f_any_operations_aborted: i32,
        h_name_mappings: *mut c_void,
        lpsz_progress_title: *const u16,
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHFileOperationW(file_op: *mut ShFileOpStructW) -> i32;
    }

    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut operation = ShFileOpStructW {
        hwnd: std::ptr::null_mut(),
        w_func: FO_DELETE,
        p_from: from.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };

    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(format!("Failed to move file to Recycle Bin. Error code: {result}"));
    }
    if operation.f_any_operations_aborted != 0 {
        return Err("Delete operation was canceled.".to_string());
    }

    Ok(())
}

#[cfg(not(windows))]
fn move_path_to_recycle_bin(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|err| err.to_string())
}
