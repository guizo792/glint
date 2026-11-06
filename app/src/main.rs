#![cfg_attr(not(test), windows_subsystem = "windows")]

mod catalog;

use base64::Engine as _;
use glint_core::config::Config;
use glint_core::engine::{
    Apps, Calculator, ClipboardHistory, Commands, Engine, Files, Item, ItemList, Target, Web,
    Windows,
};
use glint_core::live::{watch, Live};
use glint_core::store::Store;
use glint_core::walk::{WalkConfig, DEFAULT_EXCLUDES};
use glint_win::{apps, autostart, clipboard, icons, shell, windows_list};
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const WIDTH: f64 = 680.0;
const LOG_LIMIT: u64 = 1024 * 1024;
const APP_REFRESH: Duration = Duration::from_secs(300);
const INDEX_CHECK: Duration = Duration::from_secs(60);
/// Do not rebuild at startup if the snapshot is newer than this.
const FRESH_SNAPSHOT: Duration = Duration::from_secs(3600);
const STARTUP_DELAY: Duration = Duration::from_secs(10);

struct Paths {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl Paths {
    /// `GLINT_DATA_DIR` puts everything in one folder, which keeps tests away from real data.
    fn resolve() -> Paths {
        if let Ok(dir) = std::env::var("GLINT_DATA_DIR") {
            return Paths {
                config_dir: dir.clone().into(),
                data_dir: dir.into(),
            };
        }
        let under = |var: &str| {
            std::env::var(var)
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("Glint")
        };
        Paths {
            config_dir: under("APPDATA"),
            data_dir: under("LOCALAPPDATA"),
        }
    }
    fn config(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    fn history(&self) -> PathBuf {
        self.data_dir.join("history.json")
    }
    fn icons(&self) -> PathBuf {
        self.data_dir.join("icons")
    }
    fn log(&self) -> PathBuf {
        self.data_dir.join("logs").join("glint.log")
    }
}

/// Append to a size-capped log file. Failures are ignored: logging must not break the app.
fn log(paths: &Paths, message: &str) {
    let path = paths.log();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{} {message}", now());
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

struct State {
    paths: Paths,
    config: RwLock<Config>,
    config_error: RwLock<Option<String>>,
    config_stamp: Mutex<Option<SystemTime>>,
    hotkey_error: RwLock<Option<String>>,
    engine: RwLock<Arc<Engine>>,
    live: Arc<Live>,
    apps: Arc<Apps>,
    clips: Arc<ClipboardHistory>,
    clip_monitor: Mutex<Option<clipboard::Monitor>>,
    walk: WalkConfig,
    rebuild_requested: std::sync::atomic::AtomicBool,
    index_status: RwLock<String>,
    icon_cache: Mutex<HashMap<String, String>>,
}

fn build_engine(
    paths: &Paths,
    config: &Config,
    live: &Arc<Live>,
    apps: &Arc<Apps>,
    clips: &Arc<ClipboardHistory>,
) -> Arc<Engine> {
    let web = Arc::new(Web {
        bangs: config.bangs.clone(),
        quicklinks: config.quicklinks.clone(),
    });
    let windows = Windows(Box::new(|| {
        windows_list::list()
            .into_iter()
            .filter(|w| w.title != "Glint")
            .map(|w| (w.handle, w.title))
            .collect()
    }));
    Arc::new(Engine::new(
        vec![
            Arc::new(Files(live.clone())),
            apps.clone(),
            Arc::new(Commands(ItemList::new(catalog::items()))),
            Arc::new(windows),
            clips.clone(),
            Arc::new(Calculator),
            web.clone(),
        ],
        web,
        paths.history(),
    ))
}

fn config_stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Apply settings that take effect immediately. Index folders apply at next start.
fn apply_config(app: &AppHandle, state: &State) {
    let config = state.config.read().unwrap().clone();
    *state.engine.write().unwrap() = build_engine(
        &state.paths,
        &config,
        &state.live,
        &state.apps,
        &state.clips,
    );
    register_hotkey(app, state, &config.hotkey);
    if let Ok(exe) = std::env::current_exe() {
        let command = format!("\"{}\"", exe.display());
        if let Err(e) = autostart::set("Glint", &command, config.autostart) {
            log(&state.paths, &format!("autostart: {e}"));
        }
    }
    let mut monitor = state.clip_monitor.lock().unwrap();
    if config.clipboard_history && monitor.is_none() {
        let (m, rx) = clipboard::Monitor::start();
        let clips = state.clips.clone();
        std::thread::spawn(move || {
            for text in rx {
                clips.add(text);
            }
        });
        *monitor = Some(m);
    } else if !config.clipboard_history {
        *monitor = None;
        state.clips.clear();
    }
    let _ = app.emit("config-changed", ());
}

fn register_hotkey(app: &AppHandle, state: &State, hotkey: &str) {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    let result = Shortcut::from_str(hotkey)
        .map_err(|e| format!("\"{hotkey}\" is not a valid hotkey: {e}"))
        .and_then(|shortcut| {
            manager.register(shortcut).map_err(|e| {
                format!("\"{hotkey}\" could not be registered. Another app may be using it. ({e})")
            })
        });
    let error = result.err();
    if let Some(message) = &error {
        log(&state.paths, &format!("hotkey: {message}"));
    }
    let failed = error.is_some();
    *state.hotkey_error.write().unwrap() = error;
    if failed {
        open_settings_window(app, "settings");
    }
}

/// Re-read the config if the file changed, so hand edits apply without a restart.
fn reload_config_if_changed(app: &AppHandle, state: &State) {
    let path = state.paths.config();
    let stamp = config_stamp(&path);
    let mut last = state.config_stamp.lock().unwrap();
    if *last == stamp {
        return;
    }
    *last = stamp;
    drop(last);
    match Config::load(&path) {
        Ok(config) => {
            *state.config.write().unwrap() = config;
            *state.config_error.write().unwrap() = None;
            apply_config(app, state);
        }
        Err(e) => {
            log(&state.paths, &format!("config: {e}"));
            *state.config_error.write().unwrap() = Some(e);
            open_settings_window(app, "settings");
        }
    }
}

fn launcher(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("launcher")
}

/// Center horizontally, a little above the middle, on the monitor holding the cursor.
fn place_on_cursor_monitor(app: &AppHandle, window: &WebviewWindow) {
    let (Ok(cursor), Ok(monitors)) = (app.cursor_position(), app.available_monitors()) else {
        return;
    };
    let monitor = monitors.iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        cursor.x >= p.x as f64
            && cursor.y >= p.y as f64
            && cursor.x < p.x as f64 + s.width as f64
            && cursor.y < p.y as f64 + s.height as f64
    });
    if let Some(m) = monitor {
        let width = WIDTH * m.scale_factor();
        let x = m.position().x as f64 + (m.size().width as f64 - width) / 2.0;
        let y = m.position().y as f64 + m.size().height as f64 * 0.22;
        let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
}

fn show_launcher(app: &AppHandle) {
    let Some(window) = launcher(app) else { return };
    let state = app.state::<State>();
    reload_config_if_changed(app, &state);
    place_on_cursor_monitor(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("shown", ());
}

fn toggle_launcher(app: &AppHandle) {
    let Some(window) = launcher(app) else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show_launcher(app);
    }
}

fn open_settings_window(app: &AppHandle, route: &str) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let url = WebviewUrl::App(format!("index.html#{route}").into());
    let built = WebviewWindowBuilder::new(app, "settings", url)
        .title("Glint Settings")
        .inner_size(620.0, 700.0)
        .min_inner_size(480.0, 400.0)
        .build();
    if let Err(e) = built {
        log(
            &app.state::<State>().paths,
            &format!("settings window: {e}"),
        );
    }
}

#[derive(Serialize)]
struct SearchResults {
    generation: u64,
    items: Vec<Item>,
}

#[tauri::command]
async fn search(
    state: tauri::State<'_, State>,
    query: String,
    generation: u64,
    fuzzy: bool,
) -> Result<SearchResults, String> {
    let engine = state.engine.read().unwrap().clone();
    let limit = state.config.read().unwrap().max_results;
    let items =
        tauri::async_runtime::spawn_blocking(move || engine.search(&query, limit, fuzzy, now()))
            .await
            .map_err(|e| e.to_string())?;
    Ok(SearchResults { generation, items })
}

fn open_target(item: &Item) -> Result<(), String> {
    let message = |e: glint_win::Error| e.message();
    match &item.target {
        Target::Path(path) => shell::open(&path.to_string_lossy()).map_err(message),
        Target::App(target) => shell::open(&apps::shell_path(target)).map_err(message),
        Target::Url(url) => shell::open(url).map_err(message),
        Target::Copy(text) => clipboard::set_text(text).map_err(message),
        Target::System(id) => catalog::system_command(id)
            .ok_or_else(|| format!("Unknown command: {id}"))?
            .run(),
        Target::Window(handle) => windows_list::activate(*handle)
            .then_some(())
            .ok_or_else(|| "That window has closed.".to_string()),
        Target::Extension { .. } => Err("Extensions are not available.".to_string()),
    }
}

fn path_of(item: &Item) -> Result<&Path, String> {
    match &item.target {
        Target::Path(path) => Ok(path),
        _ => Err("This result has no file path.".to_string()),
    }
}

#[derive(Serialize)]
struct RunOutcome {
    /// Keep the window open, because the result list changed.
    stay_open: bool,
}

#[tauri::command]
fn run_item(
    app: AppHandle,
    state: tauri::State<'_, State>,
    key: String,
    action: String,
    query: String,
) -> Result<RunOutcome, String> {
    let engine = state.engine.read().unwrap().clone();
    let item = engine
        .shown(&key)
        .ok_or_else(|| "That result is no longer available.".to_string())?;
    let hide = || {
        if let Some(w) = launcher(&app) {
            let _ = w.hide();
        }
    };
    let text_err = |e: std::io::Error| e.to_string();
    match action.as_str() {
        "open" => {
            // Hide first: switching windows needs Glint out of the way.
            hide();
            open_target(&item)?;
            engine.record_open(&query, &item, now());
        }
        "admin" => {
            hide();
            let target = match &item.target {
                Target::App(t) => apps::shell_path(t),
                Target::Path(p) => p.display().to_string(),
                _ => return Err("This result cannot run as administrator.".to_string()),
            };
            shell::open_as_admin(&target).map_err(|e| e.message())?;
            engine.record_open(&query, &item, now());
        }
        "reveal" => {
            hide();
            shell::reveal(path_of(&item)?).map_err(text_err)?;
        }
        "copy_path" => {
            hide();
            clipboard::set_text(&path_of(&item)?.display().to_string()).map_err(|e| e.message())?;
        }
        "copy_name" => {
            hide();
            clipboard::set_text(&item.title).map_err(|e| e.message())?;
        }
        "terminal" => {
            hide();
            let path = path_of(&item)?;
            let dir = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(path)
            };
            shell::terminal_here(dir).map_err(text_err)?;
        }
        "properties" => {
            hide();
            shell::properties(&path_of(&item)?.display().to_string()).map_err(|e| e.message())?;
        }
        "pin" => {
            engine.update_history(|h| h.toggle_pin(&item.key));
            return Ok(RunOutcome { stay_open: true });
        }
        "hide" => {
            engine.update_history(|h| h.hide(&item.key));
            return Ok(RunOutcome { stay_open: true });
        }
        "forget" => {
            engine.update_history(|h| h.forget(&item.key));
            return Ok(RunOutcome { stay_open: true });
        }
        other => return Err(format!("Unknown action: {other}")),
    }
    Ok(RunOutcome { stay_open: false })
}

