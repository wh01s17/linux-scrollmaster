// SPDX-License-Identifier: GPL-3.0-or-later
//! Own profile directory only. Native writers serialize with an advisory lock;
//! legacy Python does not share that lock: do not run the two writers together.
use super::{Document, Management, MAX_DOCUMENT_BYTES};
use std::{
    ffi::{CStr, CString},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    Invalid(String),
    Io(String),
    Conflict,
    Busy,
    /// The rename succeeded. Read again before deciding what to apply/retry.
    PublishedUnconfirmed(String),
}
impl StoreError {
    pub fn may_have_published(&self) -> bool {
        matches!(self, Self::PublishedUnconfirmed(_))
    }
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) | Self::Io(e) => write!(f, "{e}"),
            Self::Conflict => {
                f.write_str("Profiles changed since they were read; reload before editing")
            }
            Self::Busy => f.write_str("Another native profile writer holds the lock"),
            Self::PublishedUnconfirmed(e) => write!(
                f,
                "Profiles published but durability/readback unconfirmed: {e}"
            ),
        }
    }
}
impl std::error::Error for StoreError {}
fn io_error(e: io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

/// Opaque exact-byte revision, tied to the directory from which it was loaded.
/// An absent file and an existing empty document are different revisions.
#[derive(Clone, Debug)]
pub struct Revision {
    directory: PathBuf,
    anchor: Option<(u64, u64)>,
    raw: Option<Vec<u8>>,
    document: Document,
}
impl Revision {
    pub fn document(&self) -> &Document {
        &self.document
    }
}
#[derive(Clone, Debug)]
pub struct ProfileStore {
    directory: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Prepared,
    Published,
}
impl ProfileStore {
    /// Explicit application-owned directory, not a path received over UI IPC.
    /// Construction and reads do not create files or arm any saved profile.
    pub fn at(directory: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let directory = directory.into();
        if !directory.is_absolute() {
            return Err(StoreError::Invalid(
                "Profile directory must be absolute".into(),
            ));
        }
        Ok(Self { directory })
    }
    /// `$XDG_CONFIG_HOME/linux-scrollmaster/profiles.json`, else under `~/.config`.
    pub fn from_environment() -> Result<Self, StoreError> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|p| PathBuf::from(p).join(".config"))
            })
            .ok_or_else(|| StoreError::Invalid("Missing profile configuration directory".into()))?;
        Self::at(config.join("linux-scrollmaster"))
    }
    pub fn load(&self) -> Result<Revision, StoreError> {
        let directory = match open_directory(&self.directory) {
            Ok(dir) => Some(dir),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_error(e)),
        };
        let raw = directory.as_ref().map(read_current).transpose()?.flatten();
        let document = raw
            .as_ref()
            .map(|b| Document::parse(b))
            .transpose()
            .map_err(StoreError::Invalid)?
            .unwrap_or_default();
        Ok(Revision {
            directory: self.directory.clone(),
            anchor: directory
                .as_ref()
                .map(anchor)
                .transpose()
                .map_err(io_error)?,
            raw,
            document,
        })
    }
    pub fn commit(&self, expected: &Revision, document: &Document) -> Result<Revision, StoreError> {
        self.commit_with(expected, document, |_| Ok(()))
    }
    /// Read-only stale-state guard at service phase boundaries. This does not
    /// replace the single-writer rule or lock a legacy writer out of the file.
    pub fn ensure_current(&self, expected: &Revision) -> Result<(), StoreError> {
        if expected.directory != self.directory {
            return Err(StoreError::Conflict);
        }
        let current = self.load()?;
        if current.anchor != expected.anchor || current.raw != expected.raw {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    /// Offline local service operation. Read/plan first, and return a changed
    /// result only after confirmed publication. Previews/exports never write.
    /// Callers needing an explicit edit revision can use load/commit instead.
    pub fn manage(&self, request: &serde_json::Value) -> Result<Management, StoreError> {
        let revision = self.load()?;
        let result = revision
            .document
            .manage(request)
            .map_err(StoreError::Invalid)?;
        if result.changed {
            self.commit(&revision, &result.document)?;
        }
        Ok(result)
    }
    pub(super) fn commit_with(
        &self,
        expected: &Revision,
        document: &Document,
        mut checkpoint: impl FnMut(Stage) -> io::Result<()>,
    ) -> Result<Revision, StoreError> {
        // Finish all validation/encoding before touching the filesystem.
        let bytes = document.encode().map_err(StoreError::Invalid)?;
        if expected.directory != self.directory {
            return Err(StoreError::Conflict);
        }
        let directory = match open_directory(&self.directory) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound && expected.anchor.is_none() => {
                create_directory(&self.directory).map_err(io_error)?;
                open_directory(&self.directory).map_err(io_error)?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(StoreError::Conflict),
            Err(e) => return Err(io_error(e)),
        };
        let identity = anchor(&directory).map_err(io_error)?;
        if expected.anchor.is_some_and(|old| old != identity) {
            return Err(StoreError::Conflict);
        }
        let lock = open_at(
            &directory,
            c".profiles-native.lock",
            libc::O_RDWR | libc::O_CREAT,
            0o600,
        )
        .map_err(io_error)?;
        validate_file(&lock, true).map_err(io_error)?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(
                if io::Error::last_os_error()
                    .raw_os_error()
                    .is_some_and(|e| e == libc::EWOULDBLOCK || e == libc::EAGAIN)
                {
                    StoreError::Busy
                } else {
                    io_error(io::Error::last_os_error())
                },
            );
        }
        // Lock file stays alive through rename, directory fsync and readback.
        if read_current(&directory)? != expected.raw {
            return Err(StoreError::Conflict);
        }
        self.confirm_directory(identity)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = CString::new(format!(
            ".profiles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        let mut temporary = Temporary {
            directory: &directory,
            name,
            created: false,
        };
        let mut file = open_at(
            &directory,
            &temporary.name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
        .map_err(io_error)?;
        temporary.created = true;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        checkpoint(Stage::Prepared).map_err(io_error)?;
        // Detect a legacy/external edit made while the temporary file was written.
        // This is not an atomic CAS against writers that ignore our lock.
        if read_current(&directory)? != expected.raw {
            return Err(StoreError::Conflict);
        }
        self.confirm_directory(identity)?;
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.name.as_ptr(),
                directory.as_raw_fd(),
                c"profiles.json".as_ptr(),
            )
        } != 0
        {
            return Err(io_error(io::Error::last_os_error()));
        }
        temporary.created = false; // Never unlink a replacement at the old temp name.
        checkpoint(Stage::Published)
            .and_then(|_| directory.sync_all())
            .map_err(|e| StoreError::PublishedUnconfirmed(e.to_string()))?;
        self.confirm_directory(identity)
            .map_err(|e| StoreError::PublishedUnconfirmed(e.to_string()))?;
        if read_current(&directory).map_err(|e| StoreError::PublishedUnconfirmed(e.to_string()))?
            != Some(bytes.clone())
        {
            return Err(StoreError::PublishedUnconfirmed(
                "Profile bytes changed after publication".into(),
            ));
        }
        Ok(Revision {
            directory: self.directory.clone(),
            anchor: Some(identity),
            raw: Some(bytes),
            document: document.clone(),
        })
    }
    fn confirm_directory(&self, expected: (u64, u64)) -> Result<(), StoreError> {
        let directory = open_directory(&self.directory).map_err(io_error)?;
        if anchor(&directory).map_err(io_error)? != expected {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
}
fn anchor(directory: &File) -> io::Result<(u64, u64)> {
    let metadata = directory.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}
fn create_directory(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            create_directory(path.parent().ok_or(e)?)?;
            return create_directory(path);
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(e),
    }
    // Persist each newly created directory entry before reporting publication
    // durable, including a previously absent XDG config directory.
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(
            path.parent()
                .ok_or_else(|| io::Error::other("Missing profile parent directory"))?,
        )?
        .sync_all()
}
fn open_directory(path: &Path) -> io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = directory.metadata()?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unsafe profile directory ownership or write permissions",
        ));
    }
    Ok(directory)
}
fn open_at(
    directory: &File,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<File> {
    // SAFETY: fd is live; constant/generated NUL-terminated basenames stay in
    // that directory. NOFOLLOW/NONBLOCK reject symlinks and avoid FIFO blocking.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            mode,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn validate_file(file: &File, private: bool) -> io::Result<()> {
    let metadata = file.metadata()?;
    let forbidden = if private { 0o077 } else { 0o022 };
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & forbidden != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unsafe profile file type, ownership, links or permissions",
        ));
    }
    Ok(())
}
fn read_current(directory: &File) -> Result<Option<Vec<u8>>, StoreError> {
    let file = match open_at(directory, c"profiles.json", libc::O_RDONLY, 0) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e)),
    };
    validate_file(&file, false).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > MAX_DOCUMENT_BYTES as u64 {
        return Err(StoreError::Invalid(
            "Profile document exceeds 128 KiB".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(StoreError::Invalid(
            "Profile document exceeds 128 KiB".into(),
        ));
    }
    Ok(Some(bytes))
}
struct Temporary<'a> {
    directory: &'a File,
    name: CString,
    created: bool,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        if self.created {
            unsafe {
                libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
            }
        }
    }
}
