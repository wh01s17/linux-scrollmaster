//! Opt-in XDG entry. Only our canonical file may be updated; no session/service edits.
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

const HEADER: &str = "# Managed by Linux ScrollMaster: autostart v1\n# Executable: ";
const FILENAME: &str = "io.github.wh01s17.linux-scrollmaster.autostart.desktop";
const MAX_BYTES: u64 = 8192;
static LOCK: Mutex<()> = Mutex::new(());

fn location() -> Result<PathBuf, String> {
    Ok(crate::lifecycle::config_root()?
        .join("autostart")
        .join(FILENAME))
}

fn entry(executable: &Path) -> Result<String, String> {
    let path = executable
        .to_str()
        .ok_or("La ruta de Linux ScrollMaster no es UTF-8")?;
    if !executable.is_absolute() || path.chars().any(char::is_control) || path.contains(['=', '%'])
    {
        return Err(
            "Esta ubicación de Linux ScrollMaster no se puede usar para inicio automático".into(),
        );
    }
    // Exec quoting, then Desktop Entry string escaping. Never execute a shell.
    let mut quoted = String::from("\"");
    for ch in path.chars() {
        match ch {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(ch);
            }
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    let exec = quoted.replace('\\', "\\\\");
    let metadata = serde_json::to_string(path).map_err(|e| e.to_string())?;
    let text = format!("{HEADER}{metadata}\n[Desktop Entry]\nType=Application\nVersion=1.0\nName=Linux ScrollMaster\nComment=Configuración del mouse al iniciar sesión\nExec={exec}\nIcon=linux-scrollmaster\nTerminal=false\nStartupNotify=false\nX-LinuxScrollMaster-Autostart-Version=1\n");
    if text.len() as u64 > MAX_BYTES {
        return Err("La ruta de Linux ScrollMaster es demasiado larga".into());
    }
    Ok(text)
}

fn read(path: &Path) -> Result<Option<(String, PathBuf)>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_BYTES {
        return Err(
            "La entrada de inicio automático no es un archivo propio válido; se conserva".into(),
        );
    }
    let mut text = String::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_BYTES {
        return Err("La entrada de inicio automático es demasiado grande; se conserva".into());
    }
    let stored = text
        .strip_prefix(HEADER)
        .and_then(|tail| tail.lines().next())
        .and_then(|line| serde_json::from_str::<String>(line).ok())
        .map(PathBuf::from)
        .ok_or("Hay una entrada de inicio automático ajena o modificada; se conserva")?;
    if entry(&stored)? != text {
        return Err("La entrada de inicio automático fue modificada; se conserva".into());
    }
    Ok(Some((text, stored)))
}

fn status(path: &Path, executable: &Path) -> Result<Value, String> {
    let stored = read(path)?;
    Ok(json!({"enabled": stored.is_some(), "can_change": true,
        "needs_update": stored.as_ref().is_some_and(|(text, exe)| exe != executable || entry(executable).is_ok_and(|current| text != &current)), "error": null}))
}