/// File types where each file has its own icon. Others share one icon per extension.
const OWN_ICON_EXTENSIONS: &[&str] = &["exe", "lnk", "ico", "url", "msc", "appref-ms"];

fn icon_source(item: &Item) -> Option<(String, String)> {
    match &item.target {
        Target::App(t) => {
            let source = apps::shell_path(t);
            Some((source.clone(), source))
        }
        Target::Path(path) => {
            let source = path.display().to_string();
            if path.is_dir() {
                return Some(("kind:folder".to_string(), source));
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext.is_empty() || OWN_ICON_EXTENSIONS.contains(&ext.as_str()) {
                Some((source.clone(), source))
            } else {
                Some((format!("ext:{ext}"), source))
            }
        }
        _ => None,
    }
}

/// A result's icon as a data URL, from memory, then disk, then the shell.
#[tauri::command]
async fn icon(state: tauri::State<'_, State>, key: String) -> Result<Option<String>, String> {
    let engine = state.engine.read().unwrap().clone();
    let Some((cache_key, source)) = engine.shown(&key).as_ref().and_then(icon_source) else {
        return Ok(None);
    };
    if let Some(url) = state.icon_cache.lock().unwrap().get(&cache_key) {
        return Ok(Some(url.clone()));
    }
    let mut hasher = DefaultHasher::new();
    cache_key.hash(&mut hasher);
    let file = state
        .paths
        .icons()
        .join(format!("{:016x}.png", hasher.finish()));
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(bytes) = std::fs::read(&file) {
            return Some(bytes);
        }
        glint_win::init_com();
        let bytes = icons::png(&source, 32).ok()?;
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&file, &bytes);
        Some(bytes)
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(bytes) = bytes else { return Ok(None) };
    let url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    state
        .icon_cache
        .lock()
        .unwrap()
        .insert(cache_key, url.clone());
    Ok(Some(url))
}

