use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static ATOMIC_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Writes a replacement beside the destination and atomically swaps it into
/// place. The original remains untouched if writing, flushing, or replacing
/// the temporary file fails.
pub fn atomic_write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Could not resolve the file's parent folder.".to_string())?;
    let name = path
        .file_name()
        .ok_or_else(|| "Could not resolve the filename.".to_string())?
        .to_string_lossy();
    let permissions = std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());

    let temporary = (0..1000)
        .find_map(|_| {
            let sequence = ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".{name}.sh148-write-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => Some(Ok((candidate, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error.to_string())),
            }
        })
        .transpose()?
        .ok_or_else(|| "Could not allocate a temporary file for the update.".to_string())?;
    let (temporary_path, mut temporary_file) = temporary;

    let write_result = (|| {
        temporary_file
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
        temporary_file
            .sync_all()
            .map_err(|error| error.to_string())?;
        drop(temporary_file);
        if let Some(permissions) = permissions {
            std::fs::set_permissions(&temporary_path, permissions)
                .map_err(|error| error.to_string())?;
        }
        if path.exists() {
            replace_file_atomically(path, &temporary_path)
        } else {
            std::fs::rename(&temporary_path, path).map_err(|error| error.to_string())
        }
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    write_result
}

#[cfg(windows)]
fn replace_file_atomically(path: &Path, replacement: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }

    const REPLACEFILE_IGNORE_MERGE_ERRORS: u32 = 0x00000002;
    let replaced: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let replacement: Vec<u16> = replacement
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both paths are valid, null-terminated UTF-16 buffers that remain
    // alive until this synchronous call returns. Optional pointers are null.
    let result = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_atomically(path: &Path, replacement: &Path) -> Result<(), String> {
    std::fs::rename(replacement, path).map_err(|error| error.to_string())
}

#[derive(Clone, Debug)]
pub struct FileSystemEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub is_dir: bool,
}

#[derive(Default)]
pub struct FilenameCollisionResolver {
    reserved: HashSet<String>,
}

impl FilenameCollisionResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve_for_create(&mut self, desired_path: &Path) -> Result<PathBuf, String> {
        self.resolve(desired_path, None)
    }

    pub fn resolve_for_rename(
        &mut self,
        desired_path: &Path,
        current_path: &Path,
    ) -> Result<PathBuf, String> {
        self.resolve(desired_path, Some(current_path))
    }

    fn resolve(
        &mut self,
        desired_path: &Path,
        allowed_existing_path: Option<&Path>,
    ) -> Result<PathBuf, String> {
        let parent = desired_path
            .parent()
            .ok_or_else(|| "Could not resolve the target folder.".to_string())?;
        let stem = desired_path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Could not resolve the target filename.".to_string())?;
        let extension = desired_path.extension().and_then(|value| value.to_str());

        for index in 0..1000 {
            let candidate_stem = if index == 0 {
                stem.to_string()
            } else {
                format!("{stem}_dup{index:03}")
            };
            let candidate = parent.join(match extension {
                Some(extension) if !extension.is_empty() => {
                    format!("{candidate_stem}.{extension}")
                }
                _ => candidate_stem,
            });
            let candidate_key = comparable_path_key(&candidate);
            let is_allowed_existing = allowed_existing_path
                .is_some_and(|allowed| comparable_path_key(allowed) == candidate_key);
            if (!candidate.exists() || is_allowed_existing)
                && !self.reserved.contains(&candidate_key)
            {
                self.reserved.insert(candidate_key);
                return Ok(candidate);
            }
        }

        Err(format!(
            "Could not find an available filename based on {}.",
            desired_path.display()
        ))
    }
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
    apply_atomic_rename_plan(plan)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrontRearNumberRemovalStats {
    pub renameable: usize,
    pub unmatched: usize,
    pub deduplicated: usize,
}

pub fn analyze_front_or_rear_number_removal(
    paths: &[PathBuf],
) -> Result<FrontRearNumberRemovalStats, String> {
    let removal_plan = build_front_or_rear_number_removal_plan(paths)?;
    Ok(removal_plan.stats())
}