fn change(path: &Path, executable: &Path, enabled: bool) -> Result<Value, String> {
    let before = read(path)?;
    if enabled {
        let contents = entry(executable)?;
        let executable_metadata = fs::metadata(executable).map_err(|_| {
            "No se encuentra este ejecutable. Reabre Linux ScrollMaster antes de activar el inicio automático"
        })?;
        if !executable_metadata.is_file() || executable_metadata.permissions().mode() & 0o111 == 0 {
            return Err(
                "Esta ubicación de Linux ScrollMaster no contiene un ejecutable válido".into(),
            );
        }
        if before.as_ref().is_some_and(|(text, _)| text == &contents) {
            return status(path, executable);
        }
        let parent = path.parent().ok_or("Ruta de inicio automático inválida")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| e.to_string())?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = parent.join(format!(
            ".scrollmaster-autostart-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let result: Result<(), String> = (|| {
            file.write_all(contents.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            drop(file);
            if read(path)? != before {
                return Err("La entrada cambió durante la operación; vuelve a intentar".into());
            }
            if before.is_none() {
                // Publish without replacing a file created concurrently by another application.
                fs::hard_link(&temporary, path).map_err(|e| e.to_string())?;
            } else {
                fs::rename(&temporary, path).map_err(|e| e.to_string())?;
            }
            fs::File::open(parent)
                .and_then(|file| file.sync_all())
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        if temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    } else if before.is_some() {
        if read(path)? != before {
            return Err("La entrada cambió durante la operación; vuelve a intentar".into());
        }
        fs::remove_file(path).map_err(|e| e.to_string())?;
        fs::File::open(path.parent().ok_or("Ruta de inicio automático inválida")?)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    let result = status(path, executable)?; // read fresh, including after an atomic publish
    if result["enabled"] != enabled {
        return Err("No se pudo confirmar la preferencia de inicio automático".into());
    }
    Ok(result)
}

fn describe() -> Value {
    let result = location().and_then(|path| {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        status(&path, &executable)
    });
    result.unwrap_or_else(
        |error| json!({"enabled":false,"can_change":false,"needs_update":false,"error":error}),
    )
}

#[tauri::command]
pub async fn autostart_preferences() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = LOCK.lock().map_err(|_| "Preferencia ocupada")?;
        Ok(describe())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_autostart(enabled: bool) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOCK.lock().map_err(|_| "Preferencia ocupada")?;
        change(
            &location()?,
            &std::env::current_exe().map_err(|e| e.to_string())?,
            enabled,
        )
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
            let root = std::env::temp_dir().join(format!(
                "scrollmaster-autostart-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> PathBuf {
            self.0.join("config/autostart").join(FILENAME)
        }
        fn executable(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"fixture only; never executed").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn defaults_and_disabling_absent_entry_never_create_configuration() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let exe = fixture.executable("linux-scrollmaster");
        assert_eq!(status(&path, &exe).unwrap()["enabled"], false);
        assert_eq!(change(&path, &exe, false).unwrap()["enabled"], false);
        assert!(!fixture.0.join("config").exists());
    }

    #[test]
    fn enabling_and_disabling_confirm_private_file_and_preserve_other_preferences() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let exe = fixture.executable("linux-scrollmaster");
        fs::create_dir_all(fixture.0.join("config/linux-scrollmaster")).unwrap();
        for name in ["profiles.json", "desktop.json"] {
            fs::write(
                fixture.0.join("config/linux-scrollmaster").join(name),
                b"unchanged",
            )
            .unwrap();
        }
        assert_eq!(change(&path, &exe, true).unwrap()["enabled"], true);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let contents = fs::read(&path).unwrap();
        change(&path, &exe, true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), contents);
        let other = path.parent().unwrap().join("other.desktop");
        fs::write(&other, b"unrelated").unwrap();
        assert_eq!(change(&path, &exe, false).unwrap()["enabled"], false);
        assert_eq!(fs::read(other).unwrap(), b"unrelated");
        for name in ["profiles.json", "desktop.json"] {
            assert_eq!(
                fs::read(fixture.0.join("config/linux-scrollmaster").join(name)).unwrap(),
                b"unchanged"
            );
        }
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn relocation_requires_explicit_update_and_removed_executable_can_be_disabled() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let old = fixture.executable("old");
        let new = fixture.executable("new");
        change(&path, &old, true).unwrap();
        assert_eq!(status(&path, &new).unwrap()["needs_update"], true);
        assert_eq!(read(&path).unwrap().unwrap().1, old);
        assert_eq!(change(&path, &new, true).unwrap()["needs_update"], false);
        assert_eq!(read(&path).unwrap().unwrap().1, new);
        fs::remove_file(&new).unwrap();
        assert_eq!(change(&path, &new, false).unwrap()["enabled"], false);
    }

    #[test]
    fn foreign_or_edited_entries_are_never_overwritten_or_removed() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let exe = fixture.executable("linux-scrollmaster");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in [
            "[Desktop Entry]\nType=Application\nExec=other\n".into(),
            entry(&exe).unwrap() + "Hidden=true\n",
            entry(&exe)
                .unwrap()
                .replace("Terminal=false", "Terminal=true"),
        ] {
            fs::write(&path, &text).unwrap();
            assert!(status(&path, &exe).is_err());
            for enabled in [true, false] {
                assert!(change(&path, &exe, enabled).is_err());
                assert_eq!(fs::read_to_string(&path).unwrap(), text);
            }
        }
    }

    #[test]
    fn symlinks_and_nonregular_entries_are_preserved() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let path = fixture.path();
        let exe = fixture.executable("linux-scrollmaster");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = fixture.0.join("target");
        fs::write(&target, entry(&exe).unwrap()).unwrap();
        symlink(&target, &path).unwrap();
        for enabled in [true, false] {
            assert!(change(&path, &exe, enabled).is_err());
        }
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(change(&path, &exe, true).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn executable_validation_prevents_broken_or_injected_entries() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let exe = fixture.executable("linux-scrollmaster");
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(change(&path, &exe, true).is_err());
        assert!(change(&path, &fixture.0.join("missing"), true).is_err());
        for name in [
            "relative",
            "/tmp/scrollmaster\nHidden=false",
            "/tmp/scrollmaster=other",
            "/tmp/scrollmaster%f",
            "/tmp/scrollmaster\0bad",
        ] {
            assert!(entry(Path::new(name)).is_err());
        }
        assert!(!fixture.0.join("config").exists());
    }

    #[test]
    fn desktop_exec_quoting_roundtrips_spaces_unicode_and_reserved_characters() {
        use gtk::gio::prelude::AppInfoExt;
        let fixture = Fixture::new();
        for name in [
            "Linux ScrollMaster app",
            "Linux ScrollMaster áé",
            "Linux ScrollMaster '$`\"\\;",
        ] {
            let exe = fixture.executable(name);
            let contents = entry(&exe).unwrap();
            let keyfile = gtk::glib::KeyFile::new();
            keyfile
                .load_from_data(&contents, gtk::glib::KeyFileFlags::NONE)
                .unwrap();
            let exec = keyfile.string("Desktop Entry", "Exec").unwrap();
            let args = gtk::glib::shell_parse_argv(exec.as_str()).unwrap();
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].to_string_lossy(), exe.to_str().unwrap());
            // Actual desktop parser, no launch or display.
            assert!(gtk::gio::DesktopAppInfo::from_keyfile(&keyfile)
                .unwrap()
                .commandline()
                .is_some());
        }
    }

    #[test]
    fn oversized_or_invalid_utf8_entry_is_preserved() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let exe = fixture.executable("linux-scrollmaster");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for bytes in [vec![b'x'; MAX_BYTES as usize + 1], vec![255, 254]] {
            fs::write(&path, &bytes).unwrap();
            assert!(change(&path, &exe, true).is_err());
            assert!(change(&path, &exe, false).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn storage_failure_does_not_change_existing_files_or_leave_temporary_entries() {
        let fixture = Fixture::new();
        let exe = fixture.executable("linux-scrollmaster");
        let blocker = fixture.0.join("config");
        fs::write(&blocker, b"keep").unwrap();
        assert!(change(&fixture.path(), &exe, true).is_err());
        assert_eq!(fs::read(&blocker).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
    }
}
