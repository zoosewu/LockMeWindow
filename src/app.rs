use crate::lock::{CursorLock, LockStatus};
use crate::processes::{ProcessChoice, enumerate_processes};
use crate::{MainWindow, ManagedEntry, PromptKind, Tray};
use lock_me_window::{
    ConfigLocation, LOCATION_FILE_NAME, Language, LockTarget, ManagedApplication, NamedEvent,
    Result, Settings, default_config_dir, set_user_registry_string, startup_command,
    user_registry_dword,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, ModelRc, StandardListViewItem, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::time::Duration;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, FindWindowW, GetSystemMetrics, SM_CXSMICON, SetForegroundWindow,
    WINEVENT_OUTOFCONTEXT,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "LockMeWindow";
const RECONCILE_INTERVAL: Duration = Duration::from_millis(16);
const TRAY_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const TRAY_THEME_INTERVAL: Duration = Duration::from_secs(3);
const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const EXPORT_FILE_NAME: &str = "lock-me-window-settings.json";

struct State {
    settings: Settings,
    location: ConfigLocation,
    default_dir: PathBuf,
    lock: CursorLock,
    status: LockStatus,
    running: Vec<ProcessChoice>,
    editing: Option<usize>,
    pending_edit: Option<ManagedApplication>,
    pending_remove: Option<usize>,
    pending_import: Option<Settings>,
    pending_location: Option<ConfigLocation>,
}

impl State {
    fn settings_path(&self) -> PathBuf {
        self.location.settings_path(&self.default_dir)
    }

    fn config_dir(&self) -> PathBuf {
        self.location
            .directory
            .clone()
            .unwrap_or_else(|| self.default_dir.clone())
    }
}

type Shared = Rc<RefCell<State>>;

thread_local! {
    static FOREGROUND_CHANGED: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

macro_rules! on {
    ($ui:expr, $state:expr, $register:ident, $handler:ident $(, $arg:ident)*) => {{
        let weak = $ui.as_weak();
        let state = Rc::clone($state);
        $ui.$register(move |$($arg),*| {
            if let Some(ui) = weak.upgrade() {
                $handler(&ui, &state $(, $arg)*);
            }
        });
    }};
}

pub fn run(start_minimized: bool, show_event: NamedEvent) -> Result<()> {
    let ui = MainWindow::new()?;
    let default_dir = default_config_dir()?;
    let (location, settings, problem) = load_config(&default_dir);
    let state = Rc::new(RefCell::new(State {
        settings,
        location,
        default_dir,
        lock: CursorLock::default(),
        status: LockStatus::Unlocked,
        running: Vec::new(),
        editing: None,
        pending_edit: None,
        pending_remove: None,
        pending_import: None,
        pending_location: None,
    }));
    apply_language(state.borrow().settings.language);
    sync_settings(&ui, &state.borrow());
    if let Some((kind, detail)) = problem {
        show_prompt(&ui, kind, &detail);
    }
    connect(&ui, &state);

    let reconcile = {
        let ui = ui.as_weak();
        let state = Rc::clone(&state);
        move || reconcile(&ui, &state)
    };
    let reconcile_timer = Timer::default();
    reconcile_timer.start(TimerMode::Repeated, RECONCILE_INTERVAL, reconcile.clone());
    FOREGROUND_CHANGED.with(|callback| *callback.borrow_mut() = Some(Box::new(reconcile)));
    let _hook = ForegroundHook::install()?;
    let _tray = TrayKeeper::start(&ui);
    listen_for_show(&ui, show_event);

    if !start_minimized {
        ui.show()?;
    }
    slint::run_event_loop_until_quit()?;
    state.borrow_mut().lock.unlock();
    Ok(())
}

fn load_config(default_dir: &Path) -> (ConfigLocation, Settings, Option<(PromptKind, String)>) {
    let location = match ConfigLocation::load_from(&default_dir.join(LOCATION_FILE_NAME)) {
        Ok(location) => location,
        Err(error) => return fall_back_to_default(default_dir, error.to_string()),
    };
    if let Some(directory) = &location.directory
        && !directory.is_dir()
    {
        return fall_back_to_default(default_dir, directory.display().to_string());
    }
    match Settings::load_from(&location.settings_path(default_dir)) {
        Ok(settings) => (location, settings, None),
        Err(error) if location.directory.is_some() => {
            fall_back_to_default(default_dir, error.to_string())
        }
        Err(error) => (
            location,
            Settings::default(),
            Some((PromptKind::LoadFailed, error.to_string())),
        ),
    }
}

fn fall_back_to_default(
    default_dir: &Path,
    detail: String,
) -> (ConfigLocation, Settings, Option<(PromptKind, String)>) {
    let location = ConfigLocation::default();
    match Settings::load_from(&location.settings_path(default_dir)) {
        Ok(settings) => (
            location,
            settings,
            Some((PromptKind::LocationFailed, detail)),
        ),
        Err(error) => (
            location,
            Settings::default(),
            Some((PromptKind::LoadFailed, error.to_string())),
        ),
    }
}

fn connect(ui: &MainWindow, state: &Shared) {
    on!(ui, state, on_add_clicked, add_clicked);
    on!(ui, state, on_edit_clicked, edit_clicked, row);
    on!(ui, state, on_remove_clicked, remove_clicked, row);
    on!(ui, state, on_running_selected, running_selected, index);
    on!(ui, state, on_refresh_running, refresh_running_clicked);
    on!(ui, state, on_editor_saved, editor_saved);
    on!(ui, state, on_prompt_accepted, prompt_accepted, kind);
    on!(ui, state, on_prompt_alternate, prompt_alternate, kind);
    on!(ui, state, on_prompt_canceled, prompt_canceled, kind);
    on!(
        ui,
        state,
        on_start_with_windows_toggled,
        start_with_windows_toggled,
        enabled
    );
    on!(ui, state, on_language_selected, language_selected, index);
    on!(ui, state, on_change_config_folder, change_config_folder);
    on!(ui, state, on_reset_config_folder, reset_config_folder);
    on!(ui, state, on_import_settings, import_settings);
    on!(ui, state, on_export_settings, export_settings);
}

fn reconcile(ui: &slint::Weak<MainWindow>, state: &Shared) {
    let Some(ui) = ui.upgrade() else {
        return;
    };
    if ui.window().is_minimized() {
        let _ = ui.hide();
    }
    // Skip this tick if a callback is still using the state.
    let Ok(mut state) = state.try_borrow_mut() else {
        return;
    };
    let state = &mut *state;
    let status = state.lock.reconcile(&state.settings.apps);
    if status != state.status {
        set_status(&ui, &status);
        state.status = status;
    }
}

fn add_clicked(ui: &MainWindow, state: &Shared) {
    open_editor(ui, &mut state.borrow_mut(), None);
}

fn edit_clicked(ui: &MainWindow, state: &Shared, row: i32) {
    if let Ok(row) = usize::try_from(row) {
        open_editor(ui, &mut state.borrow_mut(), Some(row));
    }
}

fn open_editor(ui: &MainWindow, state: &mut State, row: Option<usize>) {
    let app = match row {
        Some(row) => match state.settings.apps.get(row) {
            Some(app) => Some(app),
            None => return,
        },
        None => None,
    };
    ui.set_editor_name(app.and_then(|app| app.name.as_deref()).unwrap_or("").into());
    ui.set_editor_identity(app.map_or("", |app| app.identity.as_str()).into());
    ui.set_editor_target(app.map_or(0, |app| target_index(app.target)));
    ui.set_editor_editing(row.is_some());
    state.editing = row;
    refresh_running(ui, state);
    ui.set_editor_open(true);
}

fn remove_clicked(ui: &MainWindow, state: &Shared, row: i32) {
    let mut state = state.borrow_mut();
    let Some((row, name)) = usize::try_from(row).ok().and_then(|row| {
        Some((
            row,
            state.settings.apps.get(row)?.display_name().to_string(),
        ))
    }) else {
        return;
    };
    state.pending_remove = Some(row);
    show_prompt(ui, PromptKind::ConfirmRemove, &name);
}

fn refresh_running_clicked(ui: &MainWindow, state: &Shared) {
    refresh_running(ui, &mut state.borrow_mut());
}

fn refresh_running(ui: &MainWindow, state: &mut State) {
    match unsafe { enumerate_processes() } {
        Ok(running) => {
            let items: Vec<StandardListViewItem> = running
                .iter()
                .map(|choice| StandardListViewItem::from(choice.label().as_str()))
                .collect();
            ui.set_running_apps(ModelRc::new(VecModel::from(items)));
            ui.set_running_current(-1);
            state.running = running;
        }
        Err(error) => show_prompt(ui, PromptKind::RefreshFailed, &error.to_string()),
    }
}

fn running_selected(ui: &MainWindow, state: &Shared, index: i32) {
    let Ok(state) = state.try_borrow() else {
        return;
    };
    if let Some(choice) = usize::try_from(index)
        .ok()
        .and_then(|index| state.running.get(index))
    {
        ui.set_editor_identity(choice.identity().into());
    }
}

fn editor_saved(ui: &MainWindow, state: &Shared) {
    let mut state = state.borrow_mut();
    let identity = ui
        .get_editor_identity()
        .trim()
        .trim_matches('"')
        .to_string();
    if identity.is_empty() {
        show_prompt(ui, PromptKind::IdentityEmpty, "");
        return;
    }
    let editing = state.editing;
    let duplicate = state
        .settings
        .apps
        .iter()
        .enumerate()
        .any(|(index, app)| Some(index) != editing && app.same_identity(&identity));
    if duplicate {
        show_prompt(ui, PromptKind::IdentityDuplicate, "");
        return;
    }

    let name = ui.get_editor_name().trim().to_string();
    let app = ManagedApplication {
        identity,
        target: target_from_index(ui.get_editor_target()),
        name: (!name.is_empty()).then_some(name),
    };
    if editing.is_some() {
        show_prompt(ui, PromptKind::ConfirmEdit, app.display_name());
        state.pending_edit = Some(app);
    } else {
        state.settings.apps.push(app);
        ui.set_editor_open(false);
        settings_changed(ui, &mut state);
    }
}

fn prompt_accepted(ui: &MainWindow, state: &Shared, kind: PromptKind) {
    ui.set_prompt(PromptKind::Closed);
    let mut state = state.borrow_mut();
    let state = &mut *state;
    match kind {
        PromptKind::ConfirmEdit => {
            if let (Some(row), Some(app)) = (state.editing, state.pending_edit.take())
                && let Some(slot) = state.settings.apps.get_mut(row)
            {
                *slot = app;
                ui.set_editor_open(false);
                settings_changed(ui, state);
            }
        }
        PromptKind::ConfirmRemove => {
            if let Some(row) = state.pending_remove.take()
                && row < state.settings.apps.len()
            {
                state.settings.apps.remove(row);
                settings_changed(ui, state);
            }
        }
        PromptKind::ConfirmImport => {
            if let Some(settings) = state.pending_import.take() {
                replace_settings(ui, state, settings);
            }
        }
        PromptKind::ConfigExists => {
            if let Some(location) = state.pending_location.take() {
                match Settings::load_from(&location.settings_path(&state.default_dir)) {
                    Ok(settings) => {
                        if commit_location(ui, state, location) {
                            replace_settings(ui, state, settings);
                        }
                    }
                    Err(error) => show_prompt(ui, PromptKind::RelocateFailed, &error.to_string()),
                }
            }
        }
        _ => {}
    }
}

fn prompt_alternate(ui: &MainWindow, state: &Shared, kind: PromptKind) {
    ui.set_prompt(PromptKind::Closed);
    let mut state = state.borrow_mut();
    if kind == PromptKind::ConfigExists
        && let Some(location) = state.pending_location.take()
    {
        move_settings(ui, &mut state, location);
    }
}

fn prompt_canceled(ui: &MainWindow, state: &Shared, _kind: PromptKind) {
    ui.set_prompt(PromptKind::Closed);
    let mut state = state.borrow_mut();
    state.pending_edit = None;
    state.pending_remove = None;
    state.pending_import = None;
    state.pending_location = None;
}

fn start_with_windows_toggled(ui: &MainWindow, state: &Shared, enabled: bool) {
    let mut state = state.borrow_mut();
    if let Err(error) = configure_startup(enabled) {
        ui.set_start_with_windows(state.settings.start_with_windows);
        show_prompt(ui, PromptKind::StartupFailed, &error.to_string());
        return;
    }
    state.settings.start_with_windows = enabled;
    save_settings(ui, &state);
}

fn language_selected(ui: &MainWindow, state: &Shared, index: i32) {
    let language = language_from_index(index);
    apply_language(language);
    let mut state = state.borrow_mut();
    state.settings.language = language;
    save_settings(ui, &state);
}

fn apply_language(language: Language) {
    let translation = language.translation(sys_locale::get_locale().as_deref());
    let _ = slint::select_bundled_translation(translation);
}

fn change_config_folder(ui: &MainWindow, state: &Shared) {
    let (current, default_dir) = {
        let state = state.borrow();
        (state.config_dir(), state.default_dir.clone())
    };
    // The folder dialog runs a nested message loop, so the state must not be borrowed here.
    let Some(directory) = file_dialog(ui).set_directory(&current).pick_folder() else {
        return;
    };
    let location = ConfigLocation {
        directory: (directory != default_dir).then_some(directory),
    };
    relocate(ui, &mut state.borrow_mut(), location);
}

fn reset_config_folder(ui: &MainWindow, state: &Shared) {
    relocate(ui, &mut state.borrow_mut(), ConfigLocation::default());
}

fn relocate(ui: &MainWindow, state: &mut State, location: ConfigLocation) {
    let target = location.settings_path(&state.default_dir);
    if target == state.settings_path() {
        if commit_location(ui, state, location) {
            sync_settings(ui, state);
        }
    } else if target.exists() {
        let directory = location
            .directory
            .clone()
            .unwrap_or_else(|| state.default_dir.clone());
        show_prompt(
            ui,
            PromptKind::ConfigExists,
            &directory.display().to_string(),
        );
        state.pending_location = Some(location);
    } else {
        move_settings(ui, state, location);
    }
}

fn move_settings(ui: &MainWindow, state: &mut State, location: ConfigLocation) {
    if let Err(error) = state
        .settings
        .save_to(&location.settings_path(&state.default_dir))
    {
        show_prompt(ui, PromptKind::RelocateFailed, &error.to_string());
        return;
    }
    if commit_location(ui, state, location) {
        sync_settings(ui, state);
    }
}

fn commit_location(ui: &MainWindow, state: &mut State, location: ConfigLocation) -> bool {
    match location.save_to(&state.default_dir.join(LOCATION_FILE_NAME)) {
        Ok(()) => {
            state.location = location;
            true
        }
        Err(error) => {
            show_prompt(ui, PromptKind::RelocateFailed, &error.to_string());
            false
        }
    }
}

fn import_settings(ui: &MainWindow, state: &Shared) {
    let Some(path) = file_dialog(ui).add_filter("JSON", &["json"]).pick_file() else {
        return;
    };
    match Settings::load_from(&path) {
        Ok(settings) => {
            state.borrow_mut().pending_import = Some(settings);
            show_prompt(ui, PromptKind::ConfirmImport, &path.display().to_string());
        }
        Err(error) => show_prompt(ui, PromptKind::ImportFailed, &error.to_string()),
    }
}

fn export_settings(ui: &MainWindow, state: &Shared) {
    let Some(path) = file_dialog(ui)
        .add_filter("JSON", &["json"])
        .set_file_name(EXPORT_FILE_NAME)
        .save_file()
    else {
        return;
    };
    if let Err(error) = state.borrow().settings.save_to(&path) {
        show_prompt(ui, PromptKind::ExportFailed, &error.to_string());
    }
}

fn replace_settings(ui: &MainWindow, state: &mut State, mut settings: Settings) {
    if settings.start_with_windows != state.settings.start_with_windows
        && let Err(error) = configure_startup(settings.start_with_windows)
    {
        settings.start_with_windows = state.settings.start_with_windows;
        show_prompt(ui, PromptKind::StartupFailed, &error.to_string());
    }
    if settings.language != state.settings.language {
        apply_language(settings.language);
    }
    state.settings = settings;
    settings_changed(ui, state);
}

fn settings_changed(ui: &MainWindow, state: &mut State) {
    save_settings(ui, state);
    sync_settings(ui, state);
    state.status = state.lock.refresh(&state.settings.apps);
    set_status(ui, &state.status);
}

fn save_settings(ui: &MainWindow, state: &State) {
    if let Err(error) = state.settings.save_to(&state.settings_path()) {
        show_prompt(ui, PromptKind::SaveFailed, &error.to_string());
    }
}

fn sync_settings(ui: &MainWindow, state: &State) {
    let entries: Vec<ManagedEntry> = state
        .settings
        .apps
        .iter()
        .map(|app| ManagedEntry {
            label: app.display_name().into(),
            target: target_index(app.target),
        })
        .collect();
    ui.set_managed_apps(ModelRc::new(VecModel::from(entries)));
    ui.set_managed_current(-1);
    ui.set_start_with_windows(state.settings.start_with_windows);
    ui.set_language_index(language_index(state.settings.language));
    ui.set_config_folder(state.config_dir().display().to_string().into());
    ui.set_config_folder_custom(state.location.directory.is_some());
}

fn set_status(ui: &MainWindow, status: &LockStatus) {
    match status {
        LockStatus::Unlocked => ui.set_locked(false),
        LockStatus::Locked(name) => {
            ui.set_locked_name(name.as_str().into());
            ui.set_locked(true);
        }
    }
}

fn show_prompt(ui: &MainWindow, kind: PromptKind, detail: &str) {
    ui.set_prompt_detail(detail.into());
    ui.set_prompt(kind);
}

fn show_window(ui: &MainWindow) {
    let _ = ui.show();
    ui.window().set_minimized(false);
    if let Ok(handle) = ui.window().window_handle().window_handle()
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        unsafe { SetForegroundWindow(handle.hwnd.get() as HWND) };
    }
}

fn file_dialog(ui: &MainWindow) -> rfd::FileDialog {
    rfd::FileDialog::new().set_parent(&ui.window().window_handle())
}

fn configure_startup(enabled: bool) -> Result<()> {
    let command = if enabled {
        Some(startup_command(&std::env::current_exe()?))
    } else {
        None
    };
    set_user_registry_string(RUN_KEY, RUN_VALUE, command.as_deref())
}

fn target_index(target: LockTarget) -> i32 {
    match target {
        LockTarget::Window => 0,
        LockTarget::Monitor => 1,
    }
}

fn target_from_index(index: i32) -> LockTarget {
    if index == 1 {
        LockTarget::Monitor
    } else {
        LockTarget::Window
    }
}

fn language_index(language: Language) -> i32 {
    match language {
        Language::System => 0,
        Language::English => 1,
        Language::TraditionalChinese => 2,
    }
}

fn language_from_index(index: i32) -> Language {
    match index {
        1 => Language::English,
        2 => Language::TraditionalChinese,
        _ => Language::System,
    }
}

fn listen_for_show(ui: &MainWindow, show_event: NamedEvent) {
    let ui = ui.as_weak();
    std::thread::spawn(move || {
        while show_event.wait(INFINITE) {
            let ui = ui.clone();
            let shown = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui.upgrade() {
                    show_window(&ui);
                }
            });
            if shown.is_err() {
                break;
            }
        }
    });
}

