//! Opening things through the Windows shell, never through `cmd`, so that characters like `&`
//! in a file name are just characters.
use crate::wide;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use windows::core::{Result, PCWSTR};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CREATE_NEW_CONSOLE: u32 = 0x10;

fn execute(verb: Option<&str>, file: &str, extra_mask: u32) -> Result<()> {
    let file = wide(file);
    let verb = verb.map(wide);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | windows::Win32::UI::Shell::SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    info.fMask |= extra_mask;
    // SAFETY: `info` and the strings it points to outlive the call.
    unsafe { ShellExecuteExW(&mut info) }
}

/// Open a file, folder, URL or `shell:` path with its default handler.
pub fn open(target: &str) -> Result<()> {
    execute(None, target, 0)
}

/// Open elevated. Windows shows its own consent prompt.
pub fn open_as_admin(target: &str) -> Result<()> {
    execute(Some("runas"), target, 0)
}

/// Show the Properties dialog.
pub fn properties(path: &str) -> Result<()> {
    execute(Some("properties"), path, SEE_MASK_INVOKEIDLIST)
}

/// Open the containing folder in Explorer with the item selected.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(drop)
}

/// Open a terminal in `dir`: Windows Terminal if installed, otherwise a console.
pub fn terminal_here(dir: &Path) -> std::io::Result<()> {
    let wt = Command::new("wt").arg("-d").arg(dir).spawn();
    if wt.is_ok() {
        return Ok(());
    }
    Command::new("cmd")
        .current_dir(dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_script_whose_name_contains_shell_metacharacters() {
        // Runs a real script named with & and ^: through cmd this name would be misparsed.
        let dir = std::env::temp_dir().join(format!("glint-shell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("marker.txt");
        let script = dir.join("a&b^c %d.cmd");
        std::fs::write(
            &script,
            format!("@echo done> \"{}\"\r\n@exit\r\n", marker.display()),
        )
        .unwrap();
        open(script.to_str().unwrap()).unwrap();
        for _ in 0..100 {
            if marker.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(marker.exists(), "script did not run");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_silent_failure() {
        assert!(open("C:\\glint-no-such-file-xyz.txt").is_err());
    }
}
