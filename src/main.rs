#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

use lock_me_window::{
    LockTarget, ManagedApplication, Result, Settings, claim_single_instance, ensure_cursor_clip,
    set_user_registry_string, settings_path, startup_command,
};
use std::ffi::{OsStr, OsString};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows_sys::Win32::Foundation::{
    CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, ClientToScreen, DEFAULT_GUI_FONT, GetMonitorInfoW, GetStockObject,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, UpdateWindow,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const ID_AVAILABLE: usize = 100;
const ID_REFRESH: usize = 101;
const ID_IDENTITY: usize = 102;
const ID_TARGET: usize = 103;
const ID_ADD: usize = 104;
const ID_MANAGED: usize = 105;
const ID_REMOVE: usize = 106;
const ID_EXIT: usize = 107;
const ID_STARTUP: usize = 108;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "LockMeWindow";
const WINDOW_CLASS: &str = "LockMeWindow.Main";
const INSTANCE_MUTEX: &str = r"Local\LockMeWindow.SingleInstance";
const WM_FOREGROUND_CHANGED: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const WM_SHOW_EXISTING: u32 = WM_APP + 3;
const RECONCILE_TIMER: usize = 1;
const RECONCILE_INTERVAL_MS: u32 = 16;

static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct ProcessChoice {
    title: String,
    name: String,
    path: Option<PathBuf>,
}

impl ProcessChoice {
    fn identity(&self) -> String {
        self.path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.name.clone())
    }

    fn label(&self) -> String {
        format!("{} — {}", self.title, self.identity())
    }
}

struct ProcessIdentity {
    name: String,
    path: Option<PathBuf>,
}

struct AppState {
    hwnd: HWND,
    status: HWND,
    available_list: HWND,
    identity_edit: HWND,
    target_combo: HWND,
    managed_list: HWND,
    startup_checkbox: HWND,
    available: Vec<ProcessChoice>,
    settings: Settings,
    settings_path: PathBuf,
    hook: HWINEVENTHOOK,
    tray_visible: bool,
    foreground: HWND,
    active: Option<ManagedApplication>,
    owns_clip: bool,
}

fn main() {
    if let Err(error) = run() {
        show_error(&error.to_string());
    }
}

fn run() -> Result<()> {
    let start_minimized = std::env::args_os().any(|arg| arg == "--minimized");
    let Some(_instance) = claim_single_instance(INSTANCE_MUTEX)? else {
        if !start_minimized {
            unsafe { show_existing_instance() };
        }
        return Ok(());
    };
    unsafe { run_message_loop(start_minimized) }
}

unsafe fn show_existing_instance() {
    let class = wide(WINDOW_CLASS);
    let hwnd = FindWindowW(class.as_ptr(), null());
    if !hwnd.is_null() {
        PostMessageW(hwnd, WM_SHOW_EXISTING, 0, 0);
    }
}