pub fn remove_front_or_rear_numbers(
    paths: &[PathBuf],
) -> Result<FrontRearNumberRemovalStats, String> {
    let removal_plan = build_front_or_rear_number_removal_plan(paths)?;
    let unmatched = removal_plan.unmatched;
    let deduplicated = removal_plan.deduplicated;
    let renamed = apply_atomic_rename_plan(removal_plan.renames)?;
    Ok(FrontRearNumberRemovalStats {
        renameable: renamed,
        unmatched,
        deduplicated,
    })
}

pub type MediaPrefixRemovalStats = FrontRearNumberRemovalStats;

pub fn analyze_img_vid_prefix_removal(
    paths: &[PathBuf],
) -> Result<MediaPrefixRemovalStats, String> {
    let removal_plan = build_img_vid_prefix_removal_plan(paths)?;
    Ok(removal_plan.stats())
}

pub fn remove_img_vid_prefixes(paths: &[PathBuf]) -> Result<MediaPrefixRemovalStats, String> {
    let removal_plan = build_img_vid_prefix_removal_plan(paths)?;
    let unmatched = removal_plan.unmatched;
    let deduplicated = removal_plan.deduplicated;
    let renamed = apply_atomic_rename_plan(removal_plan.renames)?;
    Ok(MediaPrefixRemovalStats {
        renameable: renamed,
        unmatched,
        deduplicated,
    })
}

fn apply_atomic_rename_plan(plan: Vec<(PathBuf, PathBuf)>) -> Result<usize, String> {
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

    for (committed, (_, target, temporary)) in staged.iter().enumerate() {
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
    }

    Ok(staged.len())
}

fn build_front_or_rear_number_removal_plan(
    paths: &[PathBuf],
) -> Result<FrontRearNumberRemovalPlan, String> {
    let mut candidates = Vec::new();
    let mut unmatched = 0usize;
    for source in paths {
        if !source.is_file() {
            continue;
        }
        let Some(target_name) = remove_front_or_rear_number_name(source) else {
            unmatched += 1;
            continue;
        };
        let directory = source
            .parent()
            .ok_or_else(|| format!("Could not resolve the folder for {}.", source.display()))?;
        candidates.push((source.clone(), directory.join(target_name)));
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    let mut renames = Vec::new();
    let mut resolver = FilenameCollisionResolver::new();
    let mut deduplicated = 0usize;
    for (source, desired_target) in candidates {
        let target = resolver.resolve_for_create(&desired_target)?;
        if target != desired_target {
            deduplicated += 1;
        }
        renames.push((source, target));
    }

    Ok(FrontRearNumberRemovalPlan {
        renames,
        unmatched,
        deduplicated,
    })
}

fn build_img_vid_prefix_removal_plan(
    paths: &[PathBuf],
) -> Result<FrontRearNumberRemovalPlan, String> {
    let mut candidates = Vec::new();
    let mut unmatched = 0usize;
    for source in paths {
        if !source.is_file() {
            continue;
        }
        let Some(target_name) = remove_img_vid_prefix_name(source) else {
            unmatched += 1;
            continue;
        };
        let directory = source
            .parent()
            .ok_or_else(|| format!("Could not resolve the folder for {}.", source.display()))?;
        candidates.push((source.clone(), directory.join(target_name)));
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    let mut renames = Vec::new();
    let mut resolver = FilenameCollisionResolver::new();
    let mut deduplicated = 0usize;
    for (source, desired_target) in candidates {
        let target = resolver.resolve_for_create(&desired_target)?;
        if target != desired_target {
            deduplicated += 1;
        }
        renames.push((source, target));
    }

    Ok(FrontRearNumberRemovalPlan {
        renames,
        unmatched,
        deduplicated,
    })
}

struct FrontRearNumberRemovalPlan {
    renames: Vec<(PathBuf, PathBuf)>,
    unmatched: usize,
    deduplicated: usize,
}

impl FrontRearNumberRemovalPlan {
    fn stats(&self) -> FrontRearNumberRemovalStats {
        FrontRearNumberRemovalStats {
            renameable: self.renames.len(),
            unmatched: self.unmatched,
            deduplicated: self.deduplicated,
        }
    }
}

fn build_trailing_number_rename_plan(paths: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut plan = Vec::new();
    for source in paths {
        if !source.is_file() {
            continue;
        }
        let Some(target_name) = move_trailing_number_to_front_name(source) else {
            continue;
        };
        let directory = source
            .parent()
            .ok_or_else(|| format!("Could not resolve the folder for {}.", source.display()))?;
        plan.push((source.clone(), directory.join(target_name)));
    }

    validate_rename_plan(plan)
}

fn validate_rename_plan(
    mut plan: Vec<(PathBuf, PathBuf)>,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
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
            return Err(format!(
                "A target file already exists: {}",
                target.display()
            ));
        }
    }

    Ok(plan)
}

