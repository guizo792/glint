//! Windows integration: everything that talks to the OS directly.
#![cfg(windows)]

pub mod apps;
pub mod autostart;
pub mod clipboard;
pub mod icons;
pub mod shell;
pub mod system;
pub mod windows_list;

/// Encode a string as a NUL-terminated UTF-16 buffer for Win32 calls.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Initialise COM on the current thread. Safe to call repeatedly.
pub fn init_com() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    // SAFETY: no pointers involved; repeat calls on a thread only return S_FALSE.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}
