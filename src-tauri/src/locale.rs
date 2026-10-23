// SPDX-License-Identifier: GPL-3.0-or-later
//! Interface language for the native parts (tray menu). The web view keeps its
//! own copy; this file only mirrors the user's choice for text Rust draws.
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Es,
    En,
}
impl Locale {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "es" => Some(Self::Es),
            "en" => Some(Self::En),
            _ => None,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Es => "es",
            Self::En => "en",
        }
    }
}

fn path() -> Result<PathBuf, String> {
    Ok(crate::lifecycle::config_root()?.join("linux-scrollmaster/locale.json"))
}
fn load(path: &Path) -> Option<Locale> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(257)
        .read_to_string(&mut text)
        .ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    (value["version"] == 1).then_some(())?;
    Locale::parse(value["locale"].as_str()?)
}
fn save(path: &Path, locale: Locale) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid locale path")?;
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| e.to_string())?;
    crate::profile_files::write_json(path, &json!({"version":1,"locale":locale.id()}), 256)
}
/// System language when nothing was chosen: Spanish for es_*, English otherwise.
fn from_environment(vars: impl Fn(&str) -> Option<String>) -> Locale {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| vars(name).filter(|v| !v.is_empty()))
        .filter(|v| v.starts_with("es"))
        .map_or(Locale::En, |_| Locale::Es)
}
/// Saved choice, else the system language.
pub fn current() -> Locale {
    path()
        .ok()
        .and_then(|p| load(&p))
        .unwrap_or_else(|| from_environment(|name| std::env::var(name).ok()))
}

pub struct TrayText {
    pub show: &'static str,
    pub quit: &'static str,
    pub tooltip: &'static str,
}
pub fn tray_text(locale: Locale) -> TrayText {
    match locale {
        Locale::Es => TrayText {
            show: "Abrir Linux ScrollMaster",
            quit: "Salir de Linux ScrollMaster",
            tooltip: "Linux ScrollMaster · Configuración del mouse",
        },
        Locale::En => TrayText {
            show: "Open Linux ScrollMaster",
            quit: "Quit Linux ScrollMaster",
            tooltip: "Linux ScrollMaster · Mouse settings",
        },
    }
}

/// Persist the interface language and redraw the tray in it, if it is shown.
#[tauri::command]
pub async fn set_locale(app: tauri::AppHandle, locale: String) -> Result<(), String> {
    let locale = Locale::parse(&locale).ok_or("Unknown locale")?;
    save(&path()?, locale)?;
    let handle = app.clone();
    app.run_on_main_thread(move || crate::lifecycle::refresh_tray(&handle))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn location() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir()
            .join(format!(
                "scrollmaster-locale-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ))
            .join("locale.json")
    }
    #[test]
    fn choice_roundtrips_and_bad_files_fall_back() {
        let path = location();
        assert_eq!(load(&path), None);
        save(&path, Locale::En).unwrap();
        assert_eq!(load(&path), Some(Locale::En));
        save(&path, Locale::Es).unwrap();
        assert_eq!(load(&path), Some(Locale::Es));
        for bad in [
            "{bad",
            "{\"version\":2,\"locale\":\"en\"}",
            "{\"version\":1,\"locale\":\"fr\"}",
        ] {
            fs::write(&path, bad).unwrap();
            assert_eq!(load(&path), None);
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn system_language_picks_spanish_only_for_es() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            from_environment(env(&[("LANG", "es_CL.UTF-8")])),
            Locale::Es
        );
        assert_eq!(
            from_environment(env(&[("LANG", "en_US.UTF-8")])),
            Locale::En
        );
        assert_eq!(
            from_environment(env(&[("LC_ALL", "es_ES.UTF-8"), ("LANG", "en_US.UTF-8")])),
            Locale::Es
        );
        assert_eq!(
            from_environment(env(&[("LC_ALL", ""), ("LANG", "es_MX.UTF-8")])),
            Locale::Es
        );
        assert_eq!(from_environment(env(&[])), Locale::En);
    }
}
