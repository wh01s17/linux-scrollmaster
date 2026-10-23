//! Optional tray. Closing keeps the controller alive only after explicit opt-in.
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
}
impl DesktopState {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            error: Mutex::new(None),
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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
pub fn initialize(app: &tauri::AppHandle) {
    let result = path().and_then(|p| load(&p)).and_then(|enabled| {
        if enabled {
            create_tray(app)?;
        }
        app.state::<DesktopState>()
            .enabled
            .store(enabled, Ordering::SeqCst);
        Ok(())
    });
    if let Err(error) = result {
        *app.state::<DesktopState>()
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error);
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