#[tauri::command]
fn hide_launcher(app: AppHandle) {
    if let Some(w) = launcher(&app) {
        let _ = w.hide();
    }
}

#[tauri::command]
fn set_height(app: AppHandle, height: f64) {
    if let Some(w) = launcher(&app) {
        let _ = w.set_size(LogicalSize::new(WIDTH, height.clamp(40.0, 640.0)));
    }
}

#[derive(Serialize)]
struct Settings {
    config: Config,
    config_path: String,
    config_error: Option<String>,
    hotkey_error: Option<String>,
    index_status: String,
    entries: usize,
    version: &'static str,
    user_css: String,
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, State>) -> Settings {
    Settings {
        config: state.config.read().unwrap().clone(),
        config_path: state.paths.config().display().to_string(),
        config_error: state.config_error.read().unwrap().clone(),
        hotkey_error: state.hotkey_error.read().unwrap().clone(),
        index_status: state.index_status.read().unwrap().clone(),
        entries: state.live.entry_count(),
        version: env!("CARGO_PKG_VERSION"),
        user_css: std::fs::read_to_string(state.paths.config_dir.join("theme.css"))
            .unwrap_or_default(),
    }
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: tauri::State<'_, State>,
    config: Config,
) -> Result<(), String> {
    let path = state.paths.config();
    config.save(&path).map_err(|e| e.to_string())?;
    // Validate what was written, using the same rules as a hand edit.
    let checked = Config::load(&path)?;
    *state.config_stamp.lock().unwrap() = config_stamp(&path);
    *state.config.write().unwrap() = checked;
    *state.config_error.write().unwrap() = None;
    apply_config(&app, &state);
    Ok(())
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    open_settings_window(&app, "settings");
}

