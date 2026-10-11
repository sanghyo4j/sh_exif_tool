use std::collections::{HashMap, VecDeque};
use std::io::{BufReader, Read};
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Instant, SystemTime};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};

use windows_sys::core::w;
use windows_sys::Win32::Foundation::{
    GlobalFree, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, ClientToScreen, CreateBitmap, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, GdiFlush, GetDC, GetStockObject, InvalidateRect,
    MapWindowPoints, Rectangle, RedrawWindow, ReleaseDC, SelectObject, SetBkMode,
    SetStretchBltMode, SetTextColor, StretchDIBits, UpdateWindow, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DEFAULT_GUI_FONT, DIB_RGB_COLORS, DT_CENTER,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, HALFTONE,
    PAINTSTRUCT, PS_SOLID, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::Controls::{
    ImageList_Add, ImageList_Create, ImageList_Destroy, InitCommonControlsEx, SetWindowTheme,
    ShowScrollBar, BST_CHECKED, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT,
    CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, CDRF_SKIPDEFAULT, EM_GETSEL, EM_REPLACESEL,
    EM_SETSEL, HIMAGELIST, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ILC_COLOR32,
    INITCOMMONCONTROLSEX, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_IMAGE, LVIF_TEXT,
    LVIR_BOUNDS, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS, LVM_EDITLABELW,
    LVM_ENSUREVISIBLE, LVM_GETITEMTEXTW, LVM_GETNEXTITEM, LVM_GETSELECTEDCOUNT, LVM_GETSUBITEMRECT,
    LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_REDRAWITEMS, LVM_SETBKCOLOR, LVM_SETCOLUMNWIDTH,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETIMAGELIST, LVM_SETITEMCOUNT, LVM_SETITEMSTATE,
    LVM_SETITEMTEXTW, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, LVNI_SELECTED, LVN_COLUMNCLICK,
    LVN_ENDLABELEDITW, LVN_GETDISPINFOW, LVN_ITEMCHANGED, LVN_KEYDOWN, LVN_ODFINDITEMW,
    LVSIL_SMALL, LVS_EDITLABELS, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER,
    LVS_OWNERDATA, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, NMHDR, NMITEMACTIVATE, NMLISTVIEW,
    NMLVCUSTOMDRAW, NMLVDISPINFOW, NMLVFINDITEMW, NMLVKEYDOWN, NM_CLICK, NM_CUSTOMDRAW, NM_DBLCLK,
    NM_RCLICK, SBARS_SIZEGRIP, SB_SETPARTS, SB_SETTEXTW, STATUSCLASSNAMEW, TBBUTTON,
    TBSTATE_ENABLED, TBSTYLE_FLAT, TBSTYLE_TOOLTIPS, TB_ADDBUTTONSW, TB_AUTOSIZE,
    TB_BUTTONSTRUCTSIZE, TB_ENABLEBUTTON, TB_SETBITMAPSIZE, TB_SETBUTTONSIZE, TB_SETIMAGELIST,
    TOOLBARCLASSNAMEW,
};
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetCapture, GetFocus, GetKeyState, ReleaseCapture, SetActiveWindow, SetCapture,
    SetFocus, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_F2, VK_R, VK_RETURN, VK_S, VK_UP,
};
use windows_sys::Win32::UI::Shell::{
    SHGetFileInfoW, SHFILEINFOW, SHGFI_SMALLICON, SHGFI_SYSICONINDEX,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CheckMenuRadioItem, CreateIconIndirect, CreateMenu,
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClientRect, GetCursorPos, GetMessageW, GetParent, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, IsWindow,
    IsWindowVisible, LoadCursorW, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage,
    RegisterClassExW, SendMessageW, SetCursor, SetMenu, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, TrackPopupMenu, TranslateMessage, BM_GETCHECK, BS_AUTOCHECKBOX,
    BS_DEFPUSHBUTTON, CBN_EDITCHANGE, CBN_KILLFOCUS, CBN_SELCHANGE, CBS_AUTOHSCROLL, CBS_DROPDOWN,
    CB_ADDSTRING, CB_SETITEMHEIGHT, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, EN_CHANGE, EN_KILLFOCUS,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GWLP_USERDATA, HICON, HMENU,
    ICONINFO, IDC_ARROW, IDC_SIZEWE, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK,
    MB_YESNO, MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MINMAXINFO, MSG,
    SB_HORZ, SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOZORDER, SW_HIDE, SW_SHOW, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY,
    WM_NOTIFY, WM_PAINT, WM_SETFONT, WM_SETICON, WM_SIZE, WNDCLASSEXW, WS_BORDER, WS_CAPTION,
    WS_CHILD, WS_CLIPCHILDREN, WS_EX_DLGMODALFRAME, WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL,
};

use super::model::{FileFilter, FileListModel, SortColumn, TimeDisplayMode};
use crate::exif::{
    copy_exif_metadata, display_datetime_to_exif, exif_backup_path, extract_datetime_from_filename,
    extract_embedded_thumbnail, read_exif_metadata, remove_exif_metadata, remove_exif_tag,
    remove_gps_information, rewrite_basic_exif_metadata, write_aperture, write_artist,
    write_camera_make, write_camera_model, write_color_space, write_flash_fired,
    write_focal_length, write_gps_date_time, write_iso_speed, write_lens_model,
    write_metering_mode, write_orientation, write_shutter_speed, write_software,
    write_taken_date_preserving_exif, write_time_zone_offset_preserving_exif,
};
use crate::fs::{
    choose_folder, copy_file_times, copy_file_to_folder, move_file_to_recycle_bin,
    move_trailing_numbers_to_front, open_with_default_application, remove_front_or_rear_numbers,
    remove_img_vid_prefixes, rename_entry, reveal_in_file_manager, set_file_times,
    trailing_number_rename_candidate_count, FilenameCollisionResolver,
};
use crate::media::{
    is_jpeg_path, is_mp4_path, is_png_path, scan_media_file, write_mp4_media_date,
    write_mp4_media_date_with_offset, write_png_media_date, MediaScanResult,
};

const APP_TITLE: &str = crate::APP_TITLE;
const WINDOW_CLASS: windows_sys::core::PCWSTR = w!("SH148PhotoManagementTool.Win32");
const PREVIEW_CLASS: windows_sys::core::PCWSTR = w!("SH148PhotoPreview.Win32");
const SHIFT_DIALOG_CLASS: windows_sys::core::PCWSTR = w!("SH148ShiftMediaDate.Win32");
const TEXT_EDIT_DIALOG_CLASS: windows_sys::core::PCWSTR = w!("SH148MetadataEdit.Win32");
const ID_FILE_OPEN: usize = 100;
const ID_FILE_REFRESH: usize = 101;
const ID_FILE_EXIT: usize = 102;
const ID_HELP_ABOUT: usize = 103;
const ID_VIEW_SUPPORTED: usize = 120;
const ID_VIEW_IMAGES: usize = 121;
const ID_VIEW_JPEG: usize = 122;
const ID_VIEW_PNG: usize = 123;
const ID_VIEW_VIDEOS: usize = 124;
const ID_VIEW_ALL: usize = 125;
const ID_TOOL_OPEN: i32 = 200;
const ID_TOOL_REFRESH: i32 = 201;
const ID_TOOL_SAVE: i32 = 202;
const ID_TOOL_REVERT: i32 = 203;
const ID_EDIT_TAKEN_DATE: usize = 300;
const ID_EDIT_UTC_OFFSET: usize = 301;
const ID_EDIT_PATH: usize = 302;
const ID_DETAIL_INLINE_EDIT: usize = 303;
const ID_DETAIL_INLINE_OFFSET: usize = 304;
const ID_QUICK_FILE: usize = 305;
const ID_QUICK_METADATA: usize = 306;
const ID_ITEM_OPEN: usize = 400;
const ID_ITEM_REVEAL: usize = 401;
const ID_ITEM_RENAME: usize = 402;
const ID_ITEM_DELETE: usize = 403;
const ID_ITEM_COPY: usize = 404;
const ID_ITEM_PASTE: usize = 405;
const ID_ITEM_SAVE_COPY: usize = 406;
const ID_DATE_FROM_FILENAME: usize = 410;
const ID_DATE_FROM_MODIFIED: usize = 411;
const ID_DATE_FROM_CREATED: usize = 412;
const ID_FILE_DATES_FROM_MEDIA: usize = 413;
const ID_FILENAME_FROM_MEDIA: usize = 414;
const ID_MODIFIED_FROM_CREATED: usize = 415;
const ID_SHIFT_MEDIA_DATE: usize = 416;
const ID_SHIFT_GPS_DATE_TIME: usize = 417;
const ID_RENAME_MOVE_TRAILING: usize = 420;
const ID_RENAME_REMOVE_NUMBERS: usize = 421;
const ID_RENAME_REMOVE_PREFIX: usize = 422;
const ID_EXIF_CREATE: usize = 430;
const ID_EXIF_REMOVE: usize = 431;
const ID_GPS_REMOVE: usize = 432;
const ID_EXTRACT_THUMBNAIL: usize = 433;
const ID_SELECT_ALL: usize = 440;
const ID_SELECT_MISSING_EXIF: usize = 441;
const ID_SELECT_MISSING_OFFSET: usize = 442;
const ID_SELECT_DUPLICATE_DATE: usize = 443;
const ID_SCAN_SINGLE_THREAD: usize = 450;
const ID_SCAN_TWO_THREADS: usize = 451;
const ID_TIME_RECORDED: usize = 452;
const ID_TIME_UTC: usize = 453;
const ID_TIME_KST: usize = 454;
const ID_TIME_PDT: usize = 455;
const ID_TIME_PST: usize = 456;
const ID_BACKUP_BEFORE_CHANGES: usize = 457;
const ID_VIEW_MISSING_DATE: usize = 460;
const ID_VIEW_MISSING_OFFSET: usize = 461;
const ID_VIEW_DUPLICATE_DATE: usize = 462;
const ID_VIEW_SESSION_LOG: usize = 463;
const ID_VIEW_CLEAR_SESSION_LOG: usize = 464;
const ID_EXIF_COPY: usize = 470;
const ID_EXIF_PASTE: usize = 471;
const ID_DETAIL_COPY_VALUE: usize = 472;
const ID_DETAIL_REMOVE_TAG: usize = 473;
const ID_ANALYZE_EXACT_DUPLICATES: usize = 480;
const WM_SCAN_COMPLETE: u32 = WM_APP + 1;
const WM_PREVIEW_COMPLETE: u32 = WM_APP + 2;
const WM_SELECTION_SETTLED: u32 = WM_APP + 3;
const IDYES_VALUE: i32 = 6;
type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

struct ScanCompletion {
    generation: u64,
    path: PathBuf,
    result: MediaScanResult,
}

struct PreviewJob {
    generation: u64,
    path: PathBuf,
}

struct PreviewBitmap {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    elapsed_ms: u128,
}

struct PreviewCompletion {
    generation: u64,
    path: PathBuf,
    bitmap: Option<Arc<PreviewBitmap>>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
struct MediaDateShift {
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    subtract: bool,
}

struct ShiftDialogState {
    edits: [HWND; 4],
    subtract: HWND,
    accepted: bool,
}

struct TextEditDialogState {
    edit: HWND,
    accepted: bool,
    value: String,
}

#[derive(Clone, Copy)]
struct AppSettings {
    scan_threads: usize,
    time_display_mode: TimeDisplayMode,
    backup_before_changes: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            scan_threads: 2,
            time_display_mode: TimeDisplayMode::Recorded,
            backup_before_changes: false,
        }
    }
}

#[derive(Clone, Default)]
struct PendingEdit {
    taken_date: Option<String>,
    utc_offset: Option<String>,
    gps_date: Option<String>,
    gps_time: Option<String>,
    tag_values: HashMap<String, String>,
}

struct WindowState {
    toolbar: HWND,
    path_label: HWND,
    status: HWND,
    list: HWND,
    preview: HWND,
    details: HWND,
    quick_file: HWND,
    quick_metadata: HWND,
    detail_inline_edit: HWND,
    detail_inline_offset: HWND,
    detail_inline_index: i32,
    detail_inline_key: Option<String>,
    detail_inline_paths: Vec<PathBuf>,
    log: HWND,
    taken_date_label: HWND,
    taken_date_edit: HWND,
    utc_offset_label: HWND,
    utc_offset_edit: HWND,
    view_menu: HMENU,
    settings_menu: HMENU,
    icon_big: HICON,
    icon_small: HICON,
    toolbar_images: HIMAGELIST,
    model: FileListModel,
    generation: u64,
    pending_scans: usize,
    scan_total: usize,
    scan_threads: usize,
    show_only_missing_date: bool,
    show_only_missing_offset: bool,
    show_only_duplicate_date: bool,
    split_percent: i32,
    dragging_splitter: bool,
    preview_generation: u64,
    preview_path: Option<PathBuf>,
    preview_bitmap: Option<Arc<PreviewBitmap>>,
    preview_error: Option<String>,
    preview_cache: HashMap<PathBuf, Arc<PreviewBitmap>>,
    preview_sender: mpsc::Sender<PreviewJob>,
    pending_edits: HashMap<PathBuf, PendingEdit>,
    exif_clipboard: Option<PathBuf>,
    copied_files: Vec<PathBuf>,
    updating_editors: bool,
    selection_update_queued: bool,
    log_visible: bool,
    backup_before_changes: bool,
}

