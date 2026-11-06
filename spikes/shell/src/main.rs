#![cfg_attr(windows, windows_subsystem = "windows")]
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn now_us() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as u64
}

fn log(line: &str) {
    let path = std::env::temp_dir().join("glint-spike.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

#[tauri::command]
fn report(ms: f64, focused: bool) {
    log(&format!("show_ms={ms:.2} focused={focused}"));
}

#[tauri::command]
fn hide(win: WebviewWindow) {
    let _ = win.hide();
}

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _sc, ev| {
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    let t0 = now_us();
                    let Some(w) = app.get_webview_window("launcher") else { return };
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                        let _ = w.emit("shown", t0);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![report, hide])
        .setup(|app| {
            let sc = Shortcut::new(Some(Modifiers::ALT), Code::Space);
            match app.global_shortcut().register(sc) {
                Ok(()) => log("hotkey Alt+Space registered"),
                Err(e) => log(&format!("hotkey registration FAILED: {e}")),
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run");
}
