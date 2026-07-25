use std::collections::HashSet;
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

    entries.sort_by(|a, b| {
        let a_name = a
            .path
            .file_name()
            .unwrap_or(a.path.as_os_str())
            .to_string_lossy();
        let b_name = b
            .path
            .file_name()
            .unwrap_or(b.path.as_os_str())
            .to_string_lossy();
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
            .then_with(|| a_name.cmp(&b_name))
    });
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

pub fn trailing_number_rename_candidate_count(paths: &[PathBuf]) -> Result<usize, String> {
    Ok(build_trailing_number_rename_plan(paths)?.len())
}

pub fn move_trailing_numbers_to_front(paths: &[PathBuf]) -> Result<usize, String> {
    let plan = build_trailing_number_rename_plan(paths)?;
    if plan.is_empty() {
        return Ok(0);
    }

    let mut staged = Vec::with_capacity(plan.len());
    for (index, (source, target)) in plan.into_iter().enumerate() {
        let directory = source
            .parent()
            .ok_or_else(|| format!("Could not resolve the folder for {}.", source.display()))?;
        let mut attempt = 0usize;
        let temporary = loop {
            let candidate = directory.join(format!(
                ".sh148_exif_file_tool_rename_{}_{}_{}.tmp",
                std::process::id(),
                index,
                attempt
            ));
            if !candidate.exists() {
                break candidate;
            }
            attempt += 1;
        };

        if let Err(err) = std::fs::rename(&source, &temporary) {
            for (previous_source, _, previous_temporary) in staged.iter().rev() {
                let _ = std::fs::rename(previous_temporary, previous_source);
            }
            return Err(format!("Failed to stage {}: {err}", source.display()));
        }
        staged.push((source, target, temporary));
    }

    let mut committed = 0usize;
    for (_, target, temporary) in &staged {
        if let Err(err) = std::fs::rename(temporary, target) {
            for (_, committed_target, committed_temporary) in staged[..committed].iter().rev() {
                let _ = std::fs::rename(committed_target, committed_temporary);
            }
            for (source, _, staged_temporary) in staged.iter().rev() {
                if staged_temporary.exists() {
                    let _ = std::fs::rename(staged_temporary, source);
                }
            }
            return Err(format!("Failed to rename to {}: {err}", target.display()));
        }
        committed += 1;
    }

    Ok(staged.len())
}

fn build_trailing_number_rename_plan(paths: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut plan = Vec::new();
    for source in paths {
        if !source.is_file() {
            continue;
        }
        let Some(target_name) = move_trailing_number_to_front_name(&source) else {
            continue;
        };
        let directory = source
            .parent()
            .ok_or_else(|| format!("Could not resolve the folder for {}.", source.display()))?;
        plan.push((source.clone(), directory.join(target_name)));
    }

    plan.sort_by(|left, right| left.0.cmp(&right.0));
    let source_keys: HashSet<String> = plan
        .iter()
        .map(|(source, _)| comparable_path_key(source))
        .collect();
    let mut target_keys = HashSet::new();
    for (_, target) in &plan {
        let target_key = comparable_path_key(target);
        if !target_keys.insert(target_key.clone()) {
            return Err(format!("Multiple files would become {}.", target.display()));
        }
        if target.exists() && !source_keys.contains(&target_key) {
            return Err(format!("A target file already exists: {}", target.display()));
        }
    }

    Ok(plan)
}

fn move_trailing_number_to_front_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let extension = path.extension().and_then(|value| value.to_str());
    let digit_start = stem
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .map(|(index, _)| index)
        .last()?;
    let number = &stem[digit_start..];
    if !matches!(number.len(), 2 | 3) {
        return None;
    }

    let before_number = &stem[..digit_start];
    let separator = before_number.chars().next_back()?;
    if !matches!(separator, '_' | '-' | ' ') {
        return None;
    }
    let base_end = before_number.len() - separator.len_utf8();
    let base = before_number[..base_end].trim_end_matches(['_', '-', ' ']);
    if base.is_empty() {
        return None;
    }

    Some(match extension {
        Some(extension) => format!("{number}_{base}.{extension}"),
        None => format!("{number}_{base}"),
    })
}

fn comparable_path_key(path: &Path) -> String {
    let value = path.to_string_lossy().to_string();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
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
            copy_file_times(source_path, &target_path)?;
            return Ok(target_path);
        }
    }

    Err("Could not find an available copy filename.".to_string())
}

pub fn duplicate_file(source_path: &Path) -> Result<PathBuf, String> {
    if !source_path.is_file() {
        return Err("Select a file before duplicating.".to_string());
    }

    let parent = source_path
        .parent()
        .ok_or_else(|| "Selected file parent could not be resolved.".to_string())?;
    copy_file_to_folder(source_path, parent)
}