fn remove_front_or_rear_number_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let extension = path.extension().and_then(|value| value.to_str());
    let mut base = stem;
    let mut changed = false;

    if let Some((number, remainder)) = base.split_once('_') {
        if matches!(number.len(), 2 | 3) && number.chars().all(|ch| ch.is_ascii_digit()) {
            base = remainder;
            changed = true;
        }
    }

    if let Some((remainder, number)) = base.rsplit_once('_') {
        if matches!(number.len(), 2 | 3) && number.chars().all(|ch| ch.is_ascii_digit()) {
            base = remainder;
            changed = true;
        }
    }

    if !changed || base.is_empty() {
        return None;
    }

    Some(match extension {
        Some(extension) => format!("{base}.{extension}"),
        None => base.to_string(),
    })
}

fn remove_img_vid_prefix_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let prefix = stem.get(..4)?;
    if !prefix.eq_ignore_ascii_case("IMG_") && !prefix.eq_ignore_ascii_case("VID_") {
        return None;
    }
    let base = stem.get(4..)?;
    if base.is_empty() {
        return None;
    }
    Some(match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => format!("{base}.{extension}"),
        None => base.to_string(),
    })
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
    let base_stem = duplicate_copy_base_stem(&stem);
    let desired = path_with_stem_and_extension(
        parent,
        &format!("{base_stem}_copy"),
        source_path.extension().and_then(|value| value.to_str()),
    );
    let target_path = FilenameCollisionResolver::new().resolve_for_create(&desired)?;
    std::fs::copy(source_path, &target_path).map_err(|err| err.to_string())?;
    copy_file_times(source_path, &target_path)?;
    Ok(target_path)
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
    let desired_stem = if same_folder {
        format!("{}_copy", duplicate_copy_base_stem(&stem))
    } else {
        stem
    };
    let desired = path_with_stem_and_extension(
        target_dir,
        &desired_stem,
        source_path.extension().and_then(|value| value.to_str()),
    );
    let target_path = FilenameCollisionResolver::new().resolve_for_create(&desired)?;
    std::fs::copy(source_path, &target_path).map_err(|err| err.to_string())?;
    copy_file_times(source_path, &target_path)?;
    Ok(target_path)
}

pub fn copy_file_times(source_path: &Path, target_path: &Path) -> Result<(), String> {
    let metadata = source_path.metadata().map_err(|err| err.to_string())?;
    let created = metadata.created().ok();
    let modified = metadata.modified().ok();

    set_file_times(target_path, created, modified)
}

fn duplicate_copy_base_stem(stem: &str) -> String {
    let without_duplicate_suffix = stem
        .rsplit_once("_dup")
        .filter(|(_, number)| number.len() == 3 && number.chars().all(|ch| ch.is_ascii_digit()))
        .map(|(base, _)| base)
        .unwrap_or(stem);
    without_duplicate_suffix
        .strip_suffix("_copy")
        .unwrap_or(without_duplicate_suffix)
        .to_string()
}

fn path_with_stem_and_extension(parent: &Path, stem: &str, extension: Option<&str>) -> PathBuf {
    parent.join(match extension {
        Some(extension) if !extension.is_empty() => format!("{stem}.{extension}"),
        _ => stem.to_string(),
    })
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

pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("Selected path does not exist.".to_string());
    }

    reveal_in_file_manager_impl(path)
}

pub fn open_with_default_application(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("Selected path does not exist.".to_string());
    }

    open_with_default_application_impl(path)
}

