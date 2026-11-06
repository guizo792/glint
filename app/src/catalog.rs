//! Built-in commands and Windows settings pages.
use glint_core::engine::{Item, ItemKind, Target};
use glint_win::system::{SystemCommand, ALL};

const SETTINGS_PAGES: &[(&str, &str)] = &[
    ("Windows Settings", "ms-settings:"),
    ("Display settings", "ms-settings:display"),
    ("Sound settings", "ms-settings:sound"),
    ("Notifications settings", "ms-settings:notifications"),
    ("Power and battery settings", "ms-settings:powersleep"),
    ("Storage settings", "ms-settings:storagesense"),
    ("Bluetooth settings", "ms-settings:bluetooth"),
    ("Wi-Fi settings", "ms-settings:network-wifi"),
    ("Network settings", "ms-settings:network"),
    ("VPN settings", "ms-settings:network-vpn"),
    (
        "Background settings",
        "ms-settings:personalization-background",
    ),
    ("Colors settings", "ms-settings:colors"),
    ("Lock screen settings", "ms-settings:lockscreen"),
    ("Taskbar settings", "ms-settings:taskbar"),
    ("Installed apps", "ms-settings:appsfeatures"),
    ("Default apps", "ms-settings:defaultapps"),
    ("Startup apps", "ms-settings:startupapps"),
    ("Accounts settings", "ms-settings:yourinfo"),
    ("Date and time settings", "ms-settings:dateandtime"),
    ("Language settings", "ms-settings:regionlanguage"),
    ("Keyboard settings", "ms-settings:keyboard"),
    ("Mouse settings", "ms-settings:mousetouchpad"),
    ("Printers and scanners", "ms-settings:printers"),
    ("Accessibility settings", "ms-settings:easeofaccess"),
    ("Privacy settings", "ms-settings:privacy"),
    ("Windows Update", "ms-settings:windowsupdate"),
    ("About this PC", "ms-settings:about"),
    ("Control Panel", "control"),
    ("Device Manager", "devmgmt.msc"),
    ("Disk Management", "diskmgmt.msc"),
    ("Services", "services.msc"),
    ("Event Viewer", "eventvwr.msc"),
    ("Registry Editor", "regedit"),
];

fn system_id(c: SystemCommand) -> String {
    format!("{c:?}")
}

pub fn system_command(id: &str) -> Option<SystemCommand> {
    ALL.iter().copied().find(|c| system_id(*c) == id)
}

pub fn items() -> Vec<Item> {
    let mut items: Vec<Item> = ALL
        .iter()
        .map(|c| {
            let mut item = Item::new(
                format!("cmd:{}", system_id(*c)),
                c.title().to_string(),
                "System command".to_string(),
                ItemKind::Command,
                Target::System(system_id(*c)),
            );
            item.confirm = c.needs_confirm();
            item
        })
        .collect();
    items.extend(SETTINGS_PAGES.iter().map(|(title, target)| {
        Item::new(
            format!("setting:{target}"),
            title.to_string(),
            "Windows settings".to_string(),
            ItemKind::Setting,
            Target::Url(target.to_string()),
        )
    }));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_system_command_round_trips_through_its_id() {
        for c in ALL {
            assert_eq!(system_command(&system_id(*c)), Some(*c));
        }
    }

    #[test]
    fn destructive_commands_are_marked_for_confirmation() {
        let items = items();
        let confirm = |key: &str| items.iter().find(|i| i.key == key).unwrap().confirm;
        assert!(confirm("cmd:Shutdown"));
        assert!(confirm("cmd:Restart"));
        assert!(confirm("cmd:EmptyRecycleBin"));
        assert!(!confirm("cmd:Lock"));
    }

    #[test]
    fn keys_are_unique() {
        let items = items();
        let mut keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), items.len());
    }
}
