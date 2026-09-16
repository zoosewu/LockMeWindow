use crate::processes::process_identity;
use lock_me_window::{LockTarget, ManagedApplication, ensure_cursor_clip};
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ClipCursor, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId,
};

#[derive(Clone, Debug, PartialEq)]
pub enum LockStatus {
    Unlocked,
    Locked(String),
}

pub struct CursorLock {
    foreground: HWND,
    active: Option<ManagedApplication>,
    owns_clip: bool,
}

impl Default for CursorLock {
    fn default() -> Self {
        Self {
            foreground: null_mut(),
            active: None,
            owns_clip: false,
        }
    }
}

impl CursorLock {
    pub fn reconcile(&mut self, apps: &[ManagedApplication]) -> LockStatus {
        let foreground = unsafe { GetForegroundWindow() };
        if foreground != self.foreground {
            self.select(foreground, apps);
        }
        self.apply()
    }

    pub fn refresh(&mut self, apps: &[ManagedApplication]) -> LockStatus {
        self.select(unsafe { GetForegroundWindow() }, apps);
        self.apply()
    }

    pub fn unlock(&mut self) {
        if self.owns_clip {
            unsafe { ClipCursor(null()) };
            self.owns_clip = false;
        }
    }

    fn select(&mut self, foreground: HWND, apps: &[ManagedApplication]) {
        self.foreground = foreground;
        self.active = if foreground.is_null() {
            None
        } else {
            foreground_app(foreground, apps)
        };
    }

    fn apply(&mut self) -> LockStatus {
        let Some(app) = &self.active else {
            self.unlock();
            return LockStatus::Unlocked;
        };
        let rect = unsafe {
            match app.target {
                LockTarget::Window => client_rect_on_screen(self.foreground),
                LockTarget::Monitor => monitor_rect(self.foreground),
            }
        };
        if rect.is_some_and(ensure_cursor_clip) {
            self.owns_clip = true;
            LockStatus::Locked(app.display_name().to_string())
        } else {
            self.unlock();
            LockStatus::Unlocked
        }
    }
}

impl Drop for CursorLock {
    fn drop(&mut self) {
        self.unlock();
    }
}

fn foreground_app(foreground: HWND, apps: &[ManagedApplication]) -> Option<ManagedApplication> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(foreground, &mut pid) };
    let identity = unsafe { process_identity(pid) }?;
    apps.iter()
        .find(|app| app.matches(&identity.name, identity.path.as_deref()))
        .cloned()
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
