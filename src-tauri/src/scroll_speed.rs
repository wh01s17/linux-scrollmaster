//! Opt-in scroll distance on Hyprland. The value lives only in our own scroll.json and
//! is applied at runtime; it is reapplied after a compositor reload. No hypr file edits.
use linux_scrollmaster::native::scroll;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

static LOCK: Mutex<()> = Mutex::new(());
const REAPPLY_EVERY: Duration = Duration::from_secs(5);

fn path() -> Result<PathBuf, String> {
    Ok(crate::lifecycle::config_root()?.join("linux-scrollmaster/scroll.json"))
}
fn load(path: &Path) -> Result<Option<f64>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut text = String::new();
    file.take(4097)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&text)
        .ok()
        .filter(|_| text.len() <= 4096)
        .ok_or("Preferencia de desplazamiento corrupta; archivo conservado")?;
    match (&value["version"], value["factor"].as_f64()) {
        (version, Some(factor)) if *version == 1 => scroll::normalize(factor).map(Some),
        _ => {
            Err("Formato de preferencia de desplazamiento incompatible; archivo conservado".into())
        }
    }
}
fn save(path: &Path, factor: Option<f64>) -> Result<(), String> {
    // Never overwrite or remove a malformed file silently.
    load(path)?;
    let Some(factor) = factor else {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    };
    let parent = path.parent().ok_or("Ruta de preferencias inválida")?;
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| e.to_string())?;
    crate::profile_files::write_json(path, &json!({"version":1,"factor":factor}), 4096)
}

fn describe(
    saved: Result<Option<f64>, String>,
    devices: Result<Vec<scroll::ScrollDevice>, String>,
) -> Value {
    let (available, factor, error) = match devices {
        Ok(found) if found.is_empty() => (
            false,
            None,
            Some("Hyprland no muestra el MX Master como dispositivo".to_string()),
        ),
        // -1: no own factor yet, the global value applies.
        Ok(found) => (true, Some(found[0].factor).filter(|f| *f > 0.0), None),
        Err(error) => (false, None, Some(error)),
    };
    let (saved, error) = match saved {
        Ok(saved) => (saved, error),
        Err(file) => (None, Some(file)),
    };
    json!({"available": available, "factor": factor, "saved": saved,
        "min": scroll::MIN, "max": scroll::MAX, "error": error})
}
fn current() -> Value {
    describe(path().and_then(|p| load(&p)), scroll::read())
}

#[tauri::command]
pub async fn scroll_speed_preferences() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = LOCK.lock().map_err(|_| "Preferencia ocupada")?;
        Ok(current())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `None` returns the mouse to the session's global factor and forgets the preference.
#[tauri::command]
pub async fn set_scroll_speed(factor: Option<f64>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOCK.lock().map_err(|_| "Preferencia ocupada")?;
        let file = path()?;
        load(&file)?;
        match factor {
            Some(value) => {
                let value = scroll::normalize(value)?;
                scroll::apply(value)?;
                save(&file, Some(value))?;
            }
            None => {
                save(&file, None)?;
                scroll::reset()?;
            }
        }
        Ok(current())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A Hyprland config reload drops runtime device settings: put ours back.
pub fn keep_applied() {
    std::thread::spawn(|| loop {
        if let Ok(_guard) = LOCK.lock() {
            if let Ok(Some(factor)) = path().and_then(|p| load(&p)) {
                let drifted = scroll::read()
                    .is_ok_and(|found| found.iter().any(|d| !scroll::same(d.factor, factor)));
                if drifted {
                    if let Err(error) = scroll::apply(factor) {
                        eprintln!("No se pudo reaplicar la velocidad de desplazamiento: {error}");
                    }
                }
            }
        }
        std::thread::sleep(REAPPLY_EVERY);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn location() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir()
            .join(format!(
                "scrollmaster-scroll-pref-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
            .join("linux-scrollmaster/scroll.json")
    }

    #[test]
    fn preference_defaults_off_roundtrips_and_reset_removes_only_our_file() {
        let path = location();
        assert_eq!(load(&path).unwrap(), None);
        save(&path, None).unwrap();
        assert!(!path.exists());
        save(&path, Some(1.5)).unwrap();
        assert_eq!(load(&path).unwrap(), Some(1.5));
        let sibling = path.with_file_name("desktop.json");
        fs::write(&sibling, b"unchanged").unwrap();
        save(&path, None).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(&sibling).unwrap(), b"unchanged");
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn malformed_or_out_of_range_files_are_preserved() {
        let path = location();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in [
            "{",
            r#"{"version":2,"factor":1.0}"#,
            r#"{"version":1,"factor":0}"#,
            r#"{"version":1,"factor":"fast"}"#,
        ] {
            fs::write(&path, text).unwrap();
            assert!(load(&path).is_err(), "{text}");
            for factor in [Some(1.5), None] {
                assert!(save(&path, factor).is_err());
                assert_eq!(fs::read_to_string(&path).unwrap(), text);
            }
        }
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn description_separates_availability_device_value_and_saved_value() {
        let device = |factor| scroll::ScrollDevice {
            name: "logitech-usb-receiver-mouse".into(),
            factor,
        };
        let unset = describe(Ok(None), Ok(vec![device(-1.0)]));
        assert_eq!(unset["available"], true);
        assert!(unset["factor"].is_null() && unset["error"].is_null());
        let set = describe(Ok(Some(1.5)), Ok(vec![device(1.5)]));
        assert_eq!(
            (set["factor"].as_f64(), set["saved"].as_f64()),
            (Some(1.5), Some(1.5))
        );
        let missing = describe(Ok(None), Ok(vec![]));
        assert_eq!(missing["available"], false);
        let other = describe(Ok(None), Err("Missing Hyprland instance signature".into()));
        assert_eq!(other["available"], false);
        assert_eq!(other["error"], "Missing Hyprland instance signature");
        let corrupt = describe(Err("corrupta".into()), Ok(vec![device(1.0)]));
        assert_eq!(corrupt["error"], "corrupta");
    }
}