pub fn copy_file_to_folder(source_path: &Path, target_dir: &Path) -> Result<PathBuf, String> {
    if !source_path.is_file() {
        return Err("Select files only.".to_string());
    }
    if !target_dir.is_dir() {
        return Err("Paste target folder could not be resolved.".to_string());
    }

    let stem = source_path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "copy".to_string());
    let same_folder = source_path
        .parent()
        .map(|parent| parent == target_dir)
        .unwrap_or(false);
    let stem = if same_folder {
        duplicate_base_stem(&stem)
    } else {
        stem
    };
    let extension = source_path
        .extension()
        .map(|value| value.to_string_lossy().to_string());

    if !same_folder {
        let target_path = target_dir.join(match &extension {
            Some(extension) => format!("{stem}.{extension}"),
            None => stem.clone(),
        });
        if !target_path.exists() {
            std::fs::copy(source_path, &target_path).map_err(|err| err.to_string())?;
            copy_file_times(source_path, &target_path)?;
            return Ok(target_path);
        }
    }

    for index in 1..1000 {
        let file_name = match &extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let target_path = target_dir.join(file_name);

        if !target_path.exists() {
            std::fs::copy(source_path, &target_path).map_err(|err| err.to_string())?;
            copy_file_times(source_path, &target_path)?;
            return Ok(target_path);
        }
    }

    Err("Could not find an available duplicate filename.".to_string())
}

pub fn copy_file_times(source_path: &Path, target_path: &Path) -> Result<(), String> {
    let metadata = source_path.metadata().map_err(|err| err.to_string())?;
    let created = metadata.created().ok();
    let modified = metadata.modified().ok();

    set_file_times(target_path, created, modified)
}

fn duplicate_base_stem(stem: &str) -> String {
    let trimmed = stem.trim_end();
    let Some(number_end) = trimmed.strip_suffix(')') else {
        return stem.to_string();
    };
    let Some(open_index) = number_end.rfind(" (") else {
        return stem.to_string();
    };

    if number_end[open_index + 2..].parse::<u32>().is_ok() {
        number_end[..open_index].to_string()
    } else {
        stem.to_string()
    }
}

pub fn move_file_to_recycle_bin(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("Selected path does not exist.".to_string());
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
    use std::os::windows::ffi::OsStrExt;

    const EPOCH_DIFFERENCE_SECONDS: u64 = 11644473600;
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    const FILE_SHARE_READ: u32 = 0x00000001;
    const FILE_SHARE_WRITE: u32 = 0x00000002;
    const FILE_SHARE_DELETE: u32 = 0x00000004;
    const OPEN_EXISTING: u32 = 3;
    const INVALID_HANDLE_VALUE: isize = -1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x00000080;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;

    #[repr(C)]
    struct FileBasicInfo {
        creation_time: i64,
        last_access_time: i64,
        last_write_time: i64,
        change_time: i64,
        file_attributes: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *mut c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: *mut c_void,
        ) -> *mut c_void;
        fn SetFileInformationByHandle(
            hFile: *mut c_void,
            FileInformationClass: i32,
            lpFileInformation: *const c_void,
            dwBufferSize: u32,
        ) -> i32;
        fn CloseHandle(hObject: *mut c_void) -> i32;
    }

    fn system_time_to_filetime_intervals(time: SystemTime) -> Result<i64, String> {
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
        i64::try_from(intervals).map_err(|_| "Time value out of range.".to_string())
    }

    let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide_path.push(0);
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }

    let creation_time = created
        .map(system_time_to_filetime_intervals)
        .transpose()
        .map_err(|err| err.to_string())?;
    let modified_time = modified
        .map(system_time_to_filetime_intervals)
        .transpose()
        .map_err(|err| err.to_string())?;

    let info = FileBasicInfo {
        creation_time: creation_time.unwrap_or(0),
        last_access_time: 0,
        last_write_time: modified_time.unwrap_or(0),
        change_time: 0,
        file_attributes: 0,
    };

    let result = unsafe {
        SetFileInformationByHandle(
            handle,
            0,
            &info as *const FileBasicInfo as *const c_void,
            std::mem::size_of::<FileBasicInfo>() as u32,
        )
    };
    let close_result = unsafe { CloseHandle(handle) };
    if result == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if close_result == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }

    if !file_times_match(path, created, modified) {
        set_file_times_with_powershell(path, created, modified)?;
    }

    Ok(())
}

#[cfg(windows)]
fn file_times_match(
    path: &Path,
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if let Some(expected) = created {
        if metadata.created().ok() != Some(expected) {
            return false;
        }
    }
    if let Some(expected) = modified {
        if metadata.modified().ok() != Some(expected) {
            return false;
        }
    }
    true
}

