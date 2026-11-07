//! Installed apps, classic and Store, from the shell's AppsFolder.
use windows::core::Result;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
    KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: String,
    /// What the shell launches: an app ID, or a path for classic apps.
    pub target: String,
}

/// List installed apps. Needs COM initialised on this thread.
pub fn list() -> Result<Vec<App>> {
    // SAFETY: COM calls on valid interfaces; returned strings are freed once after copying.
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        let mut apps = Vec::new();
        loop {
            let mut buf: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            let _ = items.Next(&mut buf, Some(&mut fetched));
            let Some(item) = buf[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY)?;
            let target = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING)?;
            let app = (name.to_string(), target.to_string());
            CoTaskMemFree(Some(name.0 as _));
            CoTaskMemFree(Some(target.0 as _));
            if let (Ok(name), Ok(target)) = app {
                apps.push(App { name, target });
            }
        }
        Ok(apps)
    }
}

/// The shell path that launches or identifies an app.
pub fn shell_path(target: &str) -> String {
    format!("shell:AppsFolder\\{target}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_apps_with_names_and_targets() {
        crate::init_com();
        let apps = list().unwrap();
        assert!(!apps.is_empty());
        assert!(apps
            .iter()
            .all(|a| !a.name.is_empty() && !a.target.is_empty()));
    }
}
