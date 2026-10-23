// SPDX-License-Identifier: GPL-3.0-or-later
use super::descriptor;
use crate::hidpp::Hidraw;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Candidate {
    name: String,
    sysfs: PathBuf,
    node: PathBuf,
    uevent: String,
    descriptor: Vec<u8>,
    major: u32,
    minor: u32,
}
impl Candidate {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn describe(&self) -> Value {
        json!({"interface":self.name, "node":self.node, "transport":"Logi Bolt", "vendor":"046D", "product":"C548", "reports":[7,20]})
    }

    /// Verify the opened descriptor too: a hidraw number can be recycled on hotplug.
    pub fn open(&self) -> Result<Hidraw, String> {
        let fresh = inspect(&self.sysfs, &self.node)?.ok_or("Interface is no longer supported")?;
        if fresh.uevent != self.uevent
            || fresh.descriptor != self.descriptor
            || fresh.major != self.major
            || fresh.minor != self.minor
        {
            return Err("Interface changed after discovery; run discovery again".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&self.node)
            .map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.file_type().is_char_device()
            || libc::major(metadata.rdev()) != self.major
            || libc::minor(metadata.rdev()) != self.minor
        {
            return Err("The opened file is not the discovered hidraw device".into());
        }
        verify_ioctl(&file, &self.descriptor)?;
        Hidraw::from_file(file).map_err(|e| e.to_string())
    }
}

pub struct Discovery {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
}
pub fn discover() -> Result<Discovery, String> {
    scan(Path::new("/sys/class/hidraw"), Path::new("/dev"))
}

pub(super) fn valid_name(name: &str) -> bool {
    name.strip_prefix("hidraw")
        .is_some_and(|s| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit()))
}
fn bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(4097).read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > 4096 {
        return Err("Sysfs entry too large".into());
    }
    Ok(bytes)
}
fn inspect(path: &Path, node: &Path) -> Result<Option<Candidate>, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid hidraw name")?;
    if !valid_name(name) {
        return Err("Invalid hidraw name".into());
    }
    let uevent = String::from_utf8(bounded(&path.join("device/uevent"))?)
        .map_err(|_| "Invalid HID uevent")?;
    let ids: Vec<_> = uevent
        .lines()
        .filter_map(|l| l.strip_prefix("HID_ID="))
        .collect();
    if ids.len() != 1 {
        return Err("Missing or duplicate HID_ID".into());
    }
    let parts: Vec<_> = ids[0]
        .split(':')
        .map(|s| u32::from_str_radix(s, 16))
        .collect();
    if !matches!(parts.as_slice(), [Ok(3), Ok(0x046d), Ok(0xc548)]) {
        return Ok(None);
    }
    let descriptor = bounded(&path.join("device/report_descriptor"))?;
    // The receiver also exposes keyboard/mouse interfaces; leave them unopened.
    if descriptor::validate(&descriptor).is_err() {
        return Ok(None);
    }
    let dev =
        String::from_utf8(bounded(&path.join("dev"))?).map_err(|_| "Invalid device number")?;
    let (major, minor) = dev.trim().split_once(':').ok_or("Invalid device number")?;
    Ok(Some(Candidate {
        name: name.into(),
        sysfs: path.into(),
        node: node.into(),
        uevent,
        descriptor,
        major: major.parse().map_err(|_| "Invalid device major")?,
        minor: minor.parse().map_err(|_| "Invalid device minor")?,
    }))
}
pub(super) fn scan(sysfs: &Path, dev: &Path) -> Result<Discovery, String> {
    let mut result = Discovery {
        candidates: Vec::new(),
        warnings: Vec::new(),
    };
    for entry in fs::read_dir(sysfs).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if !name.to_str().is_some_and(valid_name) {
            continue;
        }
        match inspect(&entry.path(), &dev.join(&name)) {
            Ok(Some(candidate)) => result.candidates.push(candidate),
            Ok(None) => {}
            Err(error) => result
                .warnings
                .push(format!("{}: {error}", name.to_string_lossy())),
        }
    }
    result.candidates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

#[repr(C)]
#[derive(Default)]
struct RawInfo {
    bus: u32,
    vendor: i16,
    product: i16,
}
#[repr(C)]
struct RawDescriptor {
    size: u32,
    value: [u8; 4096],
}
fn verify_ioctl(file: &File, expected: &[u8]) -> Result<(), String> {
    let mut info = RawInfo::default();
    let mut size: libc::c_int = 0;
    // SAFETY: each ioctl receives a valid owned descriptor and an appropriately
    // sized, writable C-layout buffer; these metadata ioctls do not send HID setters.
    let status = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<RawInfo>(b'H' as u32, 3),
            &mut info,
        )
    };
    if status < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if info.bus != 3 || info.vendor as u16 != 0x046d || info.product as u16 != 0xc548 {
        return Err("Opened device is not Bolt C548".into());
    }
    let status = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<libc::c_int>(b'H' as u32, 1),
            &mut size,
        )
    };
    if status < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if size <= 0 || size as usize != expected.len() {
        return Err("Opened report descriptor size changed".into());
    }
    let mut descriptor = RawDescriptor {
        size: size as u32,
        value: [0; 4096],
    };
    let status = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<RawDescriptor>(b'H' as u32, 2),
            &mut descriptor,
        )
    };
    if status < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if descriptor.size as usize != expected.len() || &descriptor.value[..expected.len()] != expected
    {
        return Err("Opened report descriptor changed".into());
    }
    Ok(())
}