pub fn run() -> AppResult<()> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let init = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES | ICC_LISTVIEW_CLASSES,
        };
        InitCommonControlsEx(&init);
        let instance = GetModuleHandleW(null());
        if instance.is_null() {
            return Err(last_error("GetModuleHandleW"));
        }
        let icon_big = create_app_icon(32);
        let icon_small = create_app_icon(16);
        let app_background = CreateSolidBrush(rgb(245, 247, 250));
        let panel_background = CreateSolidBrush(rgb(250, 251, 252));
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: icon_big,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: app_background,
            lpszMenuName: null(),
            lpszClassName: WINDOW_CLASS,
            hIconSm: icon_small,
        };
        if RegisterClassExW(&class) == 0 {
            return Err(last_error("RegisterClassExW"));
        }
        let preview_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(preview_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: panel_background,
            lpszMenuName: null(),
            lpszClassName: PREVIEW_CLASS,
            hIconSm: null_mut(),
        };
        if RegisterClassExW(&preview_class) == 0 {
            return Err(last_error("Register preview class"));
        }
        let shift_dialog_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(shift_dialog_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: icon_big,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: app_background,
            lpszMenuName: null(),
            lpszClassName: SHIFT_DIALOG_CLASS,
            hIconSm: icon_small,
        };
        if RegisterClassExW(&shift_dialog_class) == 0 {
            return Err(last_error("Register shift dialog class"));
        }
        let text_edit_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(text_edit_dialog_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: icon_big,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: app_background,
            lpszMenuName: null(),
            lpszClassName: TEXT_EDIT_DIALOG_CLASS,
            hIconSm: icon_small,
        };
        if RegisterClassExW(&text_edit_class) == 0 {
            return Err(last_error("Register metadata edit dialog class"));
        }
        let title = wide(APP_TITLE);
        let hwnd = CreateWindowExW(
            0 as WINDOW_EX_STYLE,
            WINDOW_CLASS,
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1200,
            760,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(last_error("CreateWindowExW"));
        }
        let (menu, view_menu, settings_menu) = create_main_menu()?;
        if SetMenu(hwnd, menu) == 0 {
            return Err(last_error("SetMenu"));
        }
        SendMessageW(hwnd, WM_SETICON, 1, icon_big as LPARAM);
        SendMessageW(hwnd, WM_SETICON, 0, icon_small as LPARAM);
        let settings = load_app_settings();
        let model = FileListModel::load(
            std::env::current_dir()?,
            FileFilter::Supported,
            settings.time_display_mode,
        )?;
        let (preview_sender, preview_receiver) = mpsc::channel();
        start_preview_worker(hwnd, preview_receiver);
        let boxed = Box::new(create_child_windows(
            hwnd,
            instance,
            view_menu,
            settings_menu,
            icon_big,
            icon_small,
            model,
            preview_sender,
            settings,
        )?);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(boxed) as isize);
        if let Some(s) = state(hwnd) {
            update_pending_status(s);
        }
        sync_settings_menu(hwnd);
        layout_children(hwnd);
        refresh_list(hwnd, false);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        if let Some(s) = state(hwnd) {
            SetFocus(s.list);
        }
        let mut message: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                return Err(last_error("GetMessageW"));
            }
            if result == 0 {
                break;
            }
            if message.message == WM_KEYDOWN && GetKeyState(VK_CONTROL as i32) < 0 {
                match message.wParam {
                    key if key == VK_S as usize => {
                        save_pending_edits(hwnd);
                        continue;
                    }
                    key if key == VK_R as usize => {
                        revert_pending_edits(hwnd);
                        continue;
                    }
                    0x41 => {
                        if state(hwnd).is_some_and(|s| GetFocus() == s.list) {
                            select_matching_rows(hwnd, |_| true);
                            continue;
                        }
                    }
                    0x43 => {
                        if state(hwnd).is_some_and(|s| GetFocus() == s.list) {
                            copy_selected_files(hwnd);
                            continue;
                        }
                    }
                    0x56 => {
                        if state(hwnd).is_some_and(|s| GetFocus() == s.list) {
                            paste_copied_files(hwnd);
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            if message.message == WM_KEYDOWN
                && matches!(message.wParam, key if key == VK_UP as usize || key == VK_DOWN as usize)
            {
                if let Some(s) = state(hwnd) {
                    if GetFocus() == s.taken_date_edit {
                        adjust_date_component(
                            hwnd,
                            if message.wParam == VK_UP as usize {
                                1
                            } else {
                                -1
                            },
                        );
                        continue;
                    }
                    if GetFocus() == s.detail_inline_edit
                        && s.detail_inline_key.as_deref() == Some("taken_date")
                    {
                        adjust_date_text_control(
                            s.detail_inline_edit,
                            if message.wParam == VK_UP as usize {
                                1
                            } else {
                                -1
                            },
                        );
                        continue;
                    }
                }
            }
            if message.message == WM_KEYDOWN
                && message.wParam == VK_ESCAPE as usize
                && state(hwnd).is_some_and(|s| !s.pending_edits.is_empty())
            {
                confirm_revert_pending_edits(hwnd);
                continue;
            }
            if message.message == WM_KEYDOWN {
                if let Some(s) = state(hwnd) {
                    if GetFocus() == s.detail_inline_edit {
                        if message.wParam == VK_RETURN as usize {
                            commit_inline_detail_edit(hwnd);
                            continue;
                        }
                        if message.wParam == VK_ESCAPE as usize {
                            cancel_inline_detail_edit(hwnd);
                            continue;
                        }
                    }
                    if GetFocus() == s.detail_inline_offset {
                        if message.wParam == VK_RETURN as usize {
                            commit_inline_detail_edit(hwnd);
                            continue;
                        }
                        if message.wParam == VK_ESCAPE as usize {
                            cancel_inline_detail_edit(hwnd);
                            continue;
                        }
                    }
                    if GetFocus() == s.path_label {
                        if message.wParam == VK_RETURN as usize {
                            navigate_from_path_bar(hwnd);
                            continue;
                        }
                        if message.wParam == VK_ESCAPE as usize {
                            set_window_text(s.path_label, &s.model.directory.display().to_string());
                            SetFocus(s.list);
                            continue;
                        }
                    }
                }
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe fn create_child_windows(
    parent: HWND,
    instance: HINSTANCE,
    view_menu: HMENU,
    settings_menu: HMENU,
    icon_big: HICON,
    icon_small: HICON,
    mut model: FileListModel,
    preview_sender: mpsc::Sender<PreviewJob>,
    settings: AppSettings,
) -> AppResult<WindowState> {
    let toolbar = CreateWindowExW(
        0,
        TOOLBARCLASSNAMEW,
        null(),
        WS_CHILD | WS_VISIBLE | TBSTYLE_FLAT | TBSTYLE_TOOLTIPS,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    );
    SendMessageW(toolbar, TB_BUTTONSTRUCTSIZE, size_of::<TBBUTTON>(), 0);
    let toolbar_images = create_toolbar_image_list();
    SendMessageW(toolbar, TB_SETIMAGELIST, 0, toolbar_images as LPARAM);
    SendMessageW(toolbar, TB_SETBITMAPSIZE, 0, make_lparam(20, 20));
    SendMessageW(toolbar, TB_SETBUTTONSIZE, 0, make_lparam(30, 30));
    let buttons = [
        toolbar_button(0, ID_TOOL_OPEN),
        toolbar_button(1, ID_TOOL_REFRESH),
        toolbar_separator(),
        toolbar_button(2, ID_TOOL_SAVE),
        toolbar_button(3, ID_TOOL_REVERT),
    ];
    SendMessageW(
        toolbar,
        TB_ADDBUTTONSW,
        buttons.len(),
        buttons.as_ptr() as LPARAM,
    );
    SendMessageW(toolbar, TB_AUTOSIZE, 0, 0);
    let path_text = wide(&model.directory.display().to_string());
    let path_label = CreateWindowExW(
        0,
        w!("EDIT"),
        path_text.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        0,
        0,
        0,
        0,
        parent,
        ID_EDIT_PATH as HMENU,
        instance,
        null(),
    );
    let list = CreateWindowExW(
        0,
        w!("SysListView32"),
        null(),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | LVS_REPORT
            | LVS_OWNERDATA
            | LVS_SHOWSELALWAYS
            | LVS_EDITLABELS,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    );
    let status = CreateWindowExW(
        0,
        STATUSCLASSNAMEW,
        null(),
        WS_CHILD | WS_VISIBLE | SBARS_SIZEGRIP,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    );
    let preview = CreateWindowExW(
        0,
        PREVIEW_CLASS,
        null(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    );
    let details = CreateWindowExW(
        0,
        w!("SysListView32"),
        null(),
        WS_CHILD | WS_VISIBLE | LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_SINGLESEL,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    );
    let quick_file = create_button(parent, instance, ID_QUICK_FILE, "File  ▾");
    let quick_metadata = create_button(parent, instance, ID_QUICK_METADATA, "Metadata  ▾");
    let detail_inline_edit = CreateWindowExW(
        0,
        w!("EDIT"),
        null(),
        WS_CHILD | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        0,
        0,
        0,
        0,
        parent,
        ID_DETAIL_INLINE_EDIT as HMENU,
        instance,
        null(),
    );
    let log = CreateWindowExW(
        0,
        w!("EDIT"),
        null(),
        WS_CHILD
            | WS_BORDER
            | WS_VSCROLL
            | ES_MULTILINE as u32
            | ES_AUTOVSCROLL as u32
            | ES_READONLY as u32,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    );
    let taken_date_label = create_static(parent, instance, "Date Taken");
    let taken_date_edit = create_edit(parent, instance, ID_EDIT_TAKEN_DATE);
    let utc_offset_label = create_static(parent, instance, "UTC Offset");
    let utc_offset_edit = create_offset_combo(parent, instance, ID_EDIT_UTC_OFFSET, true);
    let detail_inline_offset =
        create_offset_combo(parent, instance, ID_DETAIL_INLINE_OFFSET, false);
    if toolbar.is_null()
        || path_label.is_null()
        || list.is_null()
        || status.is_null()
        || preview.is_null()
        || details.is_null()
        || quick_file.is_null()
        || quick_metadata.is_null()
        || detail_inline_edit.is_null()
        || detail_inline_offset.is_null()
        || log.is_null()
        || taken_date_label.is_null()
        || taken_date_edit.is_null()
        || utc_offset_label.is_null()
        || utc_offset_edit.is_null()
    {
        return Err(last_error("Create child controls"));
    }
    let font = GetStockObject(DEFAULT_GUI_FONT);
    SendMessageW(toolbar, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(path_label, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(list, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(details, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(quick_file, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(quick_metadata, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(detail_inline_edit, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(detail_inline_offset, WM_SETFONT, font as WPARAM, 1);
    SendMessageW(log, WM_SETFONT, font as WPARAM, 1);
    for control in [
        taken_date_label,
        taken_date_edit,
        utc_offset_label,
        utc_offset_edit,
    ] {
        SendMessageW(control, WM_SETFONT, font as WPARAM, 1);
    }
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        0,
        (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as LPARAM,
    );
    SendMessageW(
        details,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        0,
        LVS_EX_DOUBLEBUFFER as LPARAM,
    );
    let explorer_theme = wide("Explorer");
    SetWindowTheme(list, explorer_theme.as_ptr(), null());
    SetWindowTheme(details, w!(""), w!(""));
    for control in [list, details] {
        SendMessageW(control, LVM_SETBKCOLOR, 0, rgb(255, 255, 255) as LPARAM);
        SendMessageW(control, LVM_SETTEXTBKCOLOR, 0, rgb(255, 255, 255) as LPARAM);
        SendMessageW(control, LVM_SETTEXTCOLOR, 0, rgb(37, 45, 55) as LPARAM);
    }
    for (index, (title, width)) in [
        ("Filename", 300),
        ("Size", 85),
        ("Modified", 145),
        ("Media Date", 235),
    ]
    .into_iter()
    .enumerate()
    {
        let mut text = wide(title);
        let mut col: LVCOLUMNW = zeroed();
        col.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM;
        col.cx = width;
        col.pszText = text.as_mut_ptr();
        col.iSubItem = index as i32;
        SendMessageW(list, LVM_INSERTCOLUMNW, index, &col as *const _ as LPARAM);
    }
    for (index, (title, width)) in [("Property", 125), ("Value", 255)].into_iter().enumerate() {
        let mut text = wide(title);
        let mut col: LVCOLUMNW = zeroed();
        col.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM;
        col.cx = width;
        col.pszText = text.as_mut_ptr();
        col.iSubItem = index as i32;
        SendMessageW(
            details,
            LVM_INSERTCOLUMNW,
            index,
            &col as *const _ as LPARAM,
        );
    }
    populate_empty_details(details);
    EnableWindow(quick_metadata, 0);
    attach_system_image_list(list, &mut model);
    set_status_parts(status, 1200);
    Ok(WindowState {
        toolbar,
        path_label,
        status,
        list,
        preview,
        details,
        quick_file,
        quick_metadata,
        detail_inline_edit,
        detail_inline_offset,
        detail_inline_index: -1,
        detail_inline_key: None,
        detail_inline_paths: Vec::new(),
        log,
        taken_date_label,
        taken_date_edit,
        utc_offset_label,
        utc_offset_edit,
        view_menu,
        settings_menu,
        icon_big,
        icon_small,
        toolbar_images,
        model,
        generation: 0,
        pending_scans: 0,
        scan_total: 0,
        scan_threads: settings.scan_threads,
        show_only_missing_date: false,
        show_only_missing_offset: false,
        show_only_duplicate_date: false,
        split_percent: 60,
        dragging_splitter: false,
        preview_generation: 0,
        preview_path: None,
        preview_bitmap: None,
        preview_error: None,
        preview_cache: HashMap::new(),
        preview_sender,
        pending_edits: HashMap::new(),
        exif_clipboard: None,
        copied_files: Vec::new(),
        updating_editors: false,
        selection_update_queued: false,
        log_visible: false,
        backup_before_changes: settings.backup_before_changes,
    })
}

unsafe fn create_static(parent: HWND, instance: HINSTANCE, text: &str) -> HWND {
    let text = wide(text);
    CreateWindowExW(
        0,
        w!("STATIC"),
        text.as_ptr(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        null(),
    )
}

unsafe fn create_button(parent: HWND, instance: HINSTANCE, id: usize, text: &str) -> HWND {
    let text = wide(text);
    CreateWindowExW(
        0,
        w!("BUTTON"),
        text.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        0,
        0,
        0,
        0,
        parent,
        id as HMENU,
        instance,
        null(),
    )
}

unsafe fn create_edit(parent: HWND, instance: HINSTANCE, id: usize) -> HWND {
    CreateWindowExW(
        0,
        w!("EDIT"),
        null(),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        0,
        0,
        0,
        0,
        parent,
        id as HMENU,
        instance,
        null(),
    )
}

unsafe fn create_offset_combo(parent: HWND, instance: HINSTANCE, id: usize, visible: bool) -> HWND {
    let visibility = if visible { WS_VISIBLE } else { 0 };
    let combo = CreateWindowExW(
        0,
        w!("COMBOBOX"),
        null(),
        WS_CHILD
            | visibility
            | WS_BORDER
            | WS_TABSTOP
            | WS_VSCROLL
            | CBS_DROPDOWN as u32
            | CBS_AUTOHSCROLL as u32,
        0,
        0,
        0,
        0,
        parent,
        id as HMENU,
        instance,
        null(),
    );
    if !combo.is_null() {
        for choice in [
            "+00:00  UTC · International",
            "+01:00  CET · Central Europe",
            "+02:00  CEST · Central Europe",
            "+09:00  KST · South Korea",
            "-07:00  PDT · United States / Canada",
            "-08:00  PST · United States / Canada",
        ] {
            let choice = wide(choice);
            SendMessageW(combo, CB_ADDSTRING, 0, choice.as_ptr() as LPARAM);
        }
        SendMessageW(combo, CB_SETITEMHEIGHT, usize::MAX, 19);
        SendMessageW(combo, CB_SETITEMHEIGHT, 0, 22);
    }
    combo
}

unsafe fn attach_system_image_list(list: HWND, model: &mut FileListModel) {
    let mut image_list = 0usize;
    let mut icon_cache = HashMap::<String, i32>::new();
    for row in &mut model.all_rows {
        let key = if row.is_dir {
            "<folder>".to_string()
        } else {
            row.path
                .extension()
                .map(|value| value.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        };
        if let Some(icon) = icon_cache.get(&key) {
            row.icon_index = *icon;
            continue;
        }
        let path = wide(&row.path.display().to_string());
        let mut info: SHFILEINFOW = zeroed();
        let result = SHGetFileInfoW(
            path.as_ptr(),
            0,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX | SHGFI_SMALLICON,
        );
        if result != 0 {
            image_list = result;
            row.icon_index = info.iIcon;
            icon_cache.insert(key, info.iIcon);
        }
    }
    let icons: HashMap<_, _> = model
        .all_rows
        .iter()
        .map(|row| (row.path.clone(), row.icon_index))
        .collect();
    for row in &mut model.rows {
        if let Some(icon) = icons.get(&row.path) {
            row.icon_index = *icon;
        }
    }
    if image_list != 0 {
        SendMessageW(
            list,
            LVM_SETIMAGELIST,
            LVSIL_SMALL as usize,
            image_list as LPARAM,
        );
    }
}

unsafe fn update_selection_panels(hwnd: HWND, s: &mut WindowState) {
    let index = selected_index(s);
    let selected_count = SendMessageW(s.list, LVM_GETSELECTEDCOUNT, 0, 0);
    EnableWindow(s.quick_metadata, (selected_count > 0) as i32);
    if selected_count > 1 {
        s.updating_editors = true;
        set_window_text(s.taken_date_edit, "");
        set_window_text(s.utc_offset_edit, "");
        EnableWindow(s.taken_date_edit, 0);
        EnableWindow(s.utc_offset_edit, 0);
        s.updating_editors = false;
        s.preview_generation = s.preview_generation.wrapping_add(1);
        s.preview_path = None;
        s.preview_bitmap = None;
        s.preview_error = None;
        let rows: Vec<_> = selected_paths(s)
            .into_iter()
            .filter_map(|path| {
                s.model
                    .find_path(&path)
                    .and_then(|index| s.model.rows.get(index).cloned())
            })
            .filter(|row| !row.is_dir && !row.is_parent)
            .collect();
        populate_multi_details(s.details, &rows, &s.pending_edits);
        InvalidateRect(s.preview, null(), 1);
        return;
    }
    if index < 0 || selected_count != 1 {
        update_metadata_editors(s, None);
        s.preview_generation = s.preview_generation.wrapping_add(1);
        s.preview_path = None;
        s.preview_bitmap = None;
        s.preview_error = None;
        populate_empty_details(s.details);
        InvalidateRect(s.preview, null(), 1);
        return;
    }
    let row = s.model.rows[index as usize].clone();
    update_metadata_editors(s, Some(&row));
    populate_details(s.details, &row, s.pending_edits.get(&row.path));
    if row.is_dir || !crate::media::is_image_path(&row.path) {
        s.preview_generation = s.preview_generation.wrapping_add(1);
        s.preview_path = None;
        s.preview_bitmap = None;
        s.preview_error = None;
        InvalidateRect(s.preview, null(), 1);
        return;
    }
    if s.preview_path.as_ref() == Some(&row.path) {
        return;
    }
    s.preview_generation = s.preview_generation.wrapping_add(1);
    s.preview_path = Some(row.path.clone());
    if let Some(bitmap) = s.preview_cache.get(&row.path).cloned() {
        s.preview_bitmap = Some(bitmap);
        s.preview_error = None;
        InvalidateRect(s.preview, null(), 1);
        return;
    }
    s.preview_bitmap = None;
    s.preview_error = None;
    InvalidateRect(s.preview, null(), 1);
    let _ = s.preview_sender.send(PreviewJob {
        generation: s.preview_generation,
        path: row.path.clone(),
    });
    let _ = hwnd;
}

unsafe fn update_metadata_editors(s: &mut WindowState, row: Option<&super::model::FileRow>) {
    s.updating_editors = true;
    let (date, offset, date_enabled, offset_enabled) = if let Some(row) = row {
        if row.is_dir || row.is_parent {
            (String::new(), String::new(), false, false)
        } else {
            let pending = s.pending_edits.get(&row.path);
            let scan = row.scan_result.as_ref();
            let date = pending
                .and_then(|edit| edit.taken_date.clone())
                .or_else(|| scan.map(|value| value.media_date.clone()))
                .filter(|value| value != "-")
                .unwrap_or_default();
            let offset = pending
                .and_then(|edit| edit.utc_offset.clone())
                .or_else(|| {
                    scan.and_then(|value| value.recorded_offset_minutes)
                        .map(format_numeric_offset)
                })
                .or_else(|| {
                    scan.and_then(|value| value.exif_metadata.as_ref())
                        .map(|exif| {
                            [
                                &exif.offset_time_original,
                                &exif.offset_time_digitized,
                                &exif.offset_time,
                            ]
                            .into_iter()
                            .find(|value| !value.is_empty() && value.as_str() != "-")
                            .cloned()
                            .unwrap_or_default()
                        })
                })
                .unwrap_or_default();
            (
                date,
                offset,
                is_jpeg_path(&row.path) || is_png_path(&row.path) || is_mp4_path(&row.path),
                is_jpeg_path(&row.path),
            )
        }
    } else {
        (String::new(), String::new(), false, false)
    };
    set_window_text(s.taken_date_edit, &date);
    set_window_text(s.utc_offset_edit, &offset);
    EnableWindow(s.taken_date_edit, date_enabled as i32);
    EnableWindow(s.utc_offset_edit, offset_enabled as i32);
    s.updating_editors = false;
}

unsafe fn set_window_text(window: HWND, value: &str) {
    let value = wide(value);
    SetWindowTextW(window, value.as_ptr());
}

unsafe fn window_text(window: HWND) -> String {
    let length = GetWindowTextLengthW(window);
    if length <= 0 {
        return String::new();
    }
    let mut value = vec![0u16; length as usize + 1];
    GetWindowTextW(window, value.as_mut_ptr(), value.len() as i32);
    String::from_utf16_lossy(&value[..length as usize])
}

unsafe fn adjust_date_component(hwnd: HWND, direction: i32) {
    let Some(s) = state(hwnd) else { return };
    if adjust_date_text_control(s.taken_date_edit, direction) {
        stage_editor_change(hwnd, ID_EDIT_TAKEN_DATE);
    }
}

unsafe fn adjust_date_text_control(edit: HWND, direction: i32) -> bool {
    let value = window_text(edit);
    let Ok(current) = NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S") else {
        return false;
    };
    let mut selection_start = 0u32;
    let mut selection_end = 0u32;
    SendMessageW(
        edit,
        EM_GETSEL,
        &mut selection_start as *mut _ as WPARAM,
        &mut selection_end as *mut _ as LPARAM,
    );
    let (range_start, range_end, component) = match selection_start.min(18) {
        0..=3 => (0, 4, 0),
        5..=6 => (5, 7, 1),
        8..=9 => (8, 10, 2),
        11..=12 => (11, 13, 3),
        14..=15 => (14, 16, 4),
        _ => (17, 19, 5),
    };
    let adjusted = match component {
        0 => replace_date_part(current, current.year() + direction, current.month()),
        1 => {
            let month_index = current.year() * 12 + current.month0() as i32 + direction;
            replace_date_part(
                current,
                month_index.div_euclid(12),
                month_index.rem_euclid(12) as u32 + 1,
            )
        }
        2 => current.checked_add_signed(chrono::Duration::days(direction as i64)),
        3 => current.checked_add_signed(chrono::Duration::hours(direction as i64)),
        4 => current.checked_add_signed(chrono::Duration::minutes(direction as i64)),
        _ => current.checked_add_signed(chrono::Duration::seconds(direction as i64)),
    };
    let Some(adjusted) = adjusted else {
        return false;
    };
    set_window_text(edit, &adjusted.format("%Y-%m-%d %H:%M:%S").to_string());
    SendMessageW(edit, EM_SETSEL, range_start, range_end as LPARAM);
    true
}

fn replace_date_part(value: NaiveDateTime, year: i32, month: u32) -> Option<NaiveDateTime> {
    let mut day = value.day();
    let date = loop {
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            break date;
        }
        day = day.checked_sub(1)?;
    };
    date.and_hms_opt(value.hour(), value.minute(), value.second())
}

unsafe fn clear_details(details: HWND) {
    SendMessageW(details, LVM_DELETEALLITEMS, 0, 0);
}

unsafe fn populate_empty_details(details: HWND) {
    clear_details(details);
    let rows = [
        ("FILE", ""),
        ("File Name", ""),
        ("File Size", ""),
        ("Date Created", ""),
        ("Date Modified", ""),
        ("CAMERA", ""),
        ("Date Taken", ""),
        ("UTC Offset", ""),
        ("Camera Make", ""),
        ("Camera Model", ""),
        ("Lens Model", ""),
        ("EXPOSURE", ""),
        ("Exposure Time", ""),
        ("F-number", ""),
        ("ISO", ""),
        ("Focal Length", ""),
        ("Flash", ""),
        ("Metering Mode", ""),
        ("LOCATION", ""),
        ("Latitude", ""),
        ("Longitude", ""),
        ("Altitude", ""),
    ];
    for (index, (name, value)) in rows.into_iter().enumerate() {
        insert_detail_row(details, index, name, value);
    }
}

fn staged_detail_value(pending: Option<&PendingEdit>, label: &str, original: &str) -> String {
    if label == "Date Taken" || label == "Media Creation Time" {
        return pending
            .and_then(|edit| edit.taken_date.clone())
            .unwrap_or_else(|| original.to_string());
    }
    if label == "UTC Offset" {
        return pending
            .and_then(|edit| edit.utc_offset.clone())
            .unwrap_or_else(|| original.to_string());
    }
    exif_key_for_detail_label(label)
        .and_then(|key| pending.and_then(|edit| edit.tag_values.get(key).cloned()))
        .unwrap_or_else(|| original.to_string())
}

unsafe fn populate_details(
    details: HWND,
    row: &super::model::FileRow,
    pending: Option<&PendingEdit>,
) {
    clear_details(details);
    let mut values = vec![
        ("FILE", String::new()),
        ("File Name", row.name.clone()),
        (
            "File Size",
            format!("{} Byte", format_with_commas(row.size)),
        ),
        ("Date Created", format_system_time(row.created)),
        ("Date Modified", format_system_time(row.modified)),
    ];
    if let Some(scan) = &row.scan_result {
        if scan.exif_metadata.is_none() {
            values.push(("VIDEO", String::new()));
            values.push(("Media Type", scan.media_type.clone()));
            values.push((
                "Media Creation Time",
                staged_detail_value(pending, "Media Creation Time", &row.media_date),
            ));
            values.push(("Time Interpretation", scan.time_interpretation.clone()));
        }
        if let Some(exif) = &scan.exif_metadata {
            values.push(("CAMERA", String::new()));
            for (name, value) in [
                ("Date Taken", &exif.taken_date),
                ("UTC Offset", &exif.offset_time_original),
                ("Camera Make", &exif.camera_make),
                ("Camera Model", &exif.camera_model),
                ("Lens Model", &exif.lens_model),
            ] {
                let value = staged_detail_value(pending, name, value);
                values.push((name, if value == "-" { String::new() } else { value }));
            }
            values.push(("EXPOSURE", String::new()));
            for (name, value) in [
                ("Exposure Time", &exif.shutter_speed),
                ("F-number", &exif.aperture),
                ("ISO", &exif.iso_speed),
                ("Focal Length", &exif.focal_length),
                ("35 mm Equivalent", &exif.focal_length_35mm),
                ("Flash", &exif.flash_fired),
                ("Metering Mode", &exif.metering_mode),
                ("Exposure Program", &exif.exposure_program),
                ("White Balance", &exif.white_balance),
            ] {
                let value = staged_detail_value(pending, name, value);
                let always_visible = matches!(
                    name,
                    "Exposure Time"
                        | "F-number"
                        | "ISO"
                        | "Focal Length"
                        | "Flash"
                        | "Metering Mode"
                );
                if always_visible || (value != "-" && !value.is_empty()) {
                    values.push((name, value));
                }
            }
            values.push(("LOCATION", String::new()));
            for (name, value) in [
                ("Latitude", &exif.gps_latitude),
                ("Longitude", &exif.gps_longitude),
                ("Altitude", &exif.gps_altitude),
            ] {
                let value = staged_detail_value(pending, name, value);
                values.push((name, if value == "-" { String::new() } else { value }));
            }
            let gps_date = pending
                .and_then(|edit| edit.gps_date.clone())
                .unwrap_or_else(|| exif.gps_date_stamp.clone());
            let gps_time = pending
                .and_then(|edit| edit.gps_time.clone())
                .unwrap_or_else(|| exif.gps_time_stamp.clone());
            if !gps_date.is_empty() && gps_date != "-" {
                values.push(("GPS Date", gps_date));
            }
            if !gps_time.is_empty() && gps_time != "-" {
                values.push(("GPS Time", format!("{gps_time} UTC")));
            }
            values.push(("OTHER METADATA", String::new()));
            for (name, value) in [
                ("Software", &exif.software),
                ("Artist", &exif.artist),
                ("Orientation", &exif.orientation),
                ("Color Space", &exif.color_space),
                ("Image Width", &exif.image_width),
                ("Image Height", &exif.image_height),
                ("Description", &exif.image_description),
                ("Copyright", &exif.copyright),
                ("EXIF Version", &exif.exif_version),
                ("Date/Time Original", &exif.date_time_original),
                ("Date/Time Digitized", &exif.date_time_digitized),
                ("Image Date/Time", &exif.image_date_time),
            ] {
                let value = staged_detail_value(pending, name, value);
                if value != "-" && !value.is_empty() {
                    values.push((name, value));
                }
            }
        }
    }
    for (index, (name, value)) in values.into_iter().enumerate() {
        insert_detail_row(details, index, name, &value);
    }
}

fn common_detail_value<I>(mut values: I) -> String
where
    I: Iterator<Item = String>,
{
    let Some(first) = values.next() else {
        return String::new();
    };
    if values.all(|value| value == first) {
        first
    } else {
        "<Multiple values>".to_string()
    }
}

unsafe fn populate_multi_details(
    details: HWND,
    rows: &[super::model::FileRow],
    pending_edits: &HashMap<PathBuf, PendingEdit>,
) {
    clear_details(details);
    insert_detail_row(details, 0, "SELECTION", &format!("{} files", rows.len()));
    if rows.is_empty() {
        return;
    }

    let taken_date = common_detail_value(rows.iter().map(|row| {
        pending_edits
            .get(&row.path)
            .and_then(|edit| edit.taken_date.clone())
            .unwrap_or_else(|| row.media_date.clone())
    }));
    insert_detail_row(details, 1, "Date Taken", &taken_date);

    let jpeg_rows: Vec<_> = rows.iter().filter(|row| is_jpeg_path(&row.path)).collect();
    if jpeg_rows.len() == rows.len() {
        let offset = common_detail_value(jpeg_rows.into_iter().map(|row| {
            pending_edits
                .get(&row.path)
                .and_then(|edit| edit.utc_offset.clone())
                .or_else(|| {
                    row.scan_result
                        .as_ref()
                        .and_then(|scan| scan.exif_metadata.as_ref())
                        .map(|exif| exif.offset_time_original.clone())
                })
                .filter(|value| !value.is_empty() && value != "-")
                .unwrap_or_else(|| "Not set".to_string())
        }));
        insert_detail_row(details, 2, "UTC Offset", &offset);
    }
}

unsafe fn insert_detail_row(details: HWND, index: usize, name: &str, value: &str) {
    let mut name = wide(name);
    let mut item: LVITEMW = zeroed();
    item.mask = LVIF_TEXT;
    item.iItem = index as i32;
    item.pszText = name.as_mut_ptr();
    SendMessageW(details, LVM_INSERTITEMW, 0, &item as *const _ as LPARAM);
    let mut value = wide(value);
    let mut subitem: LVITEMW = zeroed();
    subitem.iSubItem = 1;
    subitem.pszText = value.as_mut_ptr();
    SendMessageW(
        details,
        LVM_SETITEMTEXTW,
        index,
        &subitem as *const _ as LPARAM,
    );
}

fn format_with_commas(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn format_system_time(value: Option<std::time::SystemTime>) -> String {
    value
        .map(DateTime::<Local>::from)
        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

unsafe fn refresh_list(hwnd: HWND, preserve: bool) {
    let Some(s) = state_mut(hwnd) else { return };
    attach_system_image_list(s.list, &mut s.model);
    let selected = if preserve {
        selected_paths(s)
    } else {
        Vec::new()
    };
    SendMessageW(s.list, LVM_SETITEMCOUNT, s.model.rows.len(), 0);
    if !s.model.rows.is_empty() {
        SendMessageW(
            s.list,
            LVM_REDRAWITEMS,
            0,
            (s.model.rows.len() - 1) as LPARAM,
        );
    }
    let path = wide(&s.model.directory.display().to_string());
    SetWindowTextW(s.path_label, path.as_ptr());
    restore_selection(s, &selected);
    update_status(s);
    begin_media_scan(hwnd);
    if let Some(s) = state_mut(hwnd) {
        update_selection_panels(hwnd, s);
    }
    layout_children(hwnd);
}

unsafe fn begin_media_scan(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    s.generation = s.generation.wrapping_add(1);
    let generation = s.generation;
    let scan_threads = s.scan_threads;
    let jobs: VecDeque<_> = s
        .model
        .all_rows
        .iter()
        .filter(|r| r.scan_pending)
        .map(|r| r.path.clone())
        .collect();
    s.pending_scans = jobs.len();
    s.scan_total = jobs.len();
    update_status(s);
    if jobs.is_empty() {
        return;
    }
    let queue = Arc::new(Mutex::new(jobs));
    let target = hwnd as usize;
    for _ in 0..scan_threads {
        let queue = Arc::clone(&queue);
        std::thread::spawn(move || loop {
            let path = queue.lock().ok().and_then(|mut q| q.pop_front());
            let Some(path) = path else { break };
            let raw = Box::into_raw(Box::new(ScanCompletion {
                generation,
                result: scan_media_file(&path),
                path,
            }));
            if unsafe { PostMessageW(target as HWND, WM_SCAN_COMPLETE, 0, raw as LPARAM) } == 0 {
                unsafe {
                    drop(Box::from_raw(raw));
                }
                break;
            }
        });
    }
}

fn start_preview_worker(hwnd: HWND, receiver: mpsc::Receiver<PreviewJob>) {
    let target = hwnd as usize;
    std::thread::spawn(move || {
        while let Ok(mut job) = receiver.recv() {
            while let Ok(newer) = receiver.try_recv() {
                job = newer;
            }
            let started = Instant::now();
            let decoded = image::ImageReader::open(&job.path)
                .map_err(|error| format!("Image open failed: {error}"))
                .and_then(|reader| {
                    reader
                        .decode()
                        .map_err(|error| format!("Image decoder: {error}"))
                });
            let error = decoded.as_ref().err().map(ToString::to_string);
            let bitmap = decoded.ok().map(|image| {
                let image = image.thumbnail(1920, 1440).to_rgba8();
                let (width, height) = image.dimensions();
                let mut bgra = image.into_raw();
                for pixel in bgra.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                Arc::new(PreviewBitmap {
                    width,
                    height,
                    bgra,
                    elapsed_ms: started.elapsed().as_millis(),
                })
            });
            let raw = Box::into_raw(Box::new(PreviewCompletion {
                generation: job.generation,
                path: job.path,
                bitmap,
                error,
            }));
            if unsafe { PostMessageW(target as HWND, WM_PREVIEW_COMPLETE, 0, raw as LPARAM) } == 0 {
                unsafe {
                    drop(Box::from_raw(raw));
                }
                break;
            }
        }
    });
}

unsafe fn handle_preview_complete(hwnd: HWND, lparam: LPARAM) {
    let completion = Box::from_raw(lparam as *mut PreviewCompletion);
    let Some(s) = state_mut(hwnd) else { return };
    if completion.generation != s.preview_generation
        || s.preview_path.as_ref() != Some(&completion.path)
    {
        return;
    }
    if let Some(bitmap) = completion.bitmap {
        if s.preview_cache.len() >= 8 {
            s.preview_cache.clear();
        }
        s.preview_cache.insert(completion.path, Arc::clone(&bitmap));
        s.preview_bitmap = Some(bitmap);
        s.preview_error = None;
    } else {
        s.preview_bitmap = None;
        s.preview_error = completion.error;
    }
    InvalidateRect(s.preview, null(), 1);
    layout_children(hwnd);
}

unsafe extern "system" fn preview_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message != WM_PAINT {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let mut paint: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(hwnd, &mut paint);
    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    let background = CreateSolidBrush(rgb(255, 255, 255));
    FillRect(dc, &client, background);
    DeleteObject(background);
    let parent = GetParent(hwnd);
    if let Some(bitmap) = state(parent).and_then(|s| s.preview_bitmap.as_ref()) {
        let available_width = client.right.max(1);
        let available_height = client.bottom.max(1);
        let scale = (available_width as f64 / bitmap.width as f64)
            .min(available_height as f64 / bitmap.height as f64);
        let width = (bitmap.width as f64 * scale).round() as i32;
        let height = (bitmap.height as f64 * scale).round() as i32;
        let left = (available_width - width) / 2;
        let top = (available_height - height) / 2;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bitmap.width as i32,
                biHeight: -(bitmap.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [zeroed()],
        };
        SetStretchBltMode(dc, HALFTONE);
        StretchDIBits(
            dc,
            left,
            top,
            width,
            height,
            0,
            0,
            bitmap.width as i32,
            bitmap.height as i32,
            bitmap.bgra.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        draw_preview_overlay(dc, left + 6, top + height - 28, bitmap);
    } else if let Some(error) = state(parent).and_then(|s| s.preview_error.as_ref()) {
        let mut text = wide(error);
        let mut rect = RECT {
            left: 16,
            top: 16,
            right: (client.right - 16).max(17),
            bottom: (client.bottom - 16).max(17),
        };
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, rgb(180, 35, 45));
        DrawTextW(dc, text.as_mut_ptr(), -1, &mut rect, DT_LEFT | DT_WORDBREAK);
    }
    EndPaint(hwnd, &paint);
    0
}

unsafe fn draw_preview_overlay(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    left: i32,
    top: i32,
    bitmap: &PreviewBitmap,
) {
    let label = format!(
        "{} x {} (loaded in {} ms)",
        bitmap.width, bitmap.height, bitmap.elapsed_ms
    );
    let width = (label.len() as i32 * 7 + 14).max(140);
    let height = 22;
    let overlay_dc = CreateCompatibleDC(dc);
    let overlay_bitmap = CreateCompatibleBitmap(dc, width, height);
    let old = SelectObject(overlay_dc, overlay_bitmap);
    let brush = CreateSolidBrush(rgb(25, 25, 25));
    FillRect(
        overlay_dc,
        &RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        brush,
    );
    AlphaBlend(
        dc,
        left,
        top,
        width,
        height,
        overlay_dc,
        0,
        0,
        width,
        height,
        BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 155,
            AlphaFormat: 0,
        },
    );
    let mut text = wide(&label);
    let mut rect = RECT {
        left: left + 7,
        top,
        right: left + width - 4,
        bottom: top + height,
    };
    let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, rgb(255, 255, 255));
    DrawTextW(
        dc,
        text.as_mut_ptr(),
        -1,
        &mut rect,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(dc, old_font);
    SelectObject(overlay_dc, old);
    DeleteObject(brush);
    DeleteObject(overlay_bitmap);
    DeleteDC(overlay_dc);
}

unsafe fn load_directory(hwnd: HWND, directory: PathBuf) {
    let (filter, time_display_mode) = state(hwnd)
        .map_or((FileFilter::Supported, TimeDisplayMode::Recorded), |s| {
            (s.model.filter, s.model.time_display_mode)
        });
    match FileListModel::load(directory, filter, time_display_mode) {
        Ok(mut model) => {
            if let Some(s) = state_mut(hwnd) {
                model.set_special_filters(
                    s.show_only_missing_date,
                    s.show_only_missing_offset,
                    s.show_only_duplicate_date,
                );
                s.model = model;
                s.pending_edits.clear();
                update_pending_status(s);
            }
            refresh_list(hwnd, false);
            if let Some(s) = state(hwnd) {
                SetFocus(s.list);
            }
        }
        Err(error) => show_error(hwnd, "Folder Load Failed", &error),
    }
}

unsafe fn navigate_from_path_bar(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let entered = window_text(s.path_label);
    let entered = entered.trim().trim_matches('"');
    if entered.is_empty() {
        set_window_text(s.path_label, &s.model.directory.display().to_string());
        return;
    }
    let directory = PathBuf::from(entered);
    if !directory.is_dir() {
        show_error(
            hwnd,
            "Folder Not Found",
            &format!("The entered path is not an accessible folder:\n{entered}"),
        );
        if let Some(s) = state(hwnd) {
            set_window_text(s.path_label, &s.model.directory.display().to_string());
            SetFocus(s.path_label);
        }
        return;
    }
    load_directory(hwnd, directory);
}

unsafe fn stage_editor_change(hwnd: HWND, control_id: usize) {
    let Some(s) = state_mut(hwnd) else { return };
    if s.updating_editors {
        return;
    }
    let selected_count = SendMessageW(s.list, LVM_GETSELECTEDCOUNT, 0, 0);
    if control_id == ID_EDIT_UTC_OFFSET && selected_count > 1 {
        let value = normalize_offset_combo_text(&window_text(s.utc_offset_edit));
        if parse_numeric_offset_minutes(&value).is_err() {
            return;
        }
        let paths = selected_paths(s);
        let mut staged = 0usize;
        for path in paths {
            if is_jpeg_path(&path) {
                s.pending_edits.entry(path).or_default().utc_offset = Some(value.clone());
                staged += 1;
            }
        }
        update_pending_status(s);
        set_status_text(
            s.status,
            0,
            &format!("Staged Time Basis for {staged} file(s). Ctrl+S to save."),
        );
        return;
    }
    if selected_count != 1 {
        return;
    }
    let index = selected_index(s);
    if index < 0 {
        return;
    }
    let row = &s.model.rows[index as usize];
    if row.is_dir || row.is_parent {
        return;
    }
    let path = row.path.clone();
    let value = match control_id {
        ID_EDIT_TAKEN_DATE => window_text(s.taken_date_edit).trim().to_string(),
        ID_EDIT_UTC_OFFSET => normalize_offset_combo_text(&window_text(s.utc_offset_edit)),
        _ => return,
    };
    let edit = s.pending_edits.entry(path.clone()).or_default();
    match control_id {
        ID_EDIT_TAKEN_DATE => edit.taken_date = Some(value),
        ID_EDIT_UTC_OFFSET => edit.utc_offset = Some(value),
        _ => {}
    }
    if edit.taken_date.is_none()
        && edit.utc_offset.is_none()
        && edit.gps_date.is_none()
        && edit.gps_time.is_none()
        && edit.tag_values.is_empty()
    {
        s.pending_edits.remove(&path);
    }
    update_pending_status(s);
}

fn normalize_offset_combo_text(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 6 {
        let candidate = &value[..6];
        let bytes = candidate.as_bytes();
        if matches!(bytes.first(), Some(b'+' | b'-'))
            && bytes.get(3) == Some(&b':')
            && bytes[1..3].iter().all(u8::is_ascii_digit)
            && bytes[4..6].iter().all(u8::is_ascii_digit)
        {
            return candidate.to_string();
        }
    }
    value.to_string()
}

unsafe fn revert_pending_edits(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    if s.pending_edits.is_empty() {
        return;
    }
    s.pending_edits.clear();
    let row = selected_index(s)
        .try_into()
        .ok()
        .and_then(|index: usize| s.model.rows.get(index).cloned());
    update_metadata_editors(s, row.as_ref());
    update_pending_status(s);
    set_status_text(s.status, 0, "Staged changes reverted.");
}

unsafe fn confirm_revert_pending_edits(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    if s.pending_edits.is_empty() {
        return;
    }
    let title = wide("Revert Changes");
    let message = wide(&format!(
        "Discard staged changes for {} file(s)?",
        s.pending_edits.len()
    ));
    if MessageBoxW(
        hwnd,
        message.as_ptr(),
        title.as_ptr(),
        MB_YESNO | MB_ICONWARNING,
    ) == IDYES_VALUE
    {
        cancel_inline_detail_edit(hwnd);
        revert_pending_edits(hwnd);
        if let Some(s) = state_mut(hwnd) {
            update_selection_panels(hwnd, s);
        }
    }
}

unsafe fn save_pending_edits(hwnd: HWND) {
    let (pending, backup_before_changes) = match state(hwnd) {
        Some(s) if !s.pending_edits.is_empty() => {
            (s.pending_edits.clone(), s.backup_before_changes)
        }
        _ => return,
    };
    let selected = state(hwnd).map(|s| selected_paths(s)).unwrap_or_default();
    let mut failed = HashMap::new();
    let mut errors = Vec::new();
    let mut updated = 0usize;
    let mut updated_paths = Vec::new();
    for (path, edit) in pending {
        let result = apply_pending_edit(&path, &edit, backup_before_changes);
        match result {
            Ok(()) => {
                updated += 1;
                updated_paths.push(path.clone());
            }
            Err(error) => {
                errors.push(format!(
                    "{}: {error}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
                failed.insert(path, edit);
            }
        }
    }
    if let Some(s) = state_mut(hwnd) {
        s.pending_edits = failed;
        update_pending_status(s);
    }
    reload_directory(hwnd, false);
    if let Some(s) = state(hwnd) {
        for path in &updated_paths {
            append_session_log(s, &format!("Updated metadata: {}", path.display()));
        }
        restore_selection(s, &selected);
        SetFocus(s.list);
        if errors.is_empty() {
            set_status_text(
                s.status,
                0,
                &format!("Saved changes for {updated} file(s)."),
            );
        }
    }
    if !errors.is_empty() {
        show_error(
            hwnd,
            if updated == 0 {
                "Apply Failed"
            } else {
                "Apply Partially Completed"
            },
            &format!(
                "{} file(s) updated; {} failed.\n\n{}",
                updated,
                errors.len(),
                errors.join("\n")
            ),
        );
    }
}

fn apply_pending_edit(
    path: &std::path::Path,
    edit: &PendingEdit,
    backup_before_changes: bool,
) -> Result<(), String> {
    if let (Some(date), Some(time)) = (edit.gps_date.as_deref(), edit.gps_time.as_deref()) {
        if !is_jpeg_path(path) {
            return Err("GPS Date/Time can be written only to JPEG files.".to_string());
        }
        ensure_win32_jpeg_backup(path, backup_before_changes)?;
        write_gps_date_time(path, date, time)?;
    }
    if !edit.tag_values.is_empty() {
        if !is_jpeg_path(path) {
            return Err("EXIF fields can be written only to JPEG files.".to_string());
        }
        ensure_win32_jpeg_backup(path, backup_before_changes)?;
        for (key, value) in &edit.tag_values {
            match key.as_str() {
                "camera_make" => write_camera_make(path, value)?,
                "camera_model" => write_camera_model(path, value)?,
                "lens_model" => write_lens_model(path, value)?,
                "software" => write_software(path, value)?,
                "artist" => write_artist(path, value)?,
                "shutter_speed" => write_shutter_speed(path, value)?,
                "aperture" => write_aperture(path, value)?,
                "iso_speed" => write_iso_speed(path, value)?,
                "focal_length" => write_focal_length(path, value)?,
                "flash_fired" => write_flash_fired(path, value)?,
                "metering_mode" => write_metering_mode(path, value)?,
                "orientation" => write_orientation(path, value)?,
                "color_space" => write_color_space(path, value)?,
                _ => return Err(format!("Unsupported editable EXIF field: {key}")),
            }
        }
    }
    let mp4_offset_minutes = if is_mp4_path(path) {
        edit.utc_offset
            .as_deref()
            .map(parse_numeric_offset_minutes)
            .transpose()?
    } else {
        None
    };
    if let Some(value) = edit.taken_date.as_deref() {
        if value.trim().is_empty() {
            return Err("Date Taken cannot be empty.".to_string());
        }
        display_datetime_to_exif(value)?;
        if is_mp4_path(path) {
            if let Some(minutes) = mp4_offset_minutes {
                write_mp4_media_date_with_offset(path, value, minutes)?;
            } else {
                write_mp4_media_date(path, value)?;
            }
        } else if is_png_path(path) {
            write_png_media_date(path, value, backup_before_changes)?;
        } else if is_jpeg_path(path) {
            let mut metadata = read_exif_metadata(path);
            if metadata.has_exif {
                write_taken_date_preserving_exif(path, value, backup_before_changes)?;
            } else {
                metadata.has_exif = true;
                metadata.taken_date = value.to_string();
                metadata.date_time_original = value.to_string();
                rewrite_basic_exif_metadata(path, &metadata, backup_before_changes)?;
            }
        } else {
            return Err("Writing Date Taken is not supported for this file type.".to_string());
        }
    }
    if let Some(offset) = edit.utc_offset.as_deref() {
        if offset.trim().is_empty() {
            return Err("UTC Offset cannot be empty. Use a value such as +09:00.".to_string());
        }
        if is_mp4_path(path) {
            if edit.taken_date.is_none() {
                let scan = scan_media_file(path);
                let recorded = if scan.recorded_media_date.trim().is_empty() {
                    scan.media_date
                } else {
                    scan.recorded_media_date
                };
                write_mp4_media_date_with_offset(
                    path,
                    &recorded,
                    mp4_offset_minutes.ok_or_else(|| "Invalid MP4 time basis.".to_string())?,
                )?;
            }
        } else if is_jpeg_path(path) {
            write_time_zone_offset_preserving_exif(path, offset, backup_before_changes)?;
        } else {
            return Err(
                "Time Basis can currently be written only to JPEG and MP4 files.".to_string(),
            );
        }
    }
    Ok(())
}

fn parse_numeric_offset_minutes(value: &str) -> Result<i32, String> {
    let normalized = normalize_offset_combo_text(value);
    let bytes = normalized.as_bytes();
    if bytes.len() != 6
        || !matches!(bytes[0], b'+' | b'-')
        || bytes[3] != b':'
        || !bytes[1..3].iter().all(u8::is_ascii_digit)
        || !bytes[4..6].iter().all(u8::is_ascii_digit)
    {
        return Err("Time Basis must use the form +09:00 or -07:00.".to_string());
    }
    let hours = std::str::from_utf8(&bytes[1..3])
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| "Invalid Time Basis hour.".to_string())?;
    let minutes = std::str::from_utf8(&bytes[4..6])
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| "Invalid Time Basis minute.".to_string())?;
    if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
        return Err("Time Basis must be between -14:00 and +14:00.".to_string());
    }
    let total = hours * 60 + minutes;
    Ok(if bytes[0] == b'-' { -total } else { total })
}

fn format_numeric_offset(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let absolute = minutes.abs();
    format!("{sign}{:02}:{:02}", absolute / 60, absolute % 60)
}

unsafe extern "system" fn shift_dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let command = wparam & 0xffff;
            if matches!(command, 1 | 2) {
                let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ShiftDialogState;
                if !data.is_null() {
                    (*data).accepted = command == 1;
                }
                DestroyWindow(hwnd);
                return 0;
            }
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe extern "system" fn text_edit_dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let command = wparam & 0xffff;
            if matches!(command, 1 | 2) {
                let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TextEditDialogState;
                if !data.is_null() {
                    (*data).accepted = command == 1;
                    if command == 1 {
                        (*data).value = window_text((*data).edit);
                    }
                }
                DestroyWindow(hwnd);
                return 0;
            }
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe fn show_text_edit_dialog(
    parent: HWND,
    label: &str,
    current: &str,
) -> Result<Option<String>, String> {
    let instance = GetModuleHandleW(null());
    let mut parent_rect: RECT = zeroed();
    GetWindowRect(parent, &mut parent_rect);
    let width = 460;
    let height = 175;
    let left = parent_rect.left + ((parent_rect.right - parent_rect.left - width) / 2).max(0);
    let top = parent_rect.top + ((parent_rect.bottom - parent_rect.top - height) / 2).max(0);
    let title = wide(&format!("Edit {label}"));
    let dialog = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        TEXT_EDIT_DIALOG_CLASS,
        title.as_ptr(),
        WS_CAPTION | WS_SYSMENU,
        left,
        top,
        width,
        height,
        parent,
        null_mut(),
        instance,
        null(),
    );
    if dialog.is_null() {
        return Err(last_error("Create metadata edit dialog").to_string());
    }
    let mut data = Box::new(TextEditDialogState {
        edit: null_mut(),
        accepted: false,
        value: String::new(),
    });
    SetWindowLongPtrW(
        dialog,
        GWLP_USERDATA,
        (&mut *data as *mut TextEditDialogState) as isize,
    );
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let label_window = create_static(dialog, instance, label);
    MoveWindow(label_window, 18, 18, 420, 20, 1);
    SendMessageW(label_window, WM_SETFONT, font as WPARAM, 1);
    data.edit = create_edit(dialog, instance, 540);
    MoveWindow(data.edit, 18, 43, 420, 26, 1);
    set_window_text(data.edit, current);
    SendMessageW(data.edit, WM_SETFONT, font as WPARAM, 1);
    for (id, text, x) in [(1usize, "Stage", 278), (2usize, "Cancel", 360)] {
        let text = wide(text);
        let button = CreateWindowExW(
            0,
            w!("BUTTON"),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | if id == 1 { BS_DEFPUSHBUTTON as u32 } else { 0 },
            x,
            92,
            76,
            28,
            dialog,
            id as HMENU,
            instance,
            null(),
        );
        SendMessageW(button, WM_SETFONT, font as WPARAM, 1);
    }
    EnableWindow(parent, 0);
    ShowWindow(dialog, SW_SHOW);
    SetFocus(data.edit);
    SendMessageW(data.edit, EM_SETSEL, 0, -1);
    let mut message: MSG = zeroed();
    while IsWindow(dialog) != 0 {
        let result = GetMessageW(&mut message, null_mut(), 0, 0);
        if result <= 0 {
            break;
        }
        if IsDialogMessageW(dialog, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    EnableWindow(parent, 1);
    SetActiveWindow(parent);
    Ok(data.accepted.then(|| data.value.clone()))
}

unsafe fn show_time_shift_dialog(
    parent: HWND,
    dialog_title: &str,
) -> Result<Option<MediaDateShift>, String> {
    let instance = GetModuleHandleW(null());
    let mut parent_rect: RECT = zeroed();
    GetWindowRect(parent, &mut parent_rect);
    let width = 390;
    let height = 230;
    let left = parent_rect.left + ((parent_rect.right - parent_rect.left - width) / 2).max(0);
    let top = parent_rect.top + ((parent_rect.bottom - parent_rect.top - height) / 2).max(0);
    let title = wide(dialog_title);
    let dialog = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        SHIFT_DIALOG_CLASS,
        title.as_ptr(),
        WS_CAPTION | WS_SYSMENU,
        left,
        top,
        width,
        height,
        parent,
        null_mut(),
        instance,
        null(),
    );
    if dialog.is_null() {
        return Err(last_error("Create Shift Media Date dialog").to_string());
    }
    let mut data = Box::new(ShiftDialogState {
        edits: [null_mut(); 4],
        subtract: null_mut(),
        accepted: false,
    });
    SetWindowLongPtrW(
        dialog,
        GWLP_USERDATA,
        (&mut *data as *mut ShiftDialogState) as isize,
    );
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let labels = ["Days", "Hours", "Minutes", "Seconds"];
    for (index, label) in labels.into_iter().enumerate() {
        let label_window = create_static(dialog, instance, label);
        MoveWindow(label_window, 18 + index as i32 * 88, 28, 74, 20, 1);
        SendMessageW(label_window, WM_SETFONT, font as WPARAM, 1);
        let edit = create_edit(dialog, instance, 520 + index);
        MoveWindow(edit, 18 + index as i32 * 88, 51, 72, 25, 1);
        set_window_text(edit, "0");
        SendMessageW(edit, WM_SETFONT, font as WPARAM, 1);
        data.edits[index] = edit;
    }
    let checkbox_text = wide("Subtract the entered duration");
    data.subtract = CreateWindowExW(
        0,
        w!("BUTTON"),
        checkbox_text.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        18,
        94,
        250,
        24,
        dialog,
        530 as HMENU,
        instance,
        null(),
    );
    SendMessageW(data.subtract, WM_SETFONT, font as WPARAM, 1);
    let hint = create_static(
        dialog,
        instance,
        "The shifted value is staged. Press Ctrl+S in the main window to save.",
    );
    MoveWindow(hint, 18, 124, 350, 20, 1);
    SendMessageW(hint, WM_SETFONT, font as WPARAM, 1);
    for (id, text, x) in [(1usize, "Stage", 210), (2usize, "Cancel", 292)] {
        let text = wide(text);
        let button = CreateWindowExW(
            0,
            w!("BUTTON"),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | if id == 1 { BS_DEFPUSHBUTTON as u32 } else { 0 },
            x,
            158,
            76,
            28,
            dialog,
            id as HMENU,
            instance,
            null(),
        );
        SendMessageW(button, WM_SETFONT, font as WPARAM, 1);
    }
    EnableWindow(parent, 0);
    ShowWindow(dialog, SW_SHOW);
    SetFocus(data.edits[0]);
    let mut message: MSG = zeroed();
    while IsWindow(dialog) != 0 {
        let result = GetMessageW(&mut message, null_mut(), 0, 0);
        if result <= 0 {
            break;
        }
        if IsDialogMessageW(dialog, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    EnableWindow(parent, 1);
    SetActiveWindow(parent);
    if !data.accepted {
        return Ok(None);
    }
    let parse = |edit: HWND, label: &str| -> Result<i64, String> {
        let value = unsafe { window_text(edit) };
        let value = value
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("{label} must be a non-negative integer."))?;
        if value < 0 {
            return Err(format!("{label} must be a non-negative integer."));
        }
        Ok(value)
    };
    let shift = MediaDateShift {
        days: parse(data.edits[0], "Days")?,
        hours: parse(data.edits[1], "Hours")?,
        minutes: parse(data.edits[2], "Minutes")?,
        seconds: parse(data.edits[3], "Seconds")?,
        subtract: SendMessageW(data.subtract, BM_GETCHECK, 0, 0) == BST_CHECKED as isize,
    };
    if shift.days == 0 && shift.hours == 0 && shift.minutes == 0 && shift.seconds == 0 {
        return Err("Enter a non-zero time shift.".to_string());
    }
    Ok(Some(shift))
}

fn shift_datetime_text(value: &str, shift: MediaDateShift) -> Option<String> {
    let current = NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
    let total = shift
        .days
        .checked_mul(86_400)?
        .checked_add(shift.hours.checked_mul(3_600)?)?
        .checked_add(shift.minutes.checked_mul(60)?)?
        .checked_add(shift.seconds)?;
    let duration = chrono::Duration::try_seconds(total)?;
    let shifted = if shift.subtract {
        current.checked_sub_signed(duration)?
    } else {
        current.checked_add_signed(duration)?
    };
    Some(shifted.format("%Y-%m-%d %H:%M:%S").to_string())
}

unsafe fn stage_shift_media_date(hwnd: HWND) {
    let shift = match show_time_shift_dialog(hwnd, "Shift Media Date") {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(error) => {
            show_error(hwnd, "Unable to Shift Media Date", &error);
            return;
        }
    };
    let Some(s) = state_mut(hwnd) else { return };
    let paths = selected_paths(s);
    let mut staged = 0usize;
    let mut skipped = Vec::new();
    for path in paths {
        let current = if let Some(value) = s
            .pending_edits
            .get(&path)
            .and_then(|edit| edit.taken_date.clone())
        {
            value
        } else if is_jpeg_path(&path) {
            read_exif_metadata(&path).taken_date
        } else if is_png_path(&path) || is_mp4_path(&path) {
            scan_media_file(&path).media_date
        } else {
            String::new()
        };
        let Some(value) = shift_datetime_text(&current, shift) else {
            skipped.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            );
            continue;
        };
        s.pending_edits.entry(path).or_default().taken_date = Some(value);
        staged += 1;
    }
    let row = selected_index(s)
        .try_into()
        .ok()
        .and_then(|index: usize| s.model.rows.get(index).cloned());
    update_metadata_editors(s, row.as_ref());
    update_pending_status(s);
    set_status_text(
        s.status,
        0,
        &format!(
            "Shifted {staged} Media Date value(s); skipped {}. Ctrl+S to save.",
            skipped.len()
        ),
    );
    if staged == 0 {
        show_error(
            hwnd,
            "Unable to Shift Media Date",
            "No selected file has a usable Media Date.",
        );
    }
}

fn combined_gps_datetime(date: &str, time: &str) -> Option<NaiveDateTime> {
    let date = date.trim().replace(':', "-");
    let time = time.trim().trim_end_matches("UTC").trim();
    NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M:%S").ok()
}

unsafe fn stage_shift_gps_date_time(hwnd: HWND) {
    let shift = match show_time_shift_dialog(hwnd, "Shift GPS Date/Time (UTC)") {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(error) => {
            show_error(hwnd, "Unable to Shift GPS Date/Time", &error);
            return;
        }
    };
    let Some(s) = state_mut(hwnd) else { return };
    let paths = selected_paths(s);
    let mut staged = 0usize;
    let mut skipped = 0usize;
    for path in paths {
        if !is_jpeg_path(&path) {
            skipped += 1;
            continue;
        }
        let existing_pending = s.pending_edits.get(&path).cloned().unwrap_or_default();
        let metadata = read_exif_metadata(&path);
        let date = existing_pending.gps_date.unwrap_or(metadata.gps_date_stamp);
        let time = existing_pending.gps_time.unwrap_or(metadata.gps_time_stamp);
        let Some(current) = combined_gps_datetime(&date, &time) else {
            skipped += 1;
            continue;
        };
        let Some(shifted) =
            shift_datetime_text(&current.format("%Y-%m-%d %H:%M:%S").to_string(), shift)
        else {
            skipped += 1;
            continue;
        };
        let Some((new_date, new_time)) = shifted.split_once(' ') else {
            skipped += 1;
            continue;
        };
        let edit = s.pending_edits.entry(path).or_default();
        edit.gps_date = Some(new_date.to_string());
        edit.gps_time = Some(new_time.to_string());
        staged += 1;
    }
    update_pending_status(s);
    set_status_text(
        s.status,
        0,
        &format!(
            "Shifted GPS Date/Time for {staged} JPEG file(s); skipped {skipped}. Ctrl+S to save."
        ),
    );
    if staged == 0 {
        show_error(
            hwnd,
            "Unable to Shift GPS Date/Time",
            "No selected JPEG has both GPS Date Stamp and GPS Time Stamp.",
        );
    } else {
        update_selection_panels(hwnd, s);
    }
}

unsafe fn update_pending_status(s: &WindowState) {
    let enabled = !s.pending_edits.is_empty();
    SendMessageW(
        s.toolbar,
        TB_ENABLEBUTTON,
        ID_TOOL_SAVE as usize,
        enabled as LPARAM,
    );
    SendMessageW(
        s.toolbar,
        TB_ENABLEBUTTON,
        ID_TOOL_REVERT as usize,
        enabled as LPARAM,
    );
    if s.pending_edits.is_empty() {
        set_status_text(s.status, 0, "");
    } else {
        set_status_text(
            s.status,
            0,
            &format!("{} file(s) staged. Ctrl+S to save.", s.pending_edits.len()),
        );
    }
}

unsafe fn detail_value_is_staged(s: &WindowState, label: &str) -> bool {
    let Some(key) = exif_key_for_detail_label(label) else {
        return false;
    };
    selected_files(s).iter().any(|path| {
        s.pending_edits.get(path).is_some_and(|edit| match key {
            "taken_date" => edit.taken_date.is_some(),
            "utc_offset" => edit.utc_offset.is_some(),
            other => edit.tag_values.contains_key(other),
        })
    })
}

fn is_detail_section(label: &str) -> bool {
    matches!(
        label,
        "FILE" | "SELECTION" | "VIDEO" | "CAMERA" | "EXPOSURE" | "LOCATION" | "OTHER METADATA"
    )
}

unsafe fn reload_directory(hwnd: HWND, preserve: bool) {
    let Some(s) = state(hwnd) else { return };
    let selected = if preserve {
        selected_paths(s)
    } else {
        Vec::new()
    };
    match FileListModel::load(
        s.model.directory.clone(),
        s.model.filter,
        s.model.time_display_mode,
    ) {
        Ok(mut model) => {
            if let Some(s) = state_mut(hwnd) {
                model.set_special_filters(
                    s.show_only_missing_date,
                    s.show_only_missing_offset,
                    s.show_only_duplicate_date,
                );
                s.model = model;
            }
            refresh_list(hwnd, false);
            if let Some(s) = state(hwnd) {
                restore_selection(s, &selected);
                update_status(s);
            }
        }
        Err(error) => show_error(hwnd, "Refresh Failed", &error),
    }
}

unsafe fn set_filter(hwnd: HWND, filter: FileFilter, command: usize) {
    if let Some(s) = state_mut(hwnd) {
        s.model.filter = filter;
        CheckMenuRadioItem(
            s.view_menu,
            ID_VIEW_SUPPORTED as u32,
            ID_VIEW_ALL as u32,
            command as u32,
            MF_BYCOMMAND,
        );
    }
    reload_directory(hwnd, false);
}

unsafe fn handle_notify(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let header = &*(lparam as *const NMHDR);
    if state(hwnd).is_some_and(|s| header.hwndFrom == s.details) {
        if header.code == NM_CUSTOMDRAW {
            let draw = &mut *(lparam as *mut NMLVCUSTOMDRAW);
            return match draw.nmcd.dwDrawStage {
                CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
                CDDS_ITEMPREPAINT => {
                    let details = state(hwnd).map(|s| s.details).unwrap_or(null_mut());
                    let label = list_view_cell_text(details, draw.nmcd.dwItemSpec as i32, 0);
                    let mut row = draw.nmcd.rc;
                    let mut client: RECT = zeroed();
                    GetClientRect(details, &mut client);
                    row.left = client.left;
                    row.right = client.right;
                    let selected = detail_row_is_selected(details, draw.nmcd.dwItemSpec as i32);
                    let background = if selected {
                        rgb(232, 235, 239)
                    } else if is_detail_section(&label) {
                        rgb(240, 242, 245)
                    } else {
                        rgb(255, 255, 255)
                    };
                    let brush = CreateSolidBrush(background);
                    FillRect(draw.nmcd.hdc, &row, brush);
                    DeleteObject(brush);
                    CDRF_NOTIFYSUBITEMDRAW as LRESULT
                }
                stage if stage == (CDDS_ITEMPREPAINT | CDDS_SUBITEM) => {
                    let index = draw.nmcd.dwItemSpec as i32;
                    let label = state(hwnd)
                        .map(|s| list_view_cell_text(s.details, index, 0))
                        .unwrap_or_default();
                    let value = state(hwnd)
                        .map(|s| list_view_cell_text(s.details, index, 1))
                        .unwrap_or_default();
                    let section = is_detail_section(&label);
                    let staged = state(hwnd).is_some_and(|s| detail_value_is_staged(s, &label));
                    let selected = state(hwnd).is_some_and(|s| {
                        detail_row_is_selected(s.details, draw.nmcd.dwItemSpec as i32)
                    });
                    let background = if selected {
                        rgb(232, 235, 239)
                    } else if section {
                        rgb(240, 242, 245)
                    } else {
                        rgb(255, 255, 255)
                    };
                    let foreground = if section {
                        rgb(30, 41, 59)
                    } else if staged && draw.iSubItem > 0 {
                        rgb(30, 92, 160)
                    } else if draw.iSubItem == 0 {
                        rgb(100, 116, 139)
                    } else {
                        rgb(30, 41, 59)
                    };
                    let brush = CreateSolidBrush(background);
                    let cell = draw.nmcd.rc;
                    FillRect(draw.nmcd.hdc, &cell, brush);
                    DeleteObject(brush);
                    let mut text = if draw.iSubItem == 0 {
                        label
                    } else if label == "UTC Offset" && !section {
                        format!("{value}     ▾")
                    } else {
                        value
                    };
                    if !text.is_empty() {
                        let mut text_rect = cell;
                        text_rect.left += if section {
                            8
                        } else if draw.iSubItem == 0 {
                            16
                        } else {
                            6
                        };
                        text_rect.right -= 6;
                        SetBkMode(draw.nmcd.hdc, TRANSPARENT as i32);
                        SetTextColor(draw.nmcd.hdc, foreground);
                        let encoded = wide(&text);
                        DrawTextW(
                            draw.nmcd.hdc,
                            encoded.as_ptr(),
                            -1,
                            &mut text_rect,
                            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                        );
                        text.clear();
                    }
                    CDRF_SKIPDEFAULT as LRESULT
                }
                _ => CDRF_DODEFAULT as LRESULT,
            };
        } else if header.code == NM_CLICK {
            let activate = &*(lparam as *const NMITEMACTIVATE);
            if activate.iItem >= 0 {
                if let Some(s) = state(hwnd) {
                    select_detail_row(s.details, activate.iItem);
                }
            }
        } else if header.code == NM_RCLICK {
            let activate = &*(lparam as *const NMITEMACTIVATE);
            if activate.iItem >= 0 {
                if let Some(s) = state(hwnd) {
                    select_detail_row(s.details, activate.iItem);
                }
            }
            show_details_context_menu(hwnd);
        } else if header.code == NM_DBLCLK {
            let activate = &*(lparam as *const NMITEMACTIVATE);
            if activate.iItem >= 0 {
                edit_detail_at(hwnd, activate.iItem);
            }
        }
        return 0;
    }
    let Some(s) = state_mut(hwnd) else { return 0 };
    if header.hwndFrom != s.list {
        return 0;
    }
    match header.code {
        NM_CUSTOMDRAW => {
            let draw = &mut *(lparam as *mut NMLVCUSTOMDRAW);
            match draw.nmcd.dwDrawStage {
                CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
                CDDS_ITEMPREPAINT => CDRF_NOTIFYSUBITEMDRAW as LRESULT,
                stage if stage == (CDDS_ITEMPREPAINT | CDDS_SUBITEM) => {
                    let index = draw.nmcd.dwItemSpec;
                    if s.model.sort_subitem() == Some(draw.iSubItem)
                        && s.model.sorted_value_is_duplicated(index)
                    {
                        draw.clrText = rgb(202, 80, 16);
                    }
                    CDRF_DODEFAULT as LRESULT
                }
                _ => CDRF_DODEFAULT as LRESULT,
            }
        }
        LVN_GETDISPINFOW => {
            let info = &mut *(lparam as *mut NMLVDISPINFOW);
            if info.item.iItem >= 0 {
                if let Some(row) = s.model.rows.get(info.item.iItem as usize) {
                    if info.item.mask & LVIF_IMAGE != 0 {
                        info.item.iImage = row.icon_index;
                    }
                    if info.item.mask & LVIF_TEXT == 0 {
                        return 0;
                    }
                    write_wide_buffer(
                        info.item.pszText,
                        info.item.cchTextMax,
                        &row.text(info.item.iSubItem as usize),
                    );
                }
            }
            0
        }
        LVN_ITEMCHANGED => {
            if !s.selection_update_queued {
                s.selection_update_queued = true;
                PostMessageW(hwnd, WM_SELECTION_SETTLED, 0, 0);
            }
            0
        }
        LVN_COLUMNCLICK => {
            let item = &*(lparam as *const NMLISTVIEW);
            let column = match item.iSubItem {
                1 => SortColumn::Size,
                2 => SortColumn::Modified,
                3 => SortColumn::MediaDate,
                _ => SortColumn::Name,
            };
            let selected = selected_paths(s);
            s.model.sort_by(column);
            if !s.model.rows.is_empty() {
                SendMessageW(
                    s.list,
                    LVM_REDRAWITEMS,
                    0,
                    (s.model.rows.len() - 1) as LPARAM,
                );
            }
            restore_selection(s, &selected);
            0
        }
        LVN_KEYDOWN => {
            let key = &*(lparam as *const NMLVKEYDOWN);
            match key.wVKey {
                VK_F2 => edit_selected(s),
                VK_DELETE => delete_selected(hwnd),
                VK_RETURN => activate_selected(hwnd),
                0x44 if GetKeyState(VK_CONTROL as i32) < 0 => delete_selected(hwnd),
                _ => {}
            }
            0
        }
        LVN_ODFINDITEMW => {
            let find = &*(lparam as *const NMLVFINDITEMW);
            if find.lvfi.psz.is_null() || s.model.rows.is_empty() {
                return -1;
            }
            let prefix = from_wide_ptr(find.lvfi.psz).to_lowercase();
            let start = (find.iStart.max(-1) as usize).wrapping_add(1) % s.model.rows.len();
            for offset in 0..s.model.rows.len() {
                let index = (start + offset) % s.model.rows.len();
                if s.model.rows[index].name.to_lowercase().starts_with(&prefix) {
                    return index as LRESULT;
                }
            }
            -1
        }
        LVN_ENDLABELEDITW => finish_rename(hwnd, lparam),
        NM_DBLCLK => {
            activate_selected(hwnd);
            0
        }
        NM_RCLICK => {
            show_file_context_menu(hwnd);
            0
        }
        _ => 0,
    }
}

unsafe fn list_view_cell_text(list: HWND, index: i32, subitem: i32) -> String {
    let mut buffer = vec![0u16; 2048];
    let mut item: LVITEMW = zeroed();
    item.iSubItem = subitem;
    item.pszText = buffer.as_mut_ptr();
    item.cchTextMax = buffer.len() as i32;
    let length = SendMessageW(
        list,
        LVM_GETITEMTEXTW,
        index as usize,
        &mut item as *mut _ as LPARAM,
    ) as usize;
    String::from_utf16_lossy(&buffer[..length.min(buffer.len())])
}

unsafe fn select_detail_row(details: HWND, index: i32) {
    let mut clear: LVITEMW = zeroed();
    clear.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
    SendMessageW(
        details,
        LVM_SETITEMSTATE,
        usize::MAX,
        &clear as *const _ as LPARAM,
    );
    let mut select: LVITEMW = zeroed();
    select.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
    select.state = LVIS_SELECTED | LVIS_FOCUSED;
    SendMessageW(
        details,
        LVM_SETITEMSTATE,
        index as usize,
        &select as *const _ as LPARAM,
    );
    InvalidateRect(details, null(), 0);
}

unsafe fn detail_row_is_selected(details: HWND, index: i32) -> bool {
    SendMessageW(
        details,
        LVM_GETNEXTITEM,
        usize::MAX,
        LVNI_SELECTED as LPARAM,
    ) as i32
        == index
}

fn exif_key_for_detail_label(label: &str) -> Option<&'static str> {
    match label {
        "Date Taken" | "Media Creation Time" => Some("taken_date"),
        "UTC Offset" => Some("utc_offset"),
        "Camera Make" => Some("camera_make"),
        "Camera Model" => Some("camera_model"),
        "Lens Model" => Some("lens_model"),
        "Software" => Some("software"),
        "Artist" => Some("artist"),
        "Shutter Speed" | "Exposure Time" => Some("shutter_speed"),
        "Aperture" | "F-number" => Some("aperture"),
        "ISO Speed" | "ISO" => Some("iso_speed"),
        "Focal Length" => Some("focal_length"),
        "35mm Focal Length" | "35 mm Equivalent" => Some("focal_length_35mm"),
        "Flash" => Some("flash_fired"),
        "Metering Mode" => Some("metering_mode"),
        "Exposure Program" => Some("exposure_program"),
        "White Balance" => Some("white_balance"),
        "Latitude" => Some("gps_latitude"),
        "Longitude" => Some("gps_longitude"),
        "Altitude" => Some("gps_altitude"),
        "GPS Date" => Some("gps_date_stamp"),
        "GPS Time" => Some("gps_time_stamp"),
        "Orientation" => Some("orientation"),
        "Color Space" => Some("color_space"),
        "Image Width" => Some("image_width"),
        "Image Height" => Some("image_height"),
        "Description" => Some("image_description"),
        "Copyright" => Some("copyright"),
        "EXIF Version" => Some("exif_version"),
        "Date/Time Original" => Some("date_time_original"),
        "Date/Time Digitized" => Some("date_time_digitized"),
        "Image Date/Time" => Some("image_date_time"),
        _ => None,
    }
}

fn detail_key_is_editable(key: &str) -> bool {
    matches!(
        key,
        "taken_date"
            | "utc_offset"
            | "camera_make"
            | "camera_model"
            | "lens_model"
            | "software"
            | "artist"
            | "shutter_speed"
            | "aperture"
            | "iso_speed"
            | "focal_length"
            | "flash_fired"
            | "metering_mode"
            | "orientation"
            | "color_space"
    )
}

unsafe fn edit_detail_at(hwnd: HWND, index: i32) {
    cancel_inline_detail_edit(hwnd);
    let Some(s) = state(hwnd) else { return };
    if index < 0 {
        return;
    }
    select_detail_row(s.details, index);
    let selected = selected_files(s);
    if selected.is_empty() {
        return;
    }
    let label = list_view_cell_text(s.details, index, 0);
    let current = list_view_cell_text(s.details, index, 1);
    let Some(key) = exif_key_for_detail_label(&label).filter(|key| detail_key_is_editable(key))
    else {
        return;
    };
    if key == "utc_offset" && !selected.iter().all(|path| is_jpeg_path(path)) {
        return;
    }
    if key != "taken_date"
        && key != "utc_offset"
        && (selected.len() != 1 || !is_jpeg_path(&selected[0]))
    {
        return;
    }

    let mut rect = RECT {
        left: LVIR_BOUNDS as i32,
        top: 1,
        right: 0,
        bottom: 0,
    };
    if SendMessageW(
        s.details,
        LVM_GETSUBITEMRECT,
        index as usize,
        &mut rect as *mut _ as LPARAM,
    ) == 0
    {
        return;
    }
    let edit = if key == "utc_offset" {
        s.detail_inline_offset
    } else {
        s.detail_inline_edit
    };
    let paths = selected;
    let mut origin = POINT {
        x: rect.left,
        y: rect.top,
    };
    MapWindowPoints(s.details, hwnd, &mut origin, 1);
    if key == "utc_offset" {
        show_offset_menu_for_detail(
            hwnd,
            origin,
            (rect.right - rect.left).max(40),
            (rect.bottom - rect.top).max(22),
            &current,
            paths,
            index,
        );
        return;
    }
    set_window_text(edit, &current);
    SetWindowPos(
        edit,
        null_mut(),
        origin.x,
        origin.y + 1,
        (rect.right - rect.left).max(40),
        (rect.bottom - rect.top - 2).max(20),
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
    );
    if let Some(s) = state_mut(hwnd) {
        s.detail_inline_index = index;
        s.detail_inline_key = Some(key.to_string());
        s.detail_inline_paths = paths;
    }
    ShowWindow(s.detail_inline_offset, SW_HIDE);
    ShowWindow(edit, SW_SHOW);
    SetFocus(edit);
    if key == "taken_date" {
        SendMessageW(edit, EM_SETSEL, 17, 19);
    } else {
        SendMessageW(edit, EM_SETSEL, 0, -1);
    }
}

unsafe fn show_offset_menu_for_detail(
    hwnd: HWND,
    origin: POINT,
    width: i32,
    row_height: i32,
    current: &str,
    paths: Vec<PathBuf>,
    index: i32,
) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let choices = [
        (1usize, "+00:00", "UTC (+00:00) · International"),
        (2, "+01:00", "CET (+01:00) · Central Europe"),
        (3, "+02:00", "CEST (+02:00) · Central Europe"),
        (4, "+09:00", "KST (+09:00) · South Korea"),
        (5, "-07:00", "PDT (-07:00) · United States / Canada"),
        (6, "-08:00", "PST (-08:00) · United States / Canada"),
    ];
    let normalized = normalize_offset_combo_text(current);
    for (command, value, label) in choices {
        let _ = item(menu, command, label, normalized == value);
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    let _ = item(menu, 7, "Custom UTC offset...", false);
    let mut anchor = POINT {
        x: origin.x,
        y: origin.y + row_height,
    };
    ClientToScreen(hwnd, &mut anchor);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        anchor.x,
        anchor.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    if command == 0 {
        SetFocus(state(hwnd).map(|s| s.list).unwrap_or(null_mut()));
        return;
    }
    if command == 7 {
        let Some(s) = state_mut(hwnd) else { return };
        s.detail_inline_index = index;
        s.detail_inline_key = Some("utc_offset".to_string());
        s.detail_inline_paths = paths;
        set_window_text(
            s.detail_inline_edit,
            if normalized.is_empty() {
                "+00:00"
            } else {
                &normalized
            },
        );
        position_child(
            s.detail_inline_edit,
            origin.x,
            origin.y + 2,
            width,
            (row_height - 4).max(20),
        );
        ShowWindow(s.detail_inline_offset, SW_HIDE);
        ShowWindow(s.detail_inline_edit, SW_SHOW);
        SetFocus(s.detail_inline_edit);
        SendMessageW(s.detail_inline_edit, EM_SETSEL, 0, -1);
        return;
    }
    let value = choices
        .iter()
        .find_map(|(id, value, _)| (*id == command as usize).then_some(*value))
        .unwrap_or("+00:00");
    let Some(s) = state_mut(hwnd) else { return };
    s.detail_inline_index = index;
    s.detail_inline_key = Some("utc_offset".to_string());
    s.detail_inline_paths = paths;
    set_window_text(s.detail_inline_edit, value);
    commit_inline_detail_edit(hwnd);
}

unsafe fn cancel_inline_detail_edit(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    if s.detail_inline_key.is_none() {
        return;
    }
    s.detail_inline_index = -1;
    s.detail_inline_key = None;
    s.detail_inline_paths.clear();
    ShowWindow(s.detail_inline_edit, SW_HIDE);
    ShowWindow(s.detail_inline_offset, SW_HIDE);
    SetFocus(s.list);
}

unsafe fn commit_inline_detail_edit(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let Some(key) = s.detail_inline_key.clone() else {
        return;
    };
    let paths = s.detail_inline_paths.clone();
    if paths.is_empty() {
        return;
    }
    let edit_window = if key == "utc_offset" && IsWindowVisible(s.detail_inline_offset) != 0 {
        s.detail_inline_offset
    } else {
        s.detail_inline_edit
    };
    let mut value = window_text(edit_window).trim().to_string();
    if key == "utc_offset" {
        value = normalize_offset_combo_text(&value);
        if parse_numeric_offset_minutes(&value).is_err() {
            show_error(
                hwnd,
                "Invalid UTC Offset",
                "Use a numeric offset such as +09:00 or -07:00.",
            );
            SetFocus(edit_window);
            return;
        }
    }
    let original = state(hwnd)
        .filter(|s| s.detail_inline_index >= 0)
        .map(|s| list_view_cell_text(s.details, s.detail_inline_index, 1))
        .unwrap_or_default();
    if value == original.trim() {
        cancel_inline_detail_edit(hwnd);
        return;
    }

    let Some(s) = state_mut(hwnd) else { return };
    for path in &paths {
        let pending = s.pending_edits.entry(path.clone()).or_default();
        match key.as_str() {
            "taken_date" => pending.taken_date = Some(value.clone()),
            "utc_offset" => pending.utc_offset = Some(value.clone()),
            _ => {
                pending.tag_values.insert(key.clone(), value.clone());
            }
        }
    }
    s.detail_inline_index = -1;
    s.detail_inline_key = None;
    s.detail_inline_paths.clear();
    ShowWindow(s.detail_inline_edit, SW_HIDE);
    ShowWindow(s.detail_inline_offset, SW_HIDE);
    update_pending_status(s);
    if paths.len() == 1 {
        if let Some(row) = s
            .model
            .find_path(&paths[0])
            .and_then(|index| s.model.rows.get(index).cloned())
        {
            populate_details(s.details, &row, s.pending_edits.get(&row.path));
        }
    } else {
        let rows: Vec<_> = paths
            .iter()
            .filter_map(|path| {
                s.model
                    .find_path(path)
                    .and_then(|index| s.model.rows.get(index).cloned())
            })
            .collect();
        populate_multi_details(s.details, &rows, &s.pending_edits);
    }
    set_status_text(
        s.status,
        0,
        &format!(
            "Metadata change staged for {} file(s). Press Ctrl+S to save.",
            paths.len()
        ),
    );
    SetFocus(s.list);
}

unsafe fn copy_text_to_clipboard(owner: HWND, value: &str) -> Result<(), String> {
    let value = wide(value);
    let byte_len = value
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| "Clipboard value is too large.".to_string())?;
    let memory = GlobalAlloc(GMEM_MOVEABLE, byte_len);
    if memory.is_null() {
        return Err("Could not allocate clipboard memory.".to_string());
    }
    let target = GlobalLock(memory) as *mut u16;
    if target.is_null() {
        GlobalFree(memory);
        return Err("Could not lock clipboard memory.".to_string());
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), target, value.len());
    GlobalUnlock(memory);
    if OpenClipboard(owner) == 0 {
        GlobalFree(memory);
        return Err("Could not open the Windows clipboard.".to_string());
    }
    EmptyClipboard();
    let stored = SetClipboardData(CF_UNICODETEXT as u32, memory);
    CloseClipboard();
    if stored.is_null() {
        GlobalFree(memory);
        return Err("Could not write to the Windows clipboard.".to_string());
    }
    Ok(())
}

unsafe fn show_details_context_menu(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let index = SendMessageW(
        s.details,
        LVM_GETNEXTITEM,
        usize::MAX,
        LVNI_SELECTED as LPARAM,
    ) as i32;
    if index < 0 {
        return;
    }
    let label = list_view_cell_text(s.details, index, 0);
    let value = list_view_cell_text(s.details, index, 1);
    if value.trim().is_empty() {
        return;
    }
    let key = exif_key_for_detail_label(&label);
    let selected = selected_files(s);
    let removable = selected.len() == 1 && is_jpeg_path(&selected[0]) && key.is_some();
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let _ = item(menu, ID_DETAIL_COPY_VALUE, "Copy Value", false);
    if removable {
        let _ = item(menu, ID_DETAIL_REMOVE_TAG, "Remove Tag...", false);
    }
    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    match command as usize {
        ID_DETAIL_COPY_VALUE => match copy_text_to_clipboard(hwnd, &value) {
            Ok(()) => {
                if let Some(s) = state(hwnd) {
                    set_status_text(s.status, 0, &format!("Copied {label}."));
                }
            }
            Err(error) => show_error(hwnd, "Copy Value Failed", &error),
        },
        ID_DETAIL_REMOVE_TAG if removable => {
            let path = selected[0].clone();
            let key = key.unwrap();
            if !confirm_operation(
                hwnd,
                "Remove Metadata Tag",
                &format!(
                    "Remove {label} from {}?",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
            ) {
                return;
            }
            let backup = state(hwnd).is_some_and(|s| s.backup_before_changes);
            match remove_exif_tag(&path, key, backup) {
                Ok(()) => {
                    if let Some(s) = state(hwnd) {
                        append_session_log(s, &format!("Removed {label}: {}", path.display()));
                    }
                    reload_directory(hwnd, false);
                    if let Some(s) = state(hwnd) {
                        restore_selection(s, &[path]);
                        SetFocus(s.list);
                    }
                }
                Err(error) => show_error(hwnd, "Remove Tag Failed", &error),
            }
        }
        _ => {}
    }
}

unsafe fn selected_index(s: &WindowState) -> i32 {
    SendMessageW(s.list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM) as i32
}

unsafe fn edit_selected(s: &WindowState) {
    let index = selected_index(s);
    if index >= 0 && !s.model.rows[index as usize].is_parent {
        SendMessageW(s.list, LVM_EDITLABELW, index as usize, 0);
    }
}

unsafe fn finish_rename(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let info = &*(lparam as *const NMLVDISPINFOW);
    if info.item.pszText.is_null() || info.item.iItem < 0 {
        return 0;
    }
    let Some(s) = state(hwnd) else { return 0 };
    let Some(row) = s.model.rows.get(info.item.iItem as usize) else {
        return 0;
    };
    if row.is_parent {
        return 0;
    }
    let name = from_wide_ptr(info.item.pszText);
    let old_path = row.path.clone();
    match rename_entry(&old_path, &name) {
        Ok(path) => {
            if let Some(s) = state_mut(hwnd) {
                if let Some(edit) = s.pending_edits.remove(&old_path) {
                    s.pending_edits.insert(path.clone(), edit);
                }
                append_session_log(
                    s,
                    &format!(
                        "{} -> {}",
                        old_path.file_name().unwrap_or_default().to_string_lossy(),
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                );
            }
            reload_directory(hwnd, false);
            if let Some(s) = state(hwnd) {
                restore_selection(s, &[path]);
            }
            1
        }
        Err(error) => {
            show_error(hwnd, "Rename Failed", &error);
            0
        }
    }
}

unsafe fn activate_selected(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let index = selected_index(s);
    if index < 0 {
        return;
    }
    let row = &s.model.rows[index as usize];
    let path = row.path.clone();
    if row.is_dir {
        load_directory(hwnd, path);
    } else if let Err(error) = open_with_default_application(&path) {
        show_error(hwnd, "Open Failed", &error);
    }
}

unsafe fn reveal_selected(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let index = selected_index(s);
    if index < 0 {
        return;
    }
    if let Err(error) = reveal_in_file_manager(&s.model.rows[index as usize].path) {
        show_error(hwnd, "Open in Explorer Failed", &error);
    }
}

unsafe fn copy_selected_files(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    let files = selected_files(s);
    if files.is_empty() {
        show_error(hwnd, "Copy Failed", "Select one or more files first.");
        return;
    }
    s.copied_files = files;
    set_status_text(
        s.status,
        0,
        &format!("Copied {} file(s).", s.copied_files.len()),
    );
}

unsafe fn paste_copied_files(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let sources = s.copied_files.clone();
    let target = s.model.directory.clone();
    if sources.is_empty() {
        show_error(hwnd, "Paste Failed", "Copy one or more files first.");
        return;
    }
    let mut copied = Vec::new();
    let mut failures = Vec::new();
    for source in &sources {
        match copy_file_to_folder(source, &target) {
            Ok(path) => copied.push(path),
            Err(error) => failures.push(format!(
                "{}: {error}",
                source.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    if let Some(s) = state(hwnd) {
        append_session_log(
            s,
            &format!("Pasted {} file(s) into {}", copied.len(), target.display()),
        );
    }
    reload_directory(hwnd, false);
    if let Some(s) = state(hwnd) {
        restore_selection(s, &copied);
        SetFocus(s.list);
    }
    if !failures.is_empty() {
        show_error(
            hwnd,
            "Paste Partially Completed",
            &format!(
                "Copied {}; failed {}.\n\n{}",
                copied.len(),
                failures.len(),
                failures.join("\n")
            ),
        );
    }
}

unsafe fn save_copy_selected(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let sources = selected_files(s);
    if sources.is_empty() {
        show_error(hwnd, "Save Copy Failed", "Select one or more files first.");
        return;
    }
    let target = match choose_folder() {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            show_error(hwnd, "Save Copy Failed", &error);
            return;
        }
    };
    let mut completed = 0usize;
    let mut failures = Vec::new();
    for source in &sources {
        match copy_file_to_folder(source, &target) {
            Ok(_) => completed += 1,
            Err(error) => failures.push(format!(
                "{}: {error}",
                source.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    if let Some(s) = state(hwnd) {
        append_session_log(
            s,
            &format!("Saved {completed} file copy/copies to {}", target.display()),
        );
    }
    show_operation_result(hwnd, "Save Copy", completed, 0, &failures);
}

unsafe fn show_file_context_menu(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    if SendMessageW(s.list, LVM_GETSELECTEDCOUNT, 0, 0) == 0 {
        return;
    }
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let _ = item(menu, ID_ITEM_OPEN, "Open", false);
    let _ = item(menu, ID_ITEM_REVEAL, "Open in Explorer", false);
    let _ = item(menu, ID_ITEM_COPY, "Copy\tCtrl+C", false);
    let _ = item(menu, ID_ITEM_PASTE, "Paste\tCtrl+V", false);
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    let _ = item(
        menu,
        ID_DATE_FROM_FILENAME,
        "Set Date Taken from Filename",
        false,
    );
    let _ = item(
        menu,
        ID_DATE_FROM_MODIFIED,
        "Set Date Taken from File Modified",
        false,
    );
    let _ = item(
        menu,
        ID_DATE_FROM_CREATED,
        "Set Date Taken from File Created",
        false,
    );
    let _ = item(
        menu,
        ID_FILENAME_FROM_MEDIA,
        "Rename from Media Date...",
        false,
    );
    let _ = item(menu, ID_SHIFT_MEDIA_DATE, "Shift Media Date...", false);
    let _ = item(
        menu,
        ID_SHIFT_GPS_DATE_TIME,
        "Shift GPS Date/Time...",
        false,
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    let _ = item(menu, ID_EXIF_CREATE, "Create EXIF Structure", false);
    let _ = item(menu, ID_EXIF_COPY, "Copy Complete EXIF", false);
    let _ = item(menu, ID_EXIF_PASTE, "Paste Complete EXIF...", false);
    let _ = item(menu, ID_GPS_REMOVE, "Remove GPS Information...", false);
    let _ = item(
        menu,
        ID_EXTRACT_THUMBNAIL,
        "Extract Embedded Thumbnail",
        false,
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    let _ = item(menu, ID_ITEM_RENAME, "Rename\tF2", false);
    let _ = item(menu, ID_ITEM_DELETE, "Delete\tDel", false);
    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    if command != 0 {
        SendMessageW(hwnd, WM_COMMAND, command as usize, 0);
    }
}

unsafe fn show_quick_action_menu(hwnd: HWND, metadata: bool) {
    let Some(s) = state(hwnd) else { return };
    let anchor_control = if metadata {
        s.quick_metadata
    } else {
        s.quick_file
    };
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    if metadata {
        let _ = item(
            menu,
            ID_DATE_FROM_FILENAME,
            "Set Media Date from Filename",
            false,
        );
        let _ = item(
            menu,
            ID_DATE_FROM_MODIFIED,
            "Set Media Date from File Timestamp",
            false,
        );
        let _ = item(menu, ID_SHIFT_MEDIA_DATE, "Shift Media Date...", false);
    } else {
        let _ = item(menu, ID_ITEM_OPEN, "Open", false);
        let _ = item(menu, ID_ITEM_REVEAL, "Open in Explorer", false);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let _ = item(menu, ID_ITEM_RENAME, "Rename", false);
        let _ = item(menu, ID_ITEM_DELETE, "Delete", false);
    }
    let mut anchor: RECT = zeroed();
    GetWindowRect(anchor_control, &mut anchor);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        anchor.left,
        anchor.bottom,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    if command != 0 {
        SendMessageW(hwnd, WM_COMMAND, command as WPARAM, 0);
    }
}

unsafe fn stage_date_from_filename(hwnd: HWND) {
    stage_selected_dates(hwnd, "filename", |row| {
        extract_datetime_from_filename(&row.path)
    });
}

unsafe fn stage_date_from_modified(hwnd: HWND) {
    stage_selected_dates(hwnd, "File Modified", |row| {
        row.modified.map(|value| format_system_time(Some(value)))
    });
}

unsafe fn stage_date_from_created(hwnd: HWND) {
    stage_selected_dates(hwnd, "File Created", |row| {
        row.created.map(|value| format_system_time(Some(value)))
    });
}

fn parse_local_system_time(value: &str) -> Result<SystemTime, String> {
    let naive = NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S")
        .map_err(|_| "Expected Media Date format: YYYY-MM-DD HH:MM:SS".to_string())?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(Into::into)
        .ok_or_else(|| "The Media Date is invalid or ambiguous in the local time zone.".to_string())
}

unsafe fn set_selected_file_dates_from_media(hwnd: HWND) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let Some(s) = state(hwnd) else { return };
    let paths = selected_files(s);
    if paths.is_empty() {
        show_error(
            hwnd,
            "Unable to Set File Dates",
            "Select one or more files first.",
        );
        return;
    }
    if !confirm_operation(
        hwnd,
        "Set File Dates",
        &format!(
            "Set both File Created and File Modified from Media Date for {} selected file(s)?",
            paths.len()
        ),
    ) {
        return;
    }
    let mut updated = 0usize;
    let mut failures = Vec::new();
    for path in &paths {
        let scan = scan_media_file(path);
        match parse_local_system_time(&scan.media_date)
            .and_then(|time| set_file_times(path, Some(time), Some(time)))
        {
            Ok(()) => updated += 1,
            Err(error) => failures.push(format!(
                "{}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    if let Some(s) = state(hwnd) {
        append_session_log(
            s,
            &format!("Set file Created/Modified from Media Date: {updated} updated"),
        );
    }
    reload_directory(hwnd, true);
    if failures.is_empty() {
        if let Some(s) = state(hwnd) {
            set_status_text(
                s.status,
                0,
                &format!("Updated file dates for {updated} file(s)."),
            );
        }
    } else {
        show_error(
            hwnd,
            "File Dates Partially Updated",
            &format!(
                "Updated {updated}; failed {}.\n\n{}",
                failures.len(),
                failures.join("\n")
            ),
        );
    }
}

unsafe fn set_selected_modified_from_created(hwnd: HWND) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let Some(s) = state(hwnd) else { return };
    let paths = selected_files(s);
    if paths.is_empty() {
        show_error(
            hwnd,
            "Unable to Set Modified Date",
            "Select one or more files first.",
        );
        return;
    }
    if !confirm_operation(
        hwnd,
        "Set Modified Date",
        &format!(
            "Set File Modified from File Created for {} selected file(s)?",
            paths.len()
        ),
    ) {
        return;
    }
    let mut updated = 0usize;
    let mut failures = Vec::new();
    for path in &paths {
        let result = path
            .metadata()
            .map_err(|error| error.to_string())
            .and_then(|metadata| metadata.created().map_err(|error| error.to_string()))
            .and_then(|created| set_file_times(path, None, Some(created)));
        match result {
            Ok(()) => updated += 1,
            Err(error) => failures.push(format!(
                "{}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    if let Some(s) = state(hwnd) {
        append_session_log(s, &format!("Set Modified from Created: {updated} file(s)"));
    }
    reload_directory(hwnd, true);
    if failures.is_empty() {
        if let Some(s) = state(hwnd) {
            set_status_text(
                s.status,
                0,
                &format!("Updated Modified date for {updated} file(s)."),
            );
        }
    } else {
        show_error(
            hwnd,
            "Modified Date Partially Updated",
            &format!(
                "Updated {updated}; failed {}.\n\n{}",
                failures.len(),
                failures.join("\n")
            ),
        );
    }
}

unsafe fn rename_selected_from_media_date(hwnd: HWND) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let Some(s) = state(hwnd) else { return };
    let paths = selected_files(s);
    if paths.is_empty() {
        show_error(hwnd, "Unable to Rename", "Select one or more files first.");
        return;
    }
    if !confirm_operation(
        hwnd,
        "Rename from Media Date",
        &format!(
            "Rename {} selected file(s) to YYYYMMDD_HHMMSS?\n\nFilename collisions use the common _dupNNN rule.",
            paths.len()
        ),
    ) {
        return;
    }
    let mut resolver = FilenameCollisionResolver::new();
    let mut renamed = Vec::new();
    let mut failures = Vec::new();
    for source in paths {
        let scan = scan_media_file(&source);
        let result = NaiveDateTime::parse_from_str(&scan.media_date, "%Y-%m-%d %H:%M:%S")
            .map_err(|_| "No usable Media Date.".to_string())
            .and_then(|date| {
                let base = date.format("%Y%m%d_%H%M%S");
                let desired = match source.extension().and_then(|value| value.to_str()) {
                    Some(extension) if !extension.is_empty() => {
                        source.with_file_name(format!("{base}.{extension}"))
                    }
                    _ => source.with_file_name(base.to_string()),
                };
                resolver
                    .resolve_for_rename(&desired, &source)
                    .and_then(|target| {
                        let name = target
                            .file_name()
                            .ok_or_else(|| "Could not resolve target filename.".to_string())?;
                        rename_entry(&source, &name.to_string_lossy())
                    })
            });
        match result {
            Ok(target) => renamed.push((source, target)),
            Err(error) => failures.push(format!(
                "{}: {error}",
                source.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    if let Some(s) = state(hwnd) {
        for (source, target) in &renamed {
            append_session_log(
                s,
                &format!(
                    "{} -> {}",
                    source.file_name().unwrap_or_default().to_string_lossy(),
                    target.file_name().unwrap_or_default().to_string_lossy()
                ),
            );
        }
    }
    let targets: Vec<_> = renamed.iter().map(|(_, target)| target.clone()).collect();
    reload_directory(hwnd, false);
    if let Some(s) = state(hwnd) {
        restore_selection(s, &targets);
        SetFocus(s.list);
    }
    if !failures.is_empty() {
        show_error(
            hwnd,
            "Rename Partially Completed",
            &format!(
                "Renamed {}; failed {}.\n\n{}",
                renamed.len(),
                failures.len(),
                failures.join("\n")
            ),
        );
    }
}

unsafe fn stage_selected_dates<F>(hwnd: HWND, source: &str, mut resolve: F)
where
    F: FnMut(&super::model::FileRow) -> Option<String>,
{
    let Some(s) = state_mut(hwnd) else { return };
    let paths = selected_paths(s);
    if paths.is_empty() {
        return;
    }
    let mut staged = 0usize;
    let mut skipped = Vec::new();
    for path in paths {
        let Some(index) = s.model.find_path(&path) else {
            continue;
        };
        let row = &s.model.rows[index];
        if row.is_dir || (!is_jpeg_path(&path) && !is_png_path(&path) && !is_mp4_path(&path)) {
            skipped.push(row.name.clone());
            continue;
        }
        let Some(value) = resolve(row).filter(|value| !value.is_empty() && value != "-") else {
            skipped.push(row.name.clone());
            continue;
        };
        s.pending_edits.entry(path).or_default().taken_date = Some(value);
        staged += 1;
    }
    let row = selected_index(s)
        .try_into()
        .ok()
        .and_then(|index: usize| s.model.rows.get(index).cloned());
    update_metadata_editors(s, row.as_ref());
    update_pending_status(s);
    if staged > 0 {
        set_status_text(
            s.status,
            0,
            &format!("Staged Date Taken from {source} for {staged} file(s). Ctrl+S to save."),
        );
    }
    if staged == 0 {
        show_error(
            hwnd,
            "Unable to Set Date Taken",
            &format!("No usable date was found in the selected file(s) from {source}."),
        );
    } else if !skipped.is_empty() {
        show_information(
            hwnd,
            "Date Taken Staged",
            &format!(
                "Staged {staged} file(s); skipped {}.\n\n{}",
                skipped.len(),
                skipped.join("\n")
            ),
        );
    }
}

unsafe fn selected_files(s: &WindowState) -> Vec<PathBuf> {
    selected_paths(s)
        .into_iter()
        .filter(|path| {
            s.model
                .rows
                .iter()
                .find(|row| row.path == *path)
                .is_some_and(|row| !row.is_dir && !row.is_parent)
        })
        .collect()
}

unsafe fn select_matching_rows<F>(hwnd: HWND, mut predicate: F)
where
    F: FnMut(&super::model::FileRow) -> bool,
{
    let Some(s) = state(hwnd) else { return };
    let mut clear: LVITEMW = zeroed();
    clear.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
    SendMessageW(
        s.list,
        LVM_SETITEMSTATE,
        usize::MAX,
        &clear as *const _ as LPARAM,
    );
    let mut first = None;
    for (index, row) in s.model.rows.iter().enumerate() {
        if row.is_parent || !predicate(row) {
            continue;
        }
        let mut item: LVITEMW = zeroed();
        item.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
        item.state = LVIS_SELECTED | if first.is_none() { LVIS_FOCUSED } else { 0 };
        SendMessageW(s.list, LVM_SETITEMSTATE, index, &item as *const _ as LPARAM);
        first.get_or_insert(index);
    }
    if let Some(index) = first {
        SendMessageW(s.list, LVM_ENSUREVISIBLE, index, 0);
    } else {
        show_information(hwnd, "Select Files", "No matching files were found.");
    }
    SetFocus(s.list);
}

unsafe fn select_duplicate_media_dates(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    if s.pending_scans > 0 {
        show_information(hwnd, "Analyze", "Wait until metadata scanning is complete.");
        return;
    }
    let mut counts = HashMap::<String, usize>::new();
    for row in &s.model.rows {
        if !row.is_dir && row.media_date != "-" && row.media_date != "..." {
            *counts.entry(row.media_date.clone()).or_default() += 1;
        }
    }
    select_matching_rows(hwnd, |row| {
        counts.get(&row.media_date).copied().unwrap_or(0) > 1
    });
}

unsafe fn analyze_exact_duplicates(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    if s.pending_scans > 0 {
        show_information(
            hwnd,
            "Analyze Duplicates",
            "Wait until folder scanning is complete.",
        );
        return;
    }
    set_status_text(s.status, 0, "Comparing duplicate candidates...");
    UpdateWindow(hwnd);
    let mut groups = HashMap::<u64, Vec<PathBuf>>::new();
    for row in &s.model.rows {
        if !row.is_dir {
            groups.entry(row.size).or_default().push(row.path.clone());
        }
    }
    let mut duplicates = std::collections::HashSet::new();
    let mut failures = Vec::new();
    for candidates in groups.into_values().filter(|group| group.len() > 1) {
        let mut representatives: Vec<PathBuf> = Vec::new();
        for candidate in candidates {
            let mut matched = false;
            for representative in &representatives {
                match files_equal(representative, &candidate) {
                    Ok(true) => {
                        duplicates.insert(representative.clone());
                        duplicates.insert(candidate.clone());
                        matched = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        failures.push(format!("{}: {error}", candidate.display()));
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                representatives.push(candidate);
            }
        }
    }
    select_matching_rows(hwnd, |row| duplicates.contains(&row.path));
    if let Some(s) = state(hwnd) {
        set_status_text(
            s.status,
            0,
            &format!(
                "Found {} byte-identical duplicate file(s).",
                duplicates.len()
            ),
        );
    }
    if !failures.is_empty() {
        show_error(
            hwnd,
            "Duplicate Analysis Partially Completed",
            &failures.into_iter().take(8).collect::<Vec<_>>().join("\n"),
        );
    }
}

fn files_equal(left: &std::path::Path, right: &std::path::Path) -> Result<bool, String> {
    let mut left = BufReader::with_capacity(
        1024 * 1024,
        std::fs::File::open(left).map_err(|e| e.to_string())?,
    );
    let mut right = BufReader::with_capacity(
        1024 * 1024,
        std::fs::File::open(right).map_err(|e| e.to_string())?,
    );
    let mut left_buffer = vec![0u8; 1024 * 1024];
    let mut right_buffer = vec![0u8; 1024 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer).map_err(|e| e.to_string())?;
        let right_read = right.read(&mut right_buffer).map_err(|e| e.to_string())?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

unsafe fn set_scan_threads(hwnd: HWND, threads: usize, command: usize) {
    let Some(s) = state_mut(hwnd) else { return };
    s.scan_threads = threads;
    CheckMenuRadioItem(
        s.settings_menu,
        ID_SCAN_SINGLE_THREAD as u32,
        ID_SCAN_TWO_THREADS as u32,
        command as u32,
        MF_BYCOMMAND,
    );
    set_status_text(
        s.status,
        0,
        &format!("Metadata scanning: {threads} thread(s)."),
    );
    save_app_settings(AppSettings {
        scan_threads: threads,
        time_display_mode: s.model.time_display_mode,
        backup_before_changes: s.backup_before_changes,
    });
}

unsafe fn set_time_display_mode(hwnd: HWND, mode: TimeDisplayMode, command: usize) {
    let Some(s) = state_mut(hwnd) else { return };
    let selected = selected_paths(s);
    s.model.set_time_display_mode(mode);
    CheckMenuRadioItem(
        s.settings_menu,
        ID_TIME_RECORDED as u32,
        ID_TIME_PST as u32,
        command as u32,
        MF_BYCOMMAND,
    );
    CheckMenuItem(
        s.settings_menu,
        ID_BACKUP_BEFORE_CHANGES as u32,
        MF_BYCOMMAND
            | if s.backup_before_changes {
                MF_CHECKED
            } else {
                0
            },
    );
    SendMessageW(s.list, LVM_SETITEMCOUNT, s.model.rows.len(), 0);
    InvalidateRect(s.list, null(), 1);
    restore_selection(s, &selected);
    update_selection_panels(hwnd, s);
    let label = match mode {
        TimeDisplayMode::Recorded => "Recorded / File Basis",
        TimeDisplayMode::Utc => "UTC (+00:00)",
        TimeDisplayMode::Kst => "KST (+09:00)",
        TimeDisplayMode::Pdt => "PDT (-07:00)",
        TimeDisplayMode::Pst => "PST (-08:00)",
    };
    set_status_text(s.status, 0, &format!("Media Date display: {label}."));
    save_app_settings(AppSettings {
        scan_threads: s.scan_threads,
        time_display_mode: mode,
        backup_before_changes: s.backup_before_changes,
    });
}

unsafe fn sync_settings_menu(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let scan_command = if s.scan_threads == 1 {
        ID_SCAN_SINGLE_THREAD
    } else {
        ID_SCAN_TWO_THREADS
    };
    CheckMenuRadioItem(
        s.settings_menu,
        ID_SCAN_SINGLE_THREAD as u32,
        ID_SCAN_TWO_THREADS as u32,
        scan_command as u32,
        MF_BYCOMMAND,
    );
    let time_command = match s.model.time_display_mode {
        TimeDisplayMode::Recorded => ID_TIME_RECORDED,
        TimeDisplayMode::Utc => ID_TIME_UTC,
        TimeDisplayMode::Kst => ID_TIME_KST,
        TimeDisplayMode::Pdt => ID_TIME_PDT,
        TimeDisplayMode::Pst => ID_TIME_PST,
    };
    CheckMenuRadioItem(
        s.settings_menu,
        ID_TIME_RECORDED as u32,
        ID_TIME_PST as u32,
        time_command as u32,
        MF_BYCOMMAND,
    );
}

fn app_settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from).map(|path| {
        path.join("SH148 Photo Management Tool")
            .join("settings.ini")
    })
}

fn load_app_settings() -> AppSettings {
    let Some(path) = app_settings_path() else {
        return AppSettings::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return AppSettings::default();
    };
    let mut settings = AppSettings::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "scan_threads" => settings.scan_threads = value.trim().parse().unwrap_or(2).clamp(1, 2),
            "time_display" => {
                settings.time_display_mode = match value.trim() {
                    "utc" => TimeDisplayMode::Utc,
                    "kst" => TimeDisplayMode::Kst,
                    "pdt" => TimeDisplayMode::Pdt,
                    "pst" => TimeDisplayMode::Pst,
                    _ => TimeDisplayMode::Recorded,
                }
            }
            "backup_before_changes" => {
                settings.backup_before_changes = matches!(value.trim(), "1" | "true" | "yes")
            }
            _ => {}
        }
    }
    settings
}

fn save_app_settings(settings: AppSettings) {
    let Some(path) = app_settings_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let mode = match settings.time_display_mode {
        TimeDisplayMode::Recorded => "recorded",
        TimeDisplayMode::Utc => "utc",
        TimeDisplayMode::Kst => "kst",
        TimeDisplayMode::Pdt => "pdt",
        TimeDisplayMode::Pst => "pst",
    };
    let _ = std::fs::write(
        path,
        format!(
            "scan_threads={}\ntime_display={mode}\nbackup_before_changes={}\n",
            settings.scan_threads,
            if settings.backup_before_changes { 1 } else { 0 }
        ),
    );
}

unsafe fn toggle_backup_before_changes(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    s.backup_before_changes = !s.backup_before_changes;
    CheckMenuItem(
        s.settings_menu,
        ID_BACKUP_BEFORE_CHANGES as u32,
        MF_BYCOMMAND
            | if s.backup_before_changes {
                MF_CHECKED
            } else {
                0
            },
    );
    save_app_settings(AppSettings {
        scan_threads: s.scan_threads,
        time_display_mode: s.model.time_display_mode,
        backup_before_changes: s.backup_before_changes,
    });
    set_status_text(
        s.status,
        0,
        if s.backup_before_changes {
            "Backup before changes enabled."
        } else {
            "Backup before changes disabled."
        },
    );
}

unsafe fn toggle_special_filter(hwnd: HWND, command: usize) {
    let Some(s) = state_mut(hwnd) else { return };
    let selected = selected_paths(s);
    let enabled = match command {
        ID_VIEW_MISSING_DATE => {
            s.show_only_missing_date = !s.show_only_missing_date;
            s.show_only_missing_date
        }
        ID_VIEW_MISSING_OFFSET => {
            s.show_only_missing_offset = !s.show_only_missing_offset;
            s.show_only_missing_offset
        }
        ID_VIEW_DUPLICATE_DATE => {
            s.show_only_duplicate_date = !s.show_only_duplicate_date;
            s.show_only_duplicate_date
        }
        _ => return,
    };
    CheckMenuItem(
        s.view_menu,
        command as u32,
        MF_BYCOMMAND | if enabled { MF_CHECKED } else { 0 },
    );
    s.model.set_special_filters(
        s.show_only_missing_date,
        s.show_only_missing_offset,
        s.show_only_duplicate_date,
    );
    SendMessageW(s.list, LVM_SETITEMCOUNT, s.model.rows.len(), 0);
    InvalidateRect(s.list, null(), 1);
    restore_selection(s, &selected);
    update_selection_panels(hwnd, s);
    update_status(s);
}

unsafe fn toggle_session_log(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    s.log_visible = !s.log_visible;
    CheckMenuItem(
        s.view_menu,
        ID_VIEW_SESSION_LOG as u32,
        MF_BYCOMMAND | if s.log_visible { MF_CHECKED } else { 0 },
    );
    ShowWindow(s.log, if s.log_visible { SW_SHOW } else { SW_HIDE });
    layout_children(hwnd);
}

unsafe fn clear_session_log(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    set_window_text(s.log, "");
    set_status_text(s.status, 0, "Session log cleared.");
}

unsafe fn append_session_log(s: &WindowState, message: &str) {
    let timestamp = Local::now().format("%y-%m-%d %H:%M:%S");
    let line = wide(&format!("[{timestamp}] {message}\r\n"));
    let end = GetWindowTextLengthW(s.log).max(0) as usize;
    SendMessageW(s.log, EM_SETSEL, end, end as LPARAM);
    SendMessageW(s.log, EM_REPLACESEL, 0, line.as_ptr() as LPARAM);
}

unsafe fn require_clean_staging(hwnd: HWND) -> bool {
    let Some(s) = state(hwnd) else { return false };
    if s.pending_edits.is_empty() {
        true
    } else {
        show_error(
            hwnd,
            "Pending Changes",
            "Save the staged metadata changes with Ctrl+S, or revert them with Ctrl+R, before running this operation.",
        );
        false
    }
}

unsafe fn confirm_operation(hwnd: HWND, title: &str, message: &str) -> bool {
    let title = wide(title);
    let message = wide(message);
    MessageBoxW(
        hwnd,
        message.as_ptr(),
        title.as_ptr(),
        MB_YESNO | MB_ICONINFORMATION,
    ) == IDYES_VALUE
}

unsafe fn run_filename_tool(hwnd: HWND, command: usize) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let Some(s) = state(hwnd) else { return };
    let paths = selected_files(s);
    if paths.is_empty() {
        show_error(hwnd, "No Files Selected", "Select one or more files first.");
        return;
    }
    let prompt = match command {
        ID_RENAME_MOVE_TRAILING => match trailing_number_rename_candidate_count(&paths) {
            Ok(0) => {
                show_information(hwnd, "No Matching Files", "No selected filename ends with a delimited 2- or 3-digit number.");
                return;
            }
            Ok(count) => format!("Move the trailing number to the front for {count} file(s)?"),
            Err(error) => {
                show_error(hwnd, "Filename Change Failed", &error);
                return;
            }
        },
        ID_RENAME_REMOVE_NUMBERS => "Remove underscored 2- or 3-digit numbers from the front or rear of matching filenames? Non-matching files will be skipped.".to_string(),
        ID_RENAME_REMOVE_PREFIX => "Remove IMG_ or VID_ from matching filenames? Non-matching files will be skipped.".to_string(),
        _ => return,
    };
    if !confirm_operation(hwnd, "Confirm Filename Change", &prompt) {
        return;
    }
    let result = match command {
        ID_RENAME_MOVE_TRAILING => {
            move_trailing_numbers_to_front(&paths).map(|count| format!("Renamed {count} file(s)."))
        }
        ID_RENAME_REMOVE_NUMBERS => remove_front_or_rear_numbers(&paths).map(|stats| {
            format!(
                "Renamed {} file(s). Skipped {}. Resolved {} collision(s) with _dupNNN.",
                stats.renameable, stats.unmatched, stats.deduplicated
            )
        }),
        ID_RENAME_REMOVE_PREFIX => remove_img_vid_prefixes(&paths).map(|stats| {
            format!(
                "Renamed {} file(s). Skipped {}. Resolved {} collision(s) with _dupNNN.",
                stats.renameable, stats.unmatched, stats.deduplicated
            )
        }),
        _ => return,
    };
    match result {
        Ok(message) => {
            reload_directory(hwnd, false);
            show_information(hwnd, "Filename Change Complete", &message);
        }
        Err(error) => show_error(hwnd, "Filename Change Failed", &error),
    }
}

unsafe fn create_exif_for_selected(hwnd: HWND) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let (paths, backup_before_changes) = state(hwnd)
        .map(|s| (selected_files(s), s.backup_before_changes))
        .unwrap_or_default();
    let jpeg: Vec<_> = paths
        .into_iter()
        .filter(|path| is_jpeg_path(path))
        .collect();
    if jpeg.is_empty() {
        show_error(
            hwnd,
            "Create EXIF Failed",
            "Select one or more JPEG files first.",
        );
        return;
    }
    let mut created = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();
    for path in jpeg {
        if read_exif_metadata(&path).has_exif {
            skipped += 1;
            continue;
        }
        let mut metadata = crate::exif::ExifMetadata::default();
        metadata.has_exif = true;
        match rewrite_basic_exif_metadata(&path, &metadata, backup_before_changes) {
            Ok(_) => created += 1,
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    reload_directory(hwnd, false);
    show_operation_result(hwnd, "Create EXIF", created, skipped, &failures);
}

unsafe fn remove_metadata_for_selected(hwnd: HWND, gps_only: bool) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let (paths, backup_before_changes) = state(hwnd)
        .map(|s| (selected_files(s), s.backup_before_changes))
        .unwrap_or_default();
    let jpeg: Vec<_> = paths
        .into_iter()
        .filter(|path| is_jpeg_path(path))
        .collect();
    if jpeg.is_empty() {
        show_error(
            hwnd,
            "Remove Metadata Failed",
            "Select one or more JPEG files first.",
        );
        return;
    }
    let label = if gps_only {
        "GPS information"
    } else {
        "all EXIF metadata"
    };
    if !confirm_operation(
        hwnd,
        "Confirm Metadata Removal",
        &format!("Remove {label} from {} selected JPEG file(s)?", jpeg.len()),
    ) {
        return;
    }
    let mut changed = 0usize;
    let mut failures = Vec::new();
    for path in jpeg {
        let result = if gps_only {
            ensure_win32_jpeg_backup(&path, backup_before_changes)
                .and_then(|()| remove_gps_information(&path))
        } else {
            remove_exif_metadata(&path, backup_before_changes)
        };
        match result {
            Ok(()) => changed += 1,
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    reload_directory(hwnd, false);
    show_operation_result(
        hwnd,
        if gps_only {
            "Remove GPS"
        } else {
            "Remove EXIF"
        },
        changed,
        0,
        &failures,
    );
}

unsafe fn extract_thumbnails_for_selected(hwnd: HWND) {
    let paths = state(hwnd).map(|s| selected_files(s)).unwrap_or_default();
    let jpeg: Vec<_> = paths
        .into_iter()
        .filter(|path| is_jpeg_path(path))
        .collect();
    if jpeg.is_empty() {
        show_error(
            hwnd,
            "Thumbnail Extraction Failed",
            "Select one or more JPEG files first.",
        );
        return;
    }
    let mut extracted = 0usize;
    let mut failures = Vec::new();
    for path in jpeg {
        match extract_embedded_thumbnail(&path) {
            Ok(_) => extracted += 1,
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    reload_directory(hwnd, false);
    show_operation_result(hwnd, "Thumbnail Extraction", extracted, 0, &failures);
}

unsafe fn copy_selected_exif(hwnd: HWND) {
    let Some(s) = state_mut(hwnd) else { return };
    let files = selected_files(s);
    if files.len() != 1 || !is_jpeg_path(&files[0]) {
        show_error(hwnd, "Copy EXIF Failed", "Select exactly one JPEG file.");
        return;
    }
    if !read_exif_metadata(&files[0]).has_exif {
        show_error(
            hwnd,
            "Copy EXIF Failed",
            "The selected JPEG does not contain EXIF metadata.",
        );
        return;
    }
    s.exif_clipboard = Some(files[0].clone());
    set_status_text(
        s.status,
        0,
        &format!(
            "Copied complete EXIF from {}.",
            files[0].file_name().unwrap_or_default().to_string_lossy()
        ),
    );
}

unsafe fn paste_exif_to_selected(hwnd: HWND) {
    if !require_clean_staging(hwnd) {
        return;
    }
    let Some(s) = state(hwnd) else { return };
    let backup_before_changes = s.backup_before_changes;
    let Some(source) = s.exif_clipboard.clone() else {
        show_error(
            hwnd,
            "Paste EXIF Failed",
            "Copy EXIF from a JPEG file first.",
        );
        return;
    };
    let targets: Vec<_> = selected_files(s)
        .into_iter()
        .filter(|path| is_jpeg_path(path) && *path != source)
        .collect();
    if targets.is_empty() {
        show_error(
            hwnd,
            "Paste EXIF Failed",
            "Select one or more target JPEG files.",
        );
        return;
    }
    if !confirm_operation(
        hwnd,
        "Confirm EXIF Paste",
        &format!(
            "Replace the complete EXIF block in {} target JPEG file(s) with metadata from {}?",
            targets.len(),
            source.file_name().unwrap_or_default().to_string_lossy()
        ),
    ) {
        return;
    }
    let mut completed = 0usize;
    let mut failures = Vec::new();
    for target in targets {
        match copy_exif_metadata(&source, &target, backup_before_changes) {
            Ok(()) => completed += 1,
            Err(error) => failures.push(format!("{}: {error}", target.display())),
        }
    }
    reload_directory(hwnd, false);
    show_operation_result(hwnd, "Paste EXIF", completed, 0, &failures);
}

fn ensure_win32_jpeg_backup(path: &std::path::Path, enabled: bool) -> Result<(), String> {
    if !enabled || !is_jpeg_path(path) {
        return Ok(());
    }
    let backup = exif_backup_path(path);
    if backup.exists() {
        return Ok(());
    }
    std::fs::copy(path, &backup).map_err(|error| error.to_string())?;
    if let Err(error) = copy_file_times(path, &backup) {
        let _ = std::fs::remove_file(&backup);
        return Err(error);
    }
    Ok(())
}

unsafe fn show_operation_result(
    hwnd: HWND,
    title: &str,
    completed: usize,
    skipped: usize,
    failures: &[String],
) {
    let mut message = format!("Completed {completed} file(s). Skipped {skipped}.");
    if !failures.is_empty() {
        message.push_str(&format!(
            " Failed {}.\n\n{}",
            failures.len(),
            failures
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if completed == 0 && !failures.is_empty() {
        show_error(hwnd, title, &message);
    } else {
        show_information(hwnd, title, &message);
    }
}

unsafe fn delete_selected(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    let previous_index = selected_index(s).max(0) as usize;
    let paths: Vec<_> = selected_paths(s)
        .into_iter()
        .filter(|p| s.model.rows.iter().any(|r| r.path == *p && !r.is_parent))
        .collect();
    if paths.is_empty() {
        return;
    }
    let names = paths
        .iter()
        .take(8)
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    let extra = if paths.len() > 8 {
        format!("\n... and {} more", paths.len() - 8)
    } else {
        String::new()
    };
    let prompt = wide(&format!(
        "Send the following {} item(s) to the Recycle Bin?\n\n{names}{extra}",
        paths.len()
    ));
    let title = wide("Delete");
    if MessageBoxW(
        hwnd,
        prompt.as_ptr(),
        title.as_ptr(),
        MB_YESNO | MB_ICONINFORMATION,
    ) != IDYES_VALUE
    {
        return;
    }
    let mut deleted = Vec::new();
    for path in paths {
        if let Err(error) = move_file_to_recycle_bin(&path) {
            show_error(hwnd, "Delete Failed", &error);
            break;
        }
        deleted.push(path);
    }
    if let Some(s) = state_mut(hwnd) {
        for path in &deleted {
            s.pending_edits.remove(path);
            append_session_log(s, &format!("Deleted: {}", path.display()));
        }
        update_pending_status(s);
    }
    reload_directory(hwnd, false);
    if let Some(s) = state(hwnd) {
        if !s.model.rows.is_empty() {
            let index = previous_index.min(s.model.rows.len() - 1);
            let mut item: LVITEMW = zeroed();
            item.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
            item.state = LVIS_SELECTED | LVIS_FOCUSED;
            SendMessageW(s.list, LVM_SETITEMSTATE, index, &item as *const _ as LPARAM);
            SendMessageW(s.list, LVM_ENSUREVISIBLE, index, 0);
        }
        SetFocus(s.list);
    }
}

unsafe fn selected_paths(s: &WindowState) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut index = -1i32;
    loop {
        index = SendMessageW(
            s.list,
            LVM_GETNEXTITEM,
            index as usize,
            LVNI_SELECTED as LPARAM,
        ) as i32;
        if index < 0 {
            break;
        }
        if let Some(row) = s.model.rows.get(index as usize) {
            result.push(row.path.clone());
        }
    }
    result
}

unsafe fn restore_selection(s: &WindowState, paths: &[PathBuf]) {
    let mut first = None;
    for path in paths {
        if let Some(index) = s.model.find_path(path) {
            let mut item: LVITEMW = zeroed();
            item.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
            item.state = LVIS_SELECTED | if first.is_none() { LVIS_FOCUSED } else { 0 };
            SendMessageW(s.list, LVM_SETITEMSTATE, index, &item as *const _ as LPARAM);
            first.get_or_insert(index);
        }
    }
    if let Some(index) = first {
        SendMessageW(s.list, LVM_ENSUREVISIBLE, index, 0);
    }
}

unsafe fn handle_scan_complete(hwnd: HWND, lparam: LPARAM) {
    let result = Box::from_raw(lparam as *mut ScanCompletion);
    let Some(s) = state_mut(hwnd) else { return };
    if result.generation != s.generation {
        return;
    }
    s.model.apply_scan_to_source(&result.path, result.result);
    s.pending_scans = s.pending_scans.saturating_sub(1);
    let scan_complete = s.pending_scans == 0;
    // Rebuild the owner-data view in small batches. Rebuilding after every one
    // of 100,000 scan completions turns a linear scan into quadratic UI work.
    if scan_complete || s.pending_scans.is_multiple_of(16) {
        let selected = selected_paths(s);
        s.model.refresh_after_scan_batch();
        SendMessageW(s.list, LVM_SETITEMCOUNT, s.model.rows.len(), 0);
        InvalidateRect(s.list, null(), 1);
        restore_selection(s, &selected);
        update_selection_panels(hwnd, s);
    }
    update_status(s);
}

unsafe fn update_status(s: &WindowState) {
    let selected = selected_files(s).len();
    let shown = s
        .model
        .rows
        .iter()
        .filter(|row| !row.is_parent && !row.is_dir)
        .count();
    let unsupported = if s.model.unsupported_files > 0 {
        format!(" ({} unsupported)", s.model.unsupported_files)
    } else {
        String::new()
    };
    set_status_text(
        s.status,
        1,
        &format!(
            "{selected} selected | {shown} shown / {} files{unsupported}",
            s.model.total_files
        ),
    );
    if s.pending_scans > 0 {
        let completed = s.scan_total.saturating_sub(s.pending_scans);
        set_status_text(
            s.status,
            2,
            &format!("Scanning {completed}/{}", s.scan_total),
        );
    } else {
        set_status_text(s.status, 2, "");
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => 0,
        WM_SIZE => {
            layout_children(hwnd);
            0
        }
        WM_GETMINMAXINFO => {
            let info = lparam as *mut MINMAXINFO;
            if !info.is_null() {
                (*info).ptMinTrackSize.x = 720;
                (*info).ptMinTrackSize.y = 480;
            }
            0
        }
        WM_COMMAND => {
            let command = wparam & 0xffff;
            let notification = (wparam >> 16) & 0xffff;
            if command == ID_DETAIL_INLINE_EDIT && notification == EN_KILLFOCUS as usize {
                commit_inline_detail_edit(hwnd);
                return 0;
            }
            if command == ID_DETAIL_INLINE_OFFSET
                && matches!(
                    notification,
                    value if value == CBN_SELCHANGE as usize || value == CBN_KILLFOCUS as usize
                )
            {
                commit_inline_detail_edit(hwnd);
                return 0;
            }
            let editor_changed =
                command == ID_EDIT_TAKEN_DATE && notification == EN_CHANGE as usize;
            let offset_changed = command == ID_EDIT_UTC_OFFSET
                && matches!(notification, value if value == CBN_EDITCHANGE as usize || value == CBN_SELCHANGE as usize);
            if editor_changed || offset_changed {
                stage_editor_change(hwnd, command);
                return 0;
            }
            match command {
                ID_QUICK_FILE => show_quick_action_menu(hwnd, false),
                ID_QUICK_METADATA => show_quick_action_menu(hwnd, true),
                ID_FILE_OPEN => match choose_folder() {
                    Ok(Some(path)) => load_directory(hwnd, path),
                    Ok(None) => {}
                    Err(e) => show_error(hwnd, "Open Folder Failed", &e),
                },
                ID_FILE_REFRESH => reload_directory(hwnd, true),
                ID_FILE_EXIT => {
                    windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                }
                ID_HELP_ABOUT => {
                    let t = wide(&format!("{}\nNative Win32 interface", crate::APP_TITLE));
                    let h = wide("About");
                    MessageBoxW(hwnd, t.as_ptr(), h.as_ptr(), MB_OK | MB_ICONINFORMATION);
                }
                ID_VIEW_SUPPORTED => set_filter(hwnd, FileFilter::Supported, ID_VIEW_SUPPORTED),
                ID_VIEW_IMAGES => set_filter(hwnd, FileFilter::Images, ID_VIEW_IMAGES),
                ID_VIEW_JPEG => set_filter(hwnd, FileFilter::Jpeg, ID_VIEW_JPEG),
                ID_VIEW_PNG => set_filter(hwnd, FileFilter::Png, ID_VIEW_PNG),
                ID_VIEW_VIDEOS => set_filter(hwnd, FileFilter::Videos, ID_VIEW_VIDEOS),
                ID_VIEW_ALL => set_filter(hwnd, FileFilter::All, ID_VIEW_ALL),
                x if x == ID_TOOL_OPEN as usize => match choose_folder() {
                    Ok(Some(path)) => load_directory(hwnd, path),
                    Ok(None) => {}
                    Err(e) => show_error(hwnd, "Open Folder Failed", &e),
                },
                x if x == ID_TOOL_REFRESH as usize => reload_directory(hwnd, true),
                x if x == ID_TOOL_SAVE as usize => save_pending_edits(hwnd),
                x if x == ID_TOOL_REVERT as usize => revert_pending_edits(hwnd),
                ID_ITEM_OPEN => activate_selected(hwnd),
                ID_ITEM_REVEAL => reveal_selected(hwnd),
                ID_ITEM_COPY => copy_selected_files(hwnd),
                ID_ITEM_PASTE => paste_copied_files(hwnd),
                ID_ITEM_SAVE_COPY => save_copy_selected(hwnd),
                ID_ITEM_RENAME => {
                    if let Some(s) = state(hwnd) {
                        edit_selected(s);
                    }
                }
                ID_ITEM_DELETE => delete_selected(hwnd),
                ID_DATE_FROM_FILENAME => stage_date_from_filename(hwnd),
                ID_DATE_FROM_MODIFIED => stage_date_from_modified(hwnd),
                ID_DATE_FROM_CREATED => stage_date_from_created(hwnd),
                ID_FILE_DATES_FROM_MEDIA => set_selected_file_dates_from_media(hwnd),
                ID_FILENAME_FROM_MEDIA => rename_selected_from_media_date(hwnd),
                ID_MODIFIED_FROM_CREATED => set_selected_modified_from_created(hwnd),
                ID_SHIFT_MEDIA_DATE => stage_shift_media_date(hwnd),
                ID_SHIFT_GPS_DATE_TIME => stage_shift_gps_date_time(hwnd),
                ID_RENAME_MOVE_TRAILING | ID_RENAME_REMOVE_NUMBERS | ID_RENAME_REMOVE_PREFIX => {
                    run_filename_tool(hwnd, command)
                }
                ID_EXIF_CREATE => create_exif_for_selected(hwnd),
                ID_EXIF_REMOVE => remove_metadata_for_selected(hwnd, false),
                ID_GPS_REMOVE => remove_metadata_for_selected(hwnd, true),
                ID_EXTRACT_THUMBNAIL => extract_thumbnails_for_selected(hwnd),
                ID_SELECT_ALL => select_matching_rows(hwnd, |_| true),
                ID_SELECT_MISSING_EXIF => select_matching_rows(hwnd, |row| {
                    is_jpeg_path(&row.path)
                        && row
                            .scan_result
                            .as_ref()
                            .and_then(|scan| scan.exif_metadata.as_ref())
                            .is_none_or(|exif| !exif.has_exif)
                }),
                ID_SELECT_MISSING_OFFSET => select_matching_rows(hwnd, |row| {
                    is_jpeg_path(&row.path)
                        && row
                            .scan_result
                            .as_ref()
                            .is_some_and(|scan| scan.recorded_offset_minutes.is_none())
                }),
                ID_SELECT_DUPLICATE_DATE => select_duplicate_media_dates(hwnd),
                ID_SCAN_SINGLE_THREAD => set_scan_threads(hwnd, 1, ID_SCAN_SINGLE_THREAD),
                ID_SCAN_TWO_THREADS => set_scan_threads(hwnd, 2, ID_SCAN_TWO_THREADS),
                ID_TIME_RECORDED => {
                    set_time_display_mode(hwnd, TimeDisplayMode::Recorded, ID_TIME_RECORDED)
                }
                ID_TIME_UTC => set_time_display_mode(hwnd, TimeDisplayMode::Utc, ID_TIME_UTC),
                ID_TIME_KST => set_time_display_mode(hwnd, TimeDisplayMode::Kst, ID_TIME_KST),
                ID_TIME_PDT => set_time_display_mode(hwnd, TimeDisplayMode::Pdt, ID_TIME_PDT),
                ID_TIME_PST => set_time_display_mode(hwnd, TimeDisplayMode::Pst, ID_TIME_PST),
                ID_BACKUP_BEFORE_CHANGES => toggle_backup_before_changes(hwnd),
                ID_VIEW_MISSING_DATE | ID_VIEW_MISSING_OFFSET | ID_VIEW_DUPLICATE_DATE => {
                    toggle_special_filter(hwnd, command)
                }
                ID_VIEW_SESSION_LOG => toggle_session_log(hwnd),
                ID_VIEW_CLEAR_SESSION_LOG => clear_session_log(hwnd),
                ID_EXIF_COPY => copy_selected_exif(hwnd),
                ID_EXIF_PASTE => paste_exif_to_selected(hwnd),
                ID_ANALYZE_EXACT_DUPLICATES => analyze_exact_duplicates(hwnd),
                _ => {}
            }
            0
        }
        WM_NOTIFY => handle_notify(hwnd, lparam),
        WM_SCAN_COMPLETE => {
            handle_scan_complete(hwnd, lparam);
            0
        }
        WM_PREVIEW_COMPLETE => {
            handle_preview_complete(hwnd, lparam);
            0
        }
        WM_SELECTION_SETTLED => {
            if let Some(s) = state_mut(hwnd) {
                s.selection_update_queued = false;
                update_status(s);
                update_selection_panels(hwnd, s);
            }
            layout_children(hwnd);
            0
        }
        WM_LBUTTONDOWN => {
            let x = (lparam as i16) as i32;
            if let Some(s) = state_mut(hwnd) {
                let mut client: RECT = zeroed();
                GetClientRect(hwnd, &mut client);
                let splitter = splitter_center(client.right, s.split_percent);
                if (x - splitter).abs() <= 6 {
                    s.dragging_splitter = true;
                    SetCapture(hwnd);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let mut relayout = false;
            if let Some(s) = state_mut(hwnd) {
                let mut client: RECT = zeroed();
                GetClientRect(hwnd, &mut client);
                if s.dragging_splitter && GetCapture() == hwnd {
                    let x = (lparam as i16) as i32;
                    let next = snapped_split_percent(client.right, x);
                    if next != s.split_percent {
                        s.split_percent = next;
                        relayout = true;
                    }
                } else {
                    let splitter = splitter_center(client.right, s.split_percent);
                    let x = (lparam as i16) as i32;
                    if (x - splitter).abs() <= 6 {
                        SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
                    }
                }
            }
            if relayout {
                layout_children(hwnd);
                UpdateWindow(hwnd);
            }
            0
        }
        WM_LBUTTONUP => {
            let mut relayout = false;
            if let Some(s) = state_mut(hwnd) {
                if s.dragging_splitter {
                    let mut client: RECT = zeroed();
                    GetClientRect(hwnd, &mut client);
                    let x = (lparam as i16) as i32;
                    let next = snapped_split_percent(client.right, x);
                    relayout = next != s.split_percent;
                    s.split_percent = next;
                    s.dragging_splitter = false;
                    ReleaseCapture();
                }
            }
            if relayout {
                layout_children(hwnd);
            }
            0
        }
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            if let Some(s) = state(hwnd) {
                let mut client: RECT = zeroed();
                let mut toolbar: RECT = zeroed();
                let mut status: RECT = zeroed();
                GetClientRect(hwnd, &mut client);
                GetClientRect(s.toolbar, &mut toolbar);
                GetClientRect(s.status, &mut status);
                let center = splitter_center(client.right, s.split_percent);
                let gutter = RECT {
                    left: center - 4,
                    top: toolbar.bottom + 28,
                    right: center + 4,
                    bottom: (client.bottom - status.bottom).max(toolbar.bottom + 28),
                };
                let gutter_brush = CreateSolidBrush(rgb(238, 240, 243));
                FillRect(dc, &gutter, gutter_brush);
                DeleteObject(gutter_brush);
                let line = RECT {
                    left: center - 1,
                    right: center + 1,
                    ..gutter
                };
                let line_brush = CreateSolidBrush(rgb(132, 143, 156));
                FillRect(dc, &line, line_brush);
                DeleteObject(line_brush);
            }
            EndPaint(hwnd, &paint);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !raw.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let s = Box::from_raw(raw);
                if !s.icon_big.is_null() {
                    DestroyIcon(s.icon_big);
                }
                if !s.icon_small.is_null() {
                    DestroyIcon(s.icon_small);
                }
                if s.toolbar_images != 0 {
                    ImageList_Destroy(s.toolbar_images);
                }
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn layout_children(hwnd: HWND) {
    let Some(s) = state(hwnd) else { return };
    SendMessageW(s.toolbar, TB_AUTOSIZE, 0, 0);
    SendMessageW(s.status, WM_SIZE, 0, 0);
    let mut client: RECT = zeroed();
    let mut tool: RECT = zeroed();
    let mut status: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    GetClientRect(s.toolbar, &mut tool);
    GetClientRect(s.status, &mut status);
    let th = tool.bottom;
    let sh = status.bottom;
    let ph = 28;
    let width = client.right;
    position_child(s.path_label, 8, th + 5, width - 16, ph - 5);
    let available_height = (client.bottom - th - ph - sh).max(0);
    let log_height = if s.log_visible {
        140.min(available_height / 2)
    } else {
        0
    };
    let content_height = available_height - log_height;
    let splitter_width = 8;
    let splitter = splitter_center(width, s.split_percent);
    let list_width = (splitter - splitter_width / 2).max(0);
    let right_x = splitter + splitter_width / 2;
    let right_width = (width - right_x).max(0);
    position_child(s.list, 0, th + ph, list_width, content_height);
    let preview_height = if s.preview_path.is_none() {
        0
    } else {
        s.preview_bitmap
            .as_ref()
            .map(|bitmap| {
                preview_height_for_bitmap(right_width, content_height, bitmap.width, bitmap.height)
            })
            .unwrap_or_else(|| preview_height_for_layout(right_width, content_height))
    };
    position_child(s.preview, right_x, th + ph, right_width, preview_height);
    let action_height = 32.min((content_height - preview_height).max(0));
    let action_top = th + ph + preview_height;
    position_child(s.quick_file, right_x + 8, action_top + 4, 70, 24);
    position_child(s.quick_metadata, right_x + 84, action_top + 4, 104, 24);
    let editor_top = action_top + action_height;
    ShowWindow(s.taken_date_label, SW_HIDE);
    ShowWindow(s.taken_date_edit, SW_HIDE);
    ShowWindow(s.utc_offset_label, SW_HIDE);
    ShowWindow(s.utc_offset_edit, SW_HIDE);
    position_child(
        s.details,
        right_x,
        editor_top,
        right_width,
        (content_height - preview_height - action_height).max(0),
    );
    let mut details_client: RECT = zeroed();
    GetClientRect(s.details, &mut details_client);
    let available_width = details_client.right.max(0);
    let desired_property_width = 160.min((available_width * 45 / 100).max(105));
    let property_width = desired_property_width.min((available_width - 70).max(0));
    let value_width = (available_width - property_width - 1).max(0);
    SendMessageW(s.details, LVM_SETCOLUMNWIDTH, 0, property_width as LPARAM);
    SendMessageW(s.details, LVM_SETCOLUMNWIDTH, 1, value_width as LPARAM);
    ShowScrollBar(s.details, SB_HORZ, 0);
    position_child(s.log, 0, th + ph + content_height, width, log_height);
    set_status_parts(s.status, width);
    update_status(s);
    // Moving the splitter or restoring a narrow window exposes parts of the parent behind
    // child controls. Recompose the complete child tree so stale list/detail columns cannot
    // remain painted in those newly exposed areas.
    RedrawWindow(
        hwnd,
        null(),
        null_mut(),
        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
    );
}

unsafe fn position_child(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
    // NOCOPYBITS is important for owner-data ListViews: copying the old client pixels while
    // adjacent controls resize leaves repeated columns and vertical trails during a live drag.
    SetWindowPos(
        hwnd,
        null_mut(),
        x,
        y,
        width.max(0),
        height.max(0),
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
    );
}

fn snapped_split_percent(width: i32, x: i32) -> i32 {
    if width <= 0 {
        return 60;
    }
    let raw_percent = (i64::from(x.clamp(0, width)) * 100) / i64::from(width);
    (((raw_percent as i32 + 5) / 10) * 10).clamp(20, 80)
}

fn splitter_center(width: i32, percent: i32) -> i32 {
    (i64::from(width.max(0)) * i64::from(percent.clamp(20, 80)) / 100) as i32
}

fn preview_height_for_layout(right_width: i32, content_height: i32) -> i32 {
    let aspect_height = (right_width * 3 / 4).max(0);
    let half_height = (content_height / 2).max(0);
    aspect_height.min(half_height)
}

fn preview_height_for_bitmap(
    right_width: i32,
    content_height: i32,
    bitmap_width: u32,
    bitmap_height: u32,
) -> i32 {
    if bitmap_width == 0 || bitmap_height == 0 {
        return preview_height_for_layout(right_width, content_height);
    }
    let aspect_height =
        i64::from(right_width.max(0)) * i64::from(bitmap_height) / i64::from(bitmap_width);
    (aspect_height as i32).min((content_height / 2).max(0))
}

unsafe fn create_main_menu() -> AppResult<(HMENU, HMENU, HMENU)> {
    let menu = CreateMenu();
    let file = CreatePopupMenu();
    item(file, ID_FILE_OPEN, "Open Folder...", false)?;
    item(file, ID_FILE_REFRESH, "Refresh", false)?;
    AppendMenuW(file, MF_SEPARATOR, 0, null());
    item(file, ID_ITEM_SAVE_COPY, "Save Copy...", false)?;
    item(file, ID_ITEM_OPEN, "Open", false)?;
    item(file, ID_ITEM_REVEAL, "Open in Explorer", false)?;
    item(file, ID_ITEM_COPY, "Copy\tCtrl+C", false)?;
    item(file, ID_ITEM_PASTE, "Paste\tCtrl+V", false)?;
    item(file, ID_ITEM_RENAME, "Rename\tF2", false)?;
    item(file, ID_ITEM_DELETE, "Delete\tDel", false)?;
    AppendMenuW(file, MF_SEPARATOR, 0, null());
    item(file, ID_FILE_EXIT, "Exit", false)?;
    popup(menu, file, "File")?;
    let metadata = CreatePopupMenu();
    item(
        metadata,
        ID_DATE_FROM_FILENAME,
        "Set Date Taken from Filename",
        false,
    )?;
    item(
        metadata,
        ID_DATE_FROM_MODIFIED,
        "Set Date Taken from File Modified",
        false,
    )?;
    item(
        metadata,
        ID_DATE_FROM_CREATED,
        "Set Date Taken from File Created",
        false,
    )?;
    item(
        metadata,
        ID_FILE_DATES_FROM_MEDIA,
        "Set File Created and Modified from Media Date...",
        false,
    )?;
    item(
        metadata,
        ID_FILENAME_FROM_MEDIA,
        "Rename File from Media Date...",
        false,
    )?;
    item(
        metadata,
        ID_MODIFIED_FROM_CREATED,
        "Set File Modified from File Created...",
        false,
    )?;
    item(metadata, ID_SHIFT_MEDIA_DATE, "Shift Media Date...", false)?;
    item(
        metadata,
        ID_SHIFT_GPS_DATE_TIME,
        "Shift GPS Date/Time...",
        false,
    )?;
    AppendMenuW(metadata, MF_SEPARATOR, 0, null());
    item(metadata, ID_EXIF_CREATE, "Create EXIF Structure", false)?;
    item(metadata, ID_EXIF_COPY, "Copy Complete EXIF", false)?;
    item(metadata, ID_EXIF_PASTE, "Paste Complete EXIF...", false)?;
    item(metadata, ID_EXIF_REMOVE, "Remove EXIF Metadata...", false)?;
    item(metadata, ID_GPS_REMOVE, "Remove GPS Information...", false)?;
    popup(menu, metadata, "Metadata")?;
    let analyze = CreatePopupMenu();
    item(
        analyze,
        ID_ANALYZE_EXACT_DUPLICATES,
        "Find Byte-Identical Duplicate Files",
        false,
    )?;
    item(
        analyze,
        ID_SELECT_DUPLICATE_DATE,
        "Select Duplicate Media Dates",
        false,
    )?;
    popup(menu, analyze, "Analyze")?;
    let select = CreatePopupMenu();
    item(select, ID_SELECT_ALL, "Select All\tCtrl+A", false)?;
    item(select, ID_SELECT_MISSING_EXIF, "Files Missing EXIF", false)?;
    item(
        select,
        ID_SELECT_MISSING_OFFSET,
        "Files Missing UTC Offset",
        false,
    )?;
    item(
        select,
        ID_SELECT_DUPLICATE_DATE,
        "Duplicate Media Dates",
        false,
    )?;
    popup(menu, select, "Select")?;
    let view = CreatePopupMenu();
    for (id, name, checked) in [
        (ID_VIEW_SUPPORTED, "Supported Media", true),
        (ID_VIEW_IMAGES, "Images", false),
        (ID_VIEW_JPEG, "Show Only JPG/JPEG", false),
        (ID_VIEW_PNG, "Show Only PNG", false),
        (ID_VIEW_VIDEOS, "Videos", false),
        (ID_VIEW_ALL, "All Files", false),
    ] {
        item(view, id, name, checked)?;
    }
    AppendMenuW(view, MF_SEPARATOR, 0, null());
    item(
        view,
        ID_VIEW_MISSING_DATE,
        "Show Only Missing Media Date",
        false,
    )?;
    item(
        view,
        ID_VIEW_MISSING_OFFSET,
        "Show Only Missing UTC Offset",
        false,
    )?;
    item(
        view,
        ID_VIEW_DUPLICATE_DATE,
        "Show Only Duplicate Media Dates",
        false,
    )?;
    AppendMenuW(view, MF_SEPARATOR, 0, null());
    item(view, ID_VIEW_SESSION_LOG, "Session Log", false)?;
    item(view, ID_VIEW_CLEAR_SESSION_LOG, "Clear Session Log", false)?;
    popup(menu, view, "View")?;
    let settings = CreatePopupMenu();
    item(
        settings,
        ID_TIME_RECORDED,
        "Media Date: Recorded / File Basis",
        true,
    )?;
    item(settings, ID_TIME_UTC, "Media Date: UTC (+00:00)", false)?;
    item(settings, ID_TIME_KST, "Media Date: KST (+09:00)", false)?;
    item(settings, ID_TIME_PDT, "Media Date: PDT (-07:00)", false)?;
    item(settings, ID_TIME_PST, "Media Date: PST (-08:00)", false)?;
    AppendMenuW(settings, MF_SEPARATOR, 0, null());
    item(
        settings,
        ID_BACKUP_BEFORE_CHANGES,
        "Backup Before Changes",
        false,
    )?;
    AppendMenuW(settings, MF_SEPARATOR, 0, null());
    item(
        settings,
        ID_SCAN_SINGLE_THREAD,
        "Metadata Scan: 1 Thread",
        false,
    )?;
    item(
        settings,
        ID_SCAN_TWO_THREADS,
        "Metadata Scan: 2 Threads",
        true,
    )?;
    popup(menu, settings, "Settings")?;
    let advanced = CreatePopupMenu();
    item(
        advanced,
        ID_RENAME_MOVE_TRAILING,
        "Move Trailing Number to Front...",
        false,
    )?;
    item(
        advanced,
        ID_RENAME_REMOVE_NUMBERS,
        "Remove Front/Rear Number...",
        false,
    )?;
    item(
        advanced,
        ID_RENAME_REMOVE_PREFIX,
        "Remove IMG_/VID_ Prefix...",
        false,
    )?;
    AppendMenuW(advanced, MF_SEPARATOR, 0, null());
    item(
        advanced,
        ID_EXTRACT_THUMBNAIL,
        "Extract Embedded Thumbnail",
        false,
    )?;
    popup(menu, advanced, "Advanced")?;
    let help = CreatePopupMenu();
    item(help, ID_HELP_ABOUT, "About", false)?;
    popup(menu, help, "Help")?;
    Ok((menu, view, settings))
}

unsafe fn item(menu: HMENU, id: usize, label: &str, checked: bool) -> AppResult<()> {
    let text = wide(label);
    let flags = MF_STRING | if checked { MF_CHECKED } else { 0 };
    if AppendMenuW(menu, flags, id, text.as_ptr()) == 0 {
        Err(last_error("AppendMenuW"))
    } else {
        Ok(())
    }
}
unsafe fn popup(menu: HMENU, child: HMENU, label: &str) -> AppResult<()> {
    let text = wide(label);
    if AppendMenuW(menu, MF_POPUP, child as usize, text.as_ptr()) == 0 {
        Err(last_error("Append popup"))
    } else {
        Ok(())
    }
}

const fn make_lparam(low: i32, high: i32) -> LPARAM {
    ((low as u32 & 0xffff) | ((high as u32 & 0xffff) << 16)) as LPARAM
}

unsafe fn create_toolbar_image_list() -> HIMAGELIST {
    const ICON_SIZE: i32 = 20;
    let images = ImageList_Create(ICON_SIZE, ICON_SIZE, ILC_COLOR32, 4, 0);
    if images == 0 {
        return 0;
    }
    let glyphs = [
        ('\u{e8b7}', rgb(47, 91, 153)),
        ('\u{e72c}', rgb(47, 91, 153)),
        ('\u{e74e}', rgb(71, 85, 105)),
        ('\u{e7a7}', rgb(71, 85, 105)),
    ];
    let screen = GetDC(null_mut());
    if screen.is_null() {
        return images;
    }
    let font = CreateFontW(
        -16,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        1,
        0,
        0,
        4,
        0,
        w!("Segoe MDL2 Assets"),
    );
    if font.is_null() {
        ReleaseDC(null_mut(), screen);
        return images;
    }
    for (glyph, color) in glyphs {
        let dc = CreateCompatibleDC(screen);
        let mut bits = null_mut();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ICON_SIZE,
                biHeight: -ICON_SIZE,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [zeroed()],
        };
        let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dc.is_null() || bitmap.is_null() || bits.is_null() {
            if !dc.is_null() {
                DeleteDC(dc);
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            continue;
        }
        let old_bitmap = SelectObject(dc, bitmap);
        let white = CreateSolidBrush(rgb(255, 255, 255));
        let rect = RECT {
            left: 0,
            top: 0,
            right: ICON_SIZE,
            bottom: ICON_SIZE,
        };
        FillRect(dc, &rect, white);
        DeleteObject(white);
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, rgb(0, 0, 0));
        let text = wide(&glyph.to_string());
        let mut text_rect = rect;
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut text_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        GdiFlush();
        SelectObject(dc, old_font);
        let pixels =
            std::slice::from_raw_parts_mut(bits as *mut u8, (ICON_SIZE * ICON_SIZE * 4) as usize);
        let red = color & 0xff;
        let green = (color >> 8) & 0xff;
        let blue = (color >> 16) & 0xff;
        for pixel in pixels.chunks_exact_mut(4) {
            let brightness = (u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])) / 3;
            let alpha = 255 - brightness;
            pixel[0] = (blue * alpha / 255) as u8;
            pixel[1] = (green * alpha / 255) as u8;
            pixel[2] = (red * alpha / 255) as u8;
            pixel[3] = alpha as u8;
        }
        SelectObject(dc, old_bitmap);
        ImageList_Add(images, bitmap, null_mut());
        DeleteObject(bitmap);
        DeleteDC(dc);
    }
    DeleteObject(font);
    ReleaseDC(null_mut(), screen);
    images
}

const fn toolbar_button(bitmap: i32, command: i32) -> TBBUTTON {
    TBBUTTON {
        iBitmap: bitmap,
        idCommand: command,
        fsState: TBSTATE_ENABLED as u8,
        fsStyle: 0,
        bReserved: [0; size_of::<usize>() - 2],
        dwData: 0,
        iString: -1,
    }
}
const fn toolbar_separator() -> TBBUTTON {
    TBBUTTON {
        iBitmap: 8,
        idCommand: 0,
        fsState: TBSTATE_ENABLED as u8,
        fsStyle: 1,
        bReserved: [0; size_of::<usize>() - 2],
        dwData: 0,
        iString: -1,
    }
}
unsafe fn set_status_parts(status: HWND, width: i32) {
    let parts = [(width * 38 / 100).max(220), (width * 82 / 100).max(520), -1];
    SendMessageW(status, SB_SETPARTS, 3, parts.as_ptr() as LPARAM);
}
unsafe fn set_status_text(status: HWND, part: usize, text: &str) {
    let text = wide(text);
    SendMessageW(status, SB_SETTEXTW, part, text.as_ptr() as LPARAM);
}
unsafe fn state(hwnd: HWND) -> Option<&'static WindowState> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState).as_ref()
}
unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut WindowState> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState).as_mut()
}
unsafe fn write_wide_buffer(target: *mut u16, capacity: i32, value: &str) {
    if target.is_null() || capacity <= 0 {
        return;
    }
    let encoded = wide(value);
    let count = encoded.len().min(capacity as usize);
    std::ptr::copy_nonoverlapping(encoded.as_ptr(), target, count);
    *target.add(count - 1) = 0;
}
unsafe fn from_wide_ptr(value: *const u16) -> String {
    let mut len = 0;
    while *value.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(value, len))
}
unsafe fn show_error(parent: HWND, title: &str, message: &str) {
    let title = wide(title);
    let message = wide(message);
    MessageBoxW(
        parent,
        message.as_ptr(),
        title.as_ptr(),
        MB_OK | MB_ICONERROR,
    );
}

unsafe fn show_information(parent: HWND, title: &str, message: &str) {
    let title = wide(title);
    let message = wide(message);
    MessageBoxW(
        parent,
        message.as_ptr(),
        title.as_ptr(),
        MB_OK | MB_ICONINFORMATION,
    );
}

unsafe fn create_app_icon(size: i32) -> HICON {
    let screen = windows_sys::Win32::Graphics::Gdi::GetDC(null_mut());
    if screen.is_null() {
        return null_mut();
    }
    let dc = CreateCompatibleDC(screen);
    let color = CreateCompatibleBitmap(screen, size, size);
    let mask = CreateBitmap(size, size, 1, 1, null());
    if dc.is_null() || color.is_null() || mask.is_null() {
        return null_mut();
    }
    let old = SelectObject(dc, color);
    let blue = CreateSolidBrush(rgb(40, 104, 149));
    let white = CreateSolidBrush(rgb(245, 249, 252));
    let green = CreateSolidBrush(rgb(110, 165, 115));
    let yellow = CreateSolidBrush(rgb(242, 189, 69));
    let border = CreatePen(PS_SOLID, 1, rgb(23, 74, 108));
    FillRect(
        dc,
        &RECT {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        },
        blue,
    );
    SelectObject(dc, border);
    SelectObject(dc, white);
    let inset = (size / 6).max(2);
    Rectangle(dc, inset, inset, size - inset / 2, size - inset / 2);
    SelectObject(dc, green);
    Rectangle(
        dc,
        inset + 2,
        size * 2 / 3,
        size - inset / 2 - 2,
        size - inset / 2 - 2,
    );
    FillRect(
        dc,
        &RECT {
            left: inset + 2,
            top: inset + 2,
            right: inset + (size / 5).max(3),
            bottom: inset + (size / 5).max(3),
        },
        yellow,
    );
    SelectObject(dc, old);
    for object in [blue, white, green, yellow, border] {
        DeleteObject(object);
    }
    DeleteDC(dc);
    windows_sys::Win32::Graphics::Gdi::ReleaseDC(null_mut(), screen);
    let info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = CreateIconIndirect(&info);
    DeleteObject(mask);
    DeleteObject(color);
    icon
}
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
fn last_error(context: &str) -> Box<dyn std::error::Error> {
    format!("{context} failed: {}", std::io::Error::last_os_error()).into()
}

#[cfg(test)]
mod tests {
    use super::{
        combined_gps_datetime, exif_key_for_detail_label, files_equal, preview_height_for_bitmap,
        preview_height_for_layout, shift_datetime_text, snapped_split_percent, splitter_center,
        MediaDateShift,
    };

    #[test]
    fn exact_duplicate_comparison_checks_contents_after_size() {
        let base = std::env::temp_dir();
        let left = base.join(format!("sh148_duplicate_left_{}.bin", std::process::id()));
        let right = base.join(format!("sh148_duplicate_right_{}.bin", std::process::id()));
        std::fs::write(&left, b"same bytes").unwrap();
        std::fs::write(&right, b"same bytes").unwrap();
        assert!(files_equal(&left, &right).unwrap());
        std::fs::write(&right, b"other data").unwrap();
        assert!(!files_equal(&left, &right).unwrap());
        let _ = std::fs::remove_file(left);
        let _ = std::fs::remove_file(right);
    }

    #[test]
    fn media_date_shift_crosses_day_boundaries_in_both_directions() {
        let add = MediaDateShift {
            days: 0,
            hours: 2,
            minutes: 5,
            seconds: 0,
            subtract: false,
        };
        assert_eq!(
            shift_datetime_text("2018-09-05 23:00:00", add).as_deref(),
            Some("2018-09-06 01:05:00")
        );
        let subtract = MediaDateShift {
            subtract: true,
            ..add
        };
        assert_eq!(
            shift_datetime_text("2018-09-05 01:00:00", subtract).as_deref(),
            Some("2018-09-04 22:55:00")
        );
    }

    #[test]
    fn gps_date_and_time_are_parsed_as_one_utc_value() {
        let value = combined_gps_datetime("2015:05:14", "23:25:43 UTC").unwrap();
        assert_eq!(
            value.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2015-05-14 23:25:43"
        );
    }

    #[test]
    fn details_context_menu_only_maps_real_exif_rows() {
        assert_eq!(
            exif_key_for_detail_label("Camera Make"),
            Some("camera_make")
        );
        assert_eq!(
            exif_key_for_detail_label("GPS Time"),
            Some("gps_time_stamp")
        );
        assert_eq!(exif_key_for_detail_label("File Size"), None);
        assert_eq!(exif_key_for_detail_label("Media Type"), None);
    }

    #[test]
    fn preview_never_uses_more_than_half_the_content_height() {
        assert_eq!(preview_height_for_layout(800, 600), 300);
        assert_eq!(preview_height_for_layout(320, 900), 240);
        assert_eq!(preview_height_for_layout(320, 0), 0);
        assert_eq!(preview_height_for_bitmap(390, 600, 1920, 1148), 233);
        assert_eq!(preview_height_for_bitmap(390, 600, 1080, 1920), 300);
    }

    #[test]
    fn splitter_snaps_to_ten_percent_steps_with_safe_bounds() {
        assert_eq!(snapped_split_percent(1000, 10), 20);
        assert_eq!(snapped_split_percent(1000, 249), 20);
        assert_eq!(snapped_split_percent(1000, 251), 30);
        assert_eq!(snapped_split_percent(1000, 744), 70);
        assert_eq!(snapped_split_percent(1000, 751), 80);
        assert_eq!(snapped_split_percent(1000, 990), 80);
        assert_eq!(splitter_center(1000, 60), 600);
    }
}
