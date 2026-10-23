//! Native file selection grants access to one user-chosen file. No general path IPC.
use gtk::prelude::*;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
};
use tauri::Manager;

const MAX_BYTES: usize = 128 * 1024;

async fn choose(app: tauri::AppHandle, save: bool) -> Result<Option<PathBuf>, String> {
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let parent = handle
                .get_webview_window("main")
                .ok_or("Sin ventana principal")?
                .gtk_window()
                .map_err(|e| e.to_string())?;
            let dialog = gtk::FileChooserNative::new(
                Some(if save {
                    "Exportar perfil de Linux ScrollMaster"
                } else {
                    "Importar perfiles de Linux ScrollMaster"
                }),
                Some(&parent),
                if save {
                    gtk::FileChooserAction::Save
                } else {
                    gtk::FileChooserAction::Open
                },
                Some(if save { "Exportar" } else { "Abrir" }),
                Some("Cancelar"),
            );
            dialog.set_modal(true);
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Perfiles JSON"));
            filter.add_pattern("*.json");
            dialog.add_filter(filter);
            if save {
                dialog.set_current_name("linux-scrollmaster-perfil.json");
                dialog.set_do_overwrite_confirmation(true);
            }
            let path = if dialog.run() == gtk::ResponseType::Accept {
                dialog.filename()
            } else {
                None
            };
            dialog.destroy();
            Ok::<_, String>(path)
        })();
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())??
}

fn read_document(path: &Path) -> Result<Value, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > MAX_BYTES {
        return Err("El archivo supera 128 KiB".into());
    }
    serde_json::from_str(&text).map_err(|_| "El archivo no contiene JSON válido".into())
}

fn write_document(path: &Path, document: &Value) -> Result<(), String> {
    if !document.get("profiles").is_some_and(Value::is_array) {
        return Err("Exportación de perfiles inválida".into());
    }
    write_json(path, document, MAX_BYTES)
}

pub(crate) fn write_json(path: &Path, document: &Value, maximum: usize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
    if bytes.len() > maximum {
        return Err("La exportación supera 128 KiB".into());
    }
    let parent = path.parent().ok_or("Ruta de exportación inválida")?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary = parent.join(format!(
        ".scrollmaster-export-{}-{stamp}",
        std::process::id()
    ));
    let result = (|| {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if temporary.exists() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[tauri::command]
pub async fn import_profile_file(app: tauri::AppHandle) -> Result<Option<Value>, String> {
    let Some(path) = choose(app, false).await? else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || read_document(&path).map(Some))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_profile_file(
    app: tauri::AppHandle,
    document: Value,
) -> Result<Option<String>, String> {
    let Some(path) = choose(app, true).await? else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        write_document(&path, &document)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "scrollmaster-files-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn json_roundtrip_export_and_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new();
        let path = f.0.join("profile.json");
        let data = serde_json::json!({"version":4,"profiles":[],"active":{}});
        write_document(&path, &data).unwrap();
        assert_eq!(read_document(&path).unwrap(), data);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn oversized_or_invalid_import_is_rejected() {
        let f = Fixture::new();
        let path = f.0.join("bad.json");
        fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(read_document(&path).is_err());
        fs::write(&path, b"{broken").unwrap();
        assert!(read_document(&path).is_err());
    }
    #[test]
    fn failed_export_does_not_remove_destination_or_leave_temporary_file() {
        let f = Fixture::new();
        let destination = f.0.join("existing");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("keep"), b"intact").unwrap();
        assert!(write_document(&destination, &serde_json::json!({"profiles":[]})).is_err());
        assert_eq!(fs::read(destination.join("keep")).unwrap(), b"intact");
        assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
    }
}
