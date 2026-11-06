//! Start with Windows, through the per-user Run key (no admin needed), plus small registry
//! helpers.
use crate::wide;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Turn autostart on or off for `name`, launching `command` at sign-in.
pub fn set(name: &str, command: &str, enabled: bool) -> Result<()> {
    let (key, value) = (wide(RUN_KEY), wide(name));
    // SAFETY: all strings outlive the calls, and the data length matches the buffer.
    unsafe {
        if enabled {
            let data = wide(command);
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr() as *const _),
                (data.len() * 2) as u32,
            )
            .ok()
        } else {
            let result = RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
            );
            if result == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                result.ok()
            }
        }
    }
}

/// The command registered for `name`, if any.
pub fn get(name: &str) -> Option<String> {
    let (key, value) = (wide(RUN_KEY), wide(name));
    let mut buf = vec![0u16; 2048];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its length are valid for the call.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut bytes),
        )
    };
    result.ok().ok()?;
    let chars = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

pub(crate) fn read_dword(key: &str, name: &str) -> Result<u32> {
    let (key, name) = (wide(key), wide(name));
    let mut out = 0u32;
    let mut bytes = 4u32;
    // SAFETY: `out` is a valid 4-byte buffer for the call.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut out as *mut _ as *mut _),
            Some(&mut bytes),
        )
        .ok()?;
    }
    Ok(out)
}

pub(crate) fn write_dword(key: &str, name: &str, value: u32) -> Result<()> {
    let (key, name) = (wide(key), wide(name));
    // SAFETY: `value` is a valid 4-byte buffer for the call.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_DWORD.0,
            Some(&value as *const _ as *const _),
            4,
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_reads_and_removes_an_entry() {
        let name = format!("GlintTest{}", std::process::id());
        set(&name, "C:\\x\\glint.exe --hidden", true).unwrap();
        assert_eq!(get(&name).as_deref(), Some("C:\\x\\glint.exe --hidden"));
        set(&name, "", false).unwrap();
        assert_eq!(get(&name), None);
        set(&name, "", false).unwrap();
    }
}
