// SPDX-License-Identifier: GPL-3.0-or-later
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

/// Our own process chain is excluded: the GUI that launched this native worker
/// and the worker that launched a guardian are not competing controllers. Any
/// other instance, Solaar, logid or the legacy Python worker still blocks.
pub fn ensure_no_controllers() -> Result<(), String> {
    let proc = Path::new("/proc");
    check_processes_except(proc, &ancestors(proc, std::process::id()))
}
#[cfg(test)]
pub(super) fn check_processes(proc: &Path) -> Result<(), String> {
    check_processes_except(proc, &[])
}
/// Self and parents via `stat` (field after the last `)` is state, then ppid).
pub(super) fn ancestors(proc: &Path, mut pid: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    while pid > 1 && chain.len() < 64 && !chain.contains(&pid) {
        chain.push(pid);
        let Ok(stat) = fs::read_to_string(proc.join(pid.to_string()).join("stat")) else {
            break;
        };
        pid = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse().ok())
            .unwrap_or(0);
    }
    chain
}
/// A process that exits during the scan reports ENOENT or, once its /proc
/// entry is already open, ESRCH. Neither is a controller.
pub(super) fn vanished(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}
pub(super) fn check_processes_except(proc: &Path, own: &[u32]) -> Result<(), String> {
    for entry in fs::read_dir(proc).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if own.contains(&pid) {
            continue;
        }
        let name = match fs::read_to_string(entry.path().join("comm")) {
            Ok(name) => name,
            Err(error) if vanished(&error) => continue,
            Err(error) => return Err(format!("Cannot check controllers: {error}")),
        };
        // /proc/comm is limited to 15 bytes, including the renamed application.
        if matches!(
            name.trim(),
            "solaar" | "logid" | "linux-scrollmas" | "linux-scrollma" | "linux-scrollmaster"
        ) {
            return Err(format!(
                "Cierra {} antes del diagnóstico nativo. No se detienen procesos automáticamente.",
                name.trim()
            ));
        }
        // Also detect the existing Python worker if the GUI has already exited.
        if name.trim().starts_with("python") {
            match fs::read(entry.path().join("cmdline")) {
                Ok(cmd)
                    if cmd.split(|b| *b == 0).any(|arg| {
                        arg.ends_with(b"/bridge/service.py") || arg == b"bridge/service.py"
                    }) =>
                {
                    return Err(
                        "El worker de Linux ScrollMaster sigue activo; espera a que termine."
                            .into(),
                    )
                }
                Ok(_) => {}
                Err(error) if vanished(&error) => {}
                Err(error) => return Err(format!("Cannot check Python worker: {error}")),
            }
        }
    }
    Ok(())
}

pub struct HardwareLease {
    _file: File,
}
impl HardwareLease {
    pub fn acquire() -> Result<Self, String> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or("XDG_RUNTIME_DIR is required for the shared hardware lease")?;
        Self::at(Path::new(&runtime))
    }
    pub(super) fn at(runtime: &Path) -> Result<Self, String> {
        if !runtime.is_absolute() {
            return Err("XDG_RUNTIME_DIR must be absolute".into());
        }
        let uid = fs::metadata("/proc/self").map_err(|e| e.to_string())?.uid();
        if uid == 0 {
            return Err("Do not run the native diagnostic as root".into());
        }
        let dir = fs::symlink_metadata(runtime).map_err(|e| e.to_string())?;
        if !dir.is_dir() || dir.uid() != uid || dir.mode() & 0o077 != 0 {
            return Err("Runtime directory must be private and owned by this user".into());
        }
        // Same lease as bridge/service.py. No truncation, profile read or preferences.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(runtime.join("linux-scrollmaster-hardware.lock"))
            .map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.uid() != uid || meta.mode() & 0o022 != 0 || meta.nlink() != 1 {
            return Err("Unsafe hardware lease file".into());
        }
        file.try_lock()
            .map_err(|e| format!("Another controller owns the hardware lease: {e}"))?;
        Ok(Self { _file: file })
    }
}
