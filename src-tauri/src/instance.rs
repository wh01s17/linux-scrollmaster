//! One desktop controller per user session. IPC accepts only a four-byte show request.
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Lease {
    _file: File,
    socket: PathBuf,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
        // Explicit unlock also releases temporary inherited/duplicated descriptors.
        let _ = self._file.unlock();
    }
}
pub enum Instance {
    Primary(Lease, UnixListener),
    Existing,
}

fn private_directory(path: &Path) -> Result<(), String> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.to_string()),
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let uid = fs::metadata("/proc/self").map_err(|e| e.to_string())?.uid();
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(
            "El directorio de sesión de Linux ScrollMaster no es privado o no pertenece al usuario"
                .into(),
        );
    }
    Ok(())
}

pub fn acquire(directory: &Path) -> Result<Instance, String> {
    private_directory(directory)?;
    let socket = directory.join("control.sock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("instance.lock"))
        .map_err(|e| e.to_string())?;
    for _ in 0..20 {
        match file.try_lock() {
            Ok(()) => {
                // Holding the lease proves any previous socket is stale.
                if socket.exists() {
                    fs::remove_file(&socket).map_err(|e| e.to_string())?;
                }
                let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
                return Ok(Instance::Primary(
                    Lease {
                        _file: file,
                        socket,
                    },
                    listener,
                ));
            }
            Err(TryLockError::WouldBlock) => {
                // The owner may be starting or exiting; retry both lease and socket.
                if let Ok(mut stream) = UnixStream::connect(&socket) {
                    stream
                        .set_write_timeout(Some(Duration::from_millis(200)))
                        .map_err(|e| e.to_string())?;
                    stream.write_all(b"show").map_err(|e| e.to_string())?;
                    return Ok(Instance::Existing);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(TryLockError::Error(e)) => return Err(e.to_string()),
        }
    }
    Err(
        "Linux ScrollMaster ya está abierta, pero su ventana no responde. No se iniciará otro controlador"
            .into(),
    )
}

pub fn runtime_directory() -> Result<PathBuf, String> {
    let runtime =
        std::env::var_os("XDG_RUNTIME_DIR").ok_or("La sesión no define XDG_RUNTIME_DIR")?;
    let root = PathBuf::from(runtime);
    if !root.is_absolute() {
        return Err("XDG_RUNTIME_DIR debe ser una ruta absoluta".into());
    }
    Ok(root.join("linux-scrollmaster"))
}

pub fn serve(listener: UnixListener, show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            if stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .is_err()
            {
                continue;
            }
            let mut command = [0; 4];
            if stream.read_exact(&mut command).is_ok() && command == *b"show" {
                show();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn directory() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "scrollmaster-instance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ))
    }
    #[test]
    fn second_instance_notifies_owner_and_release_allows_restart() {
        let path = directory();
        let Instance::Primary(lease, listener) = acquire(&path).unwrap() else {
            panic!()
        };
        assert!(matches!(acquire(&path).unwrap(), Instance::Existing));
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"show");
        drop(stream);
        drop(listener);
        let inherited = lease._file.try_clone().unwrap();
        drop(lease);
        assert!(!path.join("control.sock").exists());
        assert!(matches!(acquire(&path).unwrap(), Instance::Primary(_, _)));
        drop(inherited);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn retry_acquires_lease_when_previous_owner_exits_before_activation() {
        let path = directory();
        let Instance::Primary(lease, listener) = acquire(&path).unwrap() else {
            panic!()
        };
        drop(listener);
        fs::remove_file(path.join("control.sock")).unwrap();
        let exiting = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            drop(lease);
        });
        let restarted = acquire(&path).unwrap();
        assert!(matches!(restarted, Instance::Primary(_, _)));
        exiting.join().unwrap();
        drop(restarted);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn unsafe_directory_and_symlinks_are_rejected_without_modification() {
        let path = directory();
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(acquire(&path).is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let link = directory();
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(acquire(&link).is_err());
        fs::remove_file(link).unwrap();
        fs::remove_dir(path).unwrap();
    }
    #[test]
    fn stale_socket_is_recovered_only_with_lease() {
        let path = directory();
        private_directory(&path).unwrap();
        let socket = UnixListener::bind(path.join("control.sock")).unwrap();
        drop(socket);
        let instance = acquire(&path).unwrap();
        assert!(matches!(instance, Instance::Primary(_, _)));
        drop(instance);
        fs::remove_dir_all(path).unwrap();
    }
}
