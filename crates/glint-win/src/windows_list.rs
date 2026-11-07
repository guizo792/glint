//! Open top-level windows, for the window switcher.
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, IsIconic,
    IsWindow, IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE,
    WS_EX_TOOLWINDOW,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    pub handle: isize,
    pub title: String,
}

/// Windows a person would recognise: visible, titled, not tool windows or hidden UWP shells.
pub fn list() -> Vec<OpenWindow> {
    let mut out: Vec<OpenWindow> = Vec::new();
    // SAFETY: the callback only runs during EnumWindows, while `out` is alive.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> BOOL {
    let out = &mut *(param.0 as *mut Vec<OpenWindow>);
    if !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid())
    {
        return true.into();
    }
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let mut cloaked = 0u32;
    let _ = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut _ as *mut _,
        size_of::<u32>() as u32,
    );
    if cloaked != 0 {
        return true.into();
    }
    let len = GetWindowTextLengthW(hwnd);
    if len > 0 {
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        out.push(OpenWindow {
            handle: hwnd.0 as isize,
            title: String::from_utf16_lossy(&buf[..n as usize]),
        });
    }
    true.into()
}

/// Bring a window to the front, restoring it if minimised. Returns false if it has closed.
pub fn activate(handle: isize) -> bool {
    let hwnd = HWND(handle as *mut _);
    // SAFETY: these calls validate the handle and fail harmlessly if it is stale.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd).as_bool()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_windows_have_titles() {
        assert!(list().iter().all(|w| !w.title.is_empty()));
    }

    #[test]
    fn a_stale_handle_is_reported_not_activated() {
        assert!(!activate(0x7fff_fff0));
    }
}
