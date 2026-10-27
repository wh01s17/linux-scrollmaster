//! Opt-in tray startup and background operation, with a visible fallback.
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager,
};

pub struct DesktopState {
    enabled: AtomicBool,
    error: Mutex<Option<String>>,
    startup: Mutex<Startup>,
}
#[derive(Debug, PartialEq)]
enum Startup {
    Waiting,
    Tray,
    Window,
}
impl Startup {
    /// Resolve initial visibility once. A late panel must never hide a window
    /// opened explicitly or shown as a fallback.
    fn resolve(&mut self, enabled: bool, available: bool, expired: bool) -> bool {
        if *self != Self::Waiting {
            return false;
        }
        if !enabled || (!available && expired) {
            *self = Self::Window;
            true
        } else {
            if available {
                *self = Self::Tray;
            }
            false
        }
    }
}
impl DesktopState {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            error: Mutex::new(None),
            startup: Mutex::new(Startup::Waiting),
        }
    }
    pub fn keep_running(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}
pub(crate) fn config_root() -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .ok_or("No se encontró el directorio de configuración del usuario")?;
    if !root.is_absolute() {
        return Err("El directorio de configuración debe ser absoluto".into());
    }
    Ok(root)
}
fn path() -> Result<PathBuf, String> {
    Ok(config_root()?.join("linux-scrollmaster/desktop.json"))
}
fn load(path: &Path) -> Result<bool, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    let mut text = String::new();
    file.take(4097)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 4096 {
        return Err("Preferencias de escritorio demasiado grandes".into());
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| "Preferencias de escritorio corruptas; archivo conservado")?;
    if value["version"] != 1 || !value["tray"].is_boolean() {
        return Err(
            "Formato de preferencias de escritorio incompatible; archivo conservado".into(),
        );
    }
    Ok(value["tray"].as_bool().unwrap_or(false))
}
fn save(path: &Path, enabled: bool) -> Result<(), String> {
    // Never overwrite an existing malformed file silently.
    load(path)?;
    let parent = path.parent().ok_or("Ruta de preferencias inválida")?;
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| e.to_string())?;
    crate::profile_files::write_json(path, &json!({"version":1,"tray":enabled}), 4096)
}
fn create_tray(app: &tauri::AppHandle) -> Result<(), String> {
    if app.tray_by_id("linux-scrollmaster").is_some() {
        return Ok(());
    }
    let text = crate::locale::tray_text(crate::locale::current());
    let show =
        MenuItem::with_id(app, "show", text.show, true, None::<&str>).map_err(|e| e.to_string())?;
    let quit =
        MenuItem::with_id(app, "quit", text.quit, true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&show, &quit]).map_err(|e| e.to_string())?;
    let icon = app
        .default_window_icon()
        .ok_or("No se pudo cargar el icono propio de Linux ScrollMaster")?
        .clone();
    TrayIconBuilder::with_id("linux-scrollmaster")
        .icon(icon)
        .tooltip(text.tooltip)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "quit" => app.exit(0),
            _ => (),
        })
        // The panel sends one activation per click, and swallows the second click
        // of a fast double click: open on any left activation.
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}
/// Rebuild a visible tray so its menu follows a language change.
pub(crate) fn refresh_tray(app: &tauri::AppHandle) {
    if app.remove_tray_by_id("linux-scrollmaster").is_some() {
        if let Err(error) = create_tray(app) {
            *app.state::<DesktopState>()
                .error
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(error);
        }
    }
}
pub fn show_window(app: &tauri::AppHandle) {
    *app.state::<DesktopState>()
        .startup
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Startup::Window;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
fn resolve_startup(app: &tauri::AppHandle, available: bool, expired: bool) {
    let state = app.state::<DesktopState>();
    let show = state
        .startup
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .resolve(state.keep_running(), available, expired);
    if show {
        show_window(app);
    }
}
fn record(app: &tauri::AppHandle, error: Option<String>) {
    *app.state::<DesktopState>()
        .error
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = error;
}
/// Show the opted-in tray if it is missing. Main thread only. At login the panel may
/// not have registered its StatusNotifierWatcher yet, so a first attempt can fail.
pub(crate) fn ensure_tray(app: &tauri::AppHandle) -> bool {
    if !app.state::<DesktopState>().keep_running() {
        return false;
    }
    match create_tray(app) {
        Ok(()) => {
            record(app, None);
            true
        }
        Err(error) => {
            record(app, Some(error));
            false
        }
    }
}
/// Wait for a tray host that appears after us (autostart races the panel).
fn retry_tray(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        for attempt in 1..=TRAY_RETRIES {
            std::thread::sleep(TRAY_RETRY_DELAY);
            let (tx, rx) = mpsc::channel();
            let handle = app.clone();
            let sent = app.run_on_main_thread(move || {
                let state = handle.state::<DesktopState>();
                let available = ensure_tray(&handle);
                resolve_startup(&handle, available, attempt >= STARTUP_WAIT_RETRIES);
                let done = !state.keep_running() || available;
                let _ = tx.send(done);
            });
            if sent.is_err() || rx.recv().unwrap_or(true) {
                return;
            }
        }
    });
}
const TRAY_RETRIES: u32 = 60;
const TRAY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
// Show a fallback after 10 seconds, but keep trying the tray for two minutes.
const STARTUP_WAIT_RETRIES: u32 = 5;
pub fn initialize(app: &tauri::AppHandle) {
    match path().and_then(|p| load(&p)) {
        Ok(enabled) => {
            // The preference stands even if the tray host is not up yet.
            app.state::<DesktopState>()
                .enabled
                .store(enabled, Ordering::SeqCst);
            let available = enabled && ensure_tray(app);
            resolve_startup(app, available, false);
            if enabled && !available {
                retry_tray(app.clone());
            }
        }
        Err(error) => {
            record(app, Some(error));
            show_window(app);
        }
    }
}
#[tauri::command]
pub fn desktop_preferences(app: tauri::AppHandle) -> Value {
    let state = app.state::<DesktopState>();
    json!({"tray":state.keep_running(),"error":*state.error.lock().unwrap_or_else(|e|e.into_inner())})
}
#[tauri::command]
pub async fn set_tray(app: tauri::AppHandle, enabled: bool) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let was_enabled = handle.state::<DesktopState>().keep_running();
            if enabled {
                create_tray(&handle)?;
            }
            if let Err(error) = path().and_then(|p| save(&p, enabled)) {
                if enabled && !was_enabled {
                    handle.remove_tray_by_id("linux-scrollmaster");
                }
                return Err(error);
            }
            if !enabled {
                show_window(&handle);
                handle.remove_tray_by_id("linux-scrollmaster");
            }
            let state = handle.state::<DesktopState>();
            state.enabled.store(enabled, Ordering::SeqCst);
            *state.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(desktop_preferences(handle.clone()))
        })();
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())??
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_uses_tray_without_showing_the_window() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["app"]["windows"][0]["visible"], false);
        let mut startup = Startup::Waiting;
        assert!(!startup.resolve(true, true, false));
        assert_eq!(startup, Startup::Tray);
        assert!(!startup.resolve(true, false, true));
    }
    #[test]
    fn delayed_panel_stays_hidden_until_ready() {
        let mut startup = Startup::Waiting;
        assert!(!startup.resolve(true, false, false));
        assert_eq!(startup, Startup::Waiting);
        assert!(!startup.resolve(true, true, false));
        assert_eq!(startup, Startup::Tray);
    }
    #[test]
    fn missing_panel_shows_once_and_late_panel_keeps_window_open() {
        let mut startup = Startup::Waiting;
        assert!(startup.resolve(true, false, true));
        assert_eq!(startup, Startup::Window);
        assert!(!startup.resolve(true, true, true));
        assert_eq!(startup, Startup::Window);
    }
    #[test]
    fn disabled_tray_and_explicit_open_do_not_wait_for_panel() {
        let mut startup = Startup::Waiting;
        assert!(startup.resolve(false, false, false));
        assert_eq!(startup, Startup::Window);
        // An explicit open (IPC or tray) also cancels the startup wait.
        let mut opened = Startup::Window;
        assert!(!opened.resolve(true, false, true));
        assert!(!opened.resolve(true, true, false));
        assert_eq!(opened, Startup::Window);
    }
    fn location() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir()
            .join(format!(
                "scrollmaster-desktop-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ))
            .join("desktop.json")
    }
    #[test]
    fn tray_defaults_off_and_roundtrips_without_touching_profiles() {
        let path = location();
        assert!(!load(&path).unwrap());
        save(&path, true).unwrap();
        assert!(load(&path).unwrap());
        fs::write(path.parent().unwrap().join("profiles.json"), b"keep").unwrap();
        save(&path, false).unwrap();
        assert!(!load(&path).unwrap());
        assert_eq!(
            fs::read(path.parent().unwrap().join("profiles.json")).unwrap(),
            b"keep"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn corrupt_preferences_are_preserved() {
        let path = location();
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{bad").unwrap();
        assert!(save(&path, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{bad");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn malformed_and_large_preferences_fail_closed() {
        let path = location();
        fs::create_dir(path.parent().unwrap()).unwrap();
        for value in [
            "{\"version\":2,\"tray\":true}".to_string(),
            "{\"version\":1,\"tray\":1}".to_string(),
            " ".repeat(4097),
        ] {
            fs::write(&path, value).unwrap();
            assert!(load(&path).is_err());
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