pub fn choose_folder() -> Result<Option<PathBuf>, String> {
    choose_folder_impl()
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
            Some(time) => {
                system_time_to_filetime_intervals_for_shell(time).map(|value| value.to_string())
            }
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
        return Err(format!(
            "Failed to set file time via PowerShell. Exit code: {status}"
        ));
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
    let accessed = metadata.accessed().unwrap_or_else(|_| SystemTime::now());
    let access_ft = FileTime::from_system_time(accessed);
    let modified_ft = modified.map(FileTime::from_system_time).unwrap_or_else(|| {
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
        return Err(format!(
            "Failed to move file to Recycle Bin. Error code: {result}"
        ));
    }
    if operation.f_any_operations_aborted != 0 {
        return Err("Delete operation was canceled.".to_string());
    }

    Ok(())
}

#[cfg(not(windows))]
fn move_path_to_recycle_bin(_path: &Path) -> Result<(), String> {
    Err("Moving items to the system Trash is not implemented on this platform; no file was deleted."
        .to_string())
}

#[cfg(windows)]
fn open_in_file_manager_impl(path: &Path) -> Result<(), String> {
    let path = explorer_compatible_path(path)?;
    let target = if path.is_file() {
        path.parent()
            .ok_or_else(|| "Selected file parent could not be resolved.".to_string())?
            .to_path_buf()
    } else {
        path
    };

    shell_execute_windows(target.as_os_str(), None)
}

#[cfg(windows)]
fn choose_folder_impl() -> Result<Option<PathBuf>, String> {
    use std::ffi::{c_void, OsStr, OsString};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;

    #[repr(C)]
    struct BrowseInfoW {
        owner: *mut c_void,
        root: *const c_void,
        display_name: *mut u16,
        title: *const u16,
        flags: u32,
        callback: *const c_void,
        callback_data: isize,
        image: i32,
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHBrowseForFolderW(info: *const BrowseInfoW) -> *mut c_void;
        fn SHGetPathFromIDListW(item: *const c_void, path: *mut u16) -> i32;
    }

    #[link(name = "ole32")]
    extern "system" {
        fn OleInitialize(reserved: *mut c_void) -> i32;
        fn OleUninitialize();
        fn CoTaskMemFree(memory: *mut c_void);
    }

    const BIF_RETURNONLYFSDIRS: u32 = 0x0001;
    const BIF_NEWDIALOGSTYLE: u32 = 0x0040;

    let title: Vec<u16> = OsStr::new("Select a folder")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut display_name = [0u16; 260];
    let initialization = unsafe { OleInitialize(ptr::null_mut()) };
    let info = BrowseInfoW {
        owner: ptr::null_mut(),
        root: ptr::null(),
        display_name: display_name.as_mut_ptr(),
        title: title.as_ptr(),
        flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        callback: ptr::null(),
        callback_data: 0,
        image: 0,
    };

    let item = unsafe { SHBrowseForFolderW(&info) };
    let result = if item.is_null() {
        Ok(None)
    } else {
        let mut path = [0u16; 260];
        let converted = unsafe { SHGetPathFromIDListW(item, path.as_mut_ptr()) };
        unsafe { CoTaskMemFree(item) };
        if converted == 0 {
            Err("Windows could not resolve the selected folder.".to_string())
        } else {
            let length = path
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(path.len());
            Ok(Some(PathBuf::from(OsString::from_wide(&path[..length]))))
        }
    };

    if initialization >= 0 {
        unsafe { OleUninitialize() };
    }
    result
}

#[cfg(not(windows))]
fn choose_folder_impl() -> Result<Option<PathBuf>, String> {
    Err("Folder selection is currently supported on Windows only.".to_string())
}

#[cfg(windows)]
fn reveal_in_file_manager_impl(path: &Path) -> Result<(), String> {
    let path = explorer_compatible_path(path)?;
    if path.is_dir() {
        return open_in_file_manager_impl(&path);
    }

    let mut parameters = std::ffi::OsString::from("/select,\"");
    parameters.push(path.as_os_str());
    parameters.push("\"");
    shell_execute_windows(
        std::ffi::OsStr::new("explorer.exe"),
        Some(parameters.as_os_str()),
    )
}

#[cfg(windows)]
fn explorer_compatible_path(path: &Path) -> Result<PathBuf, String> {
    // std::fs::canonicalize() produces a verbatim `\\?\` path on Windows.
    // Explorer's command line does not reliably accept that form, so retain
    // the normal absolute drive/UNC spelling used by the application.
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|err| err.to_string())
    }
}