unsafe fn run_message_loop(start_minimized: bool) -> Result<()> {
    let instance = GetModuleHandleW(null());
    if instance.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
    if taskbar_created == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    TASKBAR_CREATED.store(taskbar_created, Ordering::Release);

    let class_name = wide(WINDOW_CLASS);
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: application_icon(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW as usize + 1) as _,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let title = wide("LockMeWindow");
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        780,
        620,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if hwnd.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    if ChangeWindowMessageFilterEx(hwnd, taskbar_created, MSGFLT_ALLOW, null_mut()) == 0 {
        DestroyWindow(hwnd);
        return Err(std::io::Error::last_os_error().into());
    }

    let path = settings_path()?;
    let settings = Settings::load_from(&path).unwrap_or_else(|error| {
        show_error(&format!(
            "Could not load settings; using defaults.\n\n{error}"
        ));
        Settings::default()
    });
    let mut state = Box::new(create_state(hwnd, settings, path)?);
    refresh_available(&mut state);
    refresh_managed(&state);

    let state_ptr = Box::into_raw(state);
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
    MAIN_WINDOW.store(hwnd as isize, Ordering::Release);

    let hook = SetWinEventHook(
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_FOREGROUND,
        null_mut(),
        Some(win_event_proc),
        0,
        0,
        WINEVENT_OUTOFCONTEXT,
    );
    if hook.is_null() {
        DestroyWindow(hwnd);
        return Err(std::io::Error::last_os_error().into());
    }
    (*state_ptr).hook = hook;
    if SetTimer(hwnd, RECONCILE_TIMER, RECONCILE_INTERVAL_MS, None) == 0 {
        DestroyWindow(hwnd);
        return Err(std::io::Error::last_os_error().into());
    }
    ensure_tray_icon(&mut *state_ptr);

    if start_minimized {
        minimize_to_tray(&mut *state_ptr);
    } else {
        ShowWindow(hwnd, SW_SHOW);
    }
    UpdateWindow(hwnd);
    PostMessageW(
        hwnd,
        WM_FOREGROUND_CHANGED,
        GetForegroundWindow() as usize,
        0,
    );

    let mut msg: MSG = zeroed();
    loop {
        let status = GetMessageW(&mut msg, null_mut(), 0, 0);
        if status == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        if status == 0 {
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    Ok(())
}

unsafe fn create_state(hwnd: HWND, settings: Settings, settings_path: PathBuf) -> Result<AppState> {
    let status = static_control(hwnd, "Cursor: unlocked", 18, 14, 730, 24);
    static_control(hwnd, "Running applications", 18, 48, 730, 22);
    let available_list = control(
        "LISTBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | LBS_NOTIFY as u32,
        WS_EX_CLIENTEDGE,
        18,
        70,
        730,
        190,
        hwnd,
        ID_AVAILABLE,
    )?;
    control(
        "BUTTON",
        "Refresh",
        WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
        0,
        648,
        268,
        100,
        30,
        hwnd,
        ID_REFRESH,
    )?;
    static_control(hwnd, "Executable name or full path", 18, 306, 400, 22);
    let identity_edit = control(
        "EDIT",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        18,
        330,
        520,
        28,
        hwnd,
        ID_IDENTITY,
    )?;
    let target_combo = control(
        "COMBOBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST as u32,
        0,
        548,
        330,
        100,
        150,
        hwnd,
        ID_TARGET,
    )?;
    for target in ["Window", "Monitor"] {
        let text = wide(target);
        SendMessageW(target_combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
    }
    SendMessageW(target_combo, CB_SETCURSEL, 0, 0);
    control(
        "BUTTON",
        "Add",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        658,
        330,
        90,
        28,
        hwnd,
        ID_ADD,
    )?;
    static_control(hwnd, "Managed applications", 18, 374, 730, 22);
    let managed_list = control(
        "LISTBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | LBS_NOTIFY as u32,
        WS_EX_CLIENTEDGE,
        18,
        398,
        730,
        125,
        hwnd,
        ID_MANAGED,
    )?;
    control(
        "BUTTON",
        "Remove",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        648,
        531,
        100,
        30,
        hwnd,
        ID_REMOVE,
    )?;
    let startup_checkbox = control(
        "BUTTON",
        "Start with Windows",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        0,
        18,
        535,
        250,
        26,
        hwnd,
        ID_STARTUP,
    )?;
    SendMessageW(
        startup_checkbox,
        BM_SETCHECK,
        if settings.start_with_windows {
            BST_CHECKED as usize
        } else {
            BST_UNCHECKED as usize
        },
        0,
    );

    Ok(AppState {
        hwnd,
        status,
        available_list,
        identity_edit,
        target_combo,
        managed_list,
        startup_checkbox,
        available: Vec::new(),
        settings,
        settings_path,
        hook: null_mut(),
        tray_visible: false,
        foreground: null_mut(),
        active: None,
        owns_clip: false,
    })
}

#[allow(clippy::too_many_arguments)]
unsafe fn control(
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    id: usize,
) -> Result<HWND> {
    let class = wide(class);
    let text = wide(text);
    let hwnd = CreateWindowExW(
        ex_style,
        class.as_ptr(),
        text.as_ptr(),
        style,
        x,
        y,
        width,
        height,
        parent,
        id as _,
        GetModuleHandleW(null()),
        null(),
    );
    if hwnd.is_null() {
        Err(std::io::Error::last_os_error().into())
    } else {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            GetStockObject(DEFAULT_GUI_FONT) as usize,
            1,
        );
        Ok(hwnd)
    }
}

unsafe fn static_control(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> HWND {
    control(
        "STATIC",
        text,
        WS_CHILD | WS_VISIBLE,
        0,
        x,
        y,
        width,
        height,
        parent,
        0,
    )
    .unwrap_or(null_mut())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            if let Some(state) = state(hwnd) {
                handle_command(state, wparam);
            }
            0
        }
        WM_FOREGROUND_CHANGED => {
            if let Some(state) = state(hwnd) {
                adjust_lock(state, wparam as HWND);
            }
            0
        }
        WM_TIMER if wparam == RECONCILE_TIMER => {
            if let Some(state) = state(hwnd) {
                reconcile_lock(state);
            }
            0
        }
        WM_SHOW_EXISTING => {
            if let Some(state) = state(hwnd) {
                restore_from_tray(state);
            }
            0
        }
        message if message == TASKBAR_CREATED.load(Ordering::Acquire) => {
            if let Some(state) = state(hwnd) {
                state.tray_visible = false;
                ensure_tray_icon(state);
            }
            0
        }
        WM_SIZE if wparam as u32 == SIZE_MINIMIZED => {
            if let Some(state) = state(hwnd) {
                minimize_to_tray(state);
            }
            0
        }
        WM_TRAY => {
            if let Some(state) = state(hwnd) {
                match lparam as u32 {
                    WM_LBUTTONUP => restore_from_tray(state),
                    WM_RBUTTONUP => show_tray_menu(state),
                    _ => {}
                }
            }
            0
        }
        WM_CLOSE => {
            if let Some(state) = state(hwnd) {
                minimize_to_tray(state);
            }
            0
        }
        WM_DESTROY => {
            MAIN_WINDOW.store(0, Ordering::Release);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !ptr.is_null() {
                let state = Box::from_raw(ptr);
                cleanup(&state);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn state(hwnd: HWND) -> Option<&'static mut AppState> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState).as_mut()
}

unsafe fn handle_command(state: &mut AppState, wparam: WPARAM) {
    let id = wparam & 0xffff;
    let notification = ((wparam >> 16) & 0xffff) as u32;
    match (id, notification) {
        (ID_AVAILABLE, LBN_SELCHANGE) => select_available(state),
        (ID_REFRESH, BN_CLICKED) => refresh_available(state),
        (ID_ADD, BN_CLICKED) => add_managed(state),
        (ID_REMOVE, BN_CLICKED) => remove_managed(state),
        (ID_STARTUP, BN_CLICKED) => change_startup(state),
        _ => {}
    }
}

unsafe fn change_startup(state: &mut AppState) {
    let enabled = SendMessageW(state.startup_checkbox, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
    if let Err(error) = configure_startup(enabled) {
        SendMessageW(
            state.startup_checkbox,
            BM_SETCHECK,
            if state.settings.start_with_windows {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
        show_error(&format!("Could not update Windows startup.\n\n{error}"));
        return;
    }
    state.settings.start_with_windows = enabled;
    save_settings(state);
}

fn configure_startup(enabled: bool) -> Result<()> {
    let command = if enabled {
        Some(startup_command(&std::env::current_exe()?))
    } else {
        None
    };
    set_user_registry_string(RUN_KEY, RUN_VALUE, command.as_deref())
}

unsafe fn select_available(state: &mut AppState) {
    let index = SendMessageW(state.available_list, LB_GETCURSEL, 0, 0);
    if index >= 0
        && let Some(choice) = state.available.get(index as usize)
    {
        let identity = wide(&choice.identity());
        SetWindowTextW(state.identity_edit, identity.as_ptr());
    }
}

unsafe fn add_managed(state: &mut AppState) {
    let identity = window_text(state.identity_edit)
        .trim()
        .trim_matches('"')
        .to_string();
    if identity.is_empty() {
        message(
            "Enter an executable name or select a running application.",
            MB_OK | MB_ICONWARNING,
        );
        return;
    }
    let target = if SendMessageW(state.target_combo, CB_GETCURSEL, 0, 0) == 1 {
        LockTarget::Monitor
    } else {
        LockTarget::Window
    };

    if state
        .settings
        .apps
        .iter()
        .any(|app| app.same_identity(&identity))
    {
        message(
            "That application is already configured.",
            MB_OK | MB_ICONWARNING,
        );
        return;
    }
    state
        .settings
        .apps
        .push(ManagedApplication { identity, target });
    save_settings(state);
    refresh_managed(state);
    adjust_lock(state, GetForegroundWindow());
}

unsafe fn remove_managed(state: &mut AppState) {
    let index = SendMessageW(state.managed_list, LB_GETCURSEL, 0, 0);
    if index < 0 {
        return;
    }
    state.settings.apps.remove(index as usize);
    save_settings(state);
    refresh_managed(state);
    adjust_lock(state, GetForegroundWindow());
}

unsafe fn refresh_available(state: &mut AppState) {
    match enumerate_processes() {
        Ok(processes) => {
            state.available = processes;
            SendMessageW(state.available_list, LB_RESETCONTENT, 0, 0);
            for choice in &state.available {
                let label = wide(&choice.label());
                SendMessageW(
                    state.available_list,
                    LB_ADDSTRING,
                    0,
                    label.as_ptr() as isize,
                );
            }
        }
        Err(error) => show_error(&format!("Could not refresh processes.\n\n{error}")),
    }
}

unsafe fn refresh_managed(state: &AppState) {
    SendMessageW(state.managed_list, LB_RESETCONTENT, 0, 0);
    for app in &state.settings.apps {
        let label = format!("{}    [{}]", app.identity, target_name(app.target));
        let label = wide(&label);
        SendMessageW(state.managed_list, LB_ADDSTRING, 0, label.as_ptr() as isize);
    }
}

fn target_name(target: LockTarget) -> &'static str {
    match target {
        LockTarget::Window => "Window",
        LockTarget::Monitor => "Monitor",
    }
}

fn save_settings(state: &AppState) {
    if let Err(error) = state.settings.save_to(&state.settings_path) {
        show_error(&format!("Could not save settings.\n\n{error}"));
    }
}

unsafe fn enumerate_processes() -> Result<Vec<ProcessChoice>> {
    let mut processes = Vec::<ProcessChoice>::new();
    if EnumWindows(Some(enum_window), &mut processes as *mut _ as isize) == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    processes.sort_by_key(|choice| choice.identity().to_lowercase());
    processes.dedup_by(|a, b| a.identity().eq_ignore_ascii_case(&b.identity()));
    processes.sort_by_key(|choice| choice.label().to_lowercase());
    Ok(processes)
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> i32 {
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 || pid == GetCurrentProcessId() {
        return 1;
    }
    let Some(identity) = process_identity(pid) else {
        return 1;
    };
    let title = match window_text(hwnd) {
        title if !title.is_empty() => title,
        _ => identity.name.clone(),
    };
    let processes = &mut *(lparam as *mut Vec<ProcessChoice>);
    processes.push(ProcessChoice {
        title,
        name: identity.name,
        path: identity.path,
    });
    1
}

unsafe fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    let path = process_path(pid);
    let name = path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .or_else(|| process_name(pid))?;
    Some(ProcessIdentity { name, path })
}

unsafe fn process_path(pid: u32) -> Option<PathBuf> {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
    CloseHandle(process);
    (ok != 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..length as usize])))
}

unsafe fn process_name(pid: u32) -> Option<String> {
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: PROCESSENTRY32W = zeroed();
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut found = None;
    if Process32FirstW(snapshot, &mut entry) != 0 {
        loop {
            if entry.th32ProcessID == pid {
                found = Some(wide_array_to_string(&entry.szExeFile));
                break;
            }
            if Process32NextW(snapshot, &mut entry) == 0 {
                break;
            }
        }
    }
    CloseHandle(snapshot);
    found
}

unsafe fn adjust_lock(state: &mut AppState, foreground: HWND) {
    state.foreground = foreground;
    state.active = None;
    if foreground.is_null() {
        unlock(state);
        return;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(foreground, &mut pid);
    let Some(identity) = process_identity(pid) else {
        unlock(state);
        return;
    };
    let Some(app) = state
        .settings
        .apps
        .iter()
        .find(|app| app.matches(&identity.name, identity.path.as_deref()))
        .cloned()
    else {
        unlock(state);
        return;
    };

    state.active = Some(app);
    apply_active_lock(state);
}

unsafe fn reconcile_lock(state: &mut AppState) {
    let foreground = GetForegroundWindow();
    if foreground != state.foreground {
        adjust_lock(state, foreground);
    } else if state.active.is_some() {
        apply_active_lock(state);
    }
}

unsafe fn apply_active_lock(state: &mut AppState) {
    let Some(app) = state.active.clone() else {
        return;
    };

    let rect = match app.target {
        LockTarget::Window => client_rect_on_screen(state.foreground),
        LockTarget::Monitor => monitor_rect(state.foreground),
    };
    if let Some(rect) = rect
        && ensure_cursor_clip(rect)
    {
        state.owns_clip = true;
        set_status(state.status, &format!("Cursor: locked to {}", app.identity));
        return;
    }
    unlock(state);
}

unsafe fn client_rect_on_screen(hwnd: HWND) -> Option<RECT> {
    let mut rect: RECT = zeroed();
    if GetClientRect(hwnd, &mut rect) == 0 {
        return None;
    }
    let mut top_left = POINT {
        x: rect.left,
        y: rect.top,
    };
    let mut bottom_right = POINT {
        x: rect.right,
        y: rect.bottom,
    };
    if ClientToScreen(hwnd, &mut top_left) == 0 || ClientToScreen(hwnd, &mut bottom_right) == 0 {
        return None;
    }
    Some(RECT {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    })
}

unsafe fn monitor_rect(hwnd: HWND) -> Option<RECT> {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if monitor.is_null() {
        return None;
    }
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    (GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.rcMonitor)
}

unsafe fn unlock(state: &mut AppState) {
    if state.owns_clip {
        ClipCursor(null());
        state.owns_clip = false;
    }
    set_status(state.status, "Cursor: unlocked");
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let main = MAIN_WINDOW.load(Ordering::Acquire) as HWND;
    if !main.is_null() {
        PostMessageW(main, WM_FOREGROUND_CHANGED, hwnd as usize, 0);
    }
}

unsafe fn minimize_to_tray(state: &mut AppState) {
    ensure_tray_icon(state);
    ShowWindow(state.hwnd, SW_HIDE);
}

unsafe fn restore_from_tray(state: &mut AppState) {
    ShowWindow(state.hwnd, SW_RESTORE);
    SetForegroundWindow(state.hwnd);
}

unsafe fn ensure_tray_icon(state: &mut AppState) -> bool {
    if state.tray_visible {
        return true;
    }
    let data = tray_data(state.hwnd);
    if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
        return false;
    }
    state.tray_visible = true;
    true
}

unsafe fn show_tray_menu(state: &mut AppState) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let exit = wide("Exit");
    AppendMenuW(menu, MF_STRING, ID_EXIT, exit.as_ptr());
    let mut cursor: POINT = zeroed();
    if GetCursorPos(&mut cursor) != 0 {
        SetForegroundWindow(state.hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            cursor.x,
            cursor.y,
            0,
            state.hwnd,
            null(),
        );
        PostMessageW(state.hwnd, WM_NULL, 0, 0);
        if command == ID_EXIT as i32 {
            DestroyWindow(state.hwnd);
        }
    }
    DestroyMenu(menu);
}

unsafe fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: application_icon(),
        ..Default::default()
    };
    copy_wide("LockMeWindow", &mut data.szTip);
    data
}

