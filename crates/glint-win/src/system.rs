//! System commands. Destructive ones are confirmed by the UI before reaching this module.
use crate::autostart::{read_dword, write_dword};
use crate::{shell, wide};
use std::process::Command;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};

const THEME_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemCommand {
    Lock,
    Sleep,
    SignOut,
    Restart,
    Shutdown,
    EmptyRecycleBin,
    ToggleDarkMode,
    TaskManager,
}

pub const ALL: &[SystemCommand] = &[
    SystemCommand::Lock,
    SystemCommand::Sleep,
    SystemCommand::SignOut,
    SystemCommand::Restart,
    SystemCommand::Shutdown,
    SystemCommand::EmptyRecycleBin,
    SystemCommand::ToggleDarkMode,
    SystemCommand::TaskManager,
];

impl SystemCommand {
    pub fn title(self) -> &'static str {
        match self {
            SystemCommand::Lock => "Lock",
            SystemCommand::Sleep => "Sleep",
            SystemCommand::SignOut => "Sign out",
            SystemCommand::Restart => "Restart",
            SystemCommand::Shutdown => "Shut down",
            SystemCommand::EmptyRecycleBin => "Empty Recycle Bin",
            SystemCommand::ToggleDarkMode => "Toggle dark mode",
            SystemCommand::TaskManager => "Task Manager",
        }
    }

    /// Whether the UI must ask first.
    pub fn needs_confirm(self) -> bool {
        matches!(
            self,
            SystemCommand::SignOut
                | SystemCommand::Restart
                | SystemCommand::Shutdown
                | SystemCommand::EmptyRecycleBin
        )
    }

    pub fn run(self) -> Result<(), String> {
        let text = |e: windows::core::Error| e.message();
        let shutdown = |flag: &str| {
            Command::new("shutdown")
                .args([flag, "/t", "0"])
                .spawn()
                .map(drop)
                .map_err(|e| e.to_string())
        };
        match self {
            // SAFETY: no pointers involved.
            SystemCommand::Lock => unsafe { LockWorkStation() }.map_err(text),
            // SAFETY: no pointers involved.
            SystemCommand::Sleep => {
                if unsafe { SetSuspendState(false, false, false) } {
                    Ok(())
                } else {
                    Err(text(windows::core::Error::from_thread()))
                }
            }
            SystemCommand::SignOut => Command::new("shutdown")
                .arg("/l")
                .spawn()
                .map(drop)
                .map_err(|e| e.to_string()),
            SystemCommand::Restart => shutdown("/r"),
            SystemCommand::Shutdown => shutdown("/s"),
            // SAFETY: null root means all drives; no other pointers involved.
            SystemCommand::EmptyRecycleBin => {
                unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION) }
                    .map_err(text)
            }
            SystemCommand::ToggleDarkMode => toggle_dark_mode(),
            SystemCommand::TaskManager => shell::open("taskmgr").map_err(text),
        }
    }
}

fn toggle_dark_mode() -> Result<(), String> {
    let light = read_dword(THEME_KEY, "AppsUseLightTheme").unwrap_or(1);
    let next = u32::from(light == 0);
    write_dword(THEME_KEY, "AppsUseLightTheme", next).map_err(|e| e.message())?;
    write_dword(THEME_KEY, "SystemUsesLightTheme", next).map_err(|e| e.message())?;
    let area = wide("ImmersiveColorSet");
    // SAFETY: `area` outlives the call, and the timeout stops a hung window blocking us.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            None,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_commands_require_confirmation() {
        for c in [
            SystemCommand::SignOut,
            SystemCommand::Restart,
            SystemCommand::Shutdown,
            SystemCommand::EmptyRecycleBin,
        ] {
            assert!(c.needs_confirm(), "{}", c.title());
        }
        assert!(!SystemCommand::Lock.needs_confirm());
    }
}
