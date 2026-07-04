use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
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

pub fn open_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("Selected path does not exist.".to_string());
    }

    open_in_file_manager_impl(path)
}

pub fn set_file_times(
    path: &Path,
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> Result<(), String> {
    if created.is_none() && modified.is_none() {
        return Ok(());
    }

    set_file_times_impl(path, created, modified)
}

#[cfg(windows)]
fn set_file_times_impl(
    path: &Path,
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::prelude::AsRawHandle;

    const EPOCH_DIFFERENCE_SECONDS: u64 = 11644473600;

    #[repr(C)]
    struct FileTime {
        dw_low_date_time: u32,
        dw_high_date_time: u32,
    }

    extern "system" {
        fn SetFileTime(
            hFile: *mut c_void,
            lpCreationTime: *const FileTime,
            lpLastAccessTime: *const FileTime,
            lpLastWriteTime: *const FileTime,
        ) -> i32;
    }

    fn system_time_to_filetime(time: SystemTime) -> Result<FileTime, String> {
        let duration = time
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| err.to_string())?;
        let intervals = duration
            .as_secs()
            .checked_add(EPOCH_DIFFERENCE_SECONDS)
            .ok_or_else(|| "Time value out of range.".to_string())?
            .checked_mul(10_000_000)
            .ok_or_else(|| "Time value out of range.".to_string())?
            .checked_add((duration.subsec_nanos() / 100) as u64)
            .ok_or_else(|| "Time value out of range.".to_string())?;
        Ok(FileTime {
            dw_low_date_time: intervals as u32,
            dw_high_date_time: (intervals >> 32) as u32,
        })
    }

    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    let handle = file.as_raw_handle() as *mut c_void;

    let creation_ptr = created
        .map(system_time_to_filetime)
        .transpose()
        .map_err(|err| err.to_string())?
        .as_ref()
        .map_or(std::ptr::null(), |t| t as *const FileTime);

    let modified_ptr = modified
        .map(system_time_to_filetime)
        .transpose()
        .map_err(|err| err.to_string())?
        .as_ref()
        .map_or(std::ptr::null(), |t| t as *const FileTime);

    let result = unsafe { SetFileTime(handle, creation_ptr, std::ptr::null(), modified_ptr) };
    if result == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }

    Ok(())
}

#[cfg(not(windows))]
fn set_file_times_impl(
    path: &Path,
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> Result<(), String> {
    use filetime::{set_file_times, FileTime};

    if created.is_some() {
        return Err("Setting created time is not supported on this platform.".to_string());
    }

    let metadata = path.metadata().map_err(|err| err.to_string())?;
    let accessed = metadata
        .accessed()
        .unwrap_or_else(|_| SystemTime::now());
    let access_ft = FileTime::from_system_time(accessed);
    let modified_ft = modified
        .map(FileTime::from_system_time)
        .unwrap_or_else(|| {
            FileTime::from_system_time(metadata.modified().unwrap_or_else(|_| SystemTime::now()))
        });
    set_file_times(path, access_ft, modified_ft).map_err(|err| err.to_string())
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

#[cfg(windows)]
fn open_in_file_manager_impl(path: &Path) -> Result<(), String> {
    let path = path.canonicalize().map_err(|err| err.to_string())?;
    let target = if path.is_file() {
        path.parent()
            .ok_or_else(|| "Selected file parent could not be resolved.".to_string())?
            .to_path_buf()
    } else {
        path
    };

    Command::new("explorer.exe")
        .arg(target)
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_in_file_manager_impl(path: &Path) -> Result<(), String> {
    let mut command = Command::new("open");
    if path.is_file() {
        command.arg("-R").arg(path);
    } else {
        command.arg(path);
    }

    command.spawn().map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn open_in_file_manager_impl(path: &Path) -> Result<(), String> {
    let target = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };

    Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(())
}