unsafe fn application_icon() -> HICON {
    let icon = LoadIconW(GetModuleHandleW(null()), std::ptr::without_provenance(1));
    if icon.is_null() {
        LoadIconW(null_mut(), IDI_APPLICATION)
    } else {
        icon
    }
}

unsafe fn cleanup(state: &AppState) {
    KillTimer(state.hwnd, RECONCILE_TIMER);
    if state.owns_clip {
        ClipCursor(null());
    }
    if !state.hook.is_null() {
        UnhookWinEvent(state.hook);
    }
    if state.tray_visible {
        let data = tray_data(state.hwnd);
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

unsafe fn set_status(hwnd: HWND, text: &str) {
    let text = wide(text);
    SetWindowTextW(hwnd, text.as_ptr());
}

unsafe fn window_text(hwnd: HWND) -> String {
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn wide_array_to_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

fn copy_wide<const N: usize>(value: &str, destination: &mut [u16; N]) {
    let value = wide(value);
    let length = value.len().min(N);
    destination[..length].copy_from_slice(&value[..length]);
}

fn show_error(text: &str) {
    unsafe { message(text, MB_OK | MB_ICONERROR) }
}

unsafe fn message(text: &str, flags: MESSAGEBOX_STYLE) {
    let text = wide(text);
    let title = wide("LockMeWindow");
    MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), flags);
}