#[tauri::command]
fn open_config_file(state: tauri::State<'_, State>) -> Result<(), String> {
    let path = state.paths.config();
    if !path.exists() {
        Config::default().save(&path).map_err(|e| e.to_string())?;
    }
    shell::open(&path.display().to_string()).map_err(|e| e.message())
}

#[tauri::command]
fn rebuild_index(state: tauri::State<'_, State>) {
    state
        .rebuild_requested
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn clear_history(state: tauri::State<'_, State>) {
    let engine = state.engine.read().unwrap().clone();
    engine.update_history(|h| *h = Default::default());
}

#[tauri::command]
fn clear_clipboard_history(state: tauri::State<'_, State>) {
    state.clips.clear();
}

/// Versions and counts for bug reports. Contains no file paths.
#[tauri::command]
fn diagnostics(state: tauri::State<'_, State>) -> Result<String, String> {
    let config = state.config.read().unwrap();
    let text = format!(
        "Glint {}\nOS: {} {}\nEntries: {}\nIndex: {}\nHotkey: {}\nHotkey error: {}\nClipboard history: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        state.live.entry_count(),
        state.index_status.read().unwrap(),
        config.hotkey,
        state.hotkey_error.read().unwrap().is_some(),
        config.clipboard_history,
    );
    clipboard::set_text(&text).map_err(|e| e.message())?;
    Ok(text)
}

/// Keep the file index current in the background: rebuild when stale, when asked, or when the
/// watcher lost track of changes.
fn index_loop(app: AppHandle) {
    let state = app.state::<State>();
    let snapshot = state.paths.data_dir.join("index").join("index.glint");
    let age = config_stamp(&snapshot).and_then(|t| t.elapsed().ok());
    let mut last = SystemTime::now();
    let mut due = age.is_none_or(|a| a > FRESH_SNAPSHOT);
    if due && age.is_some() {
        std::thread::sleep(STARTUP_DELAY);
    }
    loop {
        let hours = state.config.read().unwrap().rebuild_hours.max(1);
        due |= last
            .elapsed()
            .is_ok_and(|e| e > Duration::from_secs(hours * 3600));
        due |= state.live.take_needs_rebuild();
        due |= state
            .rebuild_requested
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        if due {
            *state.index_status.write().unwrap() = "Indexing".to_string();
            let started = SystemTime::now();
            let status = match state.live.rebuild(&state.walk) {
                Ok(stats) => {
                    let secs = started.elapsed().map_or(0, |e| e.as_secs());
                    let text = format!(
                        "Up to date. Last rebuild took {secs} s; {} folders unreadable.",
                        stats.unreadable
                    );
                    log(&state.paths, &text);
                    text
                }
                Err(e) => {
                    log(&state.paths, &format!("index: {e}"));
                    format!("Last rebuild failed: {e}")
                }
            };
            *state.index_status.write().unwrap() = status;
            last = SystemTime::now();
            due = false;
        }
        std::thread::sleep(INDEX_CHECK.min(Duration::from_secs(5)));
    }
}

fn apps_loop(state: Arc<Apps>) {
    glint_win::init_com();
    loop {
        if let Ok(list) = apps::list() {
            let items = list
                .iter()
                .map(|a| Apps::item(&a.name, &a.target))
                .collect();
            *state.0.write().unwrap() = ItemList::new(items);
        }
        std::thread::sleep(APP_REFRESH);
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Glint", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let rebuild = MenuItem::with_id(app, "rebuild", "Rebuild index", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &rebuild, &quit])?;
    let mut tray = TrayIconBuilder::with_id("glint")
        .tooltip("Glint")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_launcher(app),
            "settings" => open_settings_window(app, "settings"),
            "rebuild" => app
                .state::<State>()
                .rebuild_requested
                .store(true, std::sync::atomic::Ordering::Relaxed),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_launcher(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Log panics, and restart if the main thread died, but not in a tight loop.
fn install_panic_hook(paths: &Paths) {
    let log_path = Paths {
        config_dir: paths.config_dir.clone(),
        data_dir: paths.data_dir.clone(),
    };
    std::panic::set_hook(Box::new(move |info| {
        log(&log_path, &format!("panic: {info}"));
        let is_main = std::thread::current().name() == Some("main");
        let stamp = log_path.data_dir.join("last-restart");
        let recent = config_stamp(&stamp)
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|e| e < Duration::from_secs(60));
        if is_main && !recent {
            let _ = std::fs::write(&stamp, b"");
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        }
    }));
}

fn main() {
    let paths = Paths::resolve();
    let _ = std::fs::create_dir_all(&paths.data_dir);
    install_panic_hook(&paths);
    let first_run = !paths.config().exists();
    let (config, config_error) = match Config::load(&paths.config()) {
        Ok(config) => (config, None),
        Err(e) => (Config::default(), Some(e)),
    };

    let store = match Store::open(&paths.data_dir.join("index")) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            log(&paths, &format!("index folder: {e}"));
            std::process::exit(1);
        }
    };
    let mut walk = WalkConfig::whole_pc();
    walk.roots.extend(config.extra_roots.iter().cloned());
    walk.exclude_dirs
        .extend(config.extra_excludes.iter().cloned());
    let excludes: Vec<String> = DEFAULT_EXCLUDES
        .iter()
        .map(|s| s.to_string())
        .chain(config.extra_excludes.iter().cloned())
        .collect();
    let live = Arc::new(Live::new(store, excludes));
    let apps_provider = Arc::new(Apps::default());
    let clips = Arc::new(ClipboardHistory::default());
    let engine = build_engine(&paths, &config, &live, &apps_provider, &clips);
    let stamp = config_stamp(&paths.config());

    let state = State {
        paths,
        config: RwLock::new(config),
        config_error: RwLock::new(config_error),
        config_stamp: Mutex::new(stamp),
        hotkey_error: RwLock::new(None),
        engine: RwLock::new(engine),
        live: live.clone(),
        apps: apps_provider.clone(),
        clips,
        clip_monitor: Mutex::new(None),
        walk: walk.clone(),
        rebuild_requested: false.into(),
        index_status: RwLock::new("Starting".to_string()),
        icon_cache: Mutex::new(HashMap::new()),
    };

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_launcher(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_launcher(app);
                    }
                })
                .build(),
        )
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            search,
            run_item,
            icon,
            hide_launcher,
            set_height,
            get_settings,
            save_settings,
            open_settings,
            open_config_file,
            rebuild_index,
            clear_history,
            clear_clipboard_history,
            diagnostics,
        ])
        .on_window_event(|window, event| {
            if window.label() == "launcher" {
                if let WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = handle.state::<State>();
            setup_tray(&handle)?;
            apply_config(&handle, &state);
            if let Ok(watcher) = watch(live.clone(), &walk.roots) {
                app.manage(Mutex::new(watcher));
            }
            let indexer = handle.clone();
            std::thread::spawn(move || index_loop(indexer));
            std::thread::spawn(move || apps_loop(apps_provider));
            if first_run {
                open_settings_window(&handle, "welcome");
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        log(&Paths::resolve(), &format!("startup: {e}"));
    }
}