#[cfg(windows)]
fn shell_execute_windows(
    file: &std::ffi::OsStr,
    parameters: Option<&std::ffi::OsStr>,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_command: i32,
        ) -> *mut std::ffi::c_void;
    }

    let operation: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let file: Vec<u16> = file.encode_wide().chain(std::iter::once(0)).collect();
    let parameters: Option<Vec<u16>> =
        parameters.map(|value| value.encode_wide().chain(std::iter::once(0)).collect());
    let parameter_pointer = parameters
        .as_ref()
        .map_or(ptr::null(), |value| value.as_ptr());

    // SAFETY: all pointers refer to null-terminated UTF-16 buffers that remain
    // alive for the duration of this synchronous ShellExecuteW call.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameter_pointer,
            ptr::null(),
            1,
        )
    } as isize;

    if result <= 32 {
        Err(format!(
            "Windows Shell failed to open the selected item (code {result})."
        ))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn open_with_default_application_impl(path: &Path) -> Result<(), String> {
    let path = explorer_compatible_path(path)?;
    shell_execute_windows(path.as_os_str(), None)
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

#[cfg(target_os = "macos")]
fn reveal_in_file_manager_impl(path: &Path) -> Result<(), String> {
    let mut command = Command::new("open");
    if path.is_file() {
        command.arg("-R").arg(path);
    } else {
        command.arg(path);
    }

    command.spawn().map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_with_default_application_impl(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|err| err.to_string())?;
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

#[cfg(all(not(windows), not(target_os = "macos")))]
fn reveal_in_file_manager_impl(path: &Path) -> Result<(), String> {
    open_in_file_manager_impl(path)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn open_with_default_application_impl(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_entry_renames_a_directory_immediately() {
        let parent = std::env::temp_dir().join(format!(
            "sh148-folder-rename-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let original = parent.join("2018.06");
        std::fs::create_dir_all(&original).unwrap();

        let renamed = rename_entry(&original, "2018.06 ok").unwrap();

        assert_eq!(renamed, parent.join("2018.06 ok"));
        assert!(!original.exists());
        assert!(renamed.is_dir());
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn atomic_write_replaces_existing_contents_without_leaving_a_temporary_file() {
        let directory = std::env::temp_dir().join(format!(
            "sh148-atomic-write-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("photo.jpg");
        std::fs::write(&path, b"original").unwrap();
        let original_created =
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        set_file_times(&path, Some(original_created), None).unwrap();

        atomic_write_file(&path, b"replacement").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        assert_eq!(
            std::fs::metadata(&path).unwrap().created().unwrap(),
            original_created
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn collision_resolver_uses_stable_dup_suffixes_and_reserves_batch_names() {
        let directory = std::env::temp_dir().join(format!(
            "sh148_collision_resolver_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let desired = directory.join("photo.jpg");
        std::fs::write(&desired, b"existing").unwrap();

        let mut resolver = FilenameCollisionResolver::new();
        assert_eq!(
            resolver.resolve_for_create(&desired).unwrap(),
            directory.join("photo_dup001.jpg")
        );
        assert_eq!(
            resolver.resolve_for_create(&desired).unwrap(),
            directory.join("photo_dup002.jpg")
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

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
        assert_eq!(
            move_trailing_number_to_front_name(Path::new("20140809_131712.jpg")),
            None
        );
        assert_eq!(
            move_trailing_number_to_front_name(Path::new("photo_1.jpg")),
            None
        );
        assert_eq!(
            move_trailing_number_to_front_name(Path::new("photo_1000.jpg")),
            None
        );
        assert_eq!(
            move_trailing_number_to_front_name(Path::new("photo123.jpg")),
            None
        );
    }

    #[test]
    fn removes_only_underscored_two_or_three_digit_front_or_rear_numbers() {
        assert_eq!(
            remove_front_or_rear_number_name(Path::new("photo_001.jpg")).as_deref(),
            Some("photo.jpg")
        );
        assert_eq!(
            remove_front_or_rear_number_name(Path::new("25_photo.png")).as_deref(),
            Some("photo.png")
        );
        assert_eq!(
            remove_front_or_rear_number_name(Path::new("001_photo_25.jpg")).as_deref(),
            Some("photo.jpg")
        );
        assert_eq!(
            remove_front_or_rear_number_name(Path::new("20140809_131712.jpg")),
            None
        );
        assert_eq!(
            remove_front_or_rear_number_name(Path::new("photo_1.jpg")),
            None
        );
        assert_eq!(
            remove_front_or_rear_number_name(Path::new("photo_1000.jpg")),
            None
        );
    }

    #[test]
    fn removes_img_and_vid_prefixes_case_insensitively() {
        assert_eq!(
            remove_img_vid_prefix_name(Path::new("IMG_20150711_143113.jpg")).as_deref(),
            Some("20150711_143113.jpg")
        );
        assert_eq!(
            remove_img_vid_prefix_name(Path::new("vid_20150711_143113.mp4")).as_deref(),
            Some("20150711_143113.mp4")
        );
        assert_eq!(remove_img_vid_prefix_name(Path::new("IMAGE_001.jpg")), None);
        assert_eq!(remove_img_vid_prefix_name(Path::new("IMG_.jpg")), None);
    }

    #[test]
    fn resolves_img_vid_prefix_removal_collisions_with_common_suffixes() {
        let directory = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_prefix_removal_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("photo.jpg"), b"existing").unwrap();
        std::fs::write(directory.join("IMG_photo.jpg"), b"image").unwrap();
        std::fs::write(directory.join("VID_photo.jpg"), b"video").unwrap();

        let selected = vec![
            directory.join("IMG_photo.jpg"),
            directory.join("VID_photo.jpg"),
        ];
        let stats = remove_img_vid_prefixes(&selected).unwrap();
        assert_eq!(stats.renameable, 2);
        assert_eq!(stats.deduplicated, 2);
        assert!(directory.join("photo_dup001.jpg").is_file());
        assert!(directory.join("photo_dup002.jpg").is_file());

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resolves_front_or_rear_number_removal_name_collisions() {
        let directory = std::env::temp_dir().join(format!(
            "sh148_exif_file_tool_number_removal_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("photo_25.jpg"), b"one").unwrap();
        std::fs::write(directory.join("001_photo.jpg"), b"two").unwrap();
        std::fs::write(directory.join("keep_25.jpg"), b"three").unwrap();
        std::fs::write(directory.join("not-numbered.jpg"), b"four").unwrap();

        let selected = vec![
            directory.join("photo_25.jpg"),
            directory.join("001_photo.jpg"),
            directory.join("keep_25.jpg"),
            directory.join("not-numbered.jpg"),
        ];
        let analysis = analyze_front_or_rear_number_removal(&selected).unwrap();
        assert_eq!(analysis.renameable, 3);
        assert_eq!(analysis.unmatched, 1);
        assert_eq!(analysis.deduplicated, 1);

        let result = remove_front_or_rear_numbers(&selected).unwrap();
        assert_eq!(result, analysis);
        assert!(directory.join("photo.jpg").is_file());
        assert!(directory.join("photo_dup001.jpg").is_file());
        assert!(directory.join("keep.jpg").is_file());
        assert!(directory.join("not-numbered.jpg").is_file());

        std::fs::remove_dir_all(directory).unwrap();
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
        assert_eq!(
            trailing_number_rename_candidate_count(&selected).unwrap(),
            1
        );
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
        assert_eq!(
            source_path.metadata().unwrap().modified().unwrap(),
            modified
        );

        let target_path = duplicate_file(&source_path).unwrap();
        let target_metadata = target_path.metadata().unwrap();

        assert_eq!(target_metadata.created().unwrap(), created);
        assert_eq!(target_metadata.modified().unwrap(), modified);

        let _ = std::fs::remove_file(source_path);
        let _ = std::fs::remove_file(target_path);
    }
}