#[cfg(windows)]
fn set_file_times_with_powershell(
    path: &Path,
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    fn ticks(time: Option<SystemTime>) -> Result<String, String> {
        match time {
            Some(time) => system_time_to_filetime_intervals_for_shell(time).map(|value| value.to_string()),
            None => Ok(String::new()),
        }
    }

    fn system_time_to_filetime_intervals_for_shell(time: SystemTime) -> Result<i64, String> {
        const EPOCH_DIFFERENCE_SECONDS: u64 = 11644473600;
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
        i64::try_from(intervals).map_err(|_| "Time value out of range.".to_string())
    }

    let script = r#"
param([string]$Path, [string]$CreatedTicks, [string]$ModifiedTicks)
if ($CreatedTicks -ne '') {
    [System.IO.File]::SetCreationTimeUtc($Path, [DateTime]::FromFileTimeUtc([Int64]$CreatedTicks))
}
if ($ModifiedTicks -ne '') {
    [System.IO.File]::SetLastWriteTimeUtc($Path, [DateTime]::FromFileTimeUtc([Int64]$ModifiedTicks))
}
"#;

    let status = Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .arg(path)
        .arg(ticks(created)?)
        .arg(ticks(modified)?)
        .status()
        .map_err(|err| err.to_string())?;

    if !status.success() {
        return Err(format!("Failed to set file time via PowerShell. Exit code: {status}"));
    }
    if !file_times_match(path, created, modified) {
        return Err("File time update did not take effect.".to_string());
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
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|err| err.to_string())
    } else {
        std::fs::remove_file(path).map_err(|err| err.to_string())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_sort_is_case_insensitive_with_folders_first() {
        let directory = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_case_sort_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::create_dir(directory.join("Album")).unwrap();
        std::fs::create_dir(directory.join("albums")).unwrap();
        std::fs::create_dir(directory.join("Video")).unwrap();
        std::fs::write(directory.join("A-file.jpg"), b"a").unwrap();

        let entries = read_directory(&directory).unwrap();
        let names: Vec<_> = entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert_eq!(names, ["Album", "albums", "Video", "A-file.jpg"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn moves_only_delimited_two_or_three_digit_trailing_numbers() {
        assert_eq!(
            move_trailing_number_to_front_name(Path::new("xxxx_xxxx_001.jpg")).as_deref(),
            Some("001_xxxx_xxxx.jpg")
        );
        assert_eq!(
            move_trailing_number_to_front_name(Path::new("xxxx-acbd-25.png")).as_deref(),
            Some("25_xxxx-acbd.png")
        );
        assert_eq!(move_trailing_number_to_front_name(Path::new("20140809_131712.jpg")), None);
        assert_eq!(move_trailing_number_to_front_name(Path::new("photo_1.jpg")), None);
        assert_eq!(move_trailing_number_to_front_name(Path::new("photo_1000.jpg")), None);
        assert_eq!(move_trailing_number_to_front_name(Path::new("photo123.jpg")), None);
    }

    #[test]
    fn renames_selected_matching_files_only() {
        let directory = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_trailing_rename_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("xxxx_xxxx_001.jpg"), b"one").unwrap();
        std::fs::write(directory.join("xxxx_acbd_25.jpg"), b"two").unwrap();
        std::fs::write(directory.join("20140809_131712.jpg"), b"date").unwrap();

        let selected = vec![
            directory.join("xxxx_xxxx_001.jpg"),
            directory.join("20140809_131712.jpg"),
        ];
        assert_eq!(trailing_number_rename_candidate_count(&selected).unwrap(), 1);
        assert_eq!(move_trailing_numbers_to_front(&selected).unwrap(), 1);
        assert!(directory.join("001_xxxx_xxxx.jpg").is_file());
        assert!(directory.join("xxxx_acbd_25.jpg").is_file());
        assert!(directory.join("20140809_131712.jpg").is_file());

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn duplicate_preserves_file_times() {
        let dir = std::env::temp_dir();
        let source_path = dir.join(format!(
            "sh148_exif_file_tool_duplicate_created_source_{}.jpg",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&source_path);
        std::fs::write(&source_path, b"test").unwrap();

        let created = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_100_000);
        set_file_times(&source_path, Some(created), Some(modified)).unwrap();
        assert_eq!(source_path.metadata().unwrap().created().unwrap(), created);
        assert_eq!(source_path.metadata().unwrap().modified().unwrap(), modified);

        let target_path = duplicate_file(&source_path).unwrap();
        let target_metadata = target_path.metadata().unwrap();

        assert_eq!(target_metadata.created().unwrap(), created);
        assert_eq!(target_metadata.modified().unwrap(), modified);

        let _ = std::fs::remove_file(source_path);
        let _ = std::fs::remove_file(target_path);
    }
}