struct ForegroundHook(HWINEVENTHOOK);

impl ForegroundHook {
    fn install() -> Result<Self> {
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                null_mut(),
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_null() {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(Self(hook))
        }
    }
}

impl Drop for ForegroundHook {
    fn drop(&mut self) {
        unsafe { UnhookWinEvent(self.0) };
    }
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let _ = slint::invoke_from_event_loop(|| {
        FOREGROUND_CHANGED.with(|callback| {
            if let Some(callback) = &*callback.borrow() {
                callback();
            }
        });
    });
}

// Slint gives up on a tray icon whose first registration fails, which happens when
// LockMeWindow starts before Explorer, so the tray is only created once the taskbar exists.
struct TrayKeeper {
    tray: RefCell<Option<Tray>>,
    retry: Timer,
    theme: Timer,
}

impl TrayKeeper {
    fn start(ui: &MainWindow) -> Rc<Self> {
        let keeper = Rc::new(Self {
            tray: RefCell::new(None),
            retry: Timer::default(),
            theme: Timer::default(),
        });
        let weak_keeper = Rc::downgrade(&keeper);
        let ui = ui.as_weak();
        keeper
            .retry
            .start(TimerMode::Repeated, TRAY_RETRY_INTERVAL, move || {
                let Some(keeper) = weak_keeper.upgrade() else {
                    return;
                };
                if taskbar_exists()
                    && let Ok(tray) = create_tray(&ui)
                {
                    *keeper.tray.borrow_mut() = Some(tray);
                    keeper.retry.stop();
                }
            });

        // The user can switch the taskbar between light and dark at any time.
        let weak_keeper = Rc::downgrade(&keeper);
        keeper
            .theme
            .start(TimerMode::Repeated, TRAY_THEME_INTERVAL, move || {
                let Some(keeper) = weak_keeper.upgrade() else {
                    return;
                };
                if let Some(tray) = &*keeper.tray.borrow() {
                    let light = light_taskbar();
                    if tray.get_light_taskbar() != light {
                        tray.set_light_taskbar(light);
                    }
                }
            });
        keeper
    }
}

fn create_tray(ui: &slint::Weak<MainWindow>) -> Result<Tray> {
    let tray = Tray::new()?;
    tray.set_light_taskbar(light_taskbar());
    tray.set_small_icon(unsafe { GetSystemMetrics(SM_CXSMICON) } <= 16);
    let ui = ui.clone();
    tray.on_show_window(move || {
        if let Some(ui) = ui.upgrade() {
            show_window(&ui);
        }
    });
    tray.on_exit(|| {
        let _ = slint::quit_event_loop();
    });
    Ok(tray)
}

fn light_taskbar() -> bool {
    user_registry_dword(PERSONALIZE_KEY, "SystemUsesLightTheme").unwrap_or(0) != 0
}

fn taskbar_exists() -> bool {
    let class: Vec<u16> = "Shell_TrayWnd".encode_utf16().chain(Some(0)).collect();
    unsafe { !FindWindowW(class.as_ptr(), null()).is_null() }
}
