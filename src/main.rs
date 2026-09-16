#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

mod app;
mod lock;
mod processes;

use std::ptr::null_mut;
use window_warden::{NamedEvent, Result, claim_single_instance};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

slint::include_modules!();

const INSTANCE_MUTEX: &str = r"Local\WindowWarden.SingleInstance";
const SHOW_EVENT: &str = r"Local\WindowWarden.Show";

fn main() {
    if let Err(error) = run() {
        show_error(&error.to_string());
    }
}

fn run() -> Result<()> {
    let start_minimized = std::env::args_os().any(|arg| arg == "--minimized");
    let show_event = NamedEvent::open_or_create(SHOW_EVENT)?;
    let Some(_instance) = claim_single_instance(INSTANCE_MUTEX)? else {
        if !start_minimized {
            show_event.signal()?;
        }
        return Ok(());
    };
    app::run(start_minimized, show_event)
}

fn show_error(text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "WindowWarden".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}
